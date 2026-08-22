use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::{
    application::context_memory::{ContextCatalogItem, ContextSnapshotRecord},
    error::AppResult,
    input_artifacts::InputArtifactRecord,
};

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEnvironmentActivity {
    pub session_id: Uuid,
    pub environment_manifest_id: Uuid,
    pub environment_fingerprint: String,
    pub inherited_from_session_id: Option<Uuid>,
    pub bound_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallActivity {
    pub id: Uuid,
    pub session_id: Uuid,
    pub plugin_id: String,
    pub plugin_version: String,
    pub tool_name: String,
    pub status: String,
    pub result: Json<Value>,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchActivity {
    pub inputs: Vec<InputArtifactRecord>,
    pub environments: Vec<SessionEnvironmentActivity>,
    pub tool_calls: Vec<ToolCallActivity>,
    pub contexts: Vec<SessionContextActivity>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContextActivity {
    pub snapshot: ContextSnapshotRecord,
    pub catalog_total: usize,
    pub catalog_preview: Vec<ContextCatalogItem>,
}

pub async fn get_activity(pool: &PgPool, project_id: Uuid) -> AppResult<WorkbenchActivity> {
    let inputs = sqlx::query_as::<_, InputArtifactRecord>(
        "SELECT * FROM input_artifacts WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let environments = sqlx::query_as::<_, SessionEnvironmentActivity>(
        "SELECT session_id, environment_manifest_id, environment_fingerprint, \
         inherited_from_session_id, bound_at FROM session_environment_bindings \
         WHERE project_id = $1 ORDER BY bound_at, session_id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let tool_calls = sqlx::query_as::<_, ToolCallActivity>(
        "SELECT id, session_id, plugin_id, plugin_version, tool_name, status, result, \
         started_at, completed_at FROM tool_calls WHERE project_id = $1 \
         ORDER BY started_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let snapshots = sqlx::query_as::<_, ContextSnapshotRecord>(
        "SELECT snapshot.* FROM goal_sessions session \
         JOIN goal_context_snapshots snapshot ON snapshot.id = session.context_snapshot_id \
         WHERE session.project_id = $1 ORDER BY session.started_at, session.id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let mut contexts = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        let catalog_total: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM goal_context_snapshot_entries WHERE snapshot_id = $1",
        )
        .bind(snapshot.id)
        .fetch_one(pool)
        .await?;
        let catalog_preview = sqlx::query_as::<_, ContextCatalogItem>(
            "SELECT e.id, e.origin_goal_branch_id, e.origin_session_id, e.source_kind, \
                    e.source_record_id, e.title, e.content_hash, e.importance, \
                    e.untrusted_content, m.inheritance_kind, m.rank, m.inclusion_reason, \
                    summary.payload->>'text' AS summary \
             FROM goal_context_snapshot_entries m \
             JOIN goal_context_entries e ON e.id = m.entry_id \
             LEFT JOIN LATERAL ( \
               SELECT d.payload FROM goal_context_derivations d \
               WHERE d.entry_id = e.id AND d.kind = 'summary' \
               ORDER BY d.generation DESC LIMIT 1 \
             ) summary ON true \
             WHERE m.snapshot_id = $1 ORDER BY m.rank, e.id LIMIT 5",
        )
        .bind(snapshot.id)
        .fetch_all(pool)
        .await?;
        contexts.push(SessionContextActivity {
            snapshot,
            catalog_total: usize::try_from(catalog_total).unwrap_or(usize::MAX),
            catalog_preview,
        });
    }
    Ok(WorkbenchActivity {
        inputs,
        environments,
        tool_calls,
        contexts,
    })
}
