use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::Duration,
};

use chrono::{DateTime, Utc};
use fudian::{
    code_check_protocol::{CheckCommand, CheckRequest, CheckResult},
    runner_protocol::is_portable_workspace_file_path,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, types::Json};
use tokio::{fs, process::Command};
use uuid::Uuid;

use crate::{
    artifacts::ArtifactStore,
    config::Config,
    error::{AppError, AppResult},
    web::AppState,
};

const MAX_PROJECT_BYTES: usize = 20 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 1024 * 1024;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeFile {
    pub path: String,
    pub content: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportRequest {
    pub request_id: Uuid,
    pub source_name: String,
    pub files: Vec<CodeFile>,
    pub checks: Vec<CheckCommand>,
}

#[derive(Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeProject {
    pub project_id: Uuid,
    pub import_request_id: Uuid,
    pub source_name: String,
    pub import_hash: String,
    pub initial_commit: String,
    pub accepted_commit: String,
    pub checks: Json<Vec<CheckCommand>>,
    pub file_count: i32,
    pub size_bytes: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeAttempt {
    pub attempt_id: Uuid,
    pub project_id: Uuid,
    pub base_commit: String,
    pub workspace_key: String,
    pub candidate_commit: Option<String>,
    pub patch_artifact_id: Option<Uuid>,
    pub check_results: Json<Vec<CheckResult>>,
    pub adopted_commit: Option<String>,
    pub adopted_at: Option<DateTime<Utc>>,
}

#[derive(FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: i64,
    pub attempt_id: Uuid,
    pub kind: String,
    pub label: String,
    pub status: String,
    pub input: Json<Value>,
    pub output: Option<Json<Value>>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub fn validate_path(value: &str) -> AppResult<()> {
    if !is_portable_workspace_file_path(value)
        || value.split('/').any(|part| {
            part.starts_with(".maitu-write-")
                || matches!(part, "node_modules" | "target" | ".ssh" | ".aws")
                || part == ".env"
                || (part.starts_with(".env.") && !part.ends_with(".example"))
        })
    {
        return Err(AppError::bad_request(
            "invalid_code_path",
            "文件路径须位于项目内，不能包含凭据目录、真实环境配置或构建缓存",
        ));
    }
    Ok(())
}

fn repository(config: &Config, project: Uuid) -> PathBuf {
    config
        .repository_root
        .join("maitu-code")
        .join(project.to_string())
        .join("repository.git")
}
pub fn workspace(config: &Config, key: Uuid) -> PathBuf {
    config
        .worktree_root
        .join("maitu-code")
        .join(key.to_string())
}

pub async fn git(directory: &Path, args: &[&str]) -> AppResult<String> {
    let output = git_bytes(directory, args).await?;
    String::from_utf8(output).map_err(|_| AppError::internal("代码仓库返回了无法读取的文本"))
}

pub async fn git_bytes(directory: &Path, args: &[&str]) -> AppResult<Vec<u8>> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("core.quotepath=false")
        .arg("-c")
        .arg("user.name=Maitu")
        .arg("-c")
        .arg("user.email=maitu@local.invalid")
        .arg("-c")
        .arg("commit.gpgSign=false")
        .arg("-C")
        .arg(directory)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true)
        .output()
        .await?;
    if !output.status.success() {
        return Err(AppError::conflict(
            "code_git_operation",
            "代码仓库操作未完成；请查看任务记录，冲突需要处理后继续",
        ));
    }
    Ok(output.stdout)
}

pub async fn project(pool: &PgPool, id: Uuid) -> AppResult<Option<CodeProject>> {
    Ok(
        sqlx::query_as("SELECT * FROM maitu_code_projects WHERE project_id=$1")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn import(
    state: &AppState,
    project_id: Uuid,
    mut request: ImportRequest,
) -> AppResult<CodeProject> {
    if request.source_name.trim().is_empty()
        || request.source_name.len() > 400
        || request.files.is_empty()
        || request.files.len() > 2000
        || request.checks.is_empty()
        || request.checks.len() > 12
    {
        return Err(AppError::bad_request(
            "invalid_code_project",
            "选择代码文件并提供至少一种实际检查方式",
        ));
    }
    let mut paths = HashSet::new();
    let mut size = 0;
    for file in &request.files {
        validate_path(&file.path)?;
        if !paths.insert(file.path.to_lowercase())
            || file.content.len() > MAX_FILE_BYTES
            || file.content.contains('\0')
        {
            return Err(AppError::bad_request(
                "invalid_code_file",
                "代码文件不能重名，须为不超过 1 MiB 的文本",
            ));
        }
        size += file.content.len();
    }
    if size > MAX_PROJECT_BYTES {
        return Err(AppError::bad_request(
            "code_import_limit",
            "本次代码文本超过 20 MiB，请排除构建缓存和大型原件",
        ));
    }
    for path in &paths {
        let parts: Vec<_> = path.split('/').collect();
        for depth in 1..parts.len() {
            if paths.contains(&parts[..depth].join("/")) {
                return Err(AppError::bad_request(
                    "code_path_collision",
                    "同一路径不能同时用作文件和目录",
                ));
            }
        }
    }
    let mut check_ids = HashSet::new();
    for check in &request.checks {
        check
            .validate()
            .map_err(|message| AppError::bad_request("invalid_code_check", message))?;
        if !check_ids.insert(&check.id) {
            return Err(AppError::bad_request(
                "invalid_code_check",
                "检查标识不能重复",
            ));
        }
    }
    request.files.sort_by(|a, b| a.path.cmp(&b.path));
    let hash = hex::encode(Sha256::digest(serde_json::to_vec(&request)?));
    let mut tx = state.pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id=$1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    let existing: Option<CodeProject> = sqlx::query_as(
        "SELECT * FROM maitu_code_projects WHERE project_id=$1 OR import_request_id=$2",
    )
    .bind(project_id)
    .bind(request.request_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(existing) = existing {
        if existing.project_id == project_id
            && existing.import_request_id == request.request_id
            && existing.import_hash == hash
        {
            return Ok(existing);
        }
        return Err(AppError::conflict(
            "code_project_exists",
            "项目已有代码基线；已有导入和执行不会被覆盖",
        ));
    }
    let repo = repository(&state.config, project_id);
    let root = workspace(&state.config, request.request_id);
    if fs::try_exists(&root).await? || fs::try_exists(&repo).await? {
        return Err(AppError::conflict(
            "code_import_unfinished",
            "上次导入现场仍在，不能自动覆盖；需要先核对后恢复",
        ));
    }
    fs::create_dir_all(&repo).await?;
    fs::create_dir_all(&root).await?;
    git(&repo, &["init", "--bare", "--initial-branch=main"]).await?;
    git(&root, &["init", "--initial-branch=main"]).await?;
    for file in &request.files {
        atomic_write_file(&root, &file.path, &file.content).await?;
    }
    git(&root, &["add", "--force", "--all"]).await?;
    git(&root, &["commit", "-m", "导入项目代码基线"]).await?;
    let commit = git(&root, &["rev-parse", "HEAD"]).await?.trim().to_owned();
    git(
        &root,
        &[
            "push",
            repo.to_str()
                .ok_or_else(|| AppError::internal("仓库路径无法读取"))?,
            "HEAD:refs/heads/main",
        ],
    )
    .await?;
    let record=sqlx::query_as("INSERT INTO maitu_code_projects(project_id,import_request_id,source_name,import_hash,initial_commit,accepted_commit,checks,file_count,size_bytes) VALUES($1,$2,$3,$4,$5,$5,$6,$7,$8) RETURNING *")
        .bind(project_id).bind(request.request_id).bind(request.source_name.trim()).bind(hash).bind(commit)
        .bind(Json(&request.checks)).bind(request.files.len() as i32).bind(size as i64).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(record)
}

async fn safe_file(root: &Path, relative: &str) -> AppResult<PathBuf> {
    validate_path(relative)?;
    let root = fs::canonicalize(root).await?;
    let mut current = root.clone();
    for part in relative.split('/') {
        current.push(part);
        match fs::symlink_metadata(&current).await {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::forbidden(
                    "code_symlink",
                    "不能通过符号链接访问项目外部",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    if !current.starts_with(root) {
        return Err(AppError::forbidden(
            "code_path_escape",
            "文件不在代码工作区内",
        ));
    }
    Ok(current)
}

pub async fn read_file(root: &Path, path: &str) -> AppResult<String> {
    let path = safe_file(root, path).await?;
    let metadata = fs::metadata(&path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AppError::not_found("工作区中没有这个文件，请先列出文件确认路径")
        } else {
            error.into()
        }
    })?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(AppError::bad_request(
            "code_file_limit",
            "只能读取不超过 1 MiB 的文本文件",
        ));
    }
    fs::read_to_string(path).await.map_err(Into::into)
}

pub async fn write_file(root: &Path, path: &str, content: &str) -> AppResult<()> {
    if content.len() > MAX_FILE_BYTES || content.contains('\0') {
        return Err(AppError::bad_request(
            "code_file_limit",
            "代码文件内容超过限制或不是文本",
        ));
    }
    let mut total = content.len();
    for relative in list_files(root).await? {
        if relative != path {
            total += fs::metadata(root.join(relative)).await?.len() as usize;
        }
    }
    if total > MAX_PROJECT_BYTES {
        return Err(AppError::bad_request(
            "code_project_limit",
            "本次修改将使代码文本超过 20 MiB",
        ));
    }
    atomic_write_file(root, path, content).await
}

async fn atomic_write_file(root: &Path, path: &str, content: &str) -> AppResult<()> {
    let target = safe_file(root, path).await?;
    fs::create_dir_all(
        target
            .parent()
            .ok_or_else(|| AppError::bad_request("invalid_code_path", "文件路径不正确"))?,
    )
    .await?;
    let temporary = root.join(format!(".maitu-write-{}", Uuid::new_v4()));
    fs::write(&temporary, content).await?;
    fs::rename(&temporary, &target).await?;
    Ok(())
}

pub async fn list_files(root: &Path) -> AppResult<Vec<String>> {
    let mut dirs = vec![root.to_owned()];
    let mut result = Vec::new();
    while let Some(directory) = dirs.pop() {
        let mut entries = fs::read_dir(directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| AppError::internal("文件不在工作区内"))?
                .to_string_lossy()
                .replace('\\', "/");
            if validate_path(&relative).is_err() {
                continue;
            }
            let kind = entry.file_type().await?;
            if kind.is_symlink() {
                return Err(AppError::forbidden(
                    "code_symlink",
                    "工作区包含无法读取的符号链接",
                ));
            }
            if kind.is_dir() {
                dirs.push(path);
            } else if kind.is_file() {
                result.push(relative);
            }
            if result.len() > 2000 {
                return Err(AppError::bad_request(
                    "code_file_limit",
                    "工作区文件数量超过本轮限制",
                ));
            }
        }
    }
    result.sort();
    Ok(result)
}

pub async fn prepare(
    state: &AppState,
    attempt: Uuid,
    project_id: Uuid,
    base: &str,
    upstream: &Value,
) -> AppResult<CodeAttempt> {
    let root = workspace(&state.config, attempt);
    let repo = repository(&state.config, project_id);
    fs::create_dir_all(
        root.parent()
            .ok_or_else(|| AppError::internal("工作区目录不正确"))?,
    )
    .await?;
    if fs::try_exists(&root).await? {
        return Err(AppError::conflict(
            "code_workspace_exists",
            "本次工作区已经存在，不能盲目重新执行",
        ));
    }
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--detach",
            root.to_str()
                .ok_or_else(|| AppError::internal("工作区路径不正确"))?,
            base,
        ],
    )
    .await?;
    let record=sqlx::query_as("INSERT INTO maitu_code_attempts(attempt_id,project_id,base_commit,workspace_key) VALUES($1,$2,$3,$4) RETURNING *")
        .bind(attempt).bind(project_id).bind(base).bind(attempt.to_string()).fetch_one(&state.pool).await?;
    if let Some(parents) = upstream.as_array() {
        for parent in parents {
            if let Some(id) = parent["attemptId"]
                .as_str()
                .and_then(|value| Uuid::parse_str(value).ok())
            {
                let candidate: Option<String>=sqlx::query_scalar("SELECT candidate_commit FROM maitu_code_attempts WHERE attempt_id=$1 AND project_id=$2")
                    .bind(id).bind(project_id).fetch_optional(&state.pool).await?.flatten();
                if let Some(commit) = candidate {
                    git(&root, &["merge", "--no-edit", &commit]).await?;
                } else if parent["taskKind"] == "code" {
                    return Err(AppError::conflict(
                        "upstream_code_missing",
                        "已采用的前序代码版本无法读取，不能跳过后继续整合",
                    ));
                }
            }
        }
    }
    Ok(record)
}

pub async fn capture(
    state: &AppState,
    project: Uuid,
    attempt: Uuid,
) -> AppResult<(String, String)> {
    let root = workspace(&state.config, attempt);
    git(
        &root,
        &[
            "add",
            "--force",
            "--all",
            "--",
            ".",
            ":(exclude).maitu-write-*",
        ],
    )
    .await?;
    git(
        &root,
        &["commit", "--allow-empty", "-m", "保存编码任务成果"],
    )
    .await?;
    let commit = git(&root, &["rev-parse", "HEAD"]).await?.trim().to_owned();
    let base: String = sqlx::query_scalar(
        "SELECT base_commit FROM maitu_code_attempts WHERE attempt_id=$1 AND project_id=$2",
    )
    .bind(attempt)
    .bind(project)
    .fetch_one(&state.pool)
    .await?;
    let patch = git(
        &root,
        &["diff", "--no-ext-diff", "--binary", &base, &commit, "--"],
    )
    .await?;
    git(
        &repository(&state.config, project),
        &[
            "update-ref",
            &format!("refs/maitu/attempts/{attempt}"),
            &commit,
        ],
    )
    .await?;
    Ok((commit, patch))
}

pub async fn begin_operation(
    pool: &PgPool,
    attempt: Uuid,
    kind: &str,
    label: &str,
    input: Value,
) -> AppResult<i64> {
    Ok(sqlx::query_scalar("INSERT INTO maitu_execution_operations(attempt_id,kind,label,status,input) VALUES($1,$2,$3,'running',$4) RETURNING id")
        .bind(attempt).bind(kind).bind(label).bind(Json(input)).fetch_one(pool).await?)
}

pub async fn end_operation(pool: &PgPool, id: i64, success: bool, output: Value) -> AppResult<()> {
    sqlx::query("UPDATE maitu_execution_operations SET status=$2,output=$3,completed_at=now() WHERE id=$1 AND status='running'")
        .bind(id).bind(if output["resultUncertain"]==true {"interrupted"} else if success {"succeeded"} else {"failed"}).bind(Json(output)).execute(pool).await?;
    Ok(())
}

pub async fn operations(pool: &PgPool, task: Uuid) -> AppResult<Vec<Operation>> {
    Ok(sqlx::query_as("SELECT o.* FROM maitu_execution_operations o JOIN maitu_attempts a ON a.id=o.attempt_id WHERE a.task_id=$1 ORDER BY o.id")
        .bind(task).fetch_all(pool).await?)
}

pub async fn attempts(pool: &PgPool, task: Uuid) -> AppResult<Vec<CodeAttempt>> {
    Ok(sqlx::query_as("SELECT c.* FROM maitu_code_attempts c JOIN maitu_attempts a ON a.id=c.attempt_id WHERE a.task_id=$1 ORDER BY a.number DESC")
        .bind(task).fetch_all(pool).await?)
}

pub async fn run_check(
    state: &AppState,
    attempt: Uuid,
    key: Uuid,
    command: &CheckCommand,
) -> AppResult<CheckResult> {
    let request = CheckRequest {
        request_id: Uuid::new_v4(),
        workspace_key: key,
        command: command.clone(),
    };
    let op = begin_operation(
        &state.pool,
        attempt,
        "check",
        &command.label,
        json!(request),
    )
    .await?;
    let result = call_check_worker(&request).await;
    match &result {
        Ok(report) => end_operation(&state.pool, op, report.succeeded(), json!(report)).await?,
        Err(error) => {
            end_operation(
                &state.pool,
                op,
                false,
                json!({"error":error.public_message(),"resultUncertain":true}),
            )
            .await?
        }
    }
    result
}

pub(super) async fn call_check_worker(request: &CheckRequest) -> AppResult<CheckResult> {
    let endpoint = std::env::var("MAITU_CHECK_WORKER_URL")
        .unwrap_or_else(|_| "http://check-worker:3001".into());
    let auth = std::env::var("MAITU_CHECK_WORKER_TOKEN_FILE")
        .unwrap_or_else(|_| "/run/maitu-executor/token".into());
    let token = fs::read_to_string(auth).await.map_err(|_| {
        AppError::conflict(
            "code_worker_unavailable",
            "代码检查服务尚未启动，请按本机运行说明启用",
        )
    })?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(900))
        .build()
        .map_err(|_| AppError::internal("无法建立检查连接"))?;
    let response = client
        .post(format!("{}/checks", endpoint.trim_end_matches('/')))
        .bearer_auth(token.trim())
        .header("content-type", "application/json")
        .body(serde_json::to_vec(request)?)
        .send()
        .await
        .map_err(|_| {
            AppError::conflict(
                "code_check_connection",
                "检查连接中断，进程结果尚不确定；本次记录会保留",
            )
        })?;
    if !response.status().is_success() {
        return Err(AppError::conflict(
            "code_check_unavailable",
            "检查服务未能完成请求；请查看记录后继续",
        ));
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AppError::internal("检查结果无法读取"))?
    {
        if body.len() + chunk.len() > 512 * 1024 {
            return Err(AppError::internal("检查结果超过记录限制"));
        }
        body.extend_from_slice(&chunk);
    }
    let report: CheckResult = serde_json::from_slice(&body)?;
    if report.request_id != request.request_id || report.command != request.command {
        return Err(AppError::conflict(
            "code_check_identity",
            "检查结果与本次命令不一致",
        ));
    }
    Ok(report)
}

pub async fn ensure_worker_ready() -> AppResult<()> {
    let endpoint = std::env::var("MAITU_CHECK_WORKER_URL")
        .unwrap_or_else(|_| "http://check-worker:3001".into());
    let auth = std::env::var("MAITU_CHECK_WORKER_TOKEN_FILE")
        .unwrap_or_else(|_| "/run/maitu-executor/token".into());
    if !fs::try_exists(auth).await? {
        return Err(AppError::conflict(
            "code_worker_unavailable",
            "代码检查服务尚未启动；请使用完整的脉图启动入口",
        ));
    }
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| AppError::internal("检查连接无法建立"))?
        .get(format!("{}/api/health", endpoint.trim_end_matches('/')))
        .send()
        .await
        .map_err(|_| {
            AppError::conflict(
                "code_worker_unavailable",
                "代码检查服务暂时不可用；本次尚未调用模型",
            )
        })?;
    if !response.status().is_success() {
        return Err(AppError::conflict(
            "code_worker_unavailable",
            "代码检查服务尚未就绪；本次尚未调用模型",
        ));
    }
    Ok(())
}

pub async fn planning_context(state: &AppState, project_id: Uuid, base: &str) -> AppResult<Value> {
    let repo = repository(&state.config, project_id);
    let names = git(&repo, &["ls-tree", "-r", "--name-only", base]).await?;
    let files: Vec<&str> = names
        .lines()
        .filter(|name| validate_path(name).is_ok())
        .collect();
    let mut excerpts = Vec::new();
    for path in files
        .iter()
        .filter(|path| {
            path.to_ascii_lowercase().starts_with("readme")
                || matches!(**path, "Cargo.toml" | "package.json" | "pyproject.toml")
        })
        .take(8)
    {
        let content = git(&repo, &["show", &format!("{base}:{path}")]).await?;
        let excerpt: String = content.chars().take(8000).collect();
        excerpts
            .push(json!({"path":path,"content":excerpt,"truncated":excerpt.len()<content.len()}));
    }
    Ok(
        json!({"baseCommit":base,"fileCount":files.len(),"files":files.iter().take(300).collect::<Vec<_>>(),"fileListTruncated":files.len()>300,"excerpts":excerpts}),
    )
}

pub async fn save_candidate(
    state: &AppState,
    project_id: Uuid,
    attempt: Uuid,
    checks: &[CheckResult],
) -> AppResult<(String, Uuid)> {
    let (commit, patch) = capture(state, project_id, attempt).await?;
    let artifact = Uuid::new_v4();
    let stored = ArtifactStore::new(state.config.artifact_root.clone())
        .write_text(project_id, artifact, "changes.patch", &patch)
        .await?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO artifacts(id,project_id,title,kind,storage_path,media_type,sha256,version,status) VALUES($1,$2,'代码差异','code_patch',$3,'text/plain; charset=utf-8',$4,1,'review')")
        .bind(artifact).bind(project_id).bind(stored.storage_path).bind(stored.sha256).execute(&mut *tx).await?;
    sqlx::query("UPDATE maitu_code_attempts SET candidate_commit=$2,patch_artifact_id=$3,check_results=$4 WHERE attempt_id=$1")
        .bind(attempt).bind(&commit).bind(artifact).bind(Json(checks)).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((commit, artifact))
}

pub async fn export(state: &AppState, project_id: Uuid) -> AppResult<Vec<u8>> {
    let record = project(&state.pool, project_id)
        .await?
        .ok_or_else(|| AppError::not_found("项目尚未导入代码"))?;
    git_bytes(
        &repository(&state.config, project_id),
        &["archive", "--format=tar", &record.accepted_commit],
    )
    .await
}

pub async fn working_diff(state: &AppState, task: Uuid, attempt: Uuid) -> AppResult<String> {
    let record:CodeAttempt=sqlx::query_as("SELECT c.* FROM maitu_code_attempts c JOIN maitu_attempts a ON a.id=c.attempt_id WHERE c.attempt_id=$1 AND a.task_id=$2")
        .bind(attempt).bind(task).fetch_optional(&state.pool).await?.ok_or_else(|| AppError::not_found("本次编码工作区不存在"))?;
    let key = Uuid::parse_str(&record.workspace_key)
        .map_err(|_| AppError::internal("工作区标识无法读取"))?;
    let root = workspace(&state.config, key);
    let mut diff = git(&root, &["diff", "--no-ext-diff", &record.base_commit, "--"]).await?;
    let untracked = git(&root, &["ls-files", "--others"]).await?;
    for path in untracked.lines().filter(|path| validate_path(path).is_ok()) {
        let content = read_file(&root, path).await?;
        diff.push_str(&format!("\n新增文件：{path}\n{content}\n"));
    }
    Ok(diff)
}

pub async fn adopt(
    state: &AppState,
    task_id: Uuid,
    attempt: Uuid,
) -> AppResult<super::workflows::TaskRecord> {
    let current = super::workflows::task(&state.pool, task_id).await?;
    if current.task_kind != "code" {
        return Err(AppError::bad_request(
            "code_task_required",
            "此节点不是编码任务",
        ));
    }
    let produced: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM maitu_attempts WHERE id=$1 AND task_id=$2 AND status='produced' AND artifact_id IS NOT NULL)")
        .bind(attempt).bind(task_id).fetch_one(&state.pool).await?;
    if !produced {
        return Err(AppError::conflict(
            "code_output_unavailable",
            "只能采用已完成并保存的编码成果",
        ));
    }
    let candidate: CodeAttempt =
        sqlx::query_as("SELECT * FROM maitu_code_attempts WHERE attempt_id=$1 AND project_id=$2")
            .bind(attempt)
            .bind(current.project_id)
            .fetch_one(&state.pool)
            .await?;
    if candidate.adopted_at.is_some() {
        return Ok(current);
    }
    if candidate.check_results.is_empty()
        || !candidate.check_results.iter().all(CheckResult::succeeded)
    {
        return Err(AppError::conflict(
            "code_checks_required",
            "编码成果需要通过实际检查后再采用",
        ));
    }
    let commit = candidate
        .candidate_commit
        .as_deref()
        .ok_or_else(|| AppError::conflict("code_output_unavailable", "编码版本尚未保存"))?;
    // An advisory lock serializes adoption while releasing automatically on process
    // loss. No database transaction is held while running the project checks.
    let mut connection = state.pool.acquire().await?.detach();
    let lock_key = current.project_id.as_u128() as i64;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(lock_key)
        .fetch_one(&mut connection)
        .await?;
    if !locked {
        return Err(AppError::conflict(
            "code_adoption_busy",
            "此项目正在采用另一份编码成果，请稍后查看结果",
        ));
    }
    let record = project(&state.pool, current.project_id)
        .await?
        .ok_or_else(|| AppError::not_found("代码项目不存在"))?;
    let key = Uuid::new_v4();
    let root = workspace(&state.config, key);
    let op = begin_operation(
        &state.pool,
        attempt,
        "integration",
        "合并并检查后采用",
        json!({"workspaceKey":key,"baseCommit":record.accepted_commit,"candidateCommit":commit}),
    )
    .await?;
    let result: AppResult<super::workflows::TaskRecord>=async {
        let repo=repository(&state.config,current.project_id);
        git(&repo,&["worktree","add","--detach",root.to_str().ok_or_else(|| AppError::internal("工作区路径不正确"))?,&record.accepted_commit]).await?;
        if git(&root,&["merge","--no-edit",commit]).await.is_err() {
            let conflicts=git(&root,&["diff","--name-only","--diff-filter=U"]).await.unwrap_or_default();
            return Err(AppError::conflict("code_merge_conflict",format!("采用时遇到代码冲突；工作区和成果已保留。冲突文件：{}",conflicts.trim())));
        }
        let mut checks=Vec::new();
        for command in &record.checks.0 { checks.push(run_check(state,attempt,key,command).await?); }
        if !checks.iter().all(CheckResult::succeeded) { return Err(AppError::conflict("code_integration_failed","合并后的实际检查未通过，成果和检查记录已保留")); }
        let integrated=git(&root,&["rev-parse","HEAD"]).await?.trim().to_owned();
        git(&repo,&["update-ref",&format!("refs/maitu/adoptions/{key}"),&integrated]).await?;
        let mut tx=state.pool.begin().await?;
        let changed=sqlx::query("UPDATE maitu_code_projects SET accepted_commit=$3,updated_at=now() WHERE project_id=$1 AND accepted_commit=$2")
            .bind(current.project_id).bind(&record.accepted_commit).bind(&integrated).execute(&mut *tx).await?;
        if changed.rows_affected()!=1 { return Err(AppError::conflict("code_base_changed","项目版本已变化，请核对后重新采用")); }
        sqlx::query("UPDATE maitu_code_attempts SET adopted_commit=$2,adopted_at=now() WHERE attempt_id=$1")
            .bind(attempt).bind(&integrated).execute(&mut *tx).await?;
        sqlx::query("UPDATE artifacts SET status='approved',approved_at=COALESCE(approved_at,now()) WHERE id=$1 OR id=(SELECT artifact_id FROM maitu_attempts WHERE id=$2)")
            .bind(candidate.patch_artifact_id).bind(attempt).execute(&mut *tx).await?;
        let task=sqlx::query_as("UPDATE maitu_tasks SET accepted_attempt_id=$2,updated_at=now() WHERE id=$1 RETURNING *")
            .bind(task_id).bind(attempt).fetch_one(&mut *tx).await?;
        sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'accepted','编码成果已合并且实际检查通过，成为当前项目版本')")
            .bind(attempt).execute(&mut *tx).await?;
        sqlx::query("UPDATE maitu_execution_operations SET status='succeeded',output=$2,completed_at=now() WHERE id=$1")
            .bind(op).bind(Json(json!({"adoptedCommit":integrated,"checks":checks}))).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(task)
    }.await;
    if let Err(error) = &result {
        end_operation(
            &state.pool,
            op,
            false,
            json!({"error":error.public_message(),"workspaceKey":key,"resultUncertain":false}),
        )
        .await?;
    }
    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(lock_key)
        .execute(&mut connection)
        .await?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_paths_cannot_reach_keys_or_git_control_files() {
        for path in [
            "../outside",
            "/etc/passwd",
            ".git/config",
            "src/../secret",
            ".env",
            "a/.ssh/id_rsa",
            "target/output",
            ".maitu-write-private",
        ] {
            assert!(validate_path(path).is_err(), "{path}");
        }
        for path in [
            "assets/maitu.js",
            "docs/说明.md",
            ".gitignore",
            ".env.example",
        ] {
            assert!(validate_path(path).is_ok(), "{path}");
        }
    }
}
