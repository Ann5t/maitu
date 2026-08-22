use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    goal_domain::{canonical_json_sha256, validate_sha256_id},
    tooling::{ResolvedPluginRef, ResourcePolicy},
};

const MAX_PAYLOAD_BYTES: usize = 256 * 1024;
const MAX_ACTION_HORIZON_SECONDS: i64 = 30 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrySafety {
    Safe,
    Unsafe,
    Unknown,
}

impl RetrySafety {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Unsafe => "unsafe",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRunStatus {
    Queued,
    Running,
    Waiting,
    CancellationRequested,
    Succeeded,
    Failed,
    Cancelled,
}

impl ActionRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::CancellationRequested => "cancellation_requested",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeaseLossDisposition {
    Requeue,
    WaitForHuman,
    Fail,
    Cancel,
}

pub fn lease_loss_disposition(
    status: ActionRunStatus,
    retry_safety: RetrySafety,
    attempt_count: i32,
    max_attempts: i32,
    deadline_passed: bool,
) -> LeaseLossDisposition {
    if status == ActionRunStatus::CancellationRequested {
        return LeaseLossDisposition::Cancel;
    }
    if deadline_passed || attempt_count >= max_attempts {
        return LeaseLossDisposition::Fail;
    }
    match retry_safety {
        RetrySafety::Safe => LeaseLossDisposition::Requeue,
        RetrySafety::Unsafe | RetrySafety::Unknown => LeaseLossDisposition::WaitForHuman,
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueActionRunRequest {
    pub client_request_id: Uuid,
    pub kind: String,
    pub capability: String,
    #[serde(default = "empty_object")]
    pub payload: Value,
    pub retry_safety: RetrySafety,
    #[serde(default = "default_max_attempts")]
    pub max_attempts: i32,
    pub available_at: Option<DateTime<Utc>>,
    pub deadline_at: Option<DateTime<Utc>>,
}

impl EnqueueActionRunRequest {
    pub fn normalize(mut self, now: DateTime<Utc>) -> AppResult<Self> {
        self.kind = required_name("ActionRun kind", self.kind, 80)?;
        if !matches!(
            self.kind.as_str(),
            "agent_step" | "runner_job" | "tool_lease" | "review" | "integration" | "maintenance"
        ) {
            return Err(AppError::bad_request(
                "invalid_action_kind",
                "ActionRun kind 不在受支持集合中",
            ));
        }
        self.capability = normalize_capability(self.capability)?;
        if !self.payload.is_object() {
            return Err(AppError::bad_request(
                "invalid_action_payload",
                "ActionRun payload 必须是 JSON 对象",
            ));
        }
        if serde_json::to_vec(&self.payload)?.len() > MAX_PAYLOAD_BYTES {
            return Err(AppError::bad_request(
                "action_payload_too_large",
                "ActionRun payload 超过 256 KiB 上限",
            ));
        }
        if !(1..=10).contains(&self.max_attempts) {
            return Err(AppError::bad_request(
                "invalid_action_attempts",
                "ActionRun 最大尝试次数必须在 1 到 10 之间",
            ));
        }
        self.available_at = Some(self.available_at.unwrap_or(now));
        if let Some(deadline) = self.deadline_at {
            if deadline <= now || deadline > now + Duration::seconds(MAX_ACTION_HORIZON_SECONDS) {
                return Err(AppError::bad_request(
                    "invalid_action_deadline",
                    "ActionRun 截止时间必须在未来 30 天内",
                ));
            }
            if self
                .available_at
                .is_some_and(|available| available >= deadline)
            {
                return Err(AppError::bad_request(
                    "invalid_action_schedule",
                    "ActionRun 可用时间必须早于截止时间",
                ));
            }
        }
        Ok(self)
    }

    pub fn request_hash(&self) -> AppResult<String> {
        canonical_json_sha256(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterWorkerRequest {
    pub client_request_id: Uuid,
    pub worker_id: Uuid,
    pub worker_token: String,
    pub display_name: String,
    pub capabilities: Vec<String>,
}

impl RegisterWorkerRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        validate_secret_token("Worker token", &self.worker_token)?;
        self.display_name = required_text("Worker 显示名称", self.display_name, 120)?;
        if self.capabilities.is_empty() || self.capabilities.len() > 100 {
            return Err(AppError::bad_request(
                "invalid_worker_capabilities",
                "Worker 必须声明 1 到 100 项能力",
            ));
        }
        for capability in &mut self.capabilities {
            *capability = normalize_capability(std::mem::take(capability))?;
        }
        self.capabilities.sort();
        self.capabilities.dedup();
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimActionRunRequest {
    pub worker_id: Uuid,
    pub worker_token: String,
    pub client_request_id: Uuid,
    pub lease_token: String,
    #[serde(default = "default_soft_ttl_seconds")]
    pub soft_ttl_seconds: u32,
    #[serde(default = "default_hard_ttl_seconds")]
    pub hard_ttl_seconds: u32,
}

impl ClaimActionRunRequest {
    pub fn normalize(self) -> AppResult<Self> {
        validate_secret_token("Worker token", &self.worker_token)?;
        validate_secret_token("ActionLease token", &self.lease_token)?;
        validate_lease_ttls(self.soft_ttl_seconds, self.hard_ttl_seconds)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionLeaseCredentials {
    pub worker_id: Uuid,
    pub worker_token: String,
    pub lease_id: Uuid,
    pub lease_token: String,
    pub fencing_token: i64,
}

impl ActionLeaseCredentials {
    pub fn normalize(self) -> AppResult<Self> {
        validate_secret_token("Worker token", &self.worker_token)?;
        validate_secret_token("ActionLease token", &self.lease_token)?;
        if self.fencing_token <= 0 {
            return Err(AppError::bad_request(
                "invalid_fencing_token",
                "fencingToken 必须大于 0",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatActionRunRequest {
    #[serde(flatten)]
    pub credentials: ActionLeaseCredentials,
    #[serde(default = "default_soft_ttl_seconds")]
    pub extend_seconds: u32,
}

impl HeartbeatActionRunRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.credentials = self.credentials.normalize()?;
        if !(2..=300).contains(&self.extend_seconds) {
            return Err(AppError::bad_request(
                "invalid_heartbeat_extension",
                "心跳延长时间必须在 2 到 300 秒之间",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompleteActionRunRequest {
    #[serde(flatten)]
    pub credentials: ActionLeaseCredentials,
    #[serde(default = "empty_object")]
    pub result: Value,
}

impl CompleteActionRunRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.credentials = self.credentials.normalize()?;
        validate_result_object(&self.result)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailActionRunRequest {
    #[serde(flatten)]
    pub credentials: ActionLeaseCredentials,
    pub failure_kind: String,
    pub summary: String,
    #[serde(default = "empty_object")]
    pub detail: Value,
}

impl FailActionRunRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.credentials = self.credentials.normalize()?;
        self.failure_kind = required_name("失败类型", self.failure_kind, 80)?;
        if !matches!(
            self.failure_kind.as_str(),
            "transient" | "permanent" | "timed_out" | "unsafe_state" | "cancelled"
        ) {
            return Err(AppError::bad_request(
                "invalid_action_failure",
                "ActionRun 失败类型不受支持",
            ));
        }
        self.summary = required_text("失败摘要", self.summary, 2_000)?;
        validate_result_object(&self.detail)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelActionRunRequest {
    pub client_request_id: Uuid,
    pub reason: String,
}

impl CancelActionRunRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.reason = required_text("取消原因", self.reason, 2_000)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeActionRunRequest {
    pub client_request_id: Uuid,
    pub decision: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkNotificationReadRequest {
    pub client_request_id: Uuid,
}

impl ResumeActionRunRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.decision = required_name("恢复决定", self.decision, 40)?;
        if !matches!(self.decision.as_str(), "retry" | "fail" | "cancel") {
            return Err(AppError::bad_request(
                "invalid_action_recovery",
                "恢复决定必须是 retry、fail 或 cancel",
            ));
        }
        self.reason = required_text("恢复理由", self.reason, 2_000)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconcileActionRunsRequest {
    pub worker_id: Uuid,
    pub worker_token: String,
    #[serde(default = "default_reconcile_limit")]
    pub limit: i64,
}

impl ReconcileActionRunsRequest {
    pub fn normalize(self) -> AppResult<Self> {
        validate_secret_token("Worker token", &self.worker_token)?;
        if !(1..=500).contains(&self.limit) {
            return Err(AppError::bad_request(
                "invalid_reconcile_limit",
                "reconcile limit 必须在 1 到 500 之间",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateToolLeaseRequest {
    pub client_request_id: Uuid,
    pub plugin: ResolvedPluginRef,
    pub tool_name: String,
    #[serde(default = "empty_object")]
    pub input: Value,
    pub base_workspace_snapshot: String,
    #[serde(default)]
    pub resource_policy: Option<ResourcePolicy>,
    #[serde(default = "default_tool_soft_ttl_seconds")]
    pub soft_ttl_seconds: u32,
    #[serde(default = "default_tool_duration_seconds")]
    pub duration_seconds: u32,
}

impl CreateToolLeaseRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.plugin = self.plugin.validate()?;
        self.tool_name = required_name("持续工具名", self.tool_name, 120)?;
        validate_result_object(&self.input)?;
        self.base_workspace_snapshot =
            validate_sha256_id("workspace snapshot", self.base_workspace_snapshot)?;
        if !(2..=300).contains(&self.soft_ttl_seconds)
            || !(5..=86_400).contains(&self.duration_seconds)
            || self.soft_ttl_seconds >= self.duration_seconds
        {
            return Err(AppError::bad_request(
                "invalid_tool_lease_duration",
                "ToolLease 软到期必须为 2–300 秒，硬到期为 5 秒–24 小时且更晚",
            ));
        }
        if let Some(policy) = &self.resource_policy {
            validate_resource_policy(policy)?;
        }
        Ok(self)
    }

    pub fn request_hash(&self) -> AppResult<String> {
        canonical_json_sha256(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivateToolLeaseRequest {
    #[serde(flatten)]
    pub credentials: ActionLeaseCredentials,
    #[serde(default)]
    pub endpoint_refs: Vec<String>,
}

impl ActivateToolLeaseRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.credentials = self.credentials.normalize()?;
        if self.endpoint_refs.len() > 16 {
            return Err(AppError::bad_request(
                "too_many_tool_endpoints",
                "ToolLease endpoint 数量不能超过 16",
            ));
        }
        for endpoint in &mut self.endpoint_refs {
            *endpoint = required_text("ToolLease endpoint", std::mem::take(endpoint), 500)?;
            if !(endpoint.starts_with("http://")
                || endpoint.starts_with("https://")
                || endpoint.starts_with("ws://")
                || endpoint.starts_with("wss://"))
            {
                return Err(AppError::bad_request(
                    "invalid_tool_endpoint",
                    "ToolLease endpoint 只接受 http(s) 或 ws(s) 引用",
                ));
            }
        }
        self.endpoint_refs.sort();
        self.endpoint_refs.dedup();
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishToolLeaseRequest {
    #[serde(flatten)]
    pub credentials: ActionLeaseCredentials,
    #[serde(default)]
    pub retained_outputs: Vec<String>,
    #[serde(default = "empty_object")]
    pub result: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestToolLeaseStopRequest {
    pub client_request_id: Uuid,
    pub renewal_token: String,
    pub mode: String,
    pub reason: String,
}

impl RequestToolLeaseStopRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        validate_secret_token("ToolLease renewal token", &self.renewal_token)?;
        self.mode = required_name("ToolLease 停止方式", self.mode, 40)?;
        if !matches!(self.mode.as_str(), "release" | "cancel") {
            return Err(AppError::bad_request(
                "invalid_tool_lease_stop",
                "ToolLease 停止方式必须是 release 或 cancel",
            ));
        }
        self.reason = required_text("ToolLease 停止原因", self.reason, 2_000)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcknowledgeToolCleanupRequest {
    pub worker_id: Uuid,
    pub worker_token: String,
    pub status: String,
    pub summary: String,
}

impl AcknowledgeToolCleanupRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        validate_secret_token("Worker token", &self.worker_token)?;
        self.status = required_name("清理状态", self.status, 40)?;
        if !matches!(self.status.as_str(), "succeeded" | "failed") {
            return Err(AppError::bad_request(
                "invalid_cleanup_status",
                "清理确认状态必须是 succeeded 或 failed",
            ));
        }
        self.summary = required_text("清理摘要", self.summary, 2_000)?;
        Ok(self)
    }
}

impl FinishToolLeaseRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.credentials = self.credentials.normalize()?;
        if self.retained_outputs.len() > 100 {
            return Err(AppError::bad_request(
                "too_many_retained_outputs",
                "ToolLease 保留输出不能超过 100 项",
            ));
        }
        for output in &mut self.retained_outputs {
            *output = required_text("保留输出引用", std::mem::take(output), 500)?;
        }
        self.retained_outputs.sort();
        self.retained_outputs.dedup();
        validate_result_object(&self.result)?;
        Ok(self)
    }
}

pub fn validate_secret_token(label: &str, token: &str) -> AppResult<()> {
    if !(32..=256).contains(&token.len())
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(AppError::bad_request(
            "invalid_secret_token",
            format!("{label} 必须是 32–256 位安全 ASCII token"),
        ));
    }
    Ok(())
}

pub fn normalize_capability(value: String) -> AppResult<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 160
        || !value.bytes().enumerate().all(|(index, byte)| {
            (index > 0 && byte.is_ascii_digit())
                || byte.is_ascii_lowercase()
                || (index > 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
    {
        return Err(AppError::bad_request(
            "invalid_worker_capability",
            "Worker capability 必须是规范小写名称",
        ));
    }
    Ok(value)
}

fn validate_lease_ttls(soft: u32, hard: u32) -> AppResult<()> {
    if !(2..=300).contains(&soft) || !(5..=86_400).contains(&hard) || soft >= hard {
        return Err(AppError::bad_request(
            "invalid_action_lease_ttl",
            "ActionLease 软到期必须为 2–300 秒，硬到期为 5 秒–24 小时且更晚",
        ));
    }
    Ok(())
}

fn validate_resource_policy(policy: &ResourcePolicy) -> AppResult<()> {
    if policy.cpu_millis == 0
        || policy.memory_mi_b == 0
        || policy.disk_mi_b == 0
        || policy.pids == 0
        || policy.timeout_seconds == 0
        || policy.stdout_bytes == 0
        || policy.stderr_bytes == 0
        || policy.pids > 1024
        || policy.timeout_seconds > 86_400
    {
        return Err(AppError::bad_request(
            "invalid_resource_policy",
            "ToolLease 资源策略超出允许范围",
        ));
    }
    Ok(())
}

fn validate_result_object(value: &Value) -> AppResult<()> {
    if !value.is_object() {
        return Err(AppError::bad_request(
            "invalid_action_result",
            "结构化输入或结果必须是 JSON 对象",
        ));
    }
    if serde_json::to_vec(value)?.len() > MAX_PAYLOAD_BYTES {
        return Err(AppError::bad_request(
            "action_result_too_large",
            "结构化输入或结果超过 256 KiB 上限",
        ));
    }
    Ok(())
}

fn required_name(label: &str, value: String, max: usize) -> AppResult<String> {
    let value = required_text(label, value, max)?.to_ascii_lowercase();
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    }) {
        return Err(AppError::bad_request(
            "invalid_scheduler_name",
            format!("{label} 只能使用小写字母、数字、点、横线和下划线"),
        ));
    }
    Ok(value)
}

fn required_text(label: &str, value: String, max: usize) -> AppResult<String> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.chars().count() > max || value.contains('\0') {
        return Err(AppError::bad_request(
            "invalid_scheduler_text",
            format!("{label} 不能为空且不能超过 {max} 个字符"),
        ));
    }
    Ok(value)
}

fn empty_object() -> Value {
    serde_json::json!({})
}

const fn default_max_attempts() -> i32 {
    3
}

const fn default_soft_ttl_seconds() -> u32 {
    10
}

const fn default_hard_ttl_seconds() -> u32 {
    300
}

const fn default_tool_soft_ttl_seconds() -> u32 {
    10
}

const fn default_tool_duration_seconds() -> u32 {
    600
}

const fn default_reconcile_limit() -> i64 {
    100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_safe_lease_loss_is_automatically_retried() {
        assert_eq!(
            lease_loss_disposition(ActionRunStatus::Running, RetrySafety::Safe, 1, 3, false),
            LeaseLossDisposition::Requeue
        );
        assert_eq!(
            lease_loss_disposition(ActionRunStatus::Running, RetrySafety::Unknown, 1, 3, false),
            LeaseLossDisposition::WaitForHuman
        );
        assert_eq!(
            lease_loss_disposition(
                ActionRunStatus::CancellationRequested,
                RetrySafety::Safe,
                1,
                3,
                false
            ),
            LeaseLossDisposition::Cancel
        );
        assert_eq!(
            lease_loss_disposition(ActionRunStatus::Running, RetrySafety::Safe, 3, 3, false),
            LeaseLossDisposition::Fail
        );
    }

    #[test]
    fn action_contract_rejects_fake_retry_and_unbounded_payloads() {
        let now = Utc::now();
        let request = EnqueueActionRunRequest {
            client_request_id: Uuid::new_v4(),
            kind: "agent_step".into(),
            capability: "agent.execute".into(),
            payload: serde_json::json!({ "task": "check" }),
            retry_safety: RetrySafety::Unknown,
            max_attempts: 3,
            available_at: None,
            deadline_at: Some(now + Duration::minutes(5)),
        }
        .normalize(now)
        .unwrap();
        assert_eq!(request.capability, "agent.execute");

        let mut invalid = request;
        invalid.capability = "Bad Capability".into();
        assert_eq!(
            invalid.normalize(now).unwrap_err().code(),
            "invalid_worker_capability"
        );
    }
}
