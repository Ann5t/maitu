use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::{
    application::{
        context_memory::{ContextCatalogItem, ContextSnapshotRecord},
        plugins::PluginInstallRequestRecord,
        scheduler::{ActionRunRecord, NotificationRecord, ToolLeaseRecord},
        workspaces::GoalWorkspaceRecord,
    },
    error::AppResult,
    input_artifacts::InputArtifactRecord,
    tooling::PluginManifest,
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

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerJobActivity {
    pub id: Uuid,
    pub session_id: Uuid,
    pub status: String,
    pub spec: Json<Value>,
    pub result: Option<Json<Value>>,
    pub runtime_digest: String,
    pub output_manifest_hash: Option<String>,
    pub candidate_commit: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerFileActivity {
    pub runner_job_id: Uuid,
    pub session_id: Uuid,
    pub path: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub executable: bool,
    pub runner_status: String,
    pub candidate_commit: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionEventActivity {
    pub id: Uuid,
    pub sequence: i64,
    pub action_run_id: Uuid,
    pub session_id: Uuid,
    pub event_type: String,
    pub actor_type: String,
    pub actor_identity: Option<String>,
    pub detail: Json<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCatalogActivity {
    pub manifest: PluginManifest,
    pub installed: bool,
    pub installation_status: String,
    pub publisher_id: Option<String>,
    pub publisher_status: Option<String>,
}

#[derive(Clone, Debug, FromRow)]
struct PluginCatalogRow {
    manifest: Json<Value>,
    installation_status: Option<String>,
    publisher_id: Option<String>,
    publisher_status: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchActivity {
    pub inputs: Vec<InputArtifactRecord>,
    pub environments: Vec<SessionEnvironmentActivity>,
    pub tool_calls: Vec<ToolCallActivity>,
    pub tool_leases: Vec<ToolLeaseRecord>,
    pub action_runs: Vec<ActionRunRecord>,
    pub action_events: Vec<ActionEventActivity>,
    pub notifications: Vec<NotificationRecord>,
    pub runner_jobs: Vec<RunnerJobActivity>,
    pub runner_files: Vec<RunnerFileActivity>,
    pub workspaces: Vec<GoalWorkspaceRecord>,
    pub plugin_catalog: Vec<PluginCatalogActivity>,
    pub plugin_install_requests: Vec<PluginInstallRequestRecord>,
    pub contexts: Vec<SessionContextActivity>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionContextActivity {
    pub snapshot: ContextSnapshotRecord,
    pub catalog_total: usize,
    pub catalog_preview: Vec<ContextCatalogItem>,
}

#[derive(Clone, Debug, FromRow)]
struct ContextPreviewRow {
    snapshot_id: Uuid,
    id: Uuid,
    origin_goal_branch_id: Uuid,
    origin_session_id: Option<Uuid>,
    source_kind: String,
    source_record_id: Uuid,
    title: String,
    content_hash: String,
    importance: String,
    untrusted_content: bool,
    inheritance_kind: String,
    rank: i32,
    inclusion_reason: String,
    summary: Option<String>,
}

impl From<ContextPreviewRow> for ContextCatalogItem {
    fn from(row: ContextPreviewRow) -> Self {
        Self {
            id: row.id,
            origin_goal_branch_id: row.origin_goal_branch_id,
            origin_session_id: row.origin_session_id,
            source_kind: row.source_kind,
            source_record_id: row.source_record_id,
            title: row.title,
            content_hash: row.content_hash,
            importance: row.importance,
            untrusted_content: row.untrusted_content,
            inheritance_kind: row.inheritance_kind,
            rank: row.rank,
            inclusion_reason: row.inclusion_reason,
            summary: row.summary,
        }
    }
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
    let tool_leases = sqlx::query_as::<_, ToolLeaseRecord>(
        "SELECT * FROM tool_leases WHERE project_id = $1 ORDER BY created_at DESC, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let action_runs = sqlx::query_as::<_, ActionRunRecord>(
        "SELECT * FROM goal_action_runs WHERE project_id = $1 ORDER BY created_at DESC, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let action_events = sqlx::query_as::<_, ActionEventActivity>(
        "SELECT event.id, event.sequence, event.action_run_id, action.session_id, \
                event.event_type, event.actor_type, event.actor_identity, event.detail, \
                event.created_at \
         FROM goal_action_events event \
         JOIN goal_action_runs action ON action.id = event.action_run_id \
         WHERE event.project_id = $1 ORDER BY event.sequence DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let notifications = sqlx::query_as::<_, NotificationRecord>(
        "SELECT * FROM goal_notifications WHERE project_id = $1 ORDER BY created_at DESC, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let runner_jobs = sqlx::query_as::<_, RunnerJobActivity>(
        "SELECT id, session_id, status, spec, result, runtime_digest, output_manifest_hash, \
                candidate_commit, created_at, started_at, completed_at \
         FROM runner_jobs WHERE project_id = $1 ORDER BY created_at DESC, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let runner_files = sqlx::query_as::<_, RunnerFileActivity>(
        "SELECT file.runner_job_id, job.session_id, file.path, file.sha256, file.size_bytes, \
                file.executable, job.status AS runner_status, job.candidate_commit, \
                file.created_at \
         FROM runner_job_files file \
         JOIN runner_jobs job ON job.id = file.runner_job_id \
         WHERE file.project_id = $1 ORDER BY file.created_at DESC, file.path",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let workspaces = sqlx::query_as::<_, GoalWorkspaceRecord>(
        "SELECT * FROM goal_workspaces WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let plugin_install_requests = sqlx::query_as::<_, PluginInstallRequestRecord>(
        "SELECT * FROM plugin_install_requests WHERE project_id = $1 \
         ORDER BY created_at DESC, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let plugin_rows = sqlx::query_as::<_, PluginCatalogRow>(
        "SELECT package.manifest, installation.status AS installation_status, \
                installation.publisher_id, publisher.status AS publisher_status \
         FROM plugin_packages package \
         LEFT JOIN plugin_installations installation \
           ON installation.plugin_package_id = package.id \
         LEFT JOIN plugin_publishers publisher \
           ON publisher.publisher_id = installation.publisher_id \
         ORDER BY package.plugin_id, package.version, package.content_digest",
    )
    .fetch_all(pool)
    .await?;
    let plugin_catalog = plugin_rows
        .into_iter()
        .map(|row| {
            let manifest = serde_json::from_value::<PluginManifest>(row.manifest.0)?;
            let installed = manifest.runtime.kind == "mock"
                || (row.installation_status.as_deref() == Some("installed")
                    && row.publisher_status.as_deref() == Some("active"));
            Ok(PluginCatalogActivity {
                installation_status: if manifest.runtime.kind == "mock" {
                    "built_in".to_owned()
                } else {
                    row.installation_status
                        .clone()
                        .unwrap_or_else(|| "not_installed".to_owned())
                },
                manifest,
                installed,
                publisher_id: row.publisher_id,
                publisher_status: row.publisher_status,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let snapshots = sqlx::query_as::<_, ContextSnapshotRecord>(
        "SELECT snapshot.* FROM goal_sessions session \
         JOIN goal_context_snapshots snapshot ON snapshot.id = session.context_snapshot_id \
         WHERE session.project_id = $1 ORDER BY session.started_at, session.id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let snapshot_ids = snapshots.iter().map(|item| item.id).collect::<Vec<_>>();
    let (catalog_counts, preview_rows) = if snapshot_ids.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        let catalog_counts = sqlx::query_as::<_, (Uuid, i64)>(
            "SELECT snapshot_id, count(*) FROM goal_context_snapshot_entries \
             WHERE snapshot_id = ANY($1) GROUP BY snapshot_id",
        )
        .bind(&snapshot_ids)
        .fetch_all(pool)
        .await?;
        let preview_rows = sqlx::query_as::<_, ContextPreviewRow>(
            "SELECT snapshot_id, id, origin_goal_branch_id, origin_session_id, source_kind, \
                    source_record_id, title, content_hash, importance, untrusted_content, \
                    inheritance_kind, rank, inclusion_reason, summary \
             FROM ( \
               SELECT m.snapshot_id, e.id, e.origin_goal_branch_id, e.origin_session_id, \
                      e.source_kind, e.source_record_id, e.title, e.content_hash, \
                      e.importance, e.untrusted_content, m.inheritance_kind, m.rank, \
                      m.inclusion_reason, summary.payload->>'text' AS summary, \
                      row_number() OVER (PARTITION BY m.snapshot_id ORDER BY m.rank, e.id) AS preview_rank \
               FROM goal_context_snapshot_entries m \
               JOIN goal_context_entries e ON e.id = m.entry_id \
               LEFT JOIN LATERAL ( \
                 SELECT d.payload FROM goal_context_derivations d \
                 WHERE d.entry_id = e.id AND d.kind = 'summary' \
                 ORDER BY d.generation DESC LIMIT 1 \
               ) summary ON true \
               WHERE m.snapshot_id = ANY($1) \
             ) ranked WHERE preview_rank <= 5 ORDER BY snapshot_id, rank, id",
        )
        .bind(&snapshot_ids)
        .fetch_all(pool)
        .await?;
        (catalog_counts, preview_rows)
    };
    let catalog_counts = catalog_counts.into_iter().collect::<HashMap<_, _>>();
    let mut previews = HashMap::<Uuid, Vec<ContextCatalogItem>>::new();
    for row in preview_rows {
        previews
            .entry(row.snapshot_id)
            .or_default()
            .push(row.into());
    }
    let contexts = snapshots
        .into_iter()
        .map(|snapshot| SessionContextActivity {
            catalog_total: usize::try_from(
                catalog_counts
                    .get(&snapshot.id)
                    .copied()
                    .unwrap_or_default(),
            )
            .unwrap_or(usize::MAX),
            catalog_preview: previews.remove(&snapshot.id).unwrap_or_default(),
            snapshot,
        })
        .collect();
    Ok(WorkbenchActivity {
        inputs,
        environments,
        tool_calls,
        tool_leases,
        action_runs,
        action_events,
        notifications,
        runner_jobs,
        runner_files,
        workspaces,
        plugin_catalog,
        plugin_install_requests,
        contexts,
    })
}
