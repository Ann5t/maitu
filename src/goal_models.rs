use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, types::Json};
use uuid::Uuid;

use crate::models::Project;

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalProposalRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub parent_goal_branch_id: Option<Uuid>,
    pub parent_session_id: Option<Uuid>,
    pub status: String,
    pub current_revision: i32,
    pub approved_revision: Option<i32>,
    pub approved_goal_branch_id: Option<Uuid>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalProposalRevisionRecord {
    pub id: Uuid,
    pub proposal_id: Uuid,
    pub revision: i32,
    pub why_needed: String,
    pub contract: Json<Value>,
    pub expected_contributions: Json<Value>,
    pub exploration_plan: Json<Value>,
    pub context_inheritance: Json<Value>,
    pub tool_requirements: Json<Value>,
    pub capability_policy: Json<Value>,
    pub inferences: Json<Value>,
    pub revision_reason: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalContractVersionRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub version: i32,
    pub desired_outcome: String,
    pub hard_constraints: Json<Vec<String>>,
    pub subjective_preferences: Json<Vec<String>>,
    pub unknowns: Json<Vec<String>>,
    pub non_goals: Json<Vec<String>>,
    pub validation_plan: Json<Vec<String>>,
    pub judgment_triggers: Json<Vec<String>>,
    pub stop_conditions: Json<Vec<String>>,
    pub expected_contributions: Json<Vec<String>>,
    pub exploration_policy: Json<Value>,
    pub source_proposal_id: Option<Uuid>,
    pub supersedes_id: Option<Uuid>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalContractRevisionRequestRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub based_on_contract_version_id: Uuid,
    pub proposed_contract_version_id: Uuid,
    pub proposed_by_session_id: Option<Uuid>,
    pub status: String,
    pub reason: String,
    pub change_summary: Json<Value>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalContractRevisionDecisionRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub revision_request_id: Uuid,
    pub actor_role: String,
    pub decision: String,
    pub rationale: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalContractProvenanceRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub contract_version_id: Uuid,
    pub field_path: String,
    pub source_kind: String,
    pub source_ref: Option<String>,
    pub note: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalBranchRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub creating_proposal_id: Uuid,
    pub parent_goal_branch_id: Option<Uuid>,
    pub inherited_from_session_id: Option<Uuid>,
    pub name: String,
    pub status: String,
    pub current_contract_version_id: Uuid,
    pub head_session_id: Uuid,
    pub git_branch_name: Option<String>,
    pub worktree_path: Option<String>,
    pub base_commit: Option<String>,
    pub environment_fingerprint: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub archived_from_status: Option<String>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSessionRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_number: i32,
    pub status: String,
    pub assignment: String,
    pub agent_identity: Option<String>,
    pub contract_version_id: Uuid,
    pub environment_fingerprint: Option<String>,
    pub inherited_context: Json<Value>,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalContributionRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub artifact_id: Option<Uuid>,
    pub evidence_refs: Json<Value>,
    pub supersedes_id: Option<Uuid>,
    pub runner_job_id: Option<Uuid>,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalEvidenceRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub kind: String,
    pub stance: String,
    pub claim: String,
    pub observation: String,
    pub source_uri: Option<String>,
    pub artifact_id: Option<Uuid>,
    pub tool_call_id: Option<Uuid>,
    pub verification_status: String,
    pub content_hash: String,
    pub captured_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalContributionEvidenceRecord {
    pub contribution_id: Uuid,
    pub evidence_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalReviewGateEvidenceRecord {
    pub review_gate_id: Uuid,
    pub evidence_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalReviewGateRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub contract_version_id: Uuid,
    pub status: String,
    pub candidate_snapshot: Json<Value>,
    pub candidate_hash: String,
    pub git_base_commit: Option<String>,
    pub git_head_commit: Option<String>,
    pub git_dirty: bool,
    pub environment_fingerprint: Option<String>,
    pub test_evidence: Json<Value>,
    pub risks: Json<Value>,
    pub self_check: Json<Value>,
    pub workspace_id: Option<Uuid>,
    pub tree_id: Option<String>,
    pub workspace_snapshot: Option<String>,
    pub frozen_material: Option<Json<Value>>,
    pub frozen_candidate_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalReviewDecisionRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub review_gate_id: Uuid,
    pub actor_role: String,
    pub actor_identity: Option<String>,
    pub decision: String,
    pub rationale: String,
    pub contract_check: Json<Value>,
    pub retest_evidence: Json<Value>,
    pub selected_contribution_ids: Json<Value>,
    pub action_run_id: Option<Uuid>,
    pub action_lease_id: Option<Uuid>,
    pub worker_id: Option<Uuid>,
    pub candidate_digest: Option<String>,
    pub report_digest: Option<String>,
    pub observed_snapshot: Option<Json<Value>>,
    pub counterexamples: Json<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalIntegrationRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_goal_branch_id: Uuid,
    pub target_goal_branch_id: Option<Uuid>,
    pub review_gate_id: Uuid,
    pub kind: String,
    pub summary: String,
    pub git_integration_status: String,
    pub source_workspace_id: Option<Uuid>,
    pub target_workspace_id: Option<Uuid>,
    pub operation_id: Option<Uuid>,
    pub action_run_id: Option<Uuid>,
    pub source_head_commit: Option<String>,
    pub source_tree_id: Option<String>,
    pub source_workspace_snapshot: Option<String>,
    pub source_candidate_digest: Option<String>,
    pub expected_target_head_commit: Option<String>,
    pub expected_target_workspace_snapshot: Option<String>,
    pub selected_commits: Json<Value>,
    pub preparation_key: Option<String>,
    pub prepared_fencing_token: Option<i64>,
    pub candidate_commit: Option<String>,
    pub candidate_tree_id: Option<String>,
    pub candidate_workspace_snapshot: Option<String>,
    pub validation_report: Option<Json<Value>>,
    pub validation_report_digest: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub applied_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalIntegrationContributionRecord {
    pub integration_id: Uuid,
    pub contribution_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalAttentionRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub kind: String,
    pub status: String,
    pub dedupe_key: String,
    pub title: String,
    pub reason: String,
    pub safe_checkpoint: Option<String>,
    pub attempted: Option<String>,
    pub risk: Option<String>,
    pub user_action: Option<String>,
    pub recommendation: Option<String>,
    pub resolution: Option<String>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalEventRecord {
    pub id: Uuid,
    pub sequence: i64,
    pub project_id: Uuid,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub actor_type: String,
    pub actor_identity: Option<String>,
    pub client_request_id: Uuid,
    pub payload: Json<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalGraphSnapshot {
    pub model_version: &'static str,
    pub project: Project,
    pub proposals: Vec<GoalProposalRecord>,
    pub proposal_revisions: Vec<GoalProposalRevisionRecord>,
    pub contracts: Vec<GoalContractVersionRecord>,
    pub contract_revision_requests: Vec<GoalContractRevisionRequestRecord>,
    pub contract_revision_decisions: Vec<GoalContractRevisionDecisionRecord>,
    pub contract_provenance: Vec<GoalContractProvenanceRecord>,
    pub branches: Vec<GoalBranchRecord>,
    pub sessions: Vec<GoalSessionRecord>,
    pub contributions: Vec<GoalContributionRecord>,
    pub evidence: Vec<GoalEvidenceRecord>,
    pub contribution_evidence: Vec<GoalContributionEvidenceRecord>,
    pub review_gates: Vec<GoalReviewGateRecord>,
    pub review_gate_evidence: Vec<GoalReviewGateEvidenceRecord>,
    pub review_decisions: Vec<GoalReviewDecisionRecord>,
    pub integrations: Vec<GoalIntegrationRecord>,
    pub integration_contributions: Vec<GoalIntegrationContributionRecord>,
    pub attention_items: Vec<GoalAttentionRecord>,
    pub events: Vec<GoalEventRecord>,
}
