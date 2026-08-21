use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, types::Json};
use uuid::Uuid;

use crate::domain::IntakeContradiction;

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: Uuid,
    pub title: String,
    pub intent: String,
    pub state: String,
    pub current_focus: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub contract_status: Option<String>,
    pub attention_count: i64,
    pub artifact_count: i64,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: Uuid,
    pub title: String,
    pub intent: String,
    pub state: String,
    pub current_focus: Option<String>,
    pub completion_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeContract {
    pub id: Uuid,
    pub project_id: Uuid,
    pub desired_outcome: String,
    pub success_evidence: Json<Vec<String>>,
    pub constraints: Json<Vec<String>>,
    pub non_goals: Json<Vec<String>>,
    pub confirmation_question: String,
    pub contradictions: Json<Vec<IntakeContradiction>>,
    pub status: String,
    pub version: i32,
    pub confirmed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRun {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: String,
    pub title: String,
    pub owner: String,
    pub status: String,
    pub expected_signal: Option<String>,
    pub output_summary: Option<String>,
    pub requires_approval: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub node_id: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: Uuid,
    pub project_id: Uuid,
    pub action_run_id: Option<Uuid>,
    pub title: String,
    pub kind: String,
    pub storage_path: String,
    pub media_type: String,
    pub sha256: String,
    pub version: i32,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
    pub node_id: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityGate {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub criteria: Json<Vec<String>>,
    pub status: String,
    pub required: i32,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub node_id: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub id: Uuid,
    pub project_id: Uuid,
    pub action_run_id: Option<Uuid>,
    pub artifact_id: Option<Uuid>,
    pub kind: String,
    pub summary: String,
    pub source_uri: Option<String>,
    pub stance: String,
    pub confidence: Option<i32>,
    pub observed_at: DateTime<Utc>,
    pub node_id: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionCheckpoint {
    pub id: Uuid,
    pub project_id: Uuid,
    pub decision: String,
    pub rationale: String,
    pub confidence: Option<i32>,
    pub retry_when: Option<String>,
    pub supersedes_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub node_id: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBranch {
    pub id: Uuid,
    pub project_id: Uuid,
    pub name: String,
    pub purpose: String,
    pub status: String,
    pub is_main: i32,
    pub color: String,
    pub forked_from_node_id: Option<Uuid>,
    pub head_node_id: Option<Uuid>,
    pub client_request_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectNode {
    pub id: Uuid,
    pub project_id: Uuid,
    pub branch_id: Uuid,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub outcome: String,
    pub actor_type: String,
    pub client_request_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectNodeEdge {
    pub project_id: Uuid,
    pub parent_node_id: Uuid,
    pub child_node_id: Uuid,
    pub relation: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContribution {
    pub id: Uuid,
    pub project_id: Uuid,
    pub node_id: Uuid,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub reference_uri: Option<String>,
    pub scope: Option<String>,
    pub reopen_when: Option<String>,
    pub status: String,
    pub accepted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchMerge {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_branch_id: Uuid,
    pub target_branch_id: Uuid,
    pub result_node_id: Uuid,
    pub summary: String,
    pub client_request_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub accepted_contribution_ids: Json<Vec<Uuid>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEvent {
    pub id: Uuid,
    pub project_id: Uuid,
    pub event_type: String,
    pub actor_type: String,
    pub payload: Json<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub project: Project,
    pub contract: Option<OutcomeContract>,
    pub actions: Vec<ActionRun>,
    pub artifacts: Vec<Artifact>,
    pub quality_gates: Vec<QualityGate>,
    pub evidence: Vec<Evidence>,
    pub decisions: Vec<DecisionCheckpoint>,
    pub branches: Vec<ProjectBranch>,
    pub nodes: Vec<ProjectNode>,
    pub edges: Vec<ProjectNodeEdge>,
    pub contributions: Vec<ProjectContribution>,
    pub merges: Vec<BranchMerge>,
    pub events: Vec<ProjectEvent>,
}
