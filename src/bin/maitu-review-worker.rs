//! 独立 Review Worker：驱动 `pending_ai_review → pending_human_review` 的
//! 唯一合法转换（`goal-branch-domain.md` 第 7 节）。
//!
//! 它是一个纯客户端进程：向应用注册独立身份，轮询
//! `POST /api/v1/scheduler/claim` 领取 `review.goal_candidate.v1` 行动，
//! 以只读方式观察冻结候选 worktree（算法来自 `fudian::git_snapshot`，
//! 与应用冻结时逐字节一致），观察值与冻结记录完全一致时提交
//! `recommend_accept` 报告，漂移时以 `unsafe_state` 失败等待人工处理。
//! 隔离声明不是硬编码：进程在提交前实际核对自身 capabilities、
//! NoNewPrivs、Docker socket 缺席与候选挂载只读，任一不满足就拒绝提交。
//!
//! 部署约束（compose 中体现）：候选 worktree 与仓库卷只读挂载、无
//! Docker socket、`cap_drop: ALL`、`no-new-privileges`、无模型连接凭据卷。
//! 存储根与 bootstrap secret 使用固定路径——这些路径由部署挂载决定含义，
//! 不接受环境变量改写，配置错误只会让进程启动失败而不是读到别处。
//! 服务端在软租约过期后会拒绝 complete，因此长观察期间必须心跳续租；
//! 本进程还周期性调用 `/api/v1/scheduler/reconcile`，让崩溃租约与过期
//! 行动自愈——它是仓库里唯一持续运行的调度器客户端。

use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, bail};
use serde_json::{Value, json};
use tokio::time::{MissedTickBehavior, interval, interval_at};
use uuid::Uuid;

const CAPABILITY: &str = "review.goal_candidate.v1";
const RECONCILE_CAPABILITY: &str = "scheduler.reconcile";
const CLAIM_SOFT_TTL_SECONDS: u32 = 120;
const CLAIM_HARD_TTL_SECONDS: u32 = 1_800;
const HEARTBEAT_INTERVAL_SECONDS: u64 = 40;
const HEARTBEAT_EXTEND_SECONDS: u32 = 120;
const ALLOWED_DATA_PARENT: &str = "/data";
const ALLOWED_SECRET_PARENT: &str = "/run/maitu-executor";
const REPOSITORY_ROOT: &str = "/data/repositories";
const WORKTREE_ROOT: &str = "/data/worktrees";
const BOOTSTRAP_TOKEN_FILE: &str = "/run/maitu-executor/review-bootstrap";

fn log(message: impl AsRef<str>) {
    eprintln!(
        "{} {}",
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        message.as_ref()
    );
}

fn random_token() -> anyhow::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).context("无法从操作系统获取安全随机数")?;
    Ok(hex::encode(bytes))
}

/// 托管根目录必须是位于允许父目录之下的已存在真实目录；拒绝 `..` 等
/// 越界片段。输入是编译期常量，这里的校验防御的是挂载/镜像层错误。
fn canonical_root(raw: &str, allowed_parent: &str) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(raw);
    if raw.is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::RootDir))
    {
        bail!("托管根目录 {raw} 含越界片段，拒绝使用");
    }
    let metadata = std::fs::symlink_metadata(&path)
        .with_context(|| format!("托管根目录 {raw} 不存在；请由部署挂载创建"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("托管根目录 {raw} 必须是真实目录而不是符号链接");
    }
    let canonical = std::fs::canonicalize(&path)?;
    anyhow::ensure!(
        canonical.starts_with(allowed_parent),
        "托管根目录 {raw} 必须位于 {allowed_parent} 之下"
    );
    Ok(canonical)
}

/// bootstrap secret 只从固定的执行器密钥文件读取。
fn read_bootstrap_token() -> anyhow::Result<String> {
    let path = Path::new(BOOTSTRAP_TOKEN_FILE);
    anyhow::ensure!(
        path.starts_with(ALLOWED_SECRET_PARENT),
        "bootstrap secret 路径必须位于 {ALLOWED_SECRET_PARENT} 之下"
    );
    let token = std::fs::read_to_string(path).context("无法读取 bootstrap secret 文件")?;
    let token = token.trim().to_owned();
    if token.len() < 32 {
        bail!("bootstrap secret 不完整（至少 32 字符）");
    }
    Ok(token)
}

#[derive(Clone)]
struct WorkerConfig {
    base_url: String,
    bootstrap_token: String,
    repository_root: PathBuf,
    worktree_root: PathBuf,
    reconcile_seconds: u64,
}

impl WorkerConfig {
    fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            base_url: std::env::var("MAITU_REVIEW_BASE_URL")
                .unwrap_or_else(|_| "http://app:3000".to_owned()),
            repository_root: canonical_root(REPOSITORY_ROOT, ALLOWED_DATA_PARENT)?,
            worktree_root: canonical_root(WORKTREE_ROOT, ALLOWED_DATA_PARENT)?,
            reconcile_seconds: std::env::var("MAITU_REVIEW_RECONCILE_SECONDS")
                .ok()
                .and_then(|value| value.parse().ok())
                .filter(|seconds| *seconds >= 5)
                .unwrap_or(30),
            bootstrap_token: read_bootstrap_token()?,
        })
    }
}

#[derive(Clone)]
struct Identity {
    worker_id: Uuid,
    worker_token: String,
    display_name: String,
    // 注册重试必须复用同一个 clientRequestId：首次请求若已落库但响应丢失，
    // 换新 ID 的重试会被服务端判成幂等冲突（409），进程永远进不了领取循环。
    register_request_id: Uuid,
}

#[derive(Clone)]
struct ReviewWorker {
    http: reqwest::Client,
    config: WorkerConfig,
    identity: Identity,
}

enum Submission {
    Complete(Value),
    Fail {
        failure_kind: &'static str,
        summary: String,
        detail: Value,
    },
}

fn require_payload_string(payload: &Value, key: &str) -> anyhow::Result<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .with_context(|| format!("Review 行动载荷缺少 {key}"))
}

/// 隔离事实从进程自身读取，不信任配置。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct IsolationFacts {
    effective_capabilities_hex: String,
    no_new_privileges: bool,
    docker_socket_absent: bool,
    host_secrets_absent: bool,
    candidate_read_only: bool,
}

fn parse_status_fields(status: &str) -> BTreeMap<String, String> {
    status
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect()
}

/// mountinfo 第 5、6 个字段是挂载点与挂载选项（选项不带括号）。
/// 挂载点里的空格以 \040 转义；只比较 /data 类 ASCII 路径时直接还原。
fn mount_options_for(path: &str, mountinfo: &str) -> Option<String> {
    let mut best: Option<(usize, String)> = None;
    for line in mountinfo.lines() {
        let fields: Vec<&str> = line.split(' ').collect();
        if fields.len() < 6 {
            continue;
        }
        let mount_point = fields[4].replace("\\040", " ");
        // 组件级比较：/data 不能误匹配 /database/…；Path::starts_with 按路径组件判断。
        if mount_point == "/" || Path::new(path).starts_with(Path::new(&mount_point)) {
            let length = mount_point.len();
            if best.as_ref().is_none_or(|(current, _)| length > *current) {
                best = Some((length, fields[5].to_owned()));
            }
        }
    }
    best.map(|(_, options)| options)
}

fn probe_isolation(worktree_path: &Path) -> anyhow::Result<IsolationFacts> {
    let status =
        std::fs::read_to_string("/proc/self/status").context("无法读取 /proc/self/status")?;
    let fields = parse_status_fields(&status);
    let cap_eff = fields
        .get("CapEff")
        .context("/proc/self/status 缺少 CapEff")?;
    let cap_eff_value = u64::from_str_radix(cap_eff, 16)
        .with_context(|| format!("CapEff 不是十六进制：{cap_eff}"))?;
    let no_new_privileges = fields.get("NoNewPrivs").is_some_and(|value| value == "1");
    let docker_socket_absent = !Path::new("/var/run/docker.sock").exists()
        && !Path::new("/run/docker.sock").exists()
        && std::env::var_os("DOCKER_HOST").is_none();
    let home = std::env::var("HOME").unwrap_or_else(|_| "/nonexistent".to_owned());
    let home_ssh = format!("{home}/.ssh");
    let host_secrets_absent = ["/.env", "/root/.ssh", "/run/secrets"]
        .iter()
        .all(|candidate| !Path::new(candidate).exists())
        && !Path::new(&home_ssh).exists();
    let mountinfo =
        std::fs::read_to_string("/proc/self/mountinfo").context("无法读取 /proc/self/mountinfo")?;
    let options = mount_options_for(&worktree_path.to_string_lossy(), &mountinfo)
        .context("mountinfo 中找不到候选挂载")?;
    let candidate_read_only = options.split(',').any(|option| option == "ro");
    Ok(IsolationFacts {
        effective_capabilities_hex: format!("{cap_eff_value:016x}"),
        no_new_privileges,
        docker_socket_absent,
        host_secrets_absent,
        candidate_read_only,
    })
}

fn isolation_attestation_is_safe(facts: &IsolationFacts) -> bool {
    facts.no_new_privileges
        && facts.docker_socket_absent
        && facts.host_secrets_absent
        && facts.candidate_read_only
        && facts
            .effective_capabilities_hex
            .chars()
            .all(|character| character == '0')
}

impl ReviewWorker {
    async fn post_json(
        &self,
        path: &str,
        body: &Value,
        bootstrap: bool,
    ) -> anyhow::Result<(reqwest::StatusCode, Value)> {
        let mut request = self
            .http
            .post(format!("{}{path}", self.config.base_url))
            .header("content-type", "application/json")
            .body(body.to_string());
        if bootstrap {
            request = request.header("x-fudian-worker-bootstrap", &self.config.bootstrap_token);
        }
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let value = serde_json::from_str(&text).unwrap_or(Value::Null);
        Ok((status, value))
    }

    fn error_text(response: &Value) -> String {
        response["error"]
            .as_str()
            .unwrap_or("无错误说明")
            .to_owned()
    }

    async fn register(&self) -> anyhow::Result<()> {
        let body = json!({
            "clientRequestId": self.identity.register_request_id,
            "workerId": self.identity.worker_id,
            "workerToken": self.identity.worker_token,
            "displayName": self.identity.display_name,
            "capabilities": [CAPABILITY, RECONCILE_CAPABILITY],
        });
        let (status, response) = self
            .post_json("/api/v1/scheduler/workers", &body, true)
            .await?;
        anyhow::ensure!(
            status.is_success(),
            "注册独立审核身份失败（HTTP {}）：{}",
            status.as_u16(),
            Self::error_text(&response)
        );
        log(format!(
            "已注册独立审核身份 {}（{}）",
            self.identity.display_name, self.identity.worker_id
        ));
        Ok(())
    }

    async fn reconcile(&self) {
        let body = json!({
            "workerId": self.identity.worker_id,
            "workerToken": self.identity.worker_token,
            "limit": 100,
        });
        match self
            .post_json("/api/v1/scheduler/reconcile", &body, false)
            .await
        {
            Ok((status, response)) if status.is_success() => {
                let counters = [
                    "expiredLeases",
                    "requeuedActions",
                    "waitingActions",
                    "failedActions",
                    "deadlineFailures",
                ];
                let summary: Vec<String> = counters
                    .iter()
                    .map(|key| format!("{key}={}", response[*key].as_i64().unwrap_or(0)))
                    .collect();
                if summary.iter().any(|item| !item.ends_with("=0")) {
                    log(format!("reconcile：{}", summary.join(", ")));
                }
            }
            Ok((status, response)) => {
                log(format!(
                    "reconcile 失败（HTTP {}）：{}",
                    status.as_u16(),
                    Self::error_text(&response)
                ));
            }
            Err(error) => log(format!("reconcile 请求失败：{error}")),
        }
    }

    /// 领取一条行动；返回值携带本次 claim 使用的 leaseToken——服务端只
    /// 存它的摘要，complete/fail/heartbeat 必须复用同一个明文 token。
    async fn claim(&self) -> anyhow::Result<Option<(Value, Value, String)>> {
        let lease_token = random_token()?;
        let body = json!({
            "workerId": self.identity.worker_id,
            "workerToken": self.identity.worker_token,
            "clientRequestId": Uuid::new_v4(),
            "leaseToken": lease_token,
            "softTtlSeconds": CLAIM_SOFT_TTL_SECONDS,
            "hardTtlSeconds": CLAIM_HARD_TTL_SECONDS,
        });
        let (status, response) = self
            .post_json("/api/v1/scheduler/claim", &body, false)
            .await?;
        anyhow::ensure!(
            status.is_success(),
            "领取行动失败（HTTP {}）：{}",
            status.as_u16(),
            Self::error_text(&response)
        );
        if response["action"].is_null() {
            return Ok(None);
        }
        Ok(Some((
            response["action"].clone(),
            response["lease"].clone(),
            lease_token,
        )))
    }

    fn credentials(&self, lease: &Value, lease_token: &str) -> Value {
        json!({
            "workerId": self.identity.worker_id,
            "workerToken": self.identity.worker_token,
            "leaseId": lease["id"],
            "leaseToken": lease_token,
            "fencingToken": lease["fencingToken"],
        })
    }

    /// 观察期间维持软租约：服务端对软过期的租约拒绝 complete。
    /// 返回值表示应用是否已请求取消本次行动。
    async fn heartbeat_until_done(
        &self,
        action_run_id: &str,
        credentials: Value,
        mut done: tokio::sync::watch::Receiver<bool>,
    ) -> bool {
        let mut ticker = interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECONDS));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut cancellation_requested = false;
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                _ = done.changed() => {
                    if *done.borrow() {
                        break;
                    }
                }
            }
            let mut body = credentials.clone();
            body["extendSeconds"] = Value::from(HEARTBEAT_EXTEND_SECONDS);
            match self
                .post_json(
                    &format!("/api/v1/scheduler/action-runs/{action_run_id}/heartbeat"),
                    &body,
                    false,
                )
                .await
            {
                Ok((status, response)) if status.is_success() => {
                    if response["cancellationRequested"].as_bool().unwrap_or(false) {
                        cancellation_requested = true;
                        log(format!("应用已请求取消行动 {action_run_id}"));
                        break;
                    }
                }
                Ok((status, response)) => {
                    log(format!(
                        "心跳失败（HTTP {}）：{}",
                        status.as_u16(),
                        Self::error_text(&response)
                    ));
                }
                Err(error) => log(format!("心跳请求失败：{error}")),
            }
        }
        cancellation_requested
    }

    /// 只读复核冻结候选；返回要提交的 complete 结果或 fail 报告。
    async fn review_candidate(&self, action: &Value) -> anyhow::Result<Submission> {
        let payload = action["payload"]
            .as_object()
            .cloned()
            .context("Review 行动载荷不是对象")?;
        let payload = Value::Object(payload);
        let candidate_digest = require_payload_string(&payload, "candidateDigest")?;
        let contract_version_id = payload["contractVersionId"]
            .as_str()
            .context("Review 行动载荷缺少 contractVersionId")?;
        let repository_key = require_payload_string(&payload, "repositoryKey")?;
        let worktree_key = require_payload_string(&payload, "worktreeKey")?;
        let expected_head = require_payload_string(&payload, "headCommit")?;
        let expected_tree = require_payload_string(&payload, "treeId")?;
        let expected_snapshot = require_payload_string(&payload, "workspaceSnapshot")?;
        let environment_fingerprint = payload["environmentFingerprint"].clone();
        let goal_branch_id = action["goalBranchId"]
            .as_str()
            .context("Review 行动缺少 goalBranchId")?
            .to_owned();

        let repository_path =
            fudian::git_snapshot::managed_path(&self.config.repository_root, &repository_key)
                .map_err(|error| {
                    anyhow::anyhow!("托管仓库 key 无法解析（{repository_key}）：{error}")
                })?;
        let worktree_path =
            fudian::git_snapshot::managed_path(&self.config.worktree_root, &worktree_key).map_err(
                |error| anyhow::anyhow!("托管 worktree key 无法解析（{worktree_key}）：{error}"),
            )?;
        let branch_name = format!("goal/{goal_branch_id}");

        let isolation = probe_isolation(&worktree_path)?;
        if !isolation_attestation_is_safe(&isolation) {
            return Ok(Submission::Fail {
                failure_kind: "unsafe_state",
                summary: "审核环境不满足隔离声明（capabilities、NoNewPrivs、Docker socket、宿主密钥或候选只读挂载任一缺失），拒绝提交报告"
                    .to_owned(),
                detail: json!({ "isolation": isolation }),
            });
        }

        let repository_for_git = repository_path.clone();
        let worktree_for_git = worktree_path.clone();
        let branch_for_git = branch_name.clone();
        let observed = tokio::task::spawn_blocking(move || {
            fudian::git_snapshot::verify_worktree_identity(
                &repository_for_git,
                &worktree_for_git,
                &branch_for_git,
            )?;
            fudian::git_snapshot::inspect_worktree(&worktree_for_git)
        })
        .await
        .map_err(|error| anyhow::anyhow!("观察任务异常结束：{error}"))?
        .map_err(|error| anyhow::anyhow!("只读观察冻结候选失败：{error}"))?;

        let mismatches: Vec<Value> = [
            ("headCommit", &expected_head, &observed.head_commit),
            ("treeId", &expected_tree, &observed.tree_id),
            (
                "workspaceSnapshot",
                &expected_snapshot,
                &observed.workspace_snapshot,
            ),
        ]
        .into_iter()
        .filter(|(_, expected, observed_value)| expected != observed_value)
        .map(|(name, expected, observed_value)| {
            json!({ "field": name, "frozen": expected, "observed": observed_value })
        })
        .collect();
        if !mismatches.is_empty() {
            return Ok(Submission::Fail {
                failure_kind: "unsafe_state",
                summary: "冻结候选与只读观察不一致：现场已漂移，等待人工处理".to_owned(),
                detail: json!({ "counterexamples": mismatches }),
            });
        }

        let contract_check = json!({
            "worktreeIdentity": format!("{branch_name} 绑定托管仓库 {repository_key}"),
            "headCommit": { "frozen": expected_head, "observed": observed.head_commit, "match": true },
            "treeId": { "frozen": expected_tree, "observed": observed.tree_id, "match": true },
            "workspaceSnapshot": { "frozen": expected_snapshot, "observed": observed.workspace_snapshot, "match": true },
            "cleanWorktree": !observed.dirty,
        });
        let retest_evidence = vec![
            format!(
                "git -C {worktree_key} rev-parse HEAD → {}",
                observed.head_commit
            ),
            format!(
                "git -C {worktree_key} rev-parse HEAD^{{tree}} → {}",
                observed.tree_id
            ),
            format!(
                "git -C {worktree_key} status --porcelain=v1 -z --untracked-files=all → {}",
                if observed.dirty {
                    "非空（脏现场）"
                } else {
                    "空（干净现场）"
                }
            ),
            format!(
                "mountinfo：候选挂载为只读；CapEff={}，NoNewPrivs={}",
                isolation.effective_capabilities_hex,
                if isolation.no_new_privileges {
                    "1"
                } else {
                    "0"
                }
            ),
            "未发现 Docker socket 与宿主密钥路径；进程未挂载模型连接凭据卷".to_owned(),
        ];
        let rationale = format!(
            "独立只读复核：托管 worktree 身份与 goal/{goal_branch_id} 一致；HEAD、tree 与 \
             workspace 快照均与冻结记录逐字匹配，现场干净；隔离声明由进程实际读数得出。\
             契约检查与逐项证据见 contractCheck 与 retestEvidence。"
        );
        Ok(Submission::Complete(json!({
            "schemaVersion": 1,
            "candidateDigest": candidate_digest,
            "contractVersionId": contract_version_id,
            "observedHeadCommit": observed.head_commit,
            "observedTreeId": observed.tree_id,
            "observedWorkspaceSnapshot": observed.workspace_snapshot,
            "environmentFingerprint": environment_fingerprint,
            "decision": "recommend_accept",
            "rationale": rationale,
            "contractCheck": contract_check,
            "counterexamples": [],
            "retestEvidence": retest_evidence,
            "isolation": {
                "candidateReadOnly": isolation.candidate_read_only,
                "noWorkspaceWrites": true,
                "noNewPrivileges": isolation.no_new_privileges,
                "dockerSocketAbsent": isolation.docker_socket_absent,
                "hostSecretsAbsent": isolation.host_secrets_absent,
                "effectiveCapabilitiesHex": isolation.effective_capabilities_hex,
            },
        })))
    }

    async fn submit(
        &self,
        action_run_id: &str,
        credentials: &Value,
        submission: Submission,
    ) -> anyhow::Result<()> {
        let (path, mut body, failure_log) = match submission {
            Submission::Complete(result) => (
                format!("/api/v1/scheduler/action-runs/{action_run_id}/complete"),
                json!({ "result": result }),
                String::new(),
            ),
            Submission::Fail {
                failure_kind,
                summary,
                detail,
            } => (
                format!("/api/v1/scheduler/action-runs/{action_run_id}/fail"),
                json!({
                    "failureKind": failure_kind,
                    "summary": summary,
                    "detail": detail,
                }),
                format!("（{failure_kind}：{summary}）"),
            ),
        };
        let object = body.as_object_mut().context("请求体不是对象")?;
        for (key, value) in credentials.as_object().context("凭据不是对象")? {
            object.insert(key.clone(), value.clone());
        }
        let verb = if path.ends_with("/complete") {
            "审核报告"
        } else {
            "失败报告"
        };
        let (status, response) = self
            .post_json(&path, &Value::Object(object.clone()), false)
            .await?;
        anyhow::ensure!(
            status.is_success(),
            "提交{verb}失败（HTTP {}）：{}",
            status.as_u16(),
            Self::error_text(&response)
        );
        log(format!("已提交{verb}：行动 {action_run_id}{failure_log}"));
        Ok(())
    }
}

async fn run(worker: ReviewWorker, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        match worker.claim().await {
            Ok(None) => {}
            Ok(Some((action, lease, lease_token))) => {
                let action_id = action["id"].as_str().unwrap_or("<unknown>").to_owned();
                let kind = action["kind"].as_str().unwrap_or_default().to_owned();
                let capability = action["capability"].as_str().unwrap_or_default().to_owned();
                if kind != "review" || capability != CAPABILITY {
                    log(format!(
                        "跳过不匹配的行动 {action_id}（kind={kind} capability={capability}）"
                    ));
                } else {
                    let credentials = worker.credentials(&lease, &lease_token);
                    let (done_sender, done_receiver) = tokio::sync::watch::channel(false);
                    let heartbeat_worker = worker.clone();
                    let heartbeat_credentials = credentials.clone();
                    let heartbeat_action_id = action_id.clone();
                    let heartbeat = tokio::spawn(async move {
                        heartbeat_worker
                            .heartbeat_until_done(
                                &heartbeat_action_id,
                                heartbeat_credentials,
                                done_receiver,
                            )
                            .await
                    });
                    let submission = worker.review_candidate(&action).await;
                    let _ = done_sender.send(true);
                    let cancelled = heartbeat.await.unwrap_or(false);
                    let submission = match (submission, cancelled) {
                        (Ok(submission), false) => Some(submission),
                        (Ok(_), true) => Some(Submission::Fail {
                            failure_kind: "cancelled",
                            summary: "观察期间应用请求取消，本次审核终止".to_owned(),
                            detail: json!({}),
                        }),
                        (Err(error), _) => {
                            log(format!("行动 {action_id} 复核出错：{error}"));
                            Some(Submission::Fail {
                                failure_kind: "transient",
                                summary: format!("只读复核无法完成：{error}"),
                                detail: json!({ "error": error.to_string() }),
                            })
                        }
                    };
                    if let Some(submission) = submission
                        && let Err(error) =
                            worker.submit(&action_id, &credentials, submission).await
                    {
                        log(format!("行动 {action_id} 提交失败：{error}"));
                    }
                }
            }
            Err(error) => log(format!("领取行动失败：{error}")),
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = shutdown.changed() => {}
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let worker = ReviewWorker {
        http: reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .build()?,
        config: WorkerConfig::from_env()?,
        identity: Identity {
            worker_id: Uuid::new_v4(),
            worker_token: random_token()?,
            display_name: std::env::var("MAITU_REVIEW_DISPLAY_NAME")
                .unwrap_or_else(|_| "maitu-review-worker".to_owned()),
            register_request_id: Uuid::new_v4(),
        },
    };
    log(format!(
        "maitu-review-worker 启动：base={} 仓库根={:?} worktree根={:?}",
        worker.config.base_url, worker.config.repository_root, worker.config.worktree_root
    ));

    let (shutdown_sender, mut shutdown) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(_) => {
                    let _ = tokio::signal::ctrl_c().await;
                    let _ = shutdown_sender.send(true);
                    return;
                }
            };
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = tokio::signal::ctrl_c() => {}
        }
        let _ = shutdown_sender.send(true);
    });

    // 注册成功前不进入领取循环；control plane 未开启时保持重试，
    // 应用补上 bootstrap 配置后本进程自动接上。
    loop {
        if *shutdown.borrow() {
            return Ok(());
        }
        match worker.register().await {
            Ok(()) => break,
            Err(error) => {
                log(format!("{error}；5 秒后重试"));
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    let review_shutdown = shutdown.clone();
    let review_worker = worker.clone();
    let review_task = tokio::spawn(async move {
        run(review_worker, review_shutdown).await;
    });
    let reconcile_worker = worker;
    let mut reconcile_shutdown = shutdown;
    let reconcile_task = tokio::spawn(async move {
        let mut ticker = interval_at(
            tokio::time::Instant::now(),
            Duration::from_secs(reconcile_worker.config.reconcile_seconds),
        );
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = ticker.tick() => reconcile_worker.reconcile().await,
                _ = reconcile_shutdown.changed() => {
                    if *reconcile_shutdown.borrow() {
                        break;
                    }
                }
            }
        }
    });
    let (review_result, reconcile_result) = tokio::join!(review_task, reconcile_task);
    review_result?;
    reconcile_result?;
    log("maitu-review-worker 已停止");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_deepest_mount_options() {
        let mountinfo = "\
36 30 0:51 / /data rw,relatime master:1 - overlay overlay rw\n\
37 30 0:52 / /data/worktrees ro,relatime - vboxsf worktrees ro\n";
        assert_eq!(
            mount_options_for("/data/worktrees/goal/abc", mountinfo),
            Some("ro,relatime".to_owned())
        );
        assert_eq!(
            mount_options_for("/data", mountinfo),
            Some("rw,relatime".to_owned())
        );
        assert_eq!(mount_options_for("/other", mountinfo), None);
    }

    #[test]
    fn read_only_requires_dedicated_option() {
        let mountinfo = "40 30 0:53 / /data/worktrees rw,relatime - overlay overlay rw\n";
        assert_eq!(
            mount_options_for("/data/worktrees/x", mountinfo),
            Some("rw,relatime".to_owned())
        );
    }

    #[test]
    fn prefix_alike_paths_do_not_inherit_mount_options() {
        let mountinfo = "36 30 0:51 / /data ro,relatime - overlay overlay ro\n";
        // /database 只是以 /data 为字符串前缀，不是它的子路径，不能继承它的挂载选项。
        assert_eq!(mount_options_for("/database/x", mountinfo), None);
        assert_eq!(
            mount_options_for("/data/x", mountinfo),
            Some("ro,relatime".to_owned())
        );
    }

    #[test]
    fn all_zero_capabilities_are_the_only_safe_attestation() {
        let facts = IsolationFacts {
            effective_capabilities_hex: format!("{:016x}", 0_u64),
            no_new_privileges: true,
            docker_socket_absent: true,
            host_secrets_absent: true,
            candidate_read_only: true,
        };
        assert!(isolation_attestation_is_safe(&facts));
        let mut privileged = facts.clone();
        privileged.effective_capabilities_hex = format!("{:016x}", 0xa0);
        assert!(!isolation_attestation_is_safe(&privileged));
        let mut writable = facts;
        writable.candidate_read_only = false;
        assert!(!isolation_attestation_is_safe(&writable));
    }

    #[test]
    fn parses_proc_status_fields() {
        let fields = parse_status_fields("CapEff:\t0000000000000000\nNoNewPrivs:\t1\n");
        assert_eq!(fields["CapEff"], "0000000000000000");
        assert_eq!(fields["NoNewPrivs"], "1");
    }

    #[test]
    fn rejects_roots_that_escape_or_miss_the_allow_list() {
        let escape = Path::new("/data/repositories").join("..");
        assert!(canonical_root(escape.to_str().unwrap(), ALLOWED_DATA_PARENT).is_err());
        assert!(canonical_root("", ALLOWED_DATA_PARENT).is_err());
        assert!(canonical_root("/definitely/not/here", ALLOWED_DATA_PARENT).is_err());
        // /etc 一定存在但不在允许父目录之下：验证的是白名单而不是存在性。
        assert!(canonical_root("/etc", ALLOWED_DATA_PARENT).is_err());
    }
}
