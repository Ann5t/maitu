use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::{error::AppResult, input_artifacts::InputArtifactRecord};

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
    Ok(WorkbenchActivity {
        inputs,
        environments,
        tool_calls,
    })
}
