use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    workspace::WorkspaceCapabilityPolicy,
};

const MAX_OUTCOME_CHARS: usize = 4_000;
const MAX_LIST_ITEMS: usize = 100;
const MAX_ITEM_CHARS: usize = 2_000;

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $value:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl TryFrom<&str> for $name {
            type Error = AppError;

            fn try_from(value: &str) -> AppResult<Self> {
                match value {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(AppError::bad_request(
                        "invalid_state",
                        format!("未知的 {} 状态：{value}", stringify!($name)),
                    )),
                }
            }
        }
    };
}

string_enum!(GoalActor {
    Human => "human",
    Agent => "agent",
    ReviewAi => "review_ai",
    System => "system",
});

string_enum!(ProposalStatus {
    Draft => "draft",
    AwaitingApproval => "awaiting_approval",
    Approved => "approved",
    Cancelled => "cancelled",
});

string_enum!(GoalBranchStatus {
    Active => "active",
    Waiting => "waiting",
    ReviewPending => "review_pending",
    Integrated => "integrated",
    Completed => "completed",
    Stopped => "stopped",
    Archived => "archived",
});

string_enum!(SessionStatus {
    Running => "running",
    WaitingBranchReview => "waiting_branch_review",
    WaitingDependency => "waiting_dependency",
    WaitingJudgment => "waiting_judgment",
    ExceptionPaused => "exception_paused",
    ManualPaused => "manual_paused",
    AwaitingMergeReview => "awaiting_merge_review",
    ReviewRejected => "review_rejected",
    Accepted => "accepted",
    Stopped => "stopped",
});

string_enum!(ReviewGateStatus {
    PendingAiReview => "pending_ai_review",
    PendingHumanReview => "pending_human_review",
    Accepted => "accepted",
    PartiallyAccepted => "partially_accepted",
    Rejected => "rejected",
    Abandoned => "abandoned",
    Withdrawn => "withdrawn",
});

string_enum!(ReviewDecisionKind {
    RecommendAccept => "recommend_accept",
    RecommendReject => "recommend_reject",
    Accept => "accept",
    PartialAccept => "partial_accept",
    Reject => "reject",
    Abandon => "abandon",
    Withdraw => "withdraw",
});

string_enum!(ContractRevisionStatus {
    AwaitingApproval => "awaiting_approval",
    Accepted => "accepted",
    Rejected => "rejected",
});

impl ProposalStatus {
    pub fn revise(self) -> AppResult<Self> {
        match self {
            Self::Draft | Self::AwaitingApproval => Ok(Self::Draft),
            _ => Err(invalid_transition(
                "BranchProposal",
                self.as_str(),
                "revise",
            )),
        }
    }

    pub fn submit(self) -> AppResult<Self> {
        match self {
            Self::Draft => Ok(Self::AwaitingApproval),
            _ => Err(invalid_transition(
                "BranchProposal",
                self.as_str(),
                "submit",
            )),
        }
    }

    pub fn approve(self, actor: GoalActor) -> AppResult<Self> {
        require_human(actor)?;
        match self {
            Self::AwaitingApproval => Ok(Self::Approved),
            _ => Err(invalid_transition(
                "BranchProposal",
                self.as_str(),
                "approve",
            )),
        }
    }

    pub fn cancel(self, actor: GoalActor) -> AppResult<Self> {
        require_human(actor)?;
        match self {
            Self::Draft | Self::AwaitingApproval => Ok(Self::Cancelled),
            _ => Err(invalid_transition(
                "BranchProposal",
                self.as_str(),
                "cancel",
            )),
        }
    }
}

impl GoalBranchStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Integrated | Self::Completed | Self::Stopped | Self::Archived
        )
    }

    pub fn ensure_can_start_session(self) -> AppResult<()> {
        match self {
            Self::Active => Ok(()),
            _ => Err(invalid_transition(
                "GoalBranch",
                self.as_str(),
                "start_session",
            )),
        }
    }

    pub fn archive(self, actor: GoalActor) -> AppResult<Self> {
        require_human(actor)?;
        if matches!(self, Self::Integrated | Self::Completed | Self::Stopped) {
            return Ok(Self::Archived);
        }
        Err(invalid_transition("GoalBranch", self.as_str(), "archive"))
    }
}

impl SessionStatus {
    pub const fn is_writable(self) -> bool {
        matches!(self, Self::Running)
    }

    pub const fn is_resumable_pause(self) -> bool {
        matches!(
            self,
            Self::WaitingBranchReview
                | Self::WaitingDependency
                | Self::WaitingJudgment
                | Self::ExceptionPaused
                | Self::ManualPaused
        )
    }

    pub fn propose_child(self) -> AppResult<Self> {
        self.running_exit(Self::WaitingBranchReview, "propose_child")
    }

    pub fn request_judgment(self) -> AppResult<Self> {
        self.running_exit(Self::WaitingJudgment, "request_judgment")
    }

    pub fn pause_exception(self) -> AppResult<Self> {
        self.running_exit(Self::ExceptionPaused, "pause_exception")
    }

    pub fn pause_manual(self, actor: GoalActor) -> AppResult<Self> {
        require_human(actor)?;
        match self {
            Self::Running
            | Self::WaitingBranchReview
            | Self::WaitingDependency
            | Self::WaitingJudgment
            | Self::ExceptionPaused => Ok(Self::ManualPaused),
            _ => Err(invalid_transition(
                "AgentSessionNode",
                self.as_str(),
                "pause_manual",
            )),
        }
    }

    pub fn resume(self, has_unresolved_attention: bool) -> AppResult<Self> {
        if !self.is_resumable_pause() {
            return Err(invalid_transition(
                "AgentSessionNode",
                self.as_str(),
                "resume",
            ));
        }
        if has_unresolved_attention {
            return Err(AppError::conflict(
                "unresolved_attention",
                "仍有必须解决的待处理事项，不能恢复 Session",
            ));
        }
        Ok(Self::Running)
    }

    pub fn propose_merge(self) -> AppResult<Self> {
        self.running_exit(Self::AwaitingMergeReview, "propose_merge")
    }

    pub fn mark_review_rejected(self) -> AppResult<Self> {
        match self {
            Self::AwaitingMergeReview => Ok(Self::ReviewRejected),
            _ => Err(invalid_transition(
                "AgentSessionNode",
                self.as_str(),
                "mark_review_rejected",
            )),
        }
    }

    pub fn mark_accepted(self) -> AppResult<Self> {
        match self {
            Self::AwaitingMergeReview => Ok(Self::Accepted),
            _ => Err(invalid_transition(
                "AgentSessionNode",
                self.as_str(),
                "mark_accepted",
            )),
        }
    }

    pub fn stop(self, actor: GoalActor) -> AppResult<Self> {
        require_human(actor)?;
        match self {
            Self::Running
            | Self::WaitingBranchReview
            | Self::WaitingDependency
            | Self::WaitingJudgment
            | Self::ExceptionPaused
            | Self::ManualPaused
            | Self::ReviewRejected => Ok(Self::Stopped),
            _ => Err(invalid_transition(
                "AgentSessionNode",
                self.as_str(),
                "stop",
            )),
        }
    }

    fn running_exit(self, next: Self, action: &'static str) -> AppResult<Self> {
        match self {
            Self::Running => Ok(next),
            _ => Err(invalid_transition(
                "AgentSessionNode",
                self.as_str(),
                action,
            )),
        }
    }
}

impl ReviewGateStatus {
    pub fn record_ai_review(
        self,
        actor: GoalActor,
        decision: ReviewDecisionKind,
    ) -> AppResult<Self> {
        if actor != GoalActor::ReviewAi {
            return Err(AppError::conflict(
                "independent_reviewer_required",
                "该步骤必须由独立审核 AI 执行",
            ));
        }
        if !matches!(
            decision,
            ReviewDecisionKind::RecommendAccept | ReviewDecisionKind::RecommendReject
        ) {
            return Err(AppError::bad_request(
                "invalid_review_decision",
                "独立审核 AI 只能建议接受或建议退回",
            ));
        }
        match self {
            Self::PendingAiReview => Ok(Self::PendingHumanReview),
            _ => Err(invalid_transition(
                "ReviewGate",
                self.as_str(),
                "record_ai_review",
            )),
        }
    }

    pub fn record_human_decision(
        self,
        actor: GoalActor,
        decision: ReviewDecisionKind,
    ) -> AppResult<Self> {
        require_human(actor)?;
        if self != Self::PendingHumanReview {
            return Err(invalid_transition(
                "ReviewGate",
                self.as_str(),
                "record_human_decision",
            ));
        }
        match decision {
            ReviewDecisionKind::Accept => Ok(Self::Accepted),
            ReviewDecisionKind::PartialAccept => Ok(Self::PartiallyAccepted),
            ReviewDecisionKind::Reject => Ok(Self::Rejected),
            ReviewDecisionKind::Abandon => Ok(Self::Abandoned),
            _ => Err(AppError::bad_request(
                "invalid_review_decision",
                "用户审核结论必须是接受、部分接受、退回或放弃",
            )),
        }
    }

    pub fn withdraw(self, actor: GoalActor) -> AppResult<Self> {
        if !matches!(actor, GoalActor::Agent | GoalActor::ReviewAi) {
            return Err(AppError::conflict(
                "invalid_actor",
                "只有工作 Agent 或审核 AI 可以撤回不可信候选",
            ));
        }
        match self {
            Self::PendingAiReview | Self::PendingHumanReview => Ok(Self::Withdrawn),
            _ => Err(invalid_transition("ReviewGate", self.as_str(), "withdraw")),
        }
    }
}

impl ContractRevisionStatus {
    pub fn decide(self, actor: GoalActor, accept: bool) -> AppResult<Self> {
        require_human(actor)?;
        if self != Self::AwaitingApproval {
            return Err(invalid_transition(
                "ContractRevision",
                self.as_str(),
                if accept { "accept" } else { "reject" },
            ));
        }
        Ok(if accept {
            Self::Accepted
        } else {
            Self::Rejected
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExplorationPolicy {
    #[serde(default = "default_exploration_mode")]
    pub mode: String,
    #[serde(default)]
    pub budgets: Vec<String>,
    #[serde(default)]
    pub candidate_outputs: Vec<String>,
    #[serde(default)]
    pub uncertainty_reduction: Vec<String>,
}

impl Default for ExplorationPolicy {
    fn default() -> Self {
        Self {
            mode: default_exploration_mode(),
            budgets: Vec::new(),
            candidate_outputs: Vec::new(),
            uncertainty_reduction: Vec::new(),
        }
    }
}

impl ExplorationPolicy {
    fn validate(mut self) -> AppResult<Self> {
        self.mode = required_text("目标模式", self.mode, 40)?;
        if !matches!(self.mode.as_str(), "delivery" | "exploration" | "hybrid") {
            return Err(AppError::bad_request(
                "invalid_exploration_mode",
                "目标模式必须是交付、探索或混合",
            ));
        }
        normalize_list("探索预算", &mut self.budgets)?;
        normalize_list("探索候选产出", &mut self.candidate_outputs)?;
        normalize_list("不确定性收敛方式", &mut self.uncertainty_reduction)?;
        Ok(self)
    }

    fn validate_for_approval(self) -> AppResult<Self> {
        let policy = self.validate()?;
        if matches!(policy.mode.as_str(), "exploration" | "hybrid") {
            if policy.budgets.is_empty() {
                return Err(AppError::bad_request(
                    "insufficient_exploration_contract",
                    "探索型目标至少要写明一项时间、资源、候选数量或判断边界",
                ));
            }
            if policy.candidate_outputs.is_empty() {
                return Err(AppError::bad_request(
                    "insufficient_exploration_contract",
                    "探索型目标至少要写明一种原型、实验或候选产出",
                ));
            }
            if policy.uncertainty_reduction.is_empty() {
                return Err(AppError::bad_request(
                    "insufficient_exploration_contract",
                    "探索型目标要说明如何判断不确定性已经减少",
                ));
            }
        }
        Ok(policy)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalContractDraft {
    pub desired_outcome: String,
    #[serde(default)]
    pub hard_constraints: Vec<String>,
    #[serde(default)]
    pub subjective_preferences: Vec<String>,
    #[serde(default)]
    pub unknowns: Vec<String>,
    #[serde(default)]
    pub non_goals: Vec<String>,
    #[serde(default)]
    pub validation_plan: Vec<String>,
    #[serde(default)]
    pub judgment_triggers: Vec<String>,
    #[serde(default)]
    pub stop_conditions: Vec<String>,
    #[serde(default)]
    pub expected_contributions: Vec<String>,
    #[serde(default)]
    pub exploration: ExplorationPolicy,
}

impl GoalContractDraft {
    pub fn validate(mut self) -> AppResult<Self> {
        self.desired_outcome = required_text("目标结果", self.desired_outcome, MAX_OUTCOME_CHARS)?;
        normalize_list("硬约束", &mut self.hard_constraints)?;
        normalize_list("主观偏好", &mut self.subjective_preferences)?;
        normalize_list("未知问题", &mut self.unknowns)?;
        normalize_list("不做事项", &mut self.non_goals)?;
        normalize_list("验证计划", &mut self.validation_plan)?;
        normalize_list("判断时机", &mut self.judgment_triggers)?;
        normalize_list("停止条件", &mut self.stop_conditions)?;
        normalize_list("期望贡献", &mut self.expected_contributions)?;
        self.exploration = self.exploration.validate()?;
        Ok(self)
    }

    pub fn validate_for_approval(self) -> AppResult<Self> {
        let contract = self.validate()?;
        if contract.stop_conditions.is_empty() {
            return Err(AppError::bad_request(
                "insufficient_goal_contract",
                "批准前至少要说明一个停止或完成条件",
            ));
        }
        if contract.validation_plan.is_empty() && contract.judgment_triggers.is_empty() {
            return Err(AppError::bad_request(
                "insufficient_goal_contract",
                "批准前至少要说明验证方法或何时请用户判断",
            ));
        }
        let exploration = contract.exploration.validate_for_approval()?;
        Ok(Self {
            exploration,
            ..contract
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchProposalRevisionDraft {
    pub why_needed: String,
    pub contract: GoalContractDraft,
    #[serde(default)]
    pub expected_contributions: Vec<String>,
    #[serde(default)]
    pub exploration_plan: Vec<String>,
    #[serde(default = "empty_object")]
    pub context_inheritance: Value,
    #[serde(default)]
    pub tool_requirements: Vec<String>,
    #[serde(default)]
    pub capability_policy: WorkspaceCapabilityPolicy,
    #[serde(default)]
    pub inferences: Vec<String>,
    pub revision_reason: Option<String>,
}

impl BranchProposalRevisionDraft {
    pub fn validate(mut self, for_approval: bool) -> AppResult<Self> {
        self.why_needed = required_text("分枝原因", self.why_needed, 4_000)?;
        self.contract = if for_approval {
            self.contract.validate_for_approval()?
        } else {
            self.contract.validate()?
        };
        normalize_list("期望回流贡献", &mut self.expected_contributions)?;
        normalize_list("探索计划", &mut self.exploration_plan)?;
        normalize_list("工具需求", &mut self.tool_requirements)?;
        self.capability_policy = self.capability_policy.normalize()?;
        normalize_list("AI 推断", &mut self.inferences)?;
        if !self.context_inheritance.is_object() {
            return Err(AppError::bad_request(
                "invalid_context_inheritance",
                "上下文继承说明必须是 JSON 对象",
            ));
        }
        self.revision_reason = optional_text("修订理由", self.revision_reason, 2_000)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractSourceAnnotation {
    pub field_path: String,
    pub source_kind: String,
    pub source_ref: Option<String>,
    pub note: String,
}

impl ContractSourceAnnotation {
    pub fn validate(mut self) -> AppResult<Self> {
        self.field_path = required_text("契约来源字段", self.field_path, 240)?;
        if !self.field_path.starts_with('/') {
            return Err(AppError::bad_request(
                "invalid_contract_source",
                "契约来源字段必须是以 / 开头的 JSON Pointer",
            ));
        }
        self.source_kind = required_text("契约来源类型", self.source_kind, 80)?;
        if !matches!(
            self.source_kind.as_str(),
            "human_input"
                | "agent_inference"
                | "external_source"
                | "inherited_contract"
                | "artifact"
                | "evidence"
        ) {
            return Err(AppError::bad_request(
                "invalid_contract_source",
                "未知的契约来源类型",
            ));
        }
        self.source_ref = optional_text("契约来源引用", self.source_ref, 4_000)?;
        if matches!(
            self.source_kind.as_str(),
            "external_source" | "artifact" | "evidence"
        ) && self.source_ref.is_none()
        {
            return Err(AppError::bad_request(
                "invalid_contract_source",
                "外部资料、Artifact 或 Evidence 来源必须提供可追溯引用",
            ));
        }
        self.note = required_text("契约来源说明", self.note, 4_000)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateSnapshot {
    pub contribution_ids: Vec<Uuid>,
    #[serde(default)]
    pub evidence_ids: Vec<Uuid>,
    pub contract_version_id: Uuid,
    pub git_base_commit: Option<String>,
    pub git_head_commit: Option<String>,
    pub git_dirty: bool,
    pub environment_fingerprint: Option<String>,
    #[serde(default)]
    pub test_evidence: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    pub self_check: String,
}

impl CandidateSnapshot {
    pub fn validate(mut self) -> AppResult<Self> {
        if self.contribution_ids.is_empty() {
            return Err(AppError::bad_request(
                "missing_contributions",
                "拟合并至少要冻结一项 Contribution",
            ));
        }
        if self.contribution_ids.len() > 500 {
            return Err(AppError::bad_request(
                "too_many_contributions",
                "一次拟合并包含的 Contribution 过多",
            ));
        }
        self.contribution_ids.sort_unstable();
        let original_len = self.contribution_ids.len();
        self.contribution_ids.dedup();
        if self.contribution_ids.len() != original_len {
            return Err(AppError::bad_request(
                "duplicate_contribution",
                "候选中不能重复引用同一 Contribution",
            ));
        }
        if self.evidence_ids.len() > 1_000 {
            return Err(AppError::bad_request(
                "too_many_evidence",
                "一次拟合并包含的 Evidence 过多",
            ));
        }
        self.evidence_ids.sort_unstable();
        let original_len = self.evidence_ids.len();
        self.evidence_ids.dedup();
        if self.evidence_ids.len() != original_len {
            return Err(AppError::bad_request(
                "duplicate_evidence",
                "候选中不能重复引用同一 Evidence",
            ));
        }
        self.git_base_commit = optional_text("Git 基线", self.git_base_commit, 200)?;
        self.git_head_commit = optional_text("Git 头", self.git_head_commit, 200)?;
        self.environment_fingerprint = self
            .environment_fingerprint
            .map(|fingerprint| validate_sha256_id("环境指纹", fingerprint))
            .transpose()?;
        normalize_list("测试证据", &mut self.test_evidence)?;
        normalize_list("已知风险", &mut self.risks)?;
        self.self_check = required_text("Agent 自查", self.self_check, 8_000)?;
        Ok(self)
    }

    pub fn fingerprint(&self) -> AppResult<String> {
        canonical_json_sha256(self)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandReceiptIdentity {
    pub command_kind: String,
    pub input_hash: String,
}

impl CommandReceiptIdentity {
    pub fn from_input<T: Serialize>(command_kind: &str, input: &T) -> AppResult<Self> {
        Ok(Self {
            command_kind: required_text("命令类型", command_kind.to_owned(), 120)?,
            input_hash: canonical_json_sha256(input)?,
        })
    }

    pub fn ensure_replay_matches(&self, requested: &Self) -> AppResult<()> {
        if self == requested {
            return Ok(());
        }
        Err(AppError::conflict(
            "idempotency_conflict",
            "同一 clientRequestId 已用于不同的命令或输入",
        ))
    }
}

pub fn validate_new_running_session(
    branch_status: GoalBranchStatus,
    previous_status: Option<SessionStatus>,
    running_writer_exists: bool,
    pending_review_exists: bool,
) -> AppResult<()> {
    branch_status.ensure_can_start_session()?;
    if running_writer_exists {
        return Err(AppError::conflict(
            "branch_writer_exists",
            "该目标枝干已经有一个可写 Session",
        ));
    }
    if pending_review_exists {
        return Err(AppError::conflict(
            "pending_review_exists",
            "候选仍在审核，不能开始新的可写 Session",
        ));
    }
    if let Some(previous_status) = previous_status
        && !matches!(
            previous_status,
            SessionStatus::ReviewRejected | SessionStatus::Stopped
        )
    {
        return Err(invalid_transition(
            "AgentSessionNode",
            previous_status.as_str(),
            "start_next_session",
        ));
    }
    Ok(())
}

pub fn require_human(actor: GoalActor) -> AppResult<()> {
    if actor == GoalActor::Human {
        return Ok(());
    }
    Err(AppError::conflict(
        "human_authority_required",
        "该决定必须由用户确认",
    ))
}

pub fn canonical_json_sha256<T: Serialize>(value: &T) -> AppResult<String> {
    // Round-tripping through Value makes struct fields and object keys share the same sorted-map
    // representation, so hashing a typed value and its persisted JSON produces one fingerprint.
    let canonical_value = serde_json::to_value(value)?;
    let encoded = serde_json::to_vec(&canonical_value)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(encoded))))
}

pub fn validate_sha256_id(label: &str, value: String) -> AppResult<String> {
    let value = value.trim().to_ascii_lowercase();
    let Some(hex_value) = value.strip_prefix("sha256:") else {
        return Err(AppError::bad_request(
            "invalid_digest",
            format!("{label}必须使用 sha256 摘要"),
        ));
    };
    if hex_value.len() != 64 || !hex_value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::bad_request(
            "invalid_digest",
            format!("{label}不是有效的 SHA-256 摘要"),
        ));
    }
    Ok(value)
}

fn normalize_list(label: &str, items: &mut Vec<String>) -> AppResult<()> {
    if items.len() > MAX_LIST_ITEMS {
        return Err(AppError::bad_request(
            "too_many_items",
            format!("{label}条目过多"),
        ));
    }
    for item in items.iter_mut() {
        *item = required_text(label, std::mem::take(item), MAX_ITEM_CHARS)?;
    }
    items.sort();
    items.dedup();
    Ok(())
}

fn required_text(label: &str, value: String, max: usize) -> AppResult<String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::bad_request(
            "invalid_input",
            format!("{label}不能为空"),
        ));
    }
    if value.chars().count() > max {
        return Err(AppError::bad_request(
            "invalid_input",
            format!("{label}过长"),
        ));
    }
    Ok(value)
}

fn optional_text(label: &str, value: Option<String>, max: usize) -> AppResult<Option<String>> {
    value
        .map(|value| {
            let value = value.trim().to_owned();
            if value.is_empty() {
                Ok(None)
            } else if value.chars().count() > max {
                Err(AppError::bad_request(
                    "invalid_input",
                    format!("{label}过长"),
                ))
            } else {
                Ok(Some(value))
            }
        })
        .unwrap_or(Ok(None))
}

fn empty_object() -> Value {
    serde_json::json!({})
}

fn default_exploration_mode() -> String {
    "delivery".into()
}

fn invalid_transition(entity: &str, state: &str, action: &str) -> AppError {
    AppError::conflict(
        "invalid_state_transition",
        format!("{entity} 当前状态 {state} 不允许执行 {action}"),
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    fn exploratory_contract() -> GoalContractDraft {
        GoalContractDraft {
            desired_outcome: "做出多个可体验原型，由用户判断方向".into(),
            hard_constraints: vec!["不公开部署".into()],
            subjective_preferences: vec!["接近旧 Fudian 的克制纸张感".into()],
            unknowns: vec!["图和工作台的最佳比例".into()],
            non_goals: vec!["本轮不锁定最终布局".into()],
            validation_plan: vec!["在 390px 与桌面宽度运行原型".into()],
            judgment_triggers: vec!["出现两种都可行的布局时找用户体验".into()],
            stop_conditions: vec!["两个真实流程原型可操作后暂停评审".into()],
            expected_contributions: vec!["原型与体验证据".into()],
            exploration: ExplorationPolicy {
                mode: "exploration".into(),
                budgets: vec!["最多形成两个可操作候选后请求判断".into()],
                candidate_outputs: vec!["桌面与手机交互原型".into()],
                uncertainty_reduction: vec!["用户能明确排除至少一个方向".into()],
            },
        }
    }

    #[test]
    fn exploratory_contract_keeps_unknowns_without_fake_precision() {
        let contract = exploratory_contract().validate_for_approval().unwrap();
        assert_eq!(contract.unknowns, vec!["图和工作台的最佳比例"]);
        assert_eq!(contract.subjective_preferences.len(), 1);
    }

    #[test]
    fn exploratory_contract_requires_budget_candidates_and_uncertainty_reduction() {
        let mut contract = exploratory_contract();
        contract.exploration.budgets.clear();
        assert_eq!(
            contract.validate_for_approval().unwrap_err().code(),
            "insufficient_exploration_contract"
        );

        let mut contract = exploratory_contract();
        contract.exploration.candidate_outputs.clear();
        assert_eq!(
            contract.validate_for_approval().unwrap_err().code(),
            "insufficient_exploration_contract"
        );

        let mut contract = exploratory_contract();
        contract.exploration.uncertainty_reduction.clear();
        assert_eq!(
            contract.validate_for_approval().unwrap_err().code(),
            "insufficient_exploration_contract"
        );
    }

    #[test]
    fn approval_requires_a_stop_and_validation_boundary() {
        let mut contract = exploratory_contract();
        contract.stop_conditions.clear();
        let error = contract.validate_for_approval().unwrap_err();
        assert_eq!(error.code(), "insufficient_goal_contract");

        let mut contract = exploratory_contract();
        contract.validation_plan.clear();
        contract.judgment_triggers.clear();
        let error = contract.validate_for_approval().unwrap_err();
        assert_eq!(error.code(), "insufficient_goal_contract");
    }

    #[test]
    fn only_an_awaiting_proposal_can_be_human_approved() {
        assert_eq!(
            ProposalStatus::AwaitingApproval
                .approve(GoalActor::Human)
                .unwrap(),
            ProposalStatus::Approved
        );
        assert_eq!(
            ProposalStatus::AwaitingApproval
                .approve(GoalActor::Agent)
                .unwrap_err()
                .code(),
            "human_authority_required"
        );
        assert_eq!(
            ProposalStatus::Draft
                .approve(GoalActor::Human)
                .unwrap_err()
                .code(),
            "invalid_state_transition"
        );
    }

    #[test]
    fn a_running_session_has_explicit_pause_exits() {
        assert_eq!(
            SessionStatus::Running.propose_child().unwrap(),
            SessionStatus::WaitingBranchReview
        );
        assert_eq!(
            SessionStatus::Running.request_judgment().unwrap(),
            SessionStatus::WaitingJudgment
        );
        assert_eq!(
            SessionStatus::Running.pause_exception().unwrap(),
            SessionStatus::ExceptionPaused
        );
        assert!(!SessionStatus::AwaitingMergeReview.is_writable());
    }

    #[test]
    fn unresolved_attention_prevents_resume() {
        let error = SessionStatus::WaitingJudgment.resume(true).unwrap_err();
        assert_eq!(error.code(), "unresolved_attention");
        assert_eq!(
            SessionStatus::WaitingJudgment.resume(false).unwrap(),
            SessionStatus::Running
        );
    }

    #[test]
    fn review_ai_advises_but_only_human_decides() {
        let gate = ReviewGateStatus::PendingAiReview
            .record_ai_review(GoalActor::ReviewAi, ReviewDecisionKind::RecommendAccept)
            .unwrap();
        assert_eq!(gate, ReviewGateStatus::PendingHumanReview);
        assert_eq!(
            gate.record_human_decision(GoalActor::Agent, ReviewDecisionKind::Accept)
                .unwrap_err()
                .code(),
            "human_authority_required"
        );
        assert_eq!(
            gate.record_human_decision(GoalActor::Human, ReviewDecisionKind::Reject)
                .unwrap(),
            ReviewGateStatus::Rejected
        );
    }

    #[test]
    fn rejected_session_and_no_writer_are_required_for_next_round() {
        validate_new_running_session(
            GoalBranchStatus::Active,
            Some(SessionStatus::ReviewRejected),
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            validate_new_running_session(
                GoalBranchStatus::Active,
                Some(SessionStatus::ReviewRejected),
                true,
                false,
            )
            .unwrap_err()
            .code(),
            "branch_writer_exists"
        );
        assert_eq!(
            validate_new_running_session(
                GoalBranchStatus::Active,
                Some(SessionStatus::Accepted),
                false,
                false,
            )
            .unwrap_err()
            .code(),
            "invalid_state_transition"
        );
    }

    #[test]
    fn candidate_snapshot_is_sorted_frozen_and_hashed() {
        let first = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let second = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();
        let snapshot = CandidateSnapshot {
            contribution_ids: vec![second, first],
            evidence_ids: vec![second],
            contract_version_id: first,
            git_base_commit: Some("abc".into()),
            git_head_commit: Some("def".into()),
            git_dirty: false,
            environment_fingerprint: Some(format!("sha256:{}", "a".repeat(64))),
            test_evidence: vec!["cargo test".into()],
            risks: vec![],
            self_check: "逐条检查契约，无已知遗漏".into(),
        }
        .validate()
        .unwrap();
        assert_eq!(snapshot.contribution_ids, vec![first, second]);
        assert_eq!(snapshot.evidence_ids, vec![second]);
        assert_eq!(snapshot.fingerprint().unwrap().len(), 71);
    }

    #[test]
    fn an_idempotency_key_cannot_change_meaning() {
        let stored = CommandReceiptIdentity::from_input(
            "proposal.create",
            &json!({"title": "目标 A", "unknowns": ["x"]}),
        )
        .unwrap();
        let replay = CommandReceiptIdentity::from_input(
            "proposal.create",
            &json!({"unknowns": ["x"], "title": "目标 A"}),
        )
        .unwrap();
        stored.ensure_replay_matches(&replay).unwrap();

        let changed = CommandReceiptIdentity::from_input(
            "proposal.create",
            &json!({"title": "目标 B", "unknowns": ["x"]}),
        )
        .unwrap();
        assert_eq!(
            stored.ensure_replay_matches(&changed).unwrap_err().code(),
            "idempotency_conflict"
        );
    }

    #[test]
    fn contract_revision_requires_human_decision_and_explicit_source() {
        assert_eq!(
            ContractRevisionStatus::AwaitingApproval
                .decide(GoalActor::Agent, true)
                .unwrap_err()
                .code(),
            "human_authority_required"
        );
        assert_eq!(
            ContractRevisionStatus::AwaitingApproval
                .decide(GoalActor::Human, true)
                .unwrap(),
            ContractRevisionStatus::Accepted
        );
        assert_eq!(
            ContractSourceAnnotation {
                field_path: "/unknowns".into(),
                source_kind: "human_input".into(),
                source_ref: None,
                note: "用户在体验原型后补充".into(),
            }
            .validate()
            .unwrap()
            .field_path,
            "/unknowns"
        );
    }

    #[test]
    fn only_human_can_stop_or_archive_a_goal_branch() {
        assert_eq!(
            SessionStatus::Running
                .stop(GoalActor::Agent)
                .unwrap_err()
                .code(),
            "human_authority_required"
        );
        assert_eq!(
            SessionStatus::WaitingDependency
                .stop(GoalActor::Human)
                .unwrap(),
            SessionStatus::Stopped
        );
        assert_eq!(
            GoalBranchStatus::Completed
                .archive(GoalActor::Human)
                .unwrap(),
            GoalBranchStatus::Archived
        );
    }
}
