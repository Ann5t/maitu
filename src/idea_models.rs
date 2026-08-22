use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, types::Json};
use uuid::Uuid;

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaSummary {
    pub id: Uuid,
    pub state: String,
    pub current_revision: i32,
    pub title: String,
    pub body: String,
    pub source_kind: String,
    pub source_ref: Option<String>,
    pub updated_at: DateTime<Utc>,
    pub link_count: i64,
    pub proposal_count: i64,
    pub promoted_project_id: Option<Uuid>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaRecord {
    pub id: Uuid,
    pub state: String,
    pub current_revision: i32,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaRevisionRecord {
    pub id: Uuid,
    pub idea_id: Uuid,
    pub revision: i32,
    pub title: String,
    pub body: String,
    pub source_kind: String,
    pub source_ref: Option<String>,
    pub revision_reason: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaLinkView {
    pub id: Uuid,
    pub source_idea_id: Uuid,
    pub source_revision: i32,
    pub source_title: String,
    pub target_idea_id: Uuid,
    pub target_revision: i32,
    pub target_title: String,
    pub relation: String,
    pub rationale: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProposalRecord {
    pub id: Uuid,
    pub status: String,
    pub current_revision: i32,
    pub approved_revision: Option<i32>,
    pub approved_project_id: Option<Uuid>,
    pub approved_root_goal_proposal_id: Option<Uuid>,
    pub decision_rationale: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProposalRevisionRecord {
    pub id: Uuid,
    pub proposal_id: Uuid,
    pub revision: i32,
    pub title: String,
    pub project_intent: String,
    pub why_now: String,
    pub root_goal: Json<Value>,
    pub retained_notes: Json<Vec<String>>,
    pub omitted_notes: Json<Vec<String>>,
    pub revision_reason: Option<String>,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectProposalIdeaRecord {
    pub proposal_id: Uuid,
    pub proposal_revision: i32,
    pub idea_id: Uuid,
    pub idea_revision: i32,
    pub role: String,
    pub rationale: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaEventRecord {
    pub id: Uuid,
    pub sequence: i64,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub actor_type: String,
    pub client_request_id: Uuid,
    pub payload: Json<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaSourceRecord {
    pub id: Uuid,
    pub idea_id: Uuid,
    pub client_request_id: Uuid,
    pub request_hash: String,
    pub kind: String,
    pub original_filename: String,
    pub display_name: String,
    pub declared_media_type: Option<String>,
    pub trusted_media_type: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub storage_key: String,
    pub note: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaRevisionSourceRecord {
    pub idea_id: Uuid,
    pub idea_revision: i32,
    pub source_id: Uuid,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeaSnapshot {
    pub model_version: &'static str,
    pub idea: IdeaRecord,
    pub revisions: Vec<IdeaRevisionRecord>,
    pub sources: Vec<IdeaSourceRecord>,
    pub revision_sources: Vec<IdeaRevisionSourceRecord>,
    pub links: Vec<IdeaLinkView>,
    pub proposals: Vec<ProjectProposalRecord>,
    pub proposal_revisions: Vec<ProjectProposalRevisionRecord>,
    pub proposal_sources: Vec<ProjectProposalIdeaRecord>,
    pub events: Vec<IdeaEventRecord>,
}
