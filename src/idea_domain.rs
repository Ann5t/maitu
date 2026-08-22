use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    goal_domain::{BranchProposalRevisionDraft, CommandReceiptIdentity},
};

const MAX_IDEA_BODY: usize = 20_000;
const MAX_TITLE: usize = 240;
const MAX_LIST_ITEMS: usize = 100;
const MAX_LIST_ITEM: usize = 2_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaCommandRequest {
    pub client_request_id: Uuid,
    pub action: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachIdeaSourceQuery {
    pub client_request_id: Uuid,
    pub expected_revision: i32,
    pub filename: String,
    pub declared_media_type: Option<String>,
    pub expected_sha256: Option<String>,
    pub note: Option<String>,
}

impl IdeaCommandRequest {
    pub fn identity(&self, subject_id: Option<Uuid>) -> AppResult<CommandReceiptIdentity> {
        CommandReceiptIdentity::from_input(
            &self.action,
            &serde_json::json!({ "subjectId": subject_id, "payload": self.payload }),
        )
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaRevisionDraft {
    #[serde(default)]
    pub title: String,
    pub body: String,
    #[serde(default = "default_source_kind")]
    pub source_kind: String,
    pub source_ref: Option<String>,
    pub revision_reason: Option<String>,
}

impl IdeaRevisionDraft {
    pub fn validate(mut self, is_revision: bool) -> AppResult<Self> {
        self.body = required_text("想法内容", self.body, MAX_IDEA_BODY)?;
        self.title = self.title.trim().to_owned();
        if self.title.is_empty() {
            self.title = derive_title(&self.body, 42);
        }
        self.title = required_text("想法标题", self.title, MAX_TITLE)?;
        self.source_kind = required_text("来源类型", self.source_kind, 40)?;
        if !matches!(
            self.source_kind.as_str(),
            "text" | "file" | "image" | "audio" | "external"
        ) {
            return Err(AppError::bad_request(
                "invalid_idea_source_kind",
                "想法来源必须是 text、file、image、audio 或 external",
            ));
        }
        self.source_ref = optional_text("来源引用", self.source_ref, 2_000)?;
        if self.source_kind != "text" && self.source_ref.is_none() {
            return Err(AppError::bad_request(
                "missing_idea_source_ref",
                "文件、图片、语音或外部来源必须保留可追溯引用",
            ));
        }
        self.revision_reason = optional_text("修订理由", self.revision_reason, 2_000)?;
        if is_revision && self.revision_reason.is_none() {
            return Err(AppError::bad_request(
                "missing_revision_reason",
                "修订想法时要简短说明这次改变的原因",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProposalIdeaSource {
    pub idea_id: Uuid,
    pub idea_revision: i32,
    pub role: String,
    #[serde(default)]
    pub rationale: String,
}

impl ProjectProposalIdeaSource {
    fn validate(mut self) -> AppResult<Self> {
        if self.idea_revision < 1 {
            return Err(AppError::bad_request(
                "invalid_idea_revision",
                "ProjectProposal 必须引用一个存在的想法版本",
            ));
        }
        self.role = required_text("想法角色", self.role, 40)?;
        if !matches!(
            self.role.as_str(),
            "source" | "supporting" | "constraint" | "omitted"
        ) {
            return Err(AppError::bad_request(
                "invalid_idea_role",
                "想法角色必须是 source、supporting、constraint 或 omitted",
            ));
        }
        self.rationale = trimmed_text("采用说明", self.rationale, 4_000)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProposalRevisionDraft {
    pub title: String,
    pub project_intent: String,
    pub why_now: String,
    pub root_goal: BranchProposalRevisionDraft,
    #[serde(default)]
    pub retained_notes: Vec<String>,
    #[serde(default)]
    pub omitted_notes: Vec<String>,
    pub sources: Vec<ProjectProposalIdeaSource>,
    pub revision_reason: Option<String>,
}

impl ProjectProposalRevisionDraft {
    pub fn validate(mut self, for_approval: bool, is_revision: bool) -> AppResult<Self> {
        self.title = required_text("项目提案标题", self.title, MAX_TITLE)?;
        self.project_intent = required_text("项目意图", self.project_intent, 4_000)?;
        if self.project_intent.chars().count() < 3 {
            return Err(AppError::bad_request(
                "invalid_project_intent",
                "至少用一句话说明要推进的项目",
            ));
        }
        self.why_now = required_text("现在立项的理由", self.why_now, 4_000)?;
        self.root_goal = self.root_goal.validate(for_approval)?;
        normalize_list("保留内容", &mut self.retained_notes)?;
        normalize_list("暂不采用内容", &mut self.omitted_notes)?;
        if self.sources.is_empty() {
            return Err(AppError::bad_request(
                "missing_idea_sources",
                "ProjectProposal 至少要引用一个想法版本",
            ));
        }
        if self.sources.len() > MAX_LIST_ITEMS {
            return Err(AppError::bad_request(
                "too_many_idea_sources",
                "一个 ProjectProposal 引用的想法过多，请先分组",
            ));
        }
        let mut seen = HashSet::new();
        self.sources = self
            .sources
            .into_iter()
            .map(ProjectProposalIdeaSource::validate)
            .collect::<AppResult<Vec<_>>>()?;
        if self.sources.iter().any(|item| !seen.insert(item.idea_id)) {
            return Err(AppError::bad_request(
                "duplicate_idea_source",
                "同一个想法不能在同一提案版本里重复出现",
            ));
        }
        if !self.sources.iter().any(|item| item.role == "source") {
            return Err(AppError::bad_request(
                "missing_primary_idea_source",
                "ProjectProposal 至少需要一个 source 角色的起始想法",
            ));
        }
        self.revision_reason = optional_text("修订理由", self.revision_reason, 2_000)?;
        if is_revision && self.revision_reason.is_none() {
            return Err(AppError::bad_request(
                "missing_revision_reason",
                "修订 ProjectProposal 时要说明改变原因",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectProposalStatus {
    Draft,
    AwaitingApproval,
    Approved,
    Rejected,
    Cancelled,
}

impl ProjectProposalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn submit(self) -> AppResult<Self> {
        match self {
            Self::Draft => Ok(Self::AwaitingApproval),
            _ => Err(invalid_proposal_transition(self, "submit")),
        }
    }

    pub fn revise(self) -> AppResult<Self> {
        match self {
            Self::Draft | Self::AwaitingApproval => Ok(Self::Draft),
            _ => Err(invalid_proposal_transition(self, "revise")),
        }
    }

    pub fn approve(self) -> AppResult<Self> {
        match self {
            Self::AwaitingApproval => Ok(Self::Approved),
            _ => Err(invalid_proposal_transition(self, "approve")),
        }
    }

    pub fn reject(self) -> AppResult<Self> {
        match self {
            Self::AwaitingApproval => Ok(Self::Rejected),
            _ => Err(invalid_proposal_transition(self, "reject")),
        }
    }

    pub fn cancel(self) -> AppResult<Self> {
        match self {
            Self::Draft | Self::AwaitingApproval => Ok(Self::Cancelled),
            _ => Err(invalid_proposal_transition(self, "cancel")),
        }
    }
}

impl TryFrom<&str> for ProjectProposalStatus {
    type Error = AppError;

    fn try_from(value: &str) -> AppResult<Self> {
        match value {
            "draft" => Ok(Self::Draft),
            "awaiting_approval" => Ok(Self::AwaitingApproval),
            "approved" => Ok(Self::Approved),
            "rejected" => Ok(Self::Rejected),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(AppError::bad_request(
                "invalid_project_proposal_state",
                "ProjectProposal 状态无效",
            )),
        }
    }
}

pub fn validate_idea_relation(value: String) -> AppResult<String> {
    let relation = required_text("想法关系", value, 40)?;
    if matches!(
        relation.as_str(),
        "related" | "supports" | "contradicts" | "depends_on" | "duplicates"
    ) {
        Ok(relation)
    } else {
        Err(AppError::bad_request(
            "invalid_idea_relation",
            "想法关系必须是 related、supports、contradicts、depends_on 或 duplicates",
        ))
    }
}

pub fn clean_required(label: &str, value: String, max: usize) -> AppResult<String> {
    required_text(label, value, max)
}

fn invalid_proposal_transition(state: ProjectProposalStatus, action: &str) -> AppError {
    AppError::conflict(
        "invalid_project_proposal_transition",
        format!(
            "ProjectProposal 处于 {} 时不能执行 {action}",
            state.as_str()
        ),
    )
}

fn default_source_kind() -> String {
    "text".into()
}

fn derive_title(body: &str, max: usize) -> String {
    let first = body
        .split(['\n', '。', '！', '？', '!', '?'])
        .next()
        .unwrap_or(body)
        .trim();
    let mut title = first.chars().take(max).collect::<String>();
    if first.chars().count() > max {
        title.push('…');
    }
    title
}

fn normalize_list(label: &str, items: &mut [String]) -> AppResult<()> {
    if items.len() > MAX_LIST_ITEMS {
        return Err(AppError::bad_request(
            "too_many_items",
            format!("{label}条目过多"),
        ));
    }
    for item in items.iter_mut() {
        *item = required_text(label, std::mem::take(item), MAX_LIST_ITEM)?;
    }
    Ok(())
}

fn required_text(label: &str, value: String, max: usize) -> AppResult<String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::bad_request(
            "missing_required_text",
            format!("{label}不能为空"),
        ));
    }
    if value.chars().count() > max {
        return Err(AppError::bad_request(
            "text_too_long",
            format!("{label}不能超过 {max} 个字符"),
        ));
    }
    Ok(value)
}

fn trimmed_text(label: &str, value: String, max: usize) -> AppResult<String> {
    let value = value.trim().to_owned();
    if value.chars().count() > max {
        return Err(AppError::bad_request(
            "text_too_long",
            format!("{label}不能超过 {max} 个字符"),
        ));
    }
    Ok(value)
}

fn optional_text(label: &str, value: Option<String>, max: usize) -> AppResult<Option<String>> {
    value
        .map(|item| trimmed_text(label, item, max))
        .transpose()
        .map(|item| item.filter(|text| !text.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal_domain::{ExplorationPolicy, GoalContractDraft};

    fn proposal() -> ProjectProposalRevisionDraft {
        ProjectProposalRevisionDraft {
            title: "把想法推进成工具".into(),
            project_intent: "开发一个可验证的工具".into(),
            why_now: "关键技术条件已可验证".into(),
            root_goal: BranchProposalRevisionDraft {
                why_needed: "需要一个正式目标枝干".into(),
                contract: GoalContractDraft {
                    desired_outcome: "完成可运行工具".into(),
                    hard_constraints: vec![],
                    subjective_preferences: vec![],
                    unknowns: vec!["最终交互手感".into()],
                    non_goals: vec![],
                    validation_plan: vec!["运行端到端测试".into()],
                    judgment_triggers: vec!["可操作原型完成后".into()],
                    stop_conditions: vec!["用户接受或明确停止".into()],
                    expected_contributions: vec!["实现和证据".into()],
                    exploration: ExplorationPolicy::default(),
                },
                expected_contributions: vec!["实现和证据".into()],
                exploration_plan: vec![],
                context_inheritance: serde_json::json!({}),
                tool_requirements: vec![],
                inferences: vec![],
                revision_reason: None,
            },
            retained_notes: vec![],
            omitted_notes: vec!["暂不决定最终布局".into()],
            sources: vec![ProjectProposalIdeaSource {
                idea_id: Uuid::new_v4(),
                idea_revision: 1,
                role: "source".into(),
                rationale: "起始想法".into(),
            }],
            revision_reason: None,
        }
    }

    #[test]
    fn idea_can_start_from_one_sentence_without_a_manual_title() {
        let draft = IdeaRevisionDraft {
            title: String::new(),
            body: "我想在手机上随时推进科研项目。后续细节还不知道。".into(),
            source_kind: "text".into(),
            source_ref: None,
            revision_reason: None,
        }
        .validate(false)
        .unwrap();
        assert_eq!(draft.title, "我想在手机上随时推进科研项目");
    }

    #[test]
    fn approval_allows_honest_unknowns_but_requires_validation_and_stop() {
        assert!(proposal().validate(true, false).is_ok());
        let mut missing = proposal();
        missing.root_goal.contract.validation_plan.clear();
        missing.root_goal.contract.judgment_triggers.clear();
        assert!(missing.validate(true, false).is_err());
    }

    #[test]
    fn proposal_state_requires_review_before_approval() {
        assert!(ProjectProposalStatus::Draft.approve().is_err());
        assert_eq!(
            ProjectProposalStatus::Draft.submit().unwrap(),
            ProjectProposalStatus::AwaitingApproval
        );
    }
}
