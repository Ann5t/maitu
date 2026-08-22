use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json};
use uuid::Uuid;

use crate::{
    application::plugins,
    config::Config,
    error::{AppError, AppResult},
    goal_domain::{CommandReceiptIdentity, canonical_json_sha256},
    scheduler::{
        AcknowledgeToolCleanupRequest, ActionLeaseCredentials, ActionRunStatus,
        ActivateToolLeaseRequest, CancelActionRunRequest, ClaimActionRunRequest,
        CompleteActionRunRequest, CreateToolLeaseRequest, EnqueueActionRunRequest,
        FailActionRunRequest, FinishToolLeaseRequest, HeartbeatActionRunRequest,
        LeaseLossDisposition, MarkNotificationReadRequest, ReconcileActionRunsRequest,
        RegisterWorkerRequest, RequestToolLeaseStopRequest, ResumeActionRunRequest, RetrySafety,
        lease_loss_disposition,
    },
    tooling::{EnvironmentManifest, ResourcePolicy, validate_tool_input_schema},
    workspace::WorkspaceCapabilityPolicy,
};
use fudian::runner_protocol::{RunnerCapabilities, RunnerResourceLimits};

type DbTransaction<'a> = Transaction<'a, Postgres>;

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRunRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub client_request_id: Uuid,
    pub request_hash: String,
    pub kind: String,
    pub capability: String,
    pub subject_kind: String,
    pub subject_id: Option<Uuid>,
    pub payload: Json<Value>,
    pub retry_safety: String,
    pub status: String,
    pub attempt_count: i32,
    pub max_attempts: i32,
    pub fencing_counter: i64,
    pub available_at: DateTime<Utc>,
    pub deadline_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub result: Option<Json<Value>>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow)]
struct WorkerRecord {
    id: Uuid,
    client_request_id: Uuid,
    display_name: String,
    token_digest: String,
    capabilities: Json<Vec<String>>,
    status: String,
    registered_at: DateTime<Utc>,
    last_seen_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRegistrationResponse {
    pub replayed: bool,
    pub worker_id: Uuid,
    pub display_name: String,
    pub capabilities: Vec<String>,
    pub status: String,
    pub registered_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
struct ActionLeaseRecord {
    id: Uuid,
    action_run_id: Uuid,
    worker_id: Uuid,
    attempt_number: i32,
    fencing_token: i64,
    renewal_token_digest: String,
    status: String,
    acquired_at: DateTime<Utc>,
    last_heartbeat_at: DateTime<Utc>,
    soft_expires_at: DateTime<Utc>,
    hard_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionLeaseView {
    pub id: Uuid,
    pub action_run_id: Uuid,
    pub worker_id: Uuid,
    pub attempt_number: i32,
    pub fencing_token: i64,
    pub status: String,
    pub acquired_at: DateTime<Utc>,
    pub last_heartbeat_at: DateTime<Utc>,
    pub soft_expires_at: DateTime<Utc>,
    pub hard_expires_at: DateTime<Utc>,
}

impl From<&ActionLeaseRecord> for ActionLeaseView {
    fn from(record: &ActionLeaseRecord) -> Self {
        Self {
            id: record.id,
            action_run_id: record.action_run_id,
            worker_id: record.worker_id,
            attempt_number: record.attempt_number,
            fencing_token: record.fencing_token,
            status: record.status.clone(),
            acquired_at: record.acquired_at,
            last_heartbeat_at: record.last_heartbeat_at,
            soft_expires_at: record.soft_expires_at,
            hard_expires_at: record.hard_expires_at,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimActionRunResponse {
    pub replayed: bool,
    pub action: Option<ActionRunRecord>,
    pub lease: Option<ActionLeaseView>,
    pub retry_after_seconds: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionHeartbeatResponse {
    pub action_run_id: Uuid,
    pub status: String,
    pub cancellation_requested: bool,
    pub soft_expires_at: DateTime<Utc>,
    pub hard_expires_at: DateTime<Utc>,
    pub tool_lease_status: Option<String>,
    pub endpoint_refs: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileResponse {
    pub expired_leases: usize,
    pub requeued_actions: usize,
    pub waiting_actions: usize,
    pub failed_actions: usize,
    pub cancelled_actions: usize,
    pub deadline_failures: usize,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub action_run_id: Option<Uuid>,
    pub attention_item_id: Option<Uuid>,
    pub dedupe_key: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub summary: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLeaseRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub plugin_id: String,
    pub plugin_version: String,
    pub plugin_digest: String,
    pub tool_name: String,
    pub environment_fingerprint: String,
    pub base_workspace_snapshot: String,
    pub status: String,
    #[serde(skip_serializing)]
    pub renewal_token_digest: String,
    pub resource_policy: Json<Value>,
    pub endpoint_refs: Json<Vec<String>>,
    pub retained_outputs: Json<Vec<String>>,
    pub created_at: DateTime<Utc>,
    pub last_heartbeat_at: Option<DateTime<Utc>>,
    pub soft_expires_at: DateTime<Utc>,
    pub hard_expires_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub client_request_id: Option<Uuid>,
    pub request_hash: Option<String>,
    pub plugin_installation_id: Option<Uuid>,
    pub input: Json<Value>,
    pub runtime_image_digest: Option<String>,
    pub runtime_entry_digest: Option<String>,
    pub runner_digest: Option<String>,
    pub cleanup_status: String,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateToolLeaseResponse {
    pub replayed: bool,
    pub lease: ToolLeaseRecord,
    pub action: ActionRunRecord,
    pub renewal_token: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishToolLeaseResponse {
    pub action: ActionRunRecord,
    pub lease: ToolLeaseRecord,
}

#[derive(Clone, Debug, FromRow)]
struct ToolLeaseContext {
    goal_branch_id: Uuid,
    session_status: String,
    head_session_id: Uuid,
    environment_fingerprint: String,
    environment_manifest: Json<Value>,
    workspace_snapshot: Option<String>,
    workspace_status: String,
    workspace_dirty: bool,
    workspace_policy: Json<Value>,
}

pub async fn register_worker(
    pool: &PgPool,
    config: &Config,
    provided_bootstrap_token: Option<&str>,
    request: RegisterWorkerRequest,
) -> AppResult<WorkerRegistrationResponse> {
    let request = request.normalize()?;
    let expected = config
        .worker_bootstrap_token_digest
        .as_ref()
        .ok_or_else(|| {
            AppError::forbidden(
                "worker_registration_disabled",
                "服务器未配置 Worker bootstrap secret，control plane 默认关闭",
            )
        })?;
    let supplied = provided_bootstrap_token.ok_or_else(|| {
        AppError::forbidden(
            "worker_bootstrap_required",
            "注册 Worker 必须提供 bootstrap secret",
        )
    })?;
    if &secret_digest(supplied) != expected {
        return Err(AppError::forbidden(
            "invalid_worker_bootstrap",
            "Worker bootstrap secret 不匹配",
        ));
    }
    let token_digest = secret_digest(&request.worker_token);
    let mut transaction = pool.begin().await?;
    if let Some(existing) = sqlx::query_as::<_, WorkerRecord>(
        "SELECT * FROM scheduler_workers WHERE id = $1 OR client_request_id = $2 FOR UPDATE",
    )
    .bind(request.worker_id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.id != request.worker_id
            || existing.client_request_id != request.client_request_id
            || existing.display_name != request.display_name
            || existing.capabilities.0 != request.capabilities
            || existing.token_digest != token_digest
        {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "Worker ID 或注册请求已绑定不同身份",
            ));
        }
        transaction.commit().await?;
        return Ok(worker_response(existing, true));
    }
    let worker = sqlx::query_as::<_, WorkerRecord>(
        "INSERT INTO scheduler_workers \
         (id, client_request_id, display_name, token_digest, capabilities) \
         VALUES ($1, $2, $3, $4, $5) RETURNING *",
    )
    .bind(request.worker_id)
    .bind(request.client_request_id)
    .bind(request.display_name)
    .bind(token_digest)
    .bind(Json(request.capabilities))
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(worker_response(worker, false))
}

pub async fn enqueue_action_run(
    pool: &PgPool,
    project_id: Uuid,
    session_id: Uuid,
    request: EnqueueActionRunRequest,
) -> AppResult<(bool, ActionRunRecord)> {
    let now = Utc::now();
    // Hash the client-authored request before server-side defaults (notably
    // `available_at = now`) are materialized. Otherwise an exact replay a few
    // milliseconds later can never match its original idempotency identity.
    let request_hash = request.request_hash()?;
    let request = request.normalize(now)?;
    if !matches!(request.kind.as_str(), "agent_step" | "maintenance") {
        return Err(AppError::bad_request(
            "specialized_action_required",
            "runner、ToolLease、审核与整合行动必须通过对应的受约束入口创建",
        ));
    }
    insert_action_run(
        pool,
        project_id,
        session_id,
        request,
        &request_hash,
        "none",
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_specialized_action_run(
    transaction: &mut DbTransaction<'_>,
    project_id: Uuid,
    goal_branch_id: Uuid,
    session_id: Uuid,
    client_request_id: Uuid,
    request_hash: &str,
    kind: &str,
    capability: &str,
    subject_kind: &str,
    subject_id: Uuid,
    payload: Value,
    retry_safety: RetrySafety,
    max_attempts: i32,
    deadline_at: Option<DateTime<Utc>>,
) -> AppResult<ActionRunRecord> {
    let action = sqlx::query_as::<_, ActionRunRecord>(
        "INSERT INTO goal_action_runs \
         (id, project_id, goal_branch_id, session_id, client_request_id, request_hash, \
          kind, capability, subject_kind, subject_id, payload, retry_safety, max_attempts, \
          available_at, deadline_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, now(), $14) \
         RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(session_id)
    .bind(client_request_id)
    .bind(request_hash)
    .bind(kind)
    .bind(capability)
    .bind(subject_kind)
    .bind(subject_id)
    .bind(Json(payload))
    .bind(retry_safety.as_str())
    .bind(max_attempts)
    .bind(deadline_at)
    .fetch_one(&mut **transaction)
    .await?;
    insert_action_event(
        transaction,
        &action,
        None,
        "action.queued",
        "agent",
        None,
        json!({ "specialized": true }),
    )
    .await?;
    Ok(action)
}

async fn insert_action_run(
    pool: &PgPool,
    project_id: Uuid,
    session_id: Uuid,
    request: EnqueueActionRunRequest,
    request_hash: &str,
    subject_kind: &str,
    subject_id: Option<Uuid>,
) -> AppResult<(bool, ActionRunRecord)> {
    let mut transaction = pool.begin().await?;
    let (goal_branch_id, session_status, head_session_id): (Uuid, String, Uuid) = sqlx::query_as(
        "SELECT s.goal_branch_id, s.status, b.head_session_id \
             FROM goal_sessions s JOIN goal_branches b ON b.id = s.goal_branch_id \
             WHERE s.id = $1 AND s.project_id = $2 FOR UPDATE OF s, b",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| AppError::not_found("Agent Session 不存在"))?;
    if session_status != "running" || head_session_id != session_id {
        return Err(AppError::conflict(
            "session_not_writable",
            "只有目标枝干当前 running Session 可以创建 ActionRun",
        ));
    }
    if let Some(existing) = sqlx::query_as::<_, ActionRunRecord>(
        "SELECT * FROM goal_action_runs WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.request_hash != request_hash {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "同一 clientRequestId 已用于不同 ActionRun 输入",
            ));
        }
        transaction.commit().await?;
        return Ok((true, existing));
    }
    let action = sqlx::query_as::<_, ActionRunRecord>(
        "INSERT INTO goal_action_runs \
         (id, project_id, goal_branch_id, session_id, client_request_id, request_hash, \
          kind, capability, subject_kind, subject_id, payload, retry_safety, max_attempts, \
          available_at, deadline_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) \
         RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(session_id)
    .bind(request.client_request_id)
    .bind(request_hash)
    .bind(&request.kind)
    .bind(&request.capability)
    .bind(subject_kind)
    .bind(subject_id)
    .bind(Json(request.payload))
    .bind(request.retry_safety.as_str())
    .bind(request.max_attempts)
    .bind(
        request
            .available_at
            .expect("normalize supplies available_at"),
    )
    .bind(request.deadline_at)
    .fetch_one(&mut *transaction)
    .await?;
    insert_action_event(
        &mut transaction,
        &action,
        None,
        "action.queued",
        "agent",
        None,
        json!({}),
    )
    .await?;
    transaction.commit().await?;
    Ok((false, action))
}

pub async fn list_action_runs(
    pool: &PgPool,
    project_id: Uuid,
    session_id: Uuid,
) -> AppResult<Vec<ActionRunRecord>> {
    ensure_session_scope(pool, project_id, session_id).await?;
    Ok(sqlx::query_as::<_, ActionRunRecord>(
        "SELECT * FROM goal_action_runs WHERE project_id = $1 AND session_id = $2 \
         ORDER BY created_at DESC, id DESC",
    )
    .bind(project_id)
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_action_run(
    pool: &PgPool,
    project_id: Uuid,
    action_run_id: Uuid,
) -> AppResult<ActionRunRecord> {
    sqlx::query_as::<_, ActionRunRecord>(
        "SELECT * FROM goal_action_runs WHERE project_id = $1 AND id = $2",
    )
    .bind(project_id)
    .bind(action_run_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("ActionRun 不存在"))
}

pub async fn list_notifications(
    pool: &PgPool,
    project_id: Uuid,
) -> AppResult<Vec<NotificationRecord>> {
    let project_exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM projects WHERE id = $1)")
            .bind(project_id)
            .fetch_one(pool)
            .await?;
    if !project_exists {
        return Err(AppError::not_found("项目不存在"));
    }
    Ok(sqlx::query_as::<_, NotificationRecord>(
        "SELECT * FROM goal_notifications WHERE project_id = $1 \
         ORDER BY CASE status WHEN 'unread' THEN 0 WHEN 'read' THEN 1 ELSE 2 END, \
                  created_at DESC, id DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?)
}

pub async fn mark_notification_read(
    pool: &PgPool,
    project_id: Uuid,
    notification_id: Uuid,
    request: MarkNotificationReadRequest,
) -> AppResult<(bool, NotificationRecord)> {
    let identity =
        CommandReceiptIdentity::from_input("notification.read", &(notification_id, &request))?;
    let mut transaction = pool.begin().await?;
    let receipt: Option<(String, String, Json<Value>)> = sqlx::query_as(
        "SELECT command_kind, input_hash, result FROM goal_command_receipts \
         WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((command_kind, input_hash, result)) = receipt {
        CommandReceiptIdentity {
            command_kind,
            input_hash,
        }
        .ensure_replay_matches(&identity)?;
        let saved_id = result
            .0
            .get("notificationId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| {
                AppError::conflict("notification_receipt_corrupt", "通知命令收据缺少 ID")
            })?;
        if saved_id != notification_id {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "同一 clientRequestId 已用于另一条通知",
            ));
        }
        let notification =
            load_notification(&mut transaction, project_id, notification_id, false).await?;
        transaction.commit().await?;
        return Ok((true, notification));
    }
    let mut notification =
        load_notification(&mut transaction, project_id, notification_id, true).await?;
    if notification.status == "unread" {
        notification = sqlx::query_as::<_, NotificationRecord>(
            "UPDATE goal_notifications SET status = 'read', read_at = now() \
             WHERE id = $1 RETURNING *",
        )
        .bind(notification.id)
        .fetch_one(&mut *transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO goal_command_receipts \
         (project_id, client_request_id, command_kind, input_hash, result) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .bind(&identity.command_kind)
    .bind(&identity.input_hash)
    .bind(Json(json!({
        "notificationId": notification.id,
        "status": notification.status,
    })))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok((false, notification))
}

pub async fn create_tool_lease(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    session_id: Uuid,
    request: CreateToolLeaseRequest,
) -> AppResult<CreateToolLeaseResponse> {
    let request = request.normalize()?;
    let request_hash = request.request_hash()?;
    let installation_id: Uuid = sqlx::query_scalar(
        "SELECT i.id FROM plugin_installations i \
         JOIN plugin_packages p ON p.id = i.plugin_package_id \
         WHERE p.plugin_id = $1 AND p.version = $2 AND p.content_digest = $3",
    )
    .bind(&request.plugin.plugin_id)
    .bind(&request.plugin.version)
    .bind(&request.plugin.content_digest)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::forbidden(
            "plugin_not_installed",
            "持续工具要求准确且受信任的签名 OCI 安装证明",
        )
    })?;
    let (manifest, installation) = plugins::load_installation_by_id(
        pool,
        installation_id,
        &config.runner_runtime_digest,
        true,
    )
    .await?;
    if manifest.resolved_ref() != request.plugin || manifest.runtime.kind != "oci" {
        return Err(AppError::conflict(
            "plugin_installation_conflict",
            "ToolLease 插件引用与签名安装证明不一致",
        ));
    }
    let descriptor = manifest
        .tools
        .iter()
        .find(|tool| tool.name == request.tool_name)
        .ok_or_else(|| AppError::bad_request("tool_not_found", "插件没有声明这个工具"))?;
    let persistent = descriptor.persistent.clone().ok_or_else(|| {
        AppError::bad_request(
            "tool_is_not_persistent",
            "普通 ToolCall 不能借 ToolLease 留下后台进程",
        )
    })?;
    validate_tool_input_schema(&descriptor.input_schema, &request.input)?;
    if request.duration_seconds > persistent.max_duration_seconds {
        return Err(AppError::forbidden(
            "tool_lease_duration_denied",
            "请求时长超过签名插件声明的持续工具上限",
        ));
    }
    if manifest.permissions.network != "denied" || manifest.permissions.external_writes {
        return Err(AppError::forbidden(
            "persistent_adapter_unavailable",
            "BP-06 持续工具只支持隔离控制面；联网或外部写适配器尚未授权",
        ));
    }
    let resource_policy = request
        .resource_policy
        .clone()
        .unwrap_or_else(|| manifest.resource_hints.clone());
    ensure_resource_within(&resource_policy, &manifest.resource_hints, "插件 Manifest")?;
    let mut transaction = pool.begin().await?;
    if let Some(existing) = sqlx::query_as::<_, ToolLeaseRecord>(
        "SELECT * FROM tool_leases WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.request_hash.as_deref() != Some(request_hash.as_str()) {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "同一 clientRequestId 已用于不同 ToolLease 输入",
            ));
        }
        let action = sqlx::query_as::<_, ActionRunRecord>(
            "SELECT * FROM goal_action_runs \
             WHERE subject_kind = 'tool_lease' AND subject_id = $1",
        )
        .bind(existing.id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(CreateToolLeaseResponse {
            replayed: true,
            lease: existing,
            action,
            renewal_token: None,
        });
    }
    let context = sqlx::query_as::<_, ToolLeaseContext>(
        "SELECT s.goal_branch_id, s.status AS session_status, b.head_session_id, \
                seb.environment_fingerprint, em.manifest AS environment_manifest, \
                w.workspace_snapshot, w.status AS workspace_status, w.dirty AS workspace_dirty, \
                p.policy AS workspace_policy \
         FROM goal_sessions s \
         JOIN goal_branches b ON b.id = s.goal_branch_id \
         JOIN session_environment_bindings seb ON seb.session_id = s.id \
         JOIN environment_manifests em ON em.id = seb.environment_manifest_id \
         JOIN goal_workspaces w ON w.goal_branch_id = s.goal_branch_id \
         JOIN goal_workspace_policies p ON p.goal_branch_id = s.goal_branch_id \
         WHERE s.id = $1 AND s.project_id = $2 FOR UPDATE OF s, b, w",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| {
        AppError::conflict(
            "tool_lease_context_missing",
            "ToolLease 需要当前 Session、EnvironmentManifest 和已就绪 worktree",
        )
    })?;
    if context.session_status != "running" || context.head_session_id != session_id {
        return Err(AppError::conflict(
            "session_not_writable",
            "只有目标枝干当前 running Session 可以创建 ToolLease",
        ));
    }
    let workspace_snapshot = context.workspace_snapshot.ok_or_else(|| {
        AppError::conflict("workspace_not_ready", "ToolLease worktree 尚无安全快照")
    })?;
    if context.workspace_status != "ready"
        || context.workspace_dirty
        || workspace_snapshot != request.base_workspace_snapshot
    {
        return Err(AppError::conflict(
            "workspace_baseline_mismatch",
            "ToolLease 必须固定当前干净 worktree 的准确快照",
        ));
    }
    let environment =
        serde_json::from_value::<EnvironmentManifest>(context.environment_manifest.0)?
            .normalize()?;
    if environment.fingerprint()? != context.environment_fingerprint
        || !environment
            .plugins
            .iter()
            .any(|plugin| plugin == &request.plugin)
        || environment.network_policy != "denied"
    {
        return Err(AppError::conflict(
            "environment_plugin_mismatch",
            "Session 环境未固定该准确插件或不是断网环境",
        ));
    }
    ensure_resource_within(
        &resource_policy,
        &environment.resource_policy,
        "EnvironmentManifest",
    )?;
    let policy = serde_json::from_value::<WorkspaceCapabilityPolicy>(context.workspace_policy.0)
        .map_err(|_| AppError::conflict("workspace_policy_corrupt", "WorkspacePolicy 无法解析"))?
        .normalize()?;
    policy.authorize(
        &RunnerCapabilities::default(),
        &[],
        &runner_limits(&resource_policy),
    )?;
    let now = Utc::now();
    let soft_expires_at = now + Duration::seconds(i64::from(request.soft_ttl_seconds));
    let hard_expires_at = now + Duration::seconds(i64::from(request.duration_seconds));
    let lease_id = Uuid::new_v4();
    let renewal_token = format!(
        "tool_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let lease = sqlx::query_as::<_, ToolLeaseRecord>(
        "INSERT INTO tool_leases \
         (id, project_id, goal_branch_id, session_id, plugin_id, plugin_version, plugin_digest, \
          tool_name, environment_fingerprint, base_workspace_snapshot, status, \
          renewal_token_digest, resource_policy, soft_expires_at, hard_expires_at, \
          client_request_id, request_hash, plugin_installation_id, input, runtime_image_digest, \
          runtime_entry_digest, runner_digest) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'requested', $11, $12, $13, \
                 $14, $15, $16, $17, $18, $19, $20, $21) RETURNING *",
    )
    .bind(lease_id)
    .bind(project_id)
    .bind(context.goal_branch_id)
    .bind(session_id)
    .bind(&request.plugin.plugin_id)
    .bind(&request.plugin.version)
    .bind(&request.plugin.content_digest)
    .bind(&request.tool_name)
    .bind(&context.environment_fingerprint)
    .bind(&request.base_workspace_snapshot)
    .bind(secret_digest(&renewal_token))
    .bind(Json(serde_json::to_value(&resource_policy)?))
    .bind(soft_expires_at)
    .bind(hard_expires_at)
    .bind(request.client_request_id)
    .bind(&request_hash)
    .bind(installation.id)
    .bind(Json(request.input.clone()))
    .bind(&installation.runtime_image_digest)
    .bind(&installation.runtime_entry_digest)
    .bind(&installation.runner_digest)
    .fetch_one(&mut *transaction)
    .await?;
    let payload = json!({
        "toolLeaseId": lease_id,
        "plugin": request.plugin,
        "toolName": request.tool_name,
        "input": request.input,
        "persistent": persistent,
        "runtime": {
            "kind": "oci",
            "imageDigest": installation.runtime_image_digest,
            "entrypoint": manifest.runtime.entrypoint,
            "entryDigest": installation.runtime_entry_digest,
            "runnerDigest": installation.runner_digest,
        },
        "environmentFingerprint": context.environment_fingerprint,
        "baseWorkspaceSnapshot": workspace_snapshot,
        "resourcePolicy": resource_policy,
        "softExpiresAt": soft_expires_at,
        "hardExpiresAt": hard_expires_at,
    });
    let retry_safety = if persistent.startup_retry_safety == "safe" {
        RetrySafety::Safe
    } else {
        RetrySafety::Unsafe
    };
    let max_attempts = if retry_safety == RetrySafety::Safe {
        3
    } else {
        1
    };
    let action = insert_specialized_action_run(
        &mut transaction,
        project_id,
        context.goal_branch_id,
        session_id,
        request.client_request_id,
        &request_hash,
        "tool_lease",
        "tool.lease",
        "tool_lease",
        lease_id,
        payload,
        retry_safety,
        max_attempts,
        Some(hard_expires_at),
    )
    .await?;
    transaction.commit().await?;
    Ok(CreateToolLeaseResponse {
        replayed: false,
        lease,
        action,
        renewal_token: Some(renewal_token),
    })
}

pub async fn get_tool_lease(
    pool: &PgPool,
    project_id: Uuid,
    tool_lease_id: Uuid,
) -> AppResult<ToolLeaseRecord> {
    sqlx::query_as::<_, ToolLeaseRecord>(
        "SELECT * FROM tool_leases WHERE project_id = $1 AND id = $2",
    )
    .bind(project_id)
    .bind(tool_lease_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("ToolLease 不存在"))
}

pub async fn activate_tool_lease(
    pool: &PgPool,
    action_run_id: Uuid,
    request: ActivateToolLeaseRequest,
) -> AppResult<ToolLeaseRecord> {
    let request = request.normalize()?;
    let mut transaction = pool.begin().await?;
    let (action_lease, action) =
        verified_active_lease(&mut transaction, action_run_id, &request.credentials, true).await?;
    if action.subject_kind != "tool_lease" {
        return Err(AppError::conflict(
            "action_is_not_tool_lease",
            "该 ActionRun 不是持续工具启动行动",
        ));
    }
    let tool_lease_id = action
        .subject_id
        .ok_or_else(|| AppError::conflict("tool_lease_missing", "ActionRun 缺少 ToolLease ID"))?;
    let mut tool_lease = load_tool_lease_for_update(&mut transaction, tool_lease_id).await?;
    if tool_lease.status == "active" {
        if tool_lease.endpoint_refs.0 != request.endpoint_refs {
            return Err(AppError::conflict(
                "tool_lease_activation_conflict",
                "ToolLease 已用不同 endpoint 激活",
            ));
        }
        transaction.commit().await?;
        return Ok(tool_lease);
    }
    if tool_lease.status != "requested"
        || tool_lease.soft_expires_at <= Utc::now()
        || tool_lease.hard_expires_at <= Utc::now()
    {
        return Err(AppError::conflict(
            "tool_lease_not_activatable",
            "ToolLease 已到期或不再处于 requested 状态",
        ));
    }
    let now = Utc::now();
    let next_soft = action_lease
        .soft_expires_at
        .min(tool_lease.hard_expires_at)
        .max(tool_lease.soft_expires_at);
    tool_lease = sqlx::query_as::<_, ToolLeaseRecord>(
        "UPDATE tool_leases SET status = 'active', endpoint_refs = $1, \
         last_heartbeat_at = $2, soft_expires_at = $3, cleanup_status = 'pending' \
         WHERE id = $4 RETURNING *",
    )
    .bind(Json(request.endpoint_refs.clone()))
    .bind(now)
    .bind(next_soft)
    .bind(tool_lease.id)
    .fetch_one(&mut *transaction)
    .await?;
    insert_action_event(
        &mut transaction,
        &action,
        Some(action_lease.id),
        "tool_lease.activated",
        "worker",
        Some(action_lease.worker_id.to_string()),
        json!({ "endpointRefs": request.endpoint_refs }),
    )
    .await?;
    touch_worker(&mut transaction, action_lease.worker_id).await?;
    transaction.commit().await?;
    Ok(tool_lease)
}

pub async fn finish_tool_lease(
    pool: &PgPool,
    action_run_id: Uuid,
    request: FinishToolLeaseRequest,
) -> AppResult<FinishToolLeaseResponse> {
    let request = request.normalize()?;
    let mut transaction = pool.begin().await?;
    let (action_lease, mut action) =
        verified_active_lease(&mut transaction, action_run_id, &request.credentials, true).await?;
    if action.subject_kind != "tool_lease" {
        return Err(AppError::conflict(
            "action_is_not_tool_lease",
            "该 ActionRun 不是持续工具行动",
        ));
    }
    let tool_lease_id = action
        .subject_id
        .ok_or_else(|| AppError::conflict("tool_lease_missing", "ActionRun 缺少 ToolLease ID"))?;
    let tool_lease = load_tool_lease_for_update(&mut transaction, tool_lease_id).await?;
    if !matches!(tool_lease.status.as_str(), "requested" | "active") {
        return Err(AppError::conflict(
            "tool_lease_terminal",
            "ToolLease 已进入终态，不能再次完成",
        ));
    }
    let release_requested = action.last_error_code.as_deref() == Some("tool_release_requested");
    let cancel_requested = action.status == "cancellation_requested" && !release_requested;
    let (tool_status, action_status, action_lease_status, event_type) = if cancel_requested {
        (
            "cancelled",
            "cancelled",
            "cancelled",
            "tool_lease.cancelled",
        )
    } else {
        ("released", "succeeded", "succeeded", "tool_lease.released")
    };
    let now = Utc::now();
    let tool_lease = sqlx::query_as::<_, ToolLeaseRecord>(
        "UPDATE tool_leases SET status = $1, retained_outputs = $2, \
         cleanup_status = 'succeeded', completed_at = $3 WHERE id = $4 RETURNING *",
    )
    .bind(tool_status)
    .bind(Json(request.retained_outputs.clone()))
    .bind(now)
    .bind(tool_lease.id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE action_run_leases SET status = $1, outcome = $2, completed_at = $3 \
         WHERE id = $4",
    )
    .bind(action_lease_status)
    .bind(Json(request.result.clone()))
    .bind(now)
    .bind(action_lease.id)
    .execute(&mut *transaction)
    .await?;
    action = sqlx::query_as::<_, ActionRunRecord>(
        "UPDATE goal_action_runs SET status = $1, result = $2, updated_at = $3, \
         completed_at = $3 WHERE id = $4 RETURNING *",
    )
    .bind(action_status)
    .bind(Json(request.result))
    .bind(now)
    .bind(action.id)
    .fetch_one(&mut *transaction)
    .await?;
    insert_action_event(
        &mut transaction,
        &action,
        Some(action_lease.id),
        event_type,
        "worker",
        Some(action_lease.worker_id.to_string()),
        json!({
            "toolLeaseId": tool_lease.id,
            "retainedOutputs": tool_lease.retained_outputs,
            "cleanupStatus": tool_lease.cleanup_status,
        }),
    )
    .await?;
    touch_worker(&mut transaction, action_lease.worker_id).await?;
    transaction.commit().await?;
    Ok(FinishToolLeaseResponse {
        action,
        lease: tool_lease,
    })
}

pub async fn request_tool_lease_stop(
    pool: &PgPool,
    project_id: Uuid,
    tool_lease_id: Uuid,
    request: RequestToolLeaseStopRequest,
) -> AppResult<(bool, FinishToolLeaseResponse)> {
    let request = request.normalize()?;
    let identity =
        CommandReceiptIdentity::from_input("tool_lease.stop", &(tool_lease_id, &request))?;
    let mut transaction = pool.begin().await?;
    let mut tool_lease = load_tool_lease_for_update(&mut transaction, tool_lease_id).await?;
    if tool_lease.project_id != project_id {
        return Err(AppError::not_found("ToolLease 不存在"));
    }
    if tool_lease.renewal_token_digest != secret_digest(&request.renewal_token) {
        return Err(AppError::forbidden(
            "invalid_tool_lease_token",
            "ToolLease renewal token 不匹配",
        ));
    }
    let mut action = sqlx::query_as::<_, ActionRunRecord>(
        "SELECT * FROM goal_action_runs \
         WHERE subject_kind = 'tool_lease' AND subject_id = $1 FOR UPDATE",
    )
    .bind(tool_lease.id)
    .fetch_one(&mut *transaction)
    .await?;
    if let Some(replayed) = replay_action_command(
        &mut transaction,
        project_id,
        request.client_request_id,
        &identity,
        action.id,
    )
    .await?
    {
        transaction.commit().await?;
        return Ok((
            true,
            FinishToolLeaseResponse {
                action: replayed,
                lease: tool_lease,
            },
        ));
    }
    let now = Utc::now();
    if action.status == "queued" && tool_lease.status == "requested" {
        let tool_status = if request.mode == "release" {
            "released"
        } else {
            "cancelled"
        };
        tool_lease = sqlx::query_as::<_, ToolLeaseRecord>(
            "UPDATE tool_leases SET status = $1, cleanup_status = 'succeeded', \
             completed_at = $2 WHERE id = $3 RETURNING *",
        )
        .bind(tool_status)
        .bind(now)
        .bind(tool_lease.id)
        .fetch_one(&mut *transaction)
        .await?;
        action = sqlx::query_as::<_, ActionRunRecord>(
            "UPDATE goal_action_runs SET status = 'cancelled', last_error_code = $1, \
             last_error_summary = $2, updated_at = $3, completed_at = $3 \
             WHERE id = $4 RETURNING *",
        )
        .bind(format!("tool_{}_before_start", request.mode))
        .bind(&request.reason)
        .bind(now)
        .bind(action.id)
        .fetch_one(&mut *transaction)
        .await?;
    } else if action.status == "running" && tool_lease.status == "active" {
        action = sqlx::query_as::<_, ActionRunRecord>(
            "UPDATE goal_action_runs SET status = 'cancellation_requested', \
             last_error_code = $1, last_error_summary = $2, updated_at = $3 \
             WHERE id = $4 RETURNING *",
        )
        .bind(if request.mode == "release" {
            "tool_release_requested"
        } else {
            "tool_cancel_requested"
        })
        .bind(&request.reason)
        .bind(now)
        .bind(action.id)
        .fetch_one(&mut *transaction)
        .await?;
    } else if !matches!(
        action.status.as_str(),
        "cancellation_requested" | "succeeded" | "failed" | "cancelled"
    ) {
        return Err(AppError::conflict(
            "invalid_tool_lease_transition",
            "ToolLease 当前状态不能请求停止",
        ));
    }
    insert_action_event(
        &mut transaction,
        &action,
        None,
        &format!("tool_lease.{}_requested", request.mode),
        "human",
        None,
        json!({ "reason": request.reason }),
    )
    .await?;
    save_action_command_receipt(
        &mut transaction,
        project_id,
        request.client_request_id,
        &identity,
        &action,
    )
    .await?;
    transaction.commit().await?;
    Ok((
        false,
        FinishToolLeaseResponse {
            action,
            lease: tool_lease,
        },
    ))
}

pub async fn acknowledge_tool_cleanup(
    pool: &PgPool,
    tool_lease_id: Uuid,
    request: AcknowledgeToolCleanupRequest,
) -> AppResult<ToolLeaseRecord> {
    let request = request.normalize()?;
    let mut transaction = pool.begin().await?;
    let worker = authenticate_worker(
        &mut transaction,
        request.worker_id,
        &request.worker_token,
        Some("tool.cleanup"),
        true,
    )
    .await?;
    let tool_lease = load_tool_lease_for_update(&mut transaction, tool_lease_id).await?;
    if !matches!(
        tool_lease.status.as_str(),
        "expired" | "failed" | "cancelled"
    ) || !matches!(tool_lease.cleanup_status.as_str(), "pending" | "running")
    {
        return Err(AppError::conflict(
            "tool_cleanup_not_pending",
            "ToolLease 没有待确认的外部进程清理",
        ));
    }
    let updated = sqlx::query_as::<_, ToolLeaseRecord>(
        "UPDATE tool_leases SET cleanup_status = $1, last_error_summary = $2 \
         WHERE id = $3 RETURNING *",
    )
    .bind(&request.status)
    .bind(&request.summary)
    .bind(tool_lease.id)
    .fetch_one(&mut *transaction)
    .await?;
    let action = sqlx::query_as::<_, ActionRunRecord>(
        "SELECT * FROM goal_action_runs \
         WHERE subject_kind = 'tool_lease' AND subject_id = $1",
    )
    .bind(tool_lease.id)
    .fetch_one(&mut *transaction)
    .await?;
    insert_action_event(
        &mut transaction,
        &action,
        None,
        &format!("tool_lease.cleanup_{}", request.status),
        "worker",
        Some(worker.id.to_string()),
        json!({ "summary": request.summary }),
    )
    .await?;
    touch_worker(&mut transaction, worker.id).await?;
    transaction.commit().await?;
    Ok(updated)
}

pub async fn claim_action_run(
    pool: &PgPool,
    request: ClaimActionRunRequest,
) -> AppResult<ClaimActionRunResponse> {
    let request = request.normalize()?;
    let mut transaction = pool.begin().await?;
    let worker = authenticate_worker(
        &mut transaction,
        request.worker_id,
        &request.worker_token,
        None,
        true,
    )
    .await?;
    if let Some(existing) = sqlx::query_as::<_, ActionLeaseRecord>(
        "SELECT * FROM action_run_leases WHERE worker_id = $1 AND claim_request_id = $2",
    )
    .bind(worker.id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.renewal_token_digest != secret_digest(&request.lease_token) {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "claimRequestId 已绑定不同 ActionLease token",
            ));
        }
        let action = load_action(&mut transaction, existing.action_run_id, false).await?;
        transaction.commit().await?;
        return Ok(ClaimActionRunResponse {
            replayed: true,
            action: Some(action),
            lease: Some(ActionLeaseView::from(&existing)),
            retry_after_seconds: 0,
        });
    }
    let worker_busy: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM action_run_leases WHERE worker_id = $1 AND status = 'active')",
    )
    .bind(worker.id)
    .fetch_one(&mut *transaction)
    .await?;
    if worker_busy {
        return Err(AppError::conflict(
            "worker_capacity_exhausted",
            "该 Worker 已有 active ActionLease；并行请注册独立 Worker 身份",
        ));
    }
    let now = Utc::now();
    let action = sqlx::query_as::<_, ActionRunRecord>(
        "SELECT * FROM goal_action_runs \
         WHERE status = 'queued' AND available_at <= $1 \
           AND (deadline_at IS NULL OR deadline_at > $1) \
           AND capability = ANY($2) \
         ORDER BY available_at, created_at, id \
         FOR UPDATE SKIP LOCKED LIMIT 1",
    )
    .bind(now)
    .bind(&worker.capabilities.0)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(mut action) = action else {
        touch_worker(&mut transaction, worker.id).await?;
        transaction.commit().await?;
        return Ok(ClaimActionRunResponse {
            replayed: false,
            action: None,
            lease: None,
            retry_after_seconds: 2,
        });
    };
    let hard_candidate = now + Duration::seconds(i64::from(request.hard_ttl_seconds));
    let hard_expires_at = action
        .deadline_at
        .map_or(hard_candidate, |deadline| deadline.min(hard_candidate));
    let soft_expires_at =
        (now + Duration::seconds(i64::from(request.soft_ttl_seconds))).min(hard_expires_at);
    action = sqlx::query_as::<_, ActionRunRecord>(
        "UPDATE goal_action_runs SET status = 'running', \
         attempt_count = attempt_count + 1, fencing_counter = fencing_counter + 1, \
         started_at = COALESCE(started_at, $1), updated_at = $1 \
         WHERE id = $2 RETURNING *",
    )
    .bind(now)
    .bind(action.id)
    .fetch_one(&mut *transaction)
    .await?;
    let lease = sqlx::query_as::<_, ActionLeaseRecord>(
        "INSERT INTO action_run_leases \
         (id, action_run_id, worker_id, claim_request_id, attempt_number, fencing_token, \
          renewal_token_digest, soft_expires_at, hard_expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(action.id)
    .bind(worker.id)
    .bind(request.client_request_id)
    .bind(action.attempt_count)
    .bind(action.fencing_counter)
    .bind(secret_digest(&request.lease_token))
    .bind(soft_expires_at)
    .bind(hard_expires_at)
    .fetch_one(&mut *transaction)
    .await?;
    insert_action_event(
        &mut transaction,
        &action,
        Some(lease.id),
        "action.claimed",
        "worker",
        Some(worker.id.to_string()),
        json!({
            "attemptNumber": action.attempt_count,
            "fencingToken": action.fencing_counter,
            "softExpiresAt": soft_expires_at,
            "hardExpiresAt": hard_expires_at,
        }),
    )
    .await?;
    touch_worker(&mut transaction, worker.id).await?;
    transaction.commit().await?;
    Ok(ClaimActionRunResponse {
        replayed: false,
        action: Some(action),
        lease: Some(ActionLeaseView::from(&lease)),
        retry_after_seconds: 0,
    })
}

pub async fn heartbeat_action_run(
    pool: &PgPool,
    action_run_id: Uuid,
    request: HeartbeatActionRunRequest,
) -> AppResult<ActionHeartbeatResponse> {
    let request = request.normalize()?;
    let mut transaction = pool.begin().await?;
    let (mut lease, action) =
        verified_active_lease(&mut transaction, action_run_id, &request.credentials, true).await?;
    let now = Utc::now();
    let next_soft =
        (now + Duration::seconds(i64::from(request.extend_seconds))).min(lease.hard_expires_at);
    lease = sqlx::query_as::<_, ActionLeaseRecord>(
        "UPDATE action_run_leases SET last_heartbeat_at = $1, soft_expires_at = $2 \
         WHERE id = $3 RETURNING *",
    )
    .bind(now)
    .bind(next_soft)
    .bind(lease.id)
    .fetch_one(&mut *transaction)
    .await?;
    touch_worker(&mut transaction, lease.worker_id).await?;
    let (tool_lease_status, endpoint_refs) =
        heartbeat_linked_tool_lease(&mut transaction, &action, now, next_soft).await?;
    insert_action_event(
        &mut transaction,
        &action,
        Some(lease.id),
        "action.heartbeat",
        "worker",
        Some(lease.worker_id.to_string()),
        json!({ "softExpiresAt": next_soft }),
    )
    .await?;
    transaction.commit().await?;
    Ok(ActionHeartbeatResponse {
        action_run_id,
        status: action.status.clone(),
        cancellation_requested: action.status == "cancellation_requested",
        soft_expires_at: lease.soft_expires_at,
        hard_expires_at: lease.hard_expires_at,
        tool_lease_status,
        endpoint_refs,
    })
}

pub async fn complete_action_run(
    pool: &PgPool,
    action_run_id: Uuid,
    request: CompleteActionRunRequest,
) -> AppResult<ActionRunRecord> {
    let request = request.normalize()?;
    let mut transaction = pool.begin().await?;
    let (lease, mut action) =
        verified_active_lease(&mut transaction, action_run_id, &request.credentials, true).await?;
    if action.subject_kind == "tool_lease" {
        return Err(AppError::conflict(
            "tool_lease_finish_required",
            "持续工具必须先由 ToolLease 完成入口确认进程已清理",
        ));
    }
    if action.status == "cancellation_requested" {
        return Err(AppError::conflict(
            "action_cancellation_requested",
            "用户取消已先到达；Worker 必须确认取消而不能提交成功",
        ));
    }
    let now = Utc::now();
    sqlx::query(
        "UPDATE action_run_leases SET status = 'succeeded', outcome = $1, completed_at = $2 \
         WHERE id = $3",
    )
    .bind(Json(request.result.clone()))
    .bind(now)
    .bind(lease.id)
    .execute(&mut *transaction)
    .await?;
    action = sqlx::query_as::<_, ActionRunRecord>(
        "UPDATE goal_action_runs SET status = 'succeeded', result = $1, updated_at = $2, \
         completed_at = $2 WHERE id = $3 RETURNING *",
    )
    .bind(Json(request.result))
    .bind(now)
    .bind(action.id)
    .fetch_one(&mut *transaction)
    .await?;
    insert_action_event(
        &mut transaction,
        &action,
        Some(lease.id),
        "action.succeeded",
        "worker",
        Some(lease.worker_id.to_string()),
        json!({ "attemptNumber": lease.attempt_number }),
    )
    .await?;
    touch_worker(&mut transaction, lease.worker_id).await?;
    transaction.commit().await?;
    Ok(action)
}

pub async fn fail_action_run(
    pool: &PgPool,
    action_run_id: Uuid,
    request: FailActionRunRequest,
) -> AppResult<ActionRunRecord> {
    let request = request.normalize()?;
    let mut transaction = pool.begin().await?;
    let (lease, action) =
        verified_active_lease(&mut transaction, action_run_id, &request.credentials, true).await?;
    let now = Utc::now();
    let deadline_passed = action.deadline_at.is_some_and(|deadline| deadline <= now);
    let disposition =
        if request.failure_kind == "cancelled" || action.status == "cancellation_requested" {
            LeaseLossDisposition::Cancel
        } else if request.failure_kind == "permanent" {
            LeaseLossDisposition::Fail
        } else if request.failure_kind == "unsafe_state" || action.subject_kind == "tool_lease" {
            LeaseLossDisposition::WaitForHuman
        } else {
            lease_loss_disposition(
                parse_action_status(&action.status)?,
                parse_retry_safety(&action.retry_safety)?,
                action.attempt_count,
                action.max_attempts,
                deadline_passed,
            )
        };
    let lease_status = if disposition == LeaseLossDisposition::Cancel {
        "cancelled"
    } else {
        "failed"
    };
    sqlx::query(
        "UPDATE action_run_leases SET status = $1, outcome = $2, error_code = $3, \
         error_summary = $4, completed_at = $5 WHERE id = $6",
    )
    .bind(lease_status)
    .bind(Json(request.detail.clone()))
    .bind(&request.failure_kind)
    .bind(&request.summary)
    .bind(now)
    .bind(lease.id)
    .execute(&mut *transaction)
    .await?;
    let updated = apply_disposition(
        &mut transaction,
        &action,
        Some(lease.id),
        disposition,
        &request.failure_kind,
        &request.summary,
        request.detail,
        now,
        "worker",
        Some(lease.worker_id.to_string()),
    )
    .await?;
    finish_linked_tool_after_worker_report(
        &mut transaction,
        &updated,
        disposition,
        &request.failure_kind,
        &request.summary,
        now,
    )
    .await?;
    touch_worker(&mut transaction, lease.worker_id).await?;
    transaction.commit().await?;
    Ok(updated)
}

pub async fn cancel_action_run(
    pool: &PgPool,
    project_id: Uuid,
    action_run_id: Uuid,
    request: CancelActionRunRequest,
) -> AppResult<(bool, ActionRunRecord)> {
    let request = request.normalize()?;
    let identity = CommandReceiptIdentity::from_input("action.cancel", &(action_run_id, &request))?;
    let mut transaction = pool.begin().await?;
    if let Some(replayed) = replay_action_command(
        &mut transaction,
        project_id,
        request.client_request_id,
        &identity,
        action_run_id,
    )
    .await?
    {
        transaction.commit().await?;
        return Ok((true, replayed));
    }
    let mut action = load_action(&mut transaction, action_run_id, true).await?;
    if action.project_id != project_id {
        return Err(AppError::not_found("ActionRun 不存在"));
    }
    let now = Utc::now();
    match action.status.as_str() {
        "queued" | "waiting" => {
            action = sqlx::query_as::<_, ActionRunRecord>(
                "UPDATE goal_action_runs SET status = 'cancelled', \
                 last_error_code = 'user_cancelled', last_error_summary = $1, \
                 updated_at = $2, completed_at = $2 WHERE id = $3 RETURNING *",
            )
            .bind(&request.reason)
            .bind(now)
            .bind(action.id)
            .fetch_one(&mut *transaction)
            .await?;
            finish_unstarted_tool_lease(&mut transaction, &action, "cancelled", now).await?;
            resolve_action_issue(&mut transaction, &action, &request.reason).await?;
        }
        "running" => {
            action = sqlx::query_as::<_, ActionRunRecord>(
                "UPDATE goal_action_runs SET status = 'cancellation_requested', \
                 last_error_code = 'user_cancel_requested', last_error_summary = $1, \
                 updated_at = $2 WHERE id = $3 RETURNING *",
            )
            .bind(&request.reason)
            .bind(now)
            .bind(action.id)
            .fetch_one(&mut *transaction)
            .await?;
        }
        "cancellation_requested" | "succeeded" | "failed" | "cancelled" => {}
        _ => {
            return Err(AppError::conflict(
                "invalid_action_transition",
                "ActionRun 当前状态不允许取消",
            ));
        }
    }
    insert_action_event(
        &mut transaction,
        &action,
        None,
        "action.cancel_requested",
        "human",
        None,
        json!({ "reason": request.reason }),
    )
    .await?;
    save_action_command_receipt(
        &mut transaction,
        project_id,
        request.client_request_id,
        &identity,
        &action,
    )
    .await?;
    transaction.commit().await?;
    Ok((false, action))
}

pub async fn resume_action_run(
    pool: &PgPool,
    project_id: Uuid,
    action_run_id: Uuid,
    request: ResumeActionRunRequest,
) -> AppResult<(bool, ActionRunRecord)> {
    let request = request.normalize()?;
    let identity =
        CommandReceiptIdentity::from_input("action.resolve", &(action_run_id, &request))?;
    let mut transaction = pool.begin().await?;
    if let Some(replayed) = replay_action_command(
        &mut transaction,
        project_id,
        request.client_request_id,
        &identity,
        action_run_id,
    )
    .await?
    {
        transaction.commit().await?;
        return Ok((true, replayed));
    }
    let mut action = load_action(&mut transaction, action_run_id, true).await?;
    if action.project_id != project_id {
        return Err(AppError::not_found("ActionRun 不存在"));
    }
    if action.status != "waiting" {
        return Err(AppError::conflict(
            "action_not_waiting",
            "只有 waiting ActionRun 可以由用户选择恢复方式",
        ));
    }
    if request.decision == "retry" && action.subject_kind == "tool_lease" {
        return Err(AppError::conflict(
            "tool_lease_reacquire_required",
            "过期持续进程不能原地复活；请确认清理后创建新的 ToolLease",
        ));
    }
    let now = Utc::now();
    let (next_status, completed_at) = match request.decision.as_str() {
        "retry" => ("queued", None),
        "fail" => ("failed", Some(now)),
        "cancel" => ("cancelled", Some(now)),
        _ => unreachable!("normalized decision"),
    };
    action = sqlx::query_as::<_, ActionRunRecord>(
        "UPDATE goal_action_runs SET status = $1, available_at = $2, \
         last_error_code = $3, last_error_summary = $4, updated_at = $2, completed_at = $5 \
         WHERE id = $6 RETURNING *",
    )
    .bind(next_status)
    .bind(now)
    .bind(format!("human_{}", request.decision))
    .bind(&request.reason)
    .bind(completed_at)
    .bind(action.id)
    .fetch_one(&mut *transaction)
    .await?;
    resolve_action_issue(&mut transaction, &action, &request.reason).await?;
    insert_action_event(
        &mut transaction,
        &action,
        None,
        &format!("action.human_{}", request.decision),
        "human",
        None,
        json!({ "reason": request.reason }),
    )
    .await?;
    save_action_command_receipt(
        &mut transaction,
        project_id,
        request.client_request_id,
        &identity,
        &action,
    )
    .await?;
    transaction.commit().await?;
    Ok((false, action))
}

pub async fn reconcile_action_runs(
    pool: &PgPool,
    request: ReconcileActionRunsRequest,
) -> AppResult<ReconcileResponse> {
    let request = request.normalize()?;
    let mut transaction = pool.begin().await?;
    let worker = authenticate_worker(
        &mut transaction,
        request.worker_id,
        &request.worker_token,
        Some("scheduler.reconcile"),
        true,
    )
    .await?;
    let now = Utc::now();
    let expired = sqlx::query_as::<_, ActionLeaseRecord>(
        "SELECT * FROM action_run_leases \
         WHERE status = 'active' AND (soft_expires_at <= $1 OR hard_expires_at <= $1) \
         ORDER BY soft_expires_at, id FOR UPDATE SKIP LOCKED LIMIT $2",
    )
    .bind(now)
    .bind(request.limit)
    .fetch_all(&mut *transaction)
    .await?;
    let mut response = ReconcileResponse {
        expired_leases: 0,
        requeued_actions: 0,
        waiting_actions: 0,
        failed_actions: 0,
        cancelled_actions: 0,
        deadline_failures: 0,
    };
    for lease in expired {
        let action = load_action(&mut transaction, lease.action_run_id, true).await?;
        if !matches!(action.status.as_str(), "running" | "cancellation_requested") {
            continue;
        }
        sqlx::query(
            "UPDATE action_run_leases SET status = 'expired', error_code = 'worker_lease_expired', \
             error_summary = 'Worker 心跳超过软/硬到期', completed_at = $1 WHERE id = $2",
        )
        .bind(now)
        .bind(lease.id)
        .execute(&mut *transaction)
        .await?;
        let deadline_passed = action.deadline_at.is_some_and(|deadline| deadline <= now);
        let disposition = if action.subject_kind == "tool_lease" {
            if action.status == "cancellation_requested" {
                LeaseLossDisposition::Cancel
            } else if deadline_passed {
                LeaseLossDisposition::Fail
            } else {
                LeaseLossDisposition::WaitForHuman
            }
        } else {
            lease_loss_disposition(
                parse_action_status(&action.status)?,
                parse_retry_safety(&action.retry_safety)?,
                action.attempt_count,
                action.max_attempts,
                deadline_passed,
            )
        };
        let updated = apply_disposition(
            &mut transaction,
            &action,
            Some(lease.id),
            disposition,
            if deadline_passed {
                "action_deadline_exceeded"
            } else {
                "worker_lease_expired"
            },
            "Worker 心跳消失；旧 fencing token 已封存",
            json!({
                "workerId": lease.worker_id,
                "attemptNumber": lease.attempt_number,
                "softExpiresAt": lease.soft_expires_at,
                "hardExpiresAt": lease.hard_expires_at,
            }),
            now,
            "system",
            Some(worker.id.to_string()),
        )
        .await?;
        expire_linked_tool_lease(&mut transaction, &updated, now).await?;
        response.expired_leases += 1;
        match disposition {
            LeaseLossDisposition::Requeue => response.requeued_actions += 1,
            LeaseLossDisposition::WaitForHuman => response.waiting_actions += 1,
            LeaseLossDisposition::Fail => response.failed_actions += 1,
            LeaseLossDisposition::Cancel => response.cancelled_actions += 1,
        }
    }
    let remaining = request.limit.saturating_sub(response.expired_leases as i64);
    if remaining > 0 {
        let overdue = sqlx::query_as::<_, ActionRunRecord>(
            "SELECT * FROM goal_action_runs WHERE status = 'queued' AND deadline_at <= $1 \
             ORDER BY deadline_at, id FOR UPDATE SKIP LOCKED LIMIT $2",
        )
        .bind(now)
        .bind(remaining)
        .fetch_all(&mut *transaction)
        .await?;
        for action in overdue {
            apply_disposition(
                &mut transaction,
                &action,
                None,
                LeaseLossDisposition::Fail,
                "action_deadline_exceeded",
                "ActionRun 在获得 Worker 前已经超过固定截止时间",
                json!({}),
                now,
                "system",
                Some(worker.id.to_string()),
            )
            .await?;
            response.deadline_failures += 1;
            response.failed_actions += 1;
        }
    }
    touch_worker(&mut transaction, worker.id).await?;
    transaction.commit().await?;
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
async fn apply_disposition(
    transaction: &mut DbTransaction<'_>,
    action: &ActionRunRecord,
    lease_id: Option<Uuid>,
    disposition: LeaseLossDisposition,
    error_code: &str,
    summary: &str,
    detail: Value,
    now: DateTime<Utc>,
    actor_type: &str,
    actor_identity: Option<String>,
) -> AppResult<ActionRunRecord> {
    let (status, available_at, completed_at, event_type) = match disposition {
        LeaseLossDisposition::Requeue => (
            "queued",
            now + Duration::seconds(i64::from(action.attempt_count.clamp(1, 10))),
            None,
            "action.requeued",
        ),
        LeaseLossDisposition::WaitForHuman => ("waiting", now, None, "action.waiting_for_human"),
        LeaseLossDisposition::Fail => ("failed", now, Some(now), "action.failed"),
        LeaseLossDisposition::Cancel => ("cancelled", now, Some(now), "action.cancelled"),
    };
    let updated = sqlx::query_as::<_, ActionRunRecord>(
        "UPDATE goal_action_runs SET status = $1, available_at = $2, \
         last_error_code = $3, last_error_summary = $4, result = $5, \
         updated_at = $6, completed_at = $7 WHERE id = $8 RETURNING *",
    )
    .bind(status)
    .bind(available_at)
    .bind(error_code)
    .bind(summary)
    .bind(Json(detail.clone()))
    .bind(now)
    .bind(completed_at)
    .bind(action.id)
    .fetch_one(&mut **transaction)
    .await?;
    insert_action_event(
        transaction,
        &updated,
        lease_id,
        event_type,
        actor_type,
        actor_identity,
        json!({ "errorCode": error_code, "summary": summary, "detail": detail }),
    )
    .await?;
    if matches!(
        disposition,
        LeaseLossDisposition::WaitForHuman | LeaseLossDisposition::Fail
    ) {
        pause_for_action(transaction, &updated, error_code, summary).await?;
    }
    Ok(updated)
}

async fn pause_for_action(
    transaction: &mut DbTransaction<'_>,
    action: &ActionRunRecord,
    error_code: &str,
    summary: &str,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE goal_sessions SET status = 'exception_paused', updated_at = now() \
         WHERE id = $1 AND status = 'running'",
    )
    .bind(action.session_id)
    .execute(&mut **transaction)
    .await?;
    let dedupe_key = format!("action-run:{}", action.id);
    let title = if action.subject_kind == "tool_lease" {
        "持续工具需要处理"
    } else if error_code.contains("deadline") || error_code.contains("timed_out") {
        "后台行动已超时"
    } else if action.status == "waiting" {
        "后台行动等待你的判断"
    } else {
        "后台行动失败"
    };
    let inserted_attention = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO goal_attention_items \
         (id, project_id, goal_branch_id, session_id, kind, status, dedupe_key, title, reason, \
          safe_checkpoint, attempted, risk, user_action, recommendation) \
         VALUES ($1, $2, $3, $4, 'action_run_failure', 'open', $5, $6, $7, $8, $9, $10, $11, $12) \
         ON CONFLICT (project_id, dedupe_key) WHERE status = 'open' DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(action.project_id)
    .bind(action.goal_branch_id)
    .bind(action.session_id)
    .bind(&dedupe_key)
    .bind(title)
    .bind(summary)
    .bind(format!(
        "ActionRun {} 的第 {} 次尝试已封存；旧 fencing token 不能再写回",
        action.id, action.attempt_count
    ))
    .bind("系统已停止自动推进，并保存 Lease、错误和最后心跳")
    .bind(if action.retry_safety == "safe" {
        "继续重试可能重复计算，但不会重复外部副作用"
    } else {
        "自动重放可能重复未知或外部副作用"
    })
    .bind("查看行动现场并明确选择 retry、fail 或 cancel")
    .bind("确认 Worker/工具实际状态；不确定时不要重试")
    .fetch_optional(&mut **transaction)
    .await?;
    let attention_id = match inserted_attention {
        Some(id) => id,
        None => {
            sqlx::query_scalar(
                "SELECT id FROM goal_attention_items \
                 WHERE project_id = $1 AND dedupe_key = $2 AND status = 'open'",
            )
            .bind(action.project_id)
            .bind(&dedupe_key)
            .fetch_one(&mut **transaction)
            .await?
        }
    };
    let notification_kind = if action.subject_kind == "tool_lease" {
        "tool_lease_expired"
    } else if error_code.contains("deadline") || error_code.contains("timed_out") {
        "action_timed_out"
    } else if action.status == "waiting" {
        "action_waiting"
    } else {
        "action_failed"
    };
    let severity = if action.status == "failed" {
        "critical"
    } else {
        "warning"
    };
    let inserted_notification = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO goal_notifications \
         (id, project_id, goal_branch_id, session_id, action_run_id, attention_item_id, \
          dedupe_key, kind, severity, title, summary) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         ON CONFLICT (project_id, dedupe_key) WHERE status <> 'resolved' \
         DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(action.project_id)
    .bind(action.goal_branch_id)
    .bind(action.session_id)
    .bind(action.id)
    .bind(attention_id)
    .bind(&dedupe_key)
    .bind(notification_kind)
    .bind(severity)
    .bind(title)
    .bind(summary)
    .fetch_optional(&mut **transaction)
    .await?;
    let notification_id = match inserted_notification {
        Some(id) => id,
        None => {
            sqlx::query_scalar(
                "SELECT id FROM goal_notifications \
                 WHERE project_id = $1 AND dedupe_key = $2 AND status <> 'resolved'",
            )
            .bind(action.project_id)
            .bind(&dedupe_key)
            .fetch_one(&mut **transaction)
            .await?
        }
    };
    let outbox_payload = json!({
        "notificationId": notification_id,
        "projectId": action.project_id,
        "sessionId": action.session_id,
        "actionRunId": action.id,
        "title": title,
        "summary": summary,
    });
    let outbox_digest = canonical_json_sha256(&outbox_payload)?;
    sqlx::query(
        "INSERT INTO notification_outbox \
         (id, notification_id, adapter, dedupe_key, payload, payload_digest, status, \
          last_error, completed_at) \
         VALUES ($1, $2, 'none', $3, $4, $5, 'suppressed', \
                 'external adapter not configured', now()) \
         ON CONFLICT (dedupe_key) DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(notification_id)
    .bind(format!("none:{notification_id}"))
    .bind(Json(outbox_payload))
    .bind(outbox_digest)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn resolve_action_issue(
    transaction: &mut DbTransaction<'_>,
    action: &ActionRunRecord,
    resolution: &str,
) -> AppResult<()> {
    let dedupe_key = format!("action-run:{}", action.id);
    sqlx::query(
        "UPDATE goal_attention_items SET status = 'resolved', resolution = $1, resolved_at = now() \
         WHERE project_id = $2 AND dedupe_key = $3 AND status = 'open'",
    )
    .bind(resolution)
    .bind(action.project_id)
    .bind(&dedupe_key)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE goal_notifications SET status = 'resolved', resolved_at = now() \
         WHERE project_id = $1 AND dedupe_key = $2 AND status <> 'resolved'",
    )
    .bind(action.project_id)
    .bind(&dedupe_key)
    .execute(&mut **transaction)
    .await?;
    let open_attention: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_attention_items \
         WHERE session_id = $1 AND status = 'open')",
    )
    .bind(action.session_id)
    .fetch_one(&mut **transaction)
    .await?;
    if !open_attention {
        sqlx::query(
            "UPDATE goal_sessions SET status = 'running', updated_at = now() \
             WHERE id = $1 AND status = 'exception_paused'",
        )
        .bind(action.session_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn finish_unstarted_tool_lease(
    transaction: &mut DbTransaction<'_>,
    action: &ActionRunRecord,
    status: &str,
    now: DateTime<Utc>,
) -> AppResult<()> {
    if action.subject_kind != "tool_lease" {
        return Ok(());
    }
    sqlx::query(
        "UPDATE tool_leases SET status = $1, cleanup_status = 'succeeded', completed_at = $2 \
         WHERE id = $3 AND status = 'requested'",
    )
    .bind(status)
    .bind(now)
    .bind(action.subject_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn finish_linked_tool_after_worker_report(
    transaction: &mut DbTransaction<'_>,
    action: &ActionRunRecord,
    disposition: LeaseLossDisposition,
    error_code: &str,
    summary: &str,
    now: DateTime<Utc>,
) -> AppResult<()> {
    if action.subject_kind != "tool_lease" {
        return Ok(());
    }
    let (status, cleanup) = match disposition {
        LeaseLossDisposition::Cancel => ("cancelled", "succeeded"),
        LeaseLossDisposition::Fail | LeaseLossDisposition::WaitForHuman => ("failed", "pending"),
        LeaseLossDisposition::Requeue => return Ok(()),
    };
    sqlx::query(
        "UPDATE tool_leases SET status = $1, cleanup_status = $2, last_error_code = $3, \
         last_error_summary = $4, completed_at = $5 \
         WHERE id = $6 AND status IN ('requested', 'active')",
    )
    .bind(status)
    .bind(cleanup)
    .bind(error_code)
    .bind(summary)
    .bind(now)
    .bind(action.subject_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn expire_linked_tool_lease(
    transaction: &mut DbTransaction<'_>,
    action: &ActionRunRecord,
    now: DateTime<Utc>,
) -> AppResult<()> {
    if action.subject_kind != "tool_lease" {
        return Ok(());
    }
    let status = if action.status == "cancelled" {
        "cancelled"
    } else {
        "expired"
    };
    sqlx::query(
        "UPDATE tool_leases SET status = $1, cleanup_status = 'pending', \
         last_error_code = 'worker_lease_expired', \
         last_error_summary = 'Worker 心跳消失；必须确认外部进程已清理', completed_at = $2 \
         WHERE id = $3 AND status IN ('requested', 'active')",
    )
    .bind(status)
    .bind(now)
    .bind(action.subject_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn replay_action_command(
    transaction: &mut DbTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    identity: &CommandReceiptIdentity,
    expected_action_id: Uuid,
) -> AppResult<Option<ActionRunRecord>> {
    let receipt: Option<(String, String, Json<Value>)> = sqlx::query_as(
        "SELECT command_kind, input_hash, result FROM goal_command_receipts \
         WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(client_request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some((command_kind, input_hash, result)) = receipt else {
        return Ok(None);
    };
    CommandReceiptIdentity {
        command_kind,
        input_hash,
    }
    .ensure_replay_matches(identity)?;
    let action_id = result
        .0
        .get("actionRunId")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| AppError::conflict("action_receipt_corrupt", "Action 命令收据缺少 ID"))?;
    if action_id != expected_action_id {
        return Err(AppError::conflict(
            "idempotency_conflict",
            "同一 clientRequestId 已用于另一个 ActionRun",
        ));
    }
    Ok(Some(load_action(transaction, action_id, false).await?))
}

async fn save_action_command_receipt(
    transaction: &mut DbTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    identity: &CommandReceiptIdentity,
    action: &ActionRunRecord,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO goal_command_receipts \
         (project_id, client_request_id, command_kind, input_hash, result) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(project_id)
    .bind(client_request_id)
    .bind(&identity.command_kind)
    .bind(&identity.input_hash)
    .bind(Json(
        json!({ "actionRunId": action.id, "status": action.status }),
    ))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn worker_response(worker: WorkerRecord, replayed: bool) -> WorkerRegistrationResponse {
    WorkerRegistrationResponse {
        replayed,
        worker_id: worker.id,
        display_name: worker.display_name,
        capabilities: worker.capabilities.0,
        status: worker.status,
        registered_at: worker.registered_at,
        last_seen_at: worker.last_seen_at,
    }
}

async fn authenticate_worker(
    transaction: &mut DbTransaction<'_>,
    worker_id: Uuid,
    token: &str,
    required_capability: Option<&str>,
    require_active: bool,
) -> AppResult<WorkerRecord> {
    let worker = sqlx::query_as::<_, WorkerRecord>(
        "SELECT * FROM scheduler_workers WHERE id = $1 FOR UPDATE",
    )
    .bind(worker_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::forbidden("invalid_worker_identity", "Worker 身份或 token 不匹配"))?;
    if worker.token_digest != secret_digest(token)
        || worker.status == "revoked"
        || (require_active && worker.status != "active")
    {
        return Err(AppError::forbidden(
            "invalid_worker_identity",
            "Worker 身份、状态或 token 不匹配",
        ));
    }
    if let Some(capability) = required_capability
        && !worker
            .capabilities
            .0
            .iter()
            .any(|candidate| candidate == capability)
    {
        return Err(AppError::forbidden(
            "worker_capability_denied",
            "Worker 没有该 control-plane 能力",
        ));
    }
    Ok(worker)
}

async fn verified_active_lease(
    transaction: &mut DbTransaction<'_>,
    action_run_id: Uuid,
    credentials: &ActionLeaseCredentials,
    require_unexpired: bool,
) -> AppResult<(ActionLeaseRecord, ActionRunRecord)> {
    authenticate_worker(
        transaction,
        credentials.worker_id,
        &credentials.worker_token,
        None,
        true,
    )
    .await?;
    let lease = sqlx::query_as::<_, ActionLeaseRecord>(
        "SELECT * FROM action_run_leases WHERE id = $1 AND action_run_id = $2 FOR UPDATE",
    )
    .bind(credentials.lease_id)
    .bind(action_run_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::forbidden("invalid_action_lease", "ActionLease 身份不匹配"))?;
    if lease.worker_id != credentials.worker_id
        || lease.renewal_token_digest != secret_digest(&credentials.lease_token)
        || lease.fencing_token != credentials.fencing_token
        || lease.status != "active"
    {
        return Err(AppError::forbidden(
            "invalid_action_lease",
            "ActionLease token、Worker 或 fencing 不匹配",
        ));
    }
    let action = load_action(transaction, action_run_id, true).await?;
    if action.fencing_counter != credentials.fencing_token
        || !matches!(action.status.as_str(), "running" | "cancellation_requested")
    {
        return Err(AppError::conflict(
            "stale_action_fencing",
            "ActionRun 已经由更新的 Worker 接管或进入终态",
        ));
    }
    if require_unexpired
        && (lease.soft_expires_at <= Utc::now() || lease.hard_expires_at <= Utc::now())
    {
        return Err(AppError::conflict(
            "action_lease_expired",
            "ActionLease 已到期，必须先由 reconcile 决定恢复方式",
        ));
    }
    Ok((lease, action))
}

async fn load_action(
    transaction: &mut DbTransaction<'_>,
    action_run_id: Uuid,
    for_update: bool,
) -> AppResult<ActionRunRecord> {
    let sql = if for_update {
        "SELECT * FROM goal_action_runs WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT * FROM goal_action_runs WHERE id = $1"
    };
    sqlx::query_as::<_, ActionRunRecord>(sql)
        .bind(action_run_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| AppError::not_found("ActionRun 不存在"))
}

async fn load_notification(
    transaction: &mut DbTransaction<'_>,
    project_id: Uuid,
    notification_id: Uuid,
    for_update: bool,
) -> AppResult<NotificationRecord> {
    let sql = if for_update {
        "SELECT * FROM goal_notifications WHERE project_id = $1 AND id = $2 FOR UPDATE"
    } else {
        "SELECT * FROM goal_notifications WHERE project_id = $1 AND id = $2"
    };
    sqlx::query_as::<_, NotificationRecord>(sql)
        .bind(project_id)
        .bind(notification_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| AppError::not_found("通知不存在"))
}

async fn load_tool_lease_for_update(
    transaction: &mut DbTransaction<'_>,
    tool_lease_id: Uuid,
) -> AppResult<ToolLeaseRecord> {
    sqlx::query_as::<_, ToolLeaseRecord>("SELECT * FROM tool_leases WHERE id = $1 FOR UPDATE")
        .bind(tool_lease_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(|| AppError::not_found("ToolLease 不存在"))
}

async fn ensure_session_scope(pool: &PgPool, project_id: Uuid, session_id: Uuid) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_sessions WHERE id = $1 AND project_id = $2)",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Err(AppError::not_found("Agent Session 不存在"));
    }
    Ok(())
}

async fn touch_worker(transaction: &mut DbTransaction<'_>, worker_id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE scheduler_workers SET last_seen_at = now() WHERE id = $1")
        .bind(worker_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn heartbeat_linked_tool_lease(
    transaction: &mut DbTransaction<'_>,
    action: &ActionRunRecord,
    now: DateTime<Utc>,
    action_soft_expiry: DateTime<Utc>,
) -> AppResult<(Option<String>, Vec<String>)> {
    if action.subject_kind != "tool_lease" {
        return Ok((None, Vec::new()));
    }
    let tool_lease_id = action
        .subject_id
        .ok_or_else(|| AppError::conflict("tool_lease_missing", "ActionRun 缺少 ToolLease 身份"))?;
    let row: (String, Json<Vec<String>>, DateTime<Utc>, DateTime<Utc>) = sqlx::query_as(
        "SELECT status, endpoint_refs, soft_expires_at, hard_expires_at \
         FROM tool_leases WHERE id = $1 FOR UPDATE",
    )
    .bind(tool_lease_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::conflict("tool_lease_missing", "ActionRun 的 ToolLease 不存在"))?;
    if row.0 == "active" {
        if row.2 <= now || row.3 <= now {
            return Err(AppError::conflict(
                "tool_lease_expired",
                "ToolLease 已到期，不能由迟到心跳复活",
            ));
        }
        let next_soft = action_soft_expiry.min(row.3);
        sqlx::query(
            "UPDATE tool_leases SET last_heartbeat_at = $1, soft_expires_at = $2 \
             WHERE id = $3",
        )
        .bind(now)
        .bind(next_soft)
        .bind(tool_lease_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok((Some(row.0), row.1.0))
}

async fn insert_action_event(
    transaction: &mut DbTransaction<'_>,
    action: &ActionRunRecord,
    lease_id: Option<Uuid>,
    event_type: &str,
    actor_type: &str,
    actor_identity: Option<String>,
    detail: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO goal_action_events \
         (id, project_id, action_run_id, lease_id, event_type, actor_type, actor_identity, detail) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(Uuid::new_v4())
    .bind(action.project_id)
    .bind(action.id)
    .bind(lease_id)
    .bind(event_type)
    .bind(actor_type)
    .bind(actor_identity)
    .bind(Json(detail))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn secret_digest(value: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(value.as_bytes()))
}

fn ensure_resource_within(
    requested: &ResourcePolicy,
    maximum: &ResourcePolicy,
    boundary: &str,
) -> AppResult<()> {
    if requested.cpu_millis > maximum.cpu_millis
        || requested.memory_mi_b > maximum.memory_mi_b
        || requested.disk_mi_b > maximum.disk_mi_b
        || requested.pids > maximum.pids
        || requested.timeout_seconds > maximum.timeout_seconds
        || requested.stdout_bytes > maximum.stdout_bytes
        || requested.stderr_bytes > maximum.stderr_bytes
    {
        return Err(AppError::forbidden(
            "resource_limit_denied",
            format!("ToolLease 资源请求超过 {boundary} 固定上限"),
        ));
    }
    Ok(())
}

fn runner_limits(policy: &ResourcePolicy) -> RunnerResourceLimits {
    RunnerResourceLimits {
        cpu_millis: policy.cpu_millis,
        memory_mi_b: policy.memory_mi_b,
        disk_mi_b: policy.disk_mi_b,
        pids: policy.pids,
        timeout_seconds: policy.timeout_seconds,
        stdout_bytes: policy.stdout_bytes,
        stderr_bytes: policy.stderr_bytes,
    }
}

fn parse_retry_safety(value: &str) -> AppResult<RetrySafety> {
    match value {
        "safe" => Ok(RetrySafety::Safe),
        "unsafe" => Ok(RetrySafety::Unsafe),
        "unknown" => Ok(RetrySafety::Unknown),
        _ => Err(AppError::conflict(
            "action_state_corrupt",
            "ActionRun retrySafety 无法解析",
        )),
    }
}

fn parse_action_status(value: &str) -> AppResult<ActionRunStatus> {
    match value {
        "queued" => Ok(ActionRunStatus::Queued),
        "running" => Ok(ActionRunStatus::Running),
        "waiting" => Ok(ActionRunStatus::Waiting),
        "cancellation_requested" => Ok(ActionRunStatus::CancellationRequested),
        "succeeded" => Ok(ActionRunStatus::Succeeded),
        "failed" => Ok(ActionRunStatus::Failed),
        "cancelled" => Ok(ActionRunStatus::Cancelled),
        _ => Err(AppError::conflict(
            "action_state_corrupt",
            "ActionRun status 无法解析",
        )),
    }
}
