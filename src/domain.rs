use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub const NODE_OUTCOMES: &[&str] = &[
    "open",
    "useful",
    "refuted",
    "mixed",
    "blocked",
    "inconclusive",
];
pub const CONTRIBUTION_KINDS: &[&str] = &[
    "artifact",
    "finding",
    "evidence",
    "decision",
    "condition",
    "other",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntakeContradiction {
    pub code: String,
    pub question: String,
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProjectIntake {
    pub intent: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeConfirmation {
    pub completion_evidence: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ProjectActionRequest {
    pub action: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GraphActionRequest {
    pub action: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateBranchInput {
    pub client_request_id: Uuid,
    pub from_node_id: Uuid,
    pub name: String,
    pub purpose: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributionInput {
    pub kind: String,
    pub title: String,
    pub body: String,
    pub reference_uri: Option<String>,
    pub scope: Option<String>,
    pub reopen_when: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendProgressInput {
    pub client_request_id: Uuid,
    pub branch_id: Uuid,
    pub title: String,
    pub summary: String,
    pub outcome: String,
    pub contribution: ContributionInput,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrateBranchInput {
    pub client_request_id: Uuid,
    pub source_branch_id: Uuid,
    pub summary: String,
    pub accepted_contribution_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParkBranchInput {
    pub client_request_id: Uuid,
    pub branch_id: Uuid,
    pub reason: String,
    pub reopen_when: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutcomeContractDraft {
    pub desired_outcome: String,
    pub success_evidence: Vec<String>,
    pub constraints: Vec<String>,
    pub non_goals: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftedProject {
    pub title: String,
    pub intent: String,
    pub outcome_contract: OutcomeContractDraft,
    pub contradictions: Vec<IntakeContradiction>,
    pub confirmation_question: String,
}

pub fn normalize_intent(raw: &str) -> AppResult<String> {
    let intent = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = intent.chars().count();
    if length < 3 {
        return Err(AppError::bad_request(
            "invalid_intent",
            "至少用一句话描述你想推进的事情",
        ));
    }
    if length > 4_000 {
        return Err(AppError::bad_request(
            "invalid_intent",
            "首次描述请控制在 4000 字以内，材料可以随后添加",
        ));
    }
    Ok(intent)
}

pub fn draft_project_from_intent(raw: &str) -> AppResult<DraftedProject> {
    let intent = normalize_intent(raw)?;
    let first_statement = intent
        .split(['。', '！', '？', '!', '?', '\n'])
        .next()
        .unwrap_or(&intent)
        .trim();
    let cleaned = strip_leading_intent_words(first_statement).trim();
    let candidate = if cleaned.is_empty() {
        first_statement
    } else {
        cleaned
    };
    let title = truncate_chars(candidate, 36);

    Ok(DraftedProject {
        title,
        intent: intent.clone(),
        outcome_contract: OutcomeContractDraft {
            desired_outcome: intent,
            success_evidence: vec!["由用户确认一个能够在项目外部观察或检查的完成结果".into()],
            constraints: vec![
                "公开发布、正式提交、花费资金或联系外部人员前必须由用户批准".into(),
                "正式产物必须保留来源、版本和验收依据".into(),
            ],
            non_goals: vec!["不以生成文字的数量或 AI 自评分代替真实结果".into()],
        },
        contradictions: Vec::new(),
        confirmation_question: "这个项目在什么现实结果出现时，才算真正结束？".into(),
    })
}

fn strip_leading_intent_words(input: &str) -> &str {
    const PREFIXES: &[&str] = &[
        "请帮我",
        "我希望",
        "我想",
        "我要",
        "希望",
        "计划",
        "打算",
        "帮我",
    ];
    PREFIXES
        .iter()
        .find_map(|prefix| input.strip_prefix(prefix))
        .unwrap_or(input)
}

fn truncate_chars(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_owned();
    }
    let mut result = input
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>();
    result.push('…');
    result
}

pub fn create_project_brief_markdown(
    title: &str,
    intent: &str,
    contract: &OutcomeContractDraft,
) -> String {
    let bullets = |items: &[String]| {
        items
            .iter()
            .map(|item| format!("- {item}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "# {title}\n\n\
         > 这是由浮点根据首次描述生成的项目启动说明。它是待审阅产物，不代替外部事实。\n\n\
         ## 原始意图\n\n{intent}\n\n\
         ## 预期成果\n\n{}\n\n\
         ## 成功证据\n\n{}\n\n\
         ## 硬约束\n\n{}\n\n\
         ## 明确不做\n\n{}\n\n\
         ## 第一项推进动作\n\n{}\n",
        contract.desired_outcome,
        bullets(&contract.success_evidence),
        bullets(&contract.constraints),
        bullets(&contract.non_goals),
        propose_first_progress_action(),
    )
}

pub fn propose_first_progress_action() -> &'static str {
    "从当前目标中选出一个最关键的不确定点或未完成条件，完成一项能够产生新证据或正式产物的最小行动。"
}

pub fn validate_outcome_confirmation(input: OutcomeConfirmation) -> AppResult<String> {
    required_text("完成标准", input.completion_evidence, 2_000)
}

impl CreateBranchInput {
    pub fn validate(mut self) -> AppResult<Self> {
        self.name = required_text("分支名称", self.name, 80)?;
        self.purpose = required_text("想弄清或做成的事情", self.purpose, 2_000)?;
        Ok(self)
    }
}

impl AppendProgressInput {
    pub fn validate(mut self) -> AppResult<Self> {
        self.title = required_text("节点标题", self.title, 120)?;
        self.summary = required_text("实际发生的事情", self.summary, 8_000)?;
        if !NODE_OUTCOMES.contains(&self.outcome.as_str()) {
            return Err(AppError::bad_request("invalid_outcome", "未知的结果判断"));
        }
        self.contribution = self.contribution.validate()?;
        Ok(self)
    }
}

impl ContributionInput {
    pub fn validate(mut self) -> AppResult<Self> {
        if !CONTRIBUTION_KINDS.contains(&self.kind.as_str()) {
            return Err(AppError::bad_request(
                "invalid_contribution_kind",
                "未知的产出类型",
            ));
        }
        self.title = required_text("产出标题", self.title, 160)?;
        self.body = required_text("产出内容", self.body, 8_000)?;
        self.reference_uri = optional_text("引用地址", self.reference_uri, 4_000)?;
        if let Some(uri) = &self.reference_uri
            && !(uri.starts_with("http://") || uri.starts_with("https://"))
        {
            return Err(AppError::bad_request(
                "invalid_reference_uri",
                "引用地址必须以 http:// 或 https:// 开头",
            ));
        }
        self.scope = optional_text("适用范围", self.scope, 2_000)?;
        self.reopen_when = optional_text("重试条件", self.reopen_when, 2_000)?;
        Ok(self)
    }
}

impl IntegrateBranchInput {
    pub fn validate(mut self) -> AppResult<Self> {
        self.summary = required_text("带回主线的内容", self.summary, 8_000)?;
        if self.accepted_contribution_ids.is_empty() {
            return Err(AppError::bad_request(
                "missing_contributions",
                "至少选择一项要带回主线的产出",
            ));
        }
        if self.accepted_contribution_ids.len() > 200 {
            return Err(AppError::bad_request(
                "too_many_contributions",
                "一次合流选择的产出过多",
            ));
        }
        self.accepted_contribution_ids.sort_unstable();
        let old_len = self.accepted_contribution_ids.len();
        self.accepted_contribution_ids.dedup();
        if self.accepted_contribution_ids.len() != old_len {
            return Err(AppError::bad_request(
                "duplicate_contribution",
                "不能重复选择同一项产出",
            ));
        }
        Ok(self)
    }
}

impl ParkBranchInput {
    pub fn validate(mut self) -> AppResult<Self> {
        self.reason = required_text("暂停原因", self.reason, 4_000)?;
        self.reopen_when = optional_text("重开条件", self.reopen_when, 2_000)?;
        Ok(self)
    }
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
            if value.chars().count() > max {
                Err(AppError::bad_request(
                    "invalid_input",
                    format!("{label}过长"),
                ))
            } else if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value))
            }
        })
        .unwrap_or(Ok(None))
}

pub fn can_append_to_branch(status: &str) -> bool {
    matches!(status, "active" | "waiting")
}

pub fn can_integrate_branch(status: &str, is_main: bool) -> bool {
    !is_main && matches!(status, "active" | "waiting")
}

pub fn state_label(value: &str) -> &str {
    match value {
        "shaping" => "塑形中",
        "active" => "推进中",
        "waiting" => "等待中",
        "paused" => "已暂停",
        "completed" => "已完成",
        "stopped" => "已停止",
        "archived" => "已归档",
        _ => value,
    }
}

pub fn branch_status_label(value: &str) -> &str {
    match value {
        "active" => "探索中",
        "waiting" => "等待条件",
        "integrated" => "已带回主线",
        "closed" => "已结束",
        _ => value,
    }
}

pub fn node_kind_label(value: &str) -> &str {
    match value {
        "origin" => "起点",
        "work" => "推进",
        "result" => "结果",
        "decision" => "决定",
        "merge" => "合流",
        _ => value,
    }
}

pub fn outcome_label(value: &str) -> &str {
    match value {
        "open" => "仍在探索",
        "useful" => "产生有效增量",
        "refuted" => "原判断不成立",
        "mixed" => "部分成立",
        "blocked" => "条件不满足",
        "inconclusive" => "证据仍不足",
        _ => value,
    }
}

pub fn contribution_kind_label(value: &str) -> &str {
    match value {
        "artifact" => "文件 / 代码 / 内容",
        "finding" => "结论 / 经验",
        "evidence" => "证据 / 反馈",
        "decision" => "决定",
        "condition" => "条件变化",
        "other" => "其他产出",
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn drafts_a_generic_project_without_classifying_it() {
        let draft = draft_project_from_intent("  我想   做一个能持续赚钱的小产品。  ").unwrap();
        assert_eq!(draft.title, "做一个能持续赚钱的小产品");
        assert_eq!(draft.intent, "我想 做一个能持续赚钱的小产品。");
        assert!(draft.contradictions.is_empty());
        assert_eq!(draft.outcome_contract.constraints.len(), 2);
    }

    #[test]
    fn title_truncation_is_unicode_safe() {
        let draft = draft_project_from_intent(&format!("我想{}", "长期项目".repeat(20))).unwrap();
        assert_eq!(draft.title.chars().count(), 36);
        assert!(draft.title.ends_with('…'));
    }

    #[test]
    fn project_brief_keeps_evidence_and_constraints() {
        let draft = draft_project_from_intent("完成一次可恢复的 Rust 重写").unwrap();
        let markdown =
            create_project_brief_markdown(&draft.title, &draft.intent, &draft.outcome_contract);
        assert!(markdown.contains("## 成功证据"));
        assert!(markdown.contains("正式产物必须保留来源、版本和验收依据"));
        assert!(markdown.contains("## 第一项推进动作"));
    }

    #[test]
    fn only_live_exploration_branches_are_appendable() {
        assert!(can_append_to_branch("active"));
        assert!(can_append_to_branch("waiting"));
        assert!(!can_append_to_branch("integrated"));
        assert!(!can_integrate_branch("active", true));
        assert!(can_integrate_branch("active", false));
    }
}
