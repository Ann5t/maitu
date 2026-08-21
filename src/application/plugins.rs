use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    goal_domain::{CommandReceiptIdentity, SessionStatus},
    tooling::{
        EnvironmentManifest, MockToolBroker, PluginCatalogEntry, PluginManifest,
        PluginManifestDraft, PluginSelector, ResolvedPluginRef, ToolBroker, ToolCall, ToolResult,
        reference_mock_plugin,
    },
};

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRecord {
    pub id: Uuid,
    pub fingerprint: String,
    pub manifest: Json<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindEnvironmentRequest {
    pub client_request_id: Uuid,
    pub environment_manifest_id: Uuid,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BindEnvironmentResponse {
    pub replayed: bool,
    pub session_id: Uuid,
    pub environment_manifest_id: Uuid,
    pub environment_fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteToolRequest {
    pub client_request_id: Uuid,
    pub plugin: ResolvedPluginRef,
    pub tool_name: String,
    #[serde(default)]
    pub input: Value,
    pub base_workspace_snapshot: String,
    #[serde(default)]
    pub allowed_writes: Vec<String>,
    pub timeout_seconds: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteToolResponse {
    pub replayed: bool,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub plugin: ResolvedPluginRef,
    pub environment_fingerprint: String,
    pub result: ToolResult,
}

#[derive(Clone, Debug, FromRow)]
struct SessionToolState {
    goal_branch_id: Uuid,
    status: String,
    environment_fingerprint: Option<String>,
}

pub async fn ensure_reference_plugins(pool: &PgPool) -> AppResult<()> {
    let manifest = reference_mock_plugin("1.0.0")?;
    insert_manifest(pool, &manifest).await?;
    Ok(())
}

pub async fn register_plugin(
    pool: &PgPool,
    draft: PluginManifestDraft,
) -> AppResult<PluginManifest> {
    let manifest = draft.seal()?;
    insert_manifest(pool, &manifest).await?;
    Ok(manifest)
}

async fn insert_manifest(pool: &PgPool, manifest: &PluginManifest) -> AppResult<Uuid> {
    let manifest_value = serde_json::to_value(manifest)?;
    let package_id = Uuid::new_v4();
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO plugin_packages \
         (id, plugin_id, version, content_digest, manifest) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (plugin_id, version, content_digest) DO NOTHING \
         RETURNING id",
    )
    .bind(package_id)
    .bind(&manifest.plugin_id)
    .bind(&manifest.version)
    .bind(&manifest.content_digest)
    .bind(Json(manifest_value))
    .fetch_optional(pool)
    .await?;
    if let Some(id) = inserted {
        return Ok(id);
    }
    sqlx::query_scalar(
        "SELECT id FROM plugin_packages \
         WHERE plugin_id = $1 AND version = $2 AND content_digest = $3",
    )
    .bind(&manifest.plugin_id)
    .bind(&manifest.version)
    .bind(&manifest.content_digest)
    .fetch_one(pool)
    .await
    .map_err(AppError::from)
}

pub async fn list_catalog(pool: &PgPool) -> AppResult<Vec<PluginCatalogEntry>> {
    let rows: Vec<Json<Value>> = sqlx::query_scalar(
        "SELECT manifest FROM plugin_packages ORDER BY plugin_id, version, content_digest",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            let manifest: PluginManifest = serde_json::from_value(row.0)?;
            Ok(manifest.summary())
        })
        .collect()
}

pub async fn get_plugin(
    pool: &PgPool,
    plugin_id: &str,
    version: &str,
) -> AppResult<PluginManifest> {
    resolve_manifest(
        pool,
        PluginSelector {
            plugin_id: plugin_id.to_owned(),
            version: version.to_owned(),
        },
    )
    .await
}

pub async fn resolve_plugin(
    pool: &PgPool,
    selector: PluginSelector,
) -> AppResult<ResolvedPluginRef> {
    Ok(resolve_manifest(pool, selector).await?.resolved_ref())
}

async fn resolve_manifest(pool: &PgPool, selector: PluginSelector) -> AppResult<PluginManifest> {
    let selector = selector.normalize()?;
    let rows: Vec<Json<Value>> =
        sqlx::query_scalar("SELECT manifest FROM plugin_packages WHERE plugin_id = $1")
            .bind(&selector.plugin_id)
            .fetch_all(pool)
            .await?;
    if rows.is_empty() {
        return Err(AppError::not_found("插件不存在"));
    }
    let mut manifests = rows
        .into_iter()
        .map(|row| serde_json::from_value::<PluginManifest>(row.0))
        .collect::<Result<Vec<_>, _>>()?;
    let selected_version = if selector.version == "latest" {
        let stable = manifests
            .iter()
            .filter_map(|manifest| {
                Version::parse(&manifest.version)
                    .ok()
                    .filter(|version| version.pre.is_empty())
            })
            .max();
        stable.or_else(|| {
            manifests
                .iter()
                .filter_map(|manifest| Version::parse(&manifest.version).ok())
                .max()
        })
    } else {
        Version::parse(&selector.version).ok()
    }
    .ok_or_else(|| AppError::not_found("没有可解析的插件版本"))?;
    manifests.retain(|manifest| {
        Version::parse(&manifest.version).is_ok_and(|version| version == selected_version)
    });
    if manifests.is_empty() {
        return Err(AppError::not_found("插件版本不存在"));
    }
    manifests.sort_by(|left, right| left.content_digest.cmp(&right.content_digest));
    manifests.dedup_by(|left, right| left.content_digest == right.content_digest);
    if manifests.len() != 1 {
        return Err(AppError::conflict(
            "plugin_digest_conflict",
            "同一插件版本存在多个内容摘要，必须显式解决供应链冲突",
        ));
    }
    Ok(manifests.remove(0))
}

pub async fn create_environment(
    pool: &PgPool,
    manifest: EnvironmentManifest,
) -> AppResult<EnvironmentRecord> {
    let manifest = manifest.normalize()?;
    for plugin in &manifest.plugins {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM plugin_packages \
             WHERE plugin_id = $1 AND version = $2 AND content_digest = $3)",
        )
        .bind(&plugin.plugin_id)
        .bind(&plugin.version)
        .bind(&plugin.content_digest)
        .fetch_one(pool)
        .await?;
        if !exists {
            return Err(AppError::bad_request(
                "plugin_not_found",
                "EnvironmentManifest 引用了未注册的固定插件",
            ));
        }
    }
    let fingerprint = manifest.fingerprint()?;
    let manifest_value = serde_json::to_value(&manifest)?;
    let environment_id = Uuid::new_v4();
    let inserted = sqlx::query_as::<_, EnvironmentRecord>(
        "INSERT INTO environment_manifests (id, fingerprint, manifest) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (fingerprint) DO NOTHING \
         RETURNING *",
    )
    .bind(environment_id)
    .bind(&fingerprint)
    .bind(Json(manifest_value))
    .fetch_optional(pool)
    .await?;
    if let Some(record) = inserted {
        return Ok(record);
    }
    sqlx::query_as::<_, EnvironmentRecord>(
        "SELECT * FROM environment_manifests WHERE fingerprint = $1",
    )
    .bind(fingerprint)
    .fetch_one(pool)
    .await
    .map_err(AppError::from)
}

pub async fn get_environment(pool: &PgPool, environment_id: Uuid) -> AppResult<EnvironmentRecord> {
    sqlx::query_as::<_, EnvironmentRecord>("SELECT * FROM environment_manifests WHERE id = $1")
        .bind(environment_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("EnvironmentManifest 不存在"))
}

pub async fn bind_environment(
    pool: &PgPool,
    project_id: Uuid,
    session_id: Uuid,
    request: BindEnvironmentRequest,
) -> AppResult<BindEnvironmentResponse> {
    let mut transaction = pool.begin().await?;
    let project_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?;
    if project_exists.is_none() {
        return Err(AppError::not_found("项目不存在"));
    }
    let session: SessionToolState = sqlx::query_as(
        "SELECT goal_branch_id, status, environment_fingerprint \
         FROM goal_sessions WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| AppError::not_found("Agent Session 不存在"))?;
    let environment: EnvironmentRecord =
        sqlx::query_as("SELECT * FROM environment_manifests WHERE id = $1")
            .bind(request.environment_manifest_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| AppError::not_found("EnvironmentManifest 不存在"))?;
    validate_environment_record(&environment)?;

    let existing: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT environment_manifest_id, environment_fingerprint \
         FROM session_environment_bindings WHERE session_id = $1",
    )
    .bind(session_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((existing_id, existing_fingerprint)) = existing {
        if existing_id != environment.id || existing_fingerprint != environment.fingerprint {
            return Err(AppError::conflict(
                "environment_already_bound",
                "Session 已固定另一个环境；依赖变化必须创建新的环境与 Session",
            ));
        }
        transaction.commit().await?;
        return Ok(BindEnvironmentResponse {
            replayed: true,
            session_id,
            environment_manifest_id: existing_id,
            environment_fingerprint: existing_fingerprint,
        });
    }
    if SessionStatus::try_from(session.status.as_str())? != SessionStatus::Running {
        return Err(AppError::conflict(
            "invalid_state_transition",
            "只有 running Session 可以首次固定环境",
        ));
    }
    if session.environment_fingerprint.is_some() {
        return Err(AppError::conflict(
            "environment_already_bound",
            "Session 已有环境指纹但缺少不可变绑定，拒绝猜测性修复",
        ));
    }

    sqlx::query(
        "INSERT INTO session_environment_bindings \
         (session_id, project_id, goal_branch_id, environment_manifest_id, \
          environment_fingerprint) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(session_id)
    .bind(project_id)
    .bind(session.goal_branch_id)
    .bind(environment.id)
    .bind(&environment.fingerprint)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE goal_sessions SET environment_fingerprint = $1, updated_at = now() WHERE id = $2",
    )
    .bind(&environment.fingerprint)
    .bind(session_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE goal_branches SET environment_fingerprint = $1, updated_at = now() \
         WHERE id = $2 AND head_session_id = $3",
    )
    .bind(&environment.fingerprint)
    .bind(session.goal_branch_id)
    .bind(session_id)
    .execute(&mut *transaction)
    .await?;
    insert_tool_event(
        &mut transaction,
        project_id,
        session_id,
        "environment.bound",
        request.client_request_id,
        json!({
            "goalBranchId": session.goal_branch_id,
            "environmentManifestId": environment.id,
            "environmentFingerprint": environment.fingerprint,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(BindEnvironmentResponse {
        replayed: false,
        session_id,
        environment_manifest_id: environment.id,
        environment_fingerprint: environment.fingerprint,
    })
}

pub async fn execute_tool(
    pool: &PgPool,
    project_id: Uuid,
    session_id: Uuid,
    request: ExecuteToolRequest,
) -> AppResult<ExecuteToolResponse> {
    let request_identity = CommandReceiptIdentity::from_input("tool.execute", &request)?;
    let mut transaction = pool.begin().await?;
    let project_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?;
    if project_exists.is_none() {
        return Err(AppError::not_found("项目不存在"));
    }
    let existing: Option<(String, Json<Value>)> = sqlx::query_as(
        "SELECT request_hash, result FROM tool_calls \
         WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((request_hash, result)) = existing {
        CommandReceiptIdentity {
            command_kind: "tool.execute".into(),
            input_hash: request_hash,
        }
        .ensure_replay_matches(&request_identity)?;
        let response: ExecuteToolResponse = serde_json::from_value(result.0)?;
        transaction.commit().await?;
        return Ok(ExecuteToolResponse {
            replayed: true,
            ..response
        });
    }

    let session: SessionToolState = sqlx::query_as(
        "SELECT goal_branch_id, status, environment_fingerprint \
         FROM goal_sessions WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| AppError::not_found("Agent Session 不存在"))?;
    if SessionStatus::try_from(session.status.as_str())? != SessionStatus::Running {
        return Err(AppError::conflict(
            "invalid_state_transition",
            "只有 running Session 可以调用工具",
        ));
    }
    let environment_fingerprint = session.environment_fingerprint.ok_or_else(|| {
        AppError::conflict(
            "environment_not_bound",
            "Session 尚未固定 EnvironmentManifest",
        )
    })?;
    let environment_json: Json<Value> = sqlx::query_scalar(
        "SELECT e.manifest FROM session_environment_bindings b \
         JOIN environment_manifests e ON e.id = b.environment_manifest_id \
         WHERE b.session_id = $1 AND b.environment_fingerprint = $2",
    )
    .bind(session_id)
    .bind(&environment_fingerprint)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| {
        AppError::conflict(
            "environment_fingerprint_mismatch",
            "Session 环境指纹没有对应的不可变 Manifest",
        )
    })?;
    let environment: EnvironmentManifest = serde_json::from_value(environment_json.0)?;
    let environment = environment.normalize()?;
    if environment.fingerprint()? != environment_fingerprint {
        return Err(AppError::conflict(
            "environment_fingerprint_mismatch",
            "持久化 EnvironmentManifest 与 Session 指纹不一致",
        ));
    }
    let requested_plugin = request.plugin.clone().validate()?;
    if !environment.plugins.contains(&requested_plugin) {
        return Err(AppError::conflict(
            "tool_not_allowed",
            "Session 的固定环境没有包含该插件版本",
        ));
    }
    let manifest_json: Json<Value> = sqlx::query_scalar(
        "SELECT manifest FROM plugin_packages \
         WHERE plugin_id = $1 AND version = $2 AND content_digest = $3",
    )
    .bind(&requested_plugin.plugin_id)
    .bind(&requested_plugin.version)
    .bind(&requested_plugin.content_digest)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| AppError::not_found("固定插件包不存在"))?;
    let manifest: PluginManifest = serde_json::from_value(manifest_json.0)?;
    let call = ToolCall {
        call_id: Uuid::new_v4(),
        client_request_id: request.client_request_id,
        project_id,
        goal_branch_id: session.goal_branch_id,
        session_id,
        plugin: requested_plugin.clone(),
        tool_name: request.tool_name.clone(),
        input: request.input.clone(),
        environment_fingerprint: environment_fingerprint.clone(),
        base_workspace_snapshot: request.base_workspace_snapshot.clone(),
        allowed_writes: request.allowed_writes.clone(),
        timeout_seconds: request.timeout_seconds,
    };
    let result = MockToolBroker.execute(&manifest, call.clone()).await?;
    let response = ExecuteToolResponse {
        replayed: false,
        project_id,
        goal_branch_id: session.goal_branch_id,
        session_id,
        plugin: requested_plugin,
        environment_fingerprint: environment_fingerprint.clone(),
        result: result.clone(),
    };
    let response_json = serde_json::to_value(&response)?;
    sqlx::query(
        "INSERT INTO tool_calls \
         (id, project_id, goal_branch_id, session_id, client_request_id, request_hash, \
          plugin_id, plugin_version, plugin_digest, tool_name, input, environment_fingerprint, \
          base_workspace_snapshot, allowed_writes, timeout_seconds, status, result, \
          started_at, completed_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, \
                 $16, $17, $18, $19)",
    )
    .bind(call.call_id)
    .bind(project_id)
    .bind(session.goal_branch_id)
    .bind(session_id)
    .bind(request.client_request_id)
    .bind(&request_identity.input_hash)
    .bind(&call.plugin.plugin_id)
    .bind(&call.plugin.version)
    .bind(&call.plugin.content_digest)
    .bind(&call.tool_name)
    .bind(Json(call.input))
    .bind(&call.environment_fingerprint)
    .bind(&call.base_workspace_snapshot)
    .bind(Json(call.allowed_writes))
    .bind(call.timeout_seconds as i32)
    .bind(&result.status)
    .bind(Json(response_json))
    .bind(result.started_at)
    .bind(result.completed_at)
    .execute(&mut *transaction)
    .await?;
    insert_tool_event(
        &mut transaction,
        project_id,
        call.call_id,
        "tool.call_completed",
        request.client_request_id,
        json!({
            "sessionId": session_id,
            "goalBranchId": session.goal_branch_id,
            "plugin": call.plugin,
            "toolName": call.tool_name,
            "environmentFingerprint": environment_fingerprint,
            "baseWorkspaceSnapshot": call.base_workspace_snapshot,
            "status": result.status,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(response)
}

async fn insert_tool_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: Uuid,
    aggregate_id: Uuid,
    event_type: &str,
    client_request_id: Uuid,
    payload: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO goal_events \
         (id, project_id, aggregate_type, aggregate_id, event_type, actor_type, \
          client_request_id, payload) \
         VALUES ($1, $2, 'tool', $3, $4, 'system', $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(aggregate_id)
    .bind(event_type)
    .bind(client_request_id)
    .bind(Json(payload))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn validate_environment_record(record: &EnvironmentRecord) -> AppResult<()> {
    let manifest: EnvironmentManifest = serde_json::from_value(record.manifest.0.clone())?;
    let manifest = manifest.normalize()?;
    if manifest.fingerprint()? != record.fingerprint {
        return Err(AppError::conflict(
            "environment_fingerprint_mismatch",
            "EnvironmentManifest 内容与持久化指纹不一致",
        ));
    }
    Ok(())
}
