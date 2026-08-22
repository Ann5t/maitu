use std::collections::{BTreeMap, BTreeSet};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, types::Json};
use uuid::Uuid;

use crate::{
    application::{context_memory, workspaces},
    config::Config,
    error::{AppError, AppResult},
    goal_domain::{CommandReceiptIdentity, SessionStatus, canonical_json_sha256},
    tooling::{
        EnvironmentManifest, MockToolBroker, PluginCatalogEntry, PluginInstallStatementRequest,
        PluginManifest, PluginManifestDraft, PluginPublisherDraft, PluginResourceMetadata,
        PluginSelector, PluginSelfTest, ResolvedPluginRef, SignedPluginInstallRequest, ToolBroker,
        ToolCall, ToolResult, VerifiedPluginResource, expected_plugin_resource_media_type,
        normalize_plugin_resource_path, normalize_publisher_id, reference_mock_plugin,
        validate_tool_input_schema, verify_install_signature,
    },
    workspace::{
        FailRunnerJobRequest, FinalizeRunnerJobRequest, PrepareRunnerJobRequest,
        PrepareRunnerJobResponse, RunnerJobOutcome,
    },
};
use fudian::runner_protocol::{
    RunnerCapabilities, RunnerCommand, RunnerExecutionResult, RunnerJobSpec, RunnerResourceLimits,
};

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentRecord {
    pub id: Uuid,
    pub fingerprint: String,
    pub manifest: Json<Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPublisherRecord {
    pub publisher_id: String,
    pub display_name: String,
    pub public_key: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revocation_reason: Option<String>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallationRecord {
    pub id: Uuid,
    pub plugin_package_id: Uuid,
    pub publisher_id: String,
    pub signature: String,
    pub statement: Json<Value>,
    pub statement_digest: String,
    pub runtime_image_digest: String,
    pub runtime_entry_digest: String,
    pub runner_digest: String,
    pub self_test: Json<Value>,
    pub self_test_digest: String,
    pub status: String,
    pub installed_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revocation_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokePluginInstallationRequest {
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevokePluginPublisherRequest {
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetail {
    pub manifest: PluginManifest,
    pub installation: Option<PluginInstallationRecord>,
    pub publisher: Option<PluginPublisherRecord>,
    pub resources: Vec<PluginResourceMetadata>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginResourceSelector {
    pub plugin: ResolvedPluginRef,
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPluginContextRequest {
    pub client_request_id: Uuid,
    #[serde(default = "default_true")]
    pub include_skills: bool,
    #[serde(default)]
    pub resources: Vec<PluginResourceSelector>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginResourceContent {
    pub plugin: ResolvedPluginRef,
    pub path: String,
    pub media_type: String,
    pub content_digest: String,
    pub byte_length: u32,
    pub encoding: &'static str,
    pub content_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPluginContext {
    pub plugin: ResolvedPluginRef,
    pub display_name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub skill: Option<PluginResourceContent>,
    pub resources: Vec<PluginResourceMetadata>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPluginContextResponse {
    pub schema_version: u32,
    pub replayed: bool,
    pub project_id: Uuid,
    pub session_id: Uuid,
    pub environment_fingerprint: String,
    pub plugins: Vec<AgentPluginContext>,
    pub requested_resources: Vec<PluginResourceContent>,
}

#[derive(Clone, Debug, FromRow)]
struct PluginResourceRow {
    plugin_package_id: Uuid,
    path: String,
    media_type: String,
    content_digest: String,
    content: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePluginInstallRequest {
    pub client_request_id: Uuid,
    pub plugin_id: String,
    pub version_requirement: String,
    pub capability: String,
    pub reason: String,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallRequestRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub client_request_id: Uuid,
    pub request_hash: String,
    pub plugin_id: String,
    pub version_requirement: String,
    pub capability: String,
    pub reason: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRealToolRequest {
    pub client_request_id: Uuid,
    pub plugin: ResolvedPluginRef,
    pub tool_name: String,
    #[serde(default)]
    pub input: Value,
    pub base_workspace_snapshot: String,
    #[serde(default)]
    pub allowed_writes: Vec<String>,
    #[serde(default)]
    pub delete_paths: Vec<String>,
    pub timeout_seconds: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRealToolResponse {
    pub replayed: bool,
    pub execution_id: Uuid,
    pub runtime_image_digest: String,
    pub runtime_entry_digest: String,
    pub runner: PrepareRunnerJobResponse,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeRealToolRequest {
    pub lease_token: String,
    pub result: RunnerExecutionResult,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeRealToolResponse {
    pub replayed: bool,
    pub execution_id: Uuid,
    pub runner: RunnerJobOutcome,
    pub tool_call: ExecuteToolResponse,
}

#[derive(Clone, Debug, FromRow)]
struct ToolExecutionRecord {
    id: Uuid,
    project_id: Uuid,
    goal_branch_id: Uuid,
    session_id: Uuid,
    runner_job_id: Uuid,
    plugin_installation_id: Uuid,
    client_request_id: Uuid,
    request_hash: String,
    plugin_id: String,
    plugin_version: String,
    plugin_digest: String,
    runtime_image_digest: String,
    runtime_entry_digest: String,
    tool_name: String,
    input: Json<Value>,
    environment_fingerprint: String,
    base_workspace_snapshot: String,
}

#[derive(Clone, Debug, FromRow)]
struct RunnerAuditRecord {
    status: String,
    spec: Json<Value>,
    result: Option<Json<Value>>,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
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

pub fn seal_plugin(draft: PluginManifestDraft) -> AppResult<PluginManifest> {
    draft.seal()
}

pub async fn register_publisher(
    pool: &PgPool,
    draft: PluginPublisherDraft,
) -> AppResult<PluginPublisherRecord> {
    let draft = draft.normalize()?;
    let existing = sqlx::query_as::<_, PluginPublisherRecord>(
        "SELECT * FROM plugin_publishers WHERE publisher_id = $1 OR public_key = $2",
    )
    .bind(&draft.publisher_id)
    .bind(&draft.public_key)
    .fetch_optional(pool)
    .await?;
    if let Some(existing) = existing {
        if existing.publisher_id == draft.publisher_id
            && existing.display_name == draft.display_name
            && existing.public_key == draft.public_key
        {
            return Ok(existing);
        }
        return Err(AppError::conflict(
            "publisher_identity_conflict",
            "发布者 ID 或 Ed25519 公钥已绑定不同身份",
        ));
    }
    sqlx::query_as::<_, PluginPublisherRecord>(
        "INSERT INTO plugin_publishers (publisher_id, display_name, public_key) \
         VALUES ($1, $2, $3) RETURNING *",
    )
    .bind(draft.publisher_id)
    .bind(draft.display_name)
    .bind(draft.public_key)
    .fetch_one(pool)
    .await
    .map_err(AppError::from)
}

pub fn preview_install_statement(request: PluginInstallStatementRequest) -> AppResult<Value> {
    let request = request.normalize()?;
    Ok(json!({
        "statement": request.statement()?,
        "statementDigest": request.statement_digest()?,
    }))
}

pub async fn install_signed_plugin(
    pool: &PgPool,
    expected_runner_digest: &str,
    request: SignedPluginInstallRequest,
) -> AppResult<PluginInstallationRecord> {
    let request = request.normalize()?;
    let resources = request.verified_resources()?;
    if request.self_test.runner_digest != expected_runner_digest {
        return Err(AppError::conflict(
            "runner_digest_mismatch",
            "插件自检不是由当前固定 Runner 版本产生",
        ));
    }
    let statement = request.statement()?;
    let statement_digest = request.statement_digest()?;
    let self_test_digest = request.self_test.digest()?;
    let mut transaction = pool.begin().await?;
    let publisher: PluginPublisherRecord =
        sqlx::query_as("SELECT * FROM plugin_publishers WHERE publisher_id = $1 FOR UPDATE")
            .bind(&request.publisher_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| AppError::not_found("插件发布者不存在"))?;
    if publisher.status != "active" {
        return Err(AppError::forbidden(
            "publisher_revoked",
            "插件发布者已经撤销，不能安装新 Runtime",
        ));
    }
    verify_install_signature(&publisher.public_key, &request.signature, &statement_digest)?;
    let package_id = Uuid::new_v4();
    let manifest_value = serde_json::to_value(&request.manifest)?;
    let inserted_package: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO plugin_packages \
         (id, plugin_id, version, content_digest, manifest) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (plugin_id, version, content_digest) DO NOTHING RETURNING id",
    )
    .bind(package_id)
    .bind(&request.manifest.plugin_id)
    .bind(&request.manifest.version)
    .bind(&request.manifest.content_digest)
    .bind(Json(manifest_value))
    .fetch_optional(&mut *transaction)
    .await?;
    let package_id = if let Some(package_id) = inserted_package {
        package_id
    } else {
        sqlx::query_scalar(
            "SELECT id FROM plugin_packages \
             WHERE plugin_id = $1 AND version = $2 AND content_digest = $3",
        )
        .bind(&request.manifest.plugin_id)
        .bind(&request.manifest.version)
        .bind(&request.manifest.content_digest)
        .fetch_one(&mut *transaction)
        .await?
    };
    let locked_package_id: Uuid =
        sqlx::query_scalar("SELECT id FROM plugin_packages WHERE id = $1 FOR UPDATE")
            .bind(package_id)
            .fetch_one(&mut *transaction)
            .await?;
    persist_and_verify_plugin_resources(&mut transaction, locked_package_id, &resources).await?;
    if let Some(existing) = sqlx::query_as::<_, PluginInstallationRecord>(
        "SELECT * FROM plugin_installations WHERE plugin_package_id = $1",
    )
    .bind(package_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.publisher_id != request.publisher_id
            || existing.signature != request.signature
            || existing.statement_digest != statement_digest
            || existing.self_test_digest != self_test_digest
        {
            return Err(AppError::conflict(
                "plugin_installation_conflict",
                "同一不可变插件包已有不同安装证明",
            ));
        }
        transaction.commit().await?;
        return Ok(existing);
    }
    let installation = sqlx::query_as::<_, PluginInstallationRecord>(
        "INSERT INTO plugin_installations \
         (id, plugin_package_id, publisher_id, signature, statement, statement_digest, \
          runtime_image_digest, runtime_entry_digest, runner_digest, self_test, \
          self_test_digest) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(package_id)
    .bind(&request.publisher_id)
    .bind(&request.signature)
    .bind(Json(serde_json::to_value(&statement)?))
    .bind(&statement_digest)
    .bind(&request.manifest.runtime.content_digest)
    .bind(&request.self_test.runtime_entry_digest)
    .bind(&request.self_test.runner_digest)
    .bind(Json(serde_json::to_value(&request.self_test)?))
    .bind(&self_test_digest)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(installation)
}

pub async fn revoke_plugin_installation(
    pool: &PgPool,
    installation_id: Uuid,
    request: RevokePluginInstallationRequest,
) -> AppResult<PluginInstallationRecord> {
    let reason = request.reason.trim();
    if reason.is_empty() || reason.chars().count() > 1_000 {
        return Err(AppError::bad_request(
            "invalid_revocation_reason",
            "撤销原因必须是 1 到 1000 字符",
        ));
    }
    let record = sqlx::query_as::<_, PluginInstallationRecord>(
        "UPDATE plugin_installations SET status = 'revoked', revoked_at = now(), \
         revocation_reason = $1 WHERE id = $2 AND status = 'installed' RETURNING *",
    )
    .bind(reason)
    .bind(installation_id)
    .fetch_optional(pool)
    .await?;
    if let Some(record) = record {
        return Ok(record);
    }
    sqlx::query_as::<_, PluginInstallationRecord>(
        "SELECT * FROM plugin_installations WHERE id = $1",
    )
    .bind(installation_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("插件安装记录不存在"))
}

pub async fn revoke_plugin_publisher(
    pool: &PgPool,
    publisher_id: &str,
    request: RevokePluginPublisherRequest,
) -> AppResult<PluginPublisherRecord> {
    let publisher_id = normalize_publisher_id(publisher_id.to_owned())?;
    let reason = request.reason.trim();
    if reason.is_empty() || reason.chars().count() > 1_000 {
        return Err(AppError::bad_request(
            "invalid_revocation_reason",
            "撤销原因必须是 1 到 1000 字符",
        ));
    }
    let record = sqlx::query_as::<_, PluginPublisherRecord>(
        "UPDATE plugin_publishers SET status = 'revoked', revoked_at = now(), \
         revocation_reason = $1 WHERE publisher_id = $2 AND status = 'active' RETURNING *",
    )
    .bind(reason)
    .bind(&publisher_id)
    .fetch_optional(pool)
    .await?;
    if let Some(record) = record {
        return Ok(record);
    }
    sqlx::query_as::<_, PluginPublisherRecord>(
        "SELECT * FROM plugin_publishers WHERE publisher_id = $1",
    )
    .bind(publisher_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("插件发布者不存在"))
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

async fn persist_and_verify_plugin_resources(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    package_id: Uuid,
    resources: &[VerifiedPluginResource],
) -> AppResult<()> {
    for resource in resources {
        sqlx::query(
            "INSERT INTO plugin_package_resources \
             (id, plugin_package_id, path, media_type, content_digest, content) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (plugin_package_id, path) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(package_id)
        .bind(&resource.path)
        .bind(&resource.media_type)
        .bind(&resource.content_digest)
        .bind(&resource.content)
        .execute(&mut **transaction)
        .await?;
    }
    let stored = sqlx::query_as::<_, PluginResourceRow>(
        "SELECT plugin_package_id, path, media_type, content_digest, content \
         FROM plugin_package_resources WHERE plugin_package_id = $1 ORDER BY path",
    )
    .bind(package_id)
    .fetch_all(&mut **transaction)
    .await?;
    if stored.len() != resources.len()
        || stored.iter().zip(resources).any(|(stored, expected)| {
            stored.plugin_package_id != package_id
                || stored.path != expected.path
                || stored.media_type != expected.media_type
                || stored.content_digest != expected.content_digest
                || stored.content != expected.content
        })
    {
        return Err(AppError::conflict(
            "plugin_resource_conflict",
            "同一不可变插件包已有不同或不完整的资源内容",
        ));
    }
    Ok(())
}

async fn load_plugin_resources(
    pool: &PgPool,
    package_id: Uuid,
    manifest: &PluginManifest,
) -> AppResult<Vec<PluginResourceRow>> {
    let rows = sqlx::query_as::<_, PluginResourceRow>(
        "SELECT plugin_package_id, path, media_type, content_digest, content \
         FROM plugin_package_resources WHERE plugin_package_id = $1 ORDER BY path",
    )
    .bind(package_id)
    .fetch_all(pool)
    .await?;
    let declared = manifest
        .assets
        .iter()
        .map(|asset| (asset.path.as_str(), asset.content_digest.as_str()))
        .collect::<BTreeMap<_, _>>();
    let observed = rows
        .iter()
        .map(|row| row.path.as_str())
        .collect::<BTreeSet<_>>();
    if declared.keys().copied().collect::<BTreeSet<_>>() != observed {
        return Err(AppError::conflict(
            "plugin_resource_incomplete",
            "插件资源与不可变 Manifest 不完整对应",
        ));
    }
    for row in &rows {
        let actual = format!("sha256:{}", hex::encode(Sha256::digest(&row.content)));
        if row.plugin_package_id != package_id
            || row.content_digest != actual
            || declared.get(row.path.as_str()).copied() != Some(actual.as_str())
            || row.media_type != expected_plugin_resource_media_type(&row.path)
        {
            return Err(AppError::conflict(
                "plugin_resource_corrupt",
                format!("插件资源 {} 的内容或摘要不可信", row.path),
            ));
        }
    }
    Ok(rows)
}

fn plugin_resource_metadata(row: PluginResourceRow) -> PluginResourceMetadata {
    PluginResourceMetadata {
        path: row.path,
        media_type: row.media_type,
        content_digest: row.content_digest,
        byte_length: row.content.len() as u32,
    }
}

pub async fn list_catalog(pool: &PgPool) -> AppResult<Vec<PluginCatalogEntry>> {
    let rows: Vec<(Json<Value>, Option<String>)> = sqlx::query_as(
        "SELECT p.manifest, publisher.publisher_id \
         FROM plugin_packages p \
         LEFT JOIN plugin_installations i \
           ON i.plugin_package_id = p.id AND i.status = 'installed' \
         LEFT JOIN plugin_publishers publisher \
           ON publisher.publisher_id = i.publisher_id AND publisher.status = 'active' \
         ORDER BY p.plugin_id, p.version, p.content_digest",
    )
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|(row, publisher_id)| {
            let manifest: PluginManifest = serde_json::from_value(row.0)?;
            let mut summary = manifest.summary();
            summary.installed = publisher_id.is_some() || manifest.runtime.kind == "mock";
            summary.publisher_id = publisher_id;
            Ok(summary)
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

pub async fn get_plugin_detail(
    pool: &PgPool,
    plugin_id: &str,
    version: &str,
) -> AppResult<PluginDetail> {
    let manifest = get_plugin(pool, plugin_id, version).await?;
    let installation = sqlx::query_as::<_, PluginInstallationRecord>(
        "SELECT i.* FROM plugin_installations i \
         JOIN plugin_packages p ON p.id = i.plugin_package_id \
         WHERE p.plugin_id = $1 AND p.version = $2 AND p.content_digest = $3",
    )
    .bind(&manifest.plugin_id)
    .bind(&manifest.version)
    .bind(&manifest.content_digest)
    .fetch_optional(pool)
    .await?;
    let publisher = if let Some(installation) = &installation {
        sqlx::query_as::<_, PluginPublisherRecord>(
            "SELECT * FROM plugin_publishers WHERE publisher_id = $1",
        )
        .bind(&installation.publisher_id)
        .fetch_optional(pool)
        .await?
    } else {
        None
    };
    let package_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM plugin_packages \
         WHERE plugin_id = $1 AND version = $2 AND content_digest = $3",
    )
    .bind(&manifest.plugin_id)
    .bind(&manifest.version)
    .bind(&manifest.content_digest)
    .fetch_one(pool)
    .await?;
    let resources = load_plugin_resources(pool, package_id, &manifest)
        .await?
        .into_iter()
        .map(plugin_resource_metadata)
        .collect();
    Ok(PluginDetail {
        manifest,
        installation,
        publisher,
        resources,
    })
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
        let package: Option<(Json<Value>, bool)> = sqlx::query_as(
            "SELECT p.manifest, EXISTS (\
               SELECT 1 FROM plugin_installations i \
               JOIN plugin_publishers publisher ON publisher.publisher_id = i.publisher_id \
               WHERE i.plugin_package_id = p.id AND i.status = 'installed' \
                 AND publisher.status = 'active'\
             ) AS trusted \
             FROM plugin_packages p \
             WHERE p.plugin_id = $1 AND p.version = $2 AND p.content_digest = $3",
        )
        .bind(&plugin.plugin_id)
        .bind(&plugin.version)
        .bind(&plugin.content_digest)
        .fetch_optional(pool)
        .await?;
        let Some((stored_manifest, trusted)) = package else {
            return Err(AppError::bad_request(
                "plugin_not_found",
                "EnvironmentManifest 引用了未注册的固定插件",
            ));
        };
        let stored_manifest =
            serde_json::from_value::<PluginManifest>(stored_manifest.0)?.verify_seal()?;
        if stored_manifest.runtime.kind == "oci" && !trusted {
            return Err(AppError::forbidden(
                "plugin_not_installed",
                "EnvironmentManifest 不能固定未签名、已撤销或发布者失效的 OCI 插件",
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
        let context_snapshot_id: Option<Uuid> =
            sqlx::query_scalar("SELECT context_snapshot_id FROM goal_sessions WHERE id = $1")
                .bind(session_id)
                .fetch_one(&mut *transaction)
                .await?;
        if context_snapshot_id.is_none() {
            context_memory::create_snapshot(
                &mut transaction,
                project_id,
                session_id,
                None,
                None,
                request.client_request_id,
            )
            .await?;
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
    let previous_snapshot: Option<Uuid> =
        sqlx::query_scalar("SELECT context_snapshot_id FROM goal_sessions WHERE id = $1")
            .bind(session_id)
            .fetch_one(&mut *transaction)
            .await?;
    context_memory::create_snapshot(
        &mut transaction,
        project_id,
        session_id,
        previous_snapshot,
        previous_snapshot.map(|_| session_id),
        request.client_request_id,
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

pub async fn read_plugin_context(
    pool: &PgPool,
    expected_runner_digest: &str,
    project_id: Uuid,
    session_id: Uuid,
    mut request: ReadPluginContextRequest,
) -> AppResult<ReadPluginContextResponse> {
    if request.resources.len() > 16 {
        return Err(AppError::bad_request(
            "too_many_plugin_resources",
            "一次最多按需读取 16 个插件资源",
        ));
    }
    for selector in &mut request.resources {
        selector.plugin = selector.plugin.clone().validate()?;
        selector.path = normalize_plugin_resource_path(std::mem::take(&mut selector.path))?;
    }
    request.resources.sort_by(|left, right| {
        left.plugin
            .cmp(&right.plugin)
            .then_with(|| left.path.cmp(&right.path))
    });
    if request.resources.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(AppError::bad_request(
            "duplicate_plugin_resource",
            "按需资源请求不能包含重复路径",
        ));
    }
    let request_hash = canonical_json_sha256(&request)?;
    let binding: Option<(String, Json<Value>)> = sqlx::query_as(
        "SELECT b.environment_fingerprint, e.manifest \
         FROM session_environment_bindings b \
         JOIN environment_manifests e ON e.id = b.environment_manifest_id \
         WHERE b.project_id = $1 AND b.session_id = $2",
    )
    .bind(project_id)
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    let Some((environment_fingerprint, environment_json)) = binding else {
        let session_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM goal_sessions WHERE id = $1 AND project_id = $2)",
        )
        .bind(session_id)
        .bind(project_id)
        .fetch_one(pool)
        .await?;
        return if session_exists {
            Err(AppError::conflict(
                "environment_not_bound",
                "Session 尚未固定 EnvironmentManifest",
            ))
        } else {
            Err(AppError::not_found("Agent Session 不存在"))
        };
    };
    let environment =
        serde_json::from_value::<EnvironmentManifest>(environment_json.0)?.normalize()?;
    if environment.fingerprint()? != environment_fingerprint {
        return Err(AppError::conflict(
            "environment_fingerprint_mismatch",
            "Session 的 EnvironmentManifest 内容与指纹不一致",
        ));
    }

    let mut bundles = BTreeMap::new();
    for plugin in &environment.plugins {
        let package: Option<(Uuid, Json<Value>)> = sqlx::query_as(
            "SELECT id, manifest FROM plugin_packages \
             WHERE plugin_id = $1 AND version = $2 AND content_digest = $3",
        )
        .bind(&plugin.plugin_id)
        .bind(&plugin.version)
        .bind(&plugin.content_digest)
        .fetch_optional(pool)
        .await?;
        let Some((package_id, manifest_json)) = package else {
            return Err(AppError::conflict(
                "plugin_package_missing",
                "Session 固定的插件包不存在",
            ));
        };
        let manifest = serde_json::from_value::<PluginManifest>(manifest_json.0)?.verify_seal()?;
        if manifest.resolved_ref() != *plugin {
            return Err(AppError::conflict(
                "plugin_package_corrupt",
                "插件包内容与 EnvironmentManifest 固定引用不一致",
            ));
        }
        if manifest.runtime.kind == "oci" {
            load_active_installation(pool, package_id, &manifest, expected_runner_digest).await?;
        }
        let resources = load_plugin_resources(pool, package_id, &manifest).await?;
        bundles.insert(plugin.clone(), (manifest, resources));
    }

    let mut plugins = Vec::with_capacity(environment.plugins.len());
    for plugin in &environment.plugins {
        let (manifest, resources) = bundles.get(plugin).ok_or_else(|| {
            AppError::conflict("plugin_package_missing", "Session 插件上下文不完整")
        })?;
        let skill = if request.include_skills {
            manifest
                .skill
                .as_ref()
                .map(|skill| {
                    let row = resources
                        .iter()
                        .find(|row| row.path == skill.entry)
                        .ok_or_else(|| {
                            AppError::conflict(
                                "plugin_resource_incomplete",
                                "插件 Skill 入口没有对应的不可变内容",
                            )
                        })?;
                    let content = plugin_resource_content(plugin.clone(), row)?;
                    if content.text.is_none() {
                        return Err(AppError::conflict(
                            "invalid_plugin_skill",
                            "插件 Skill 不是可注入的 UTF-8 文本",
                        ));
                    }
                    Ok(content)
                })
                .transpose()?
        } else {
            None
        };
        plugins.push(AgentPluginContext {
            plugin: plugin.clone(),
            display_name: manifest.display_name.clone(),
            description: manifest.description.clone(),
            capabilities: manifest.capabilities.clone(),
            skill,
            resources: resources
                .iter()
                .cloned()
                .map(plugin_resource_metadata)
                .collect(),
        });
    }

    let mut requested_resources = Vec::with_capacity(request.resources.len());
    for selector in &request.resources {
        let Some((_, resources)) = bundles.get(&selector.plugin) else {
            return Err(AppError::forbidden(
                "plugin_not_in_environment",
                "只能读取当前 Session 固定环境中的插件资源",
            ));
        };
        let row = resources
            .iter()
            .find(|row| row.path == selector.path)
            .ok_or_else(|| AppError::not_found("插件资源不存在"))?;
        requested_resources.push(plugin_resource_content(selector.plugin.clone(), row)?);
    }

    let disclosed_bytes = plugins
        .iter()
        .filter_map(|plugin| plugin.skill.as_ref())
        .chain(requested_resources.iter())
        .try_fold(0_u64, |total, resource| {
            total
                .checked_add(u64::from(resource.byte_length))
                .ok_or_else(|| {
                    AppError::bad_request("plugin_context_too_large", "插件上下文披露大小溢出")
                })
        })?;
    if disclosed_bytes > 1024 * 1024 {
        return Err(AppError::bad_request(
            "plugin_context_too_large",
            "单次插件上下文最多披露 1 MiB；请关闭自动 Skill 并按需选择",
        ));
    }

    let served_resources = plugins
        .iter()
        .filter_map(|plugin| {
            plugin.skill.as_ref().map(|skill| {
                json!({
                    "plugin": plugin.plugin,
                    "path": skill.path,
                    "contentDigest": skill.content_digest,
                    "purpose": "skill_bootstrap",
                })
            })
        })
        .chain(requested_resources.iter().map(|resource| {
            json!({
                "plugin": resource.plugin,
                "path": resource.path,
                "contentDigest": resource.content_digest,
                "purpose": "on_demand",
            })
        }))
        .collect::<Vec<_>>();
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO plugin_resource_reads \
         (id, project_id, session_id, client_request_id, request_hash, \
          environment_fingerprint, requested_resources, served_resources) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (project_id, client_request_id) DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(session_id)
    .bind(request.client_request_id)
    .bind(&request_hash)
    .bind(&environment_fingerprint)
    .bind(Json(serde_json::to_value(&request.resources)?))
    .bind(Json(serde_json::to_value(&served_resources)?))
    .fetch_optional(pool)
    .await?;
    if inserted.is_none() {
        let saved: (String, Uuid, String) = sqlx::query_as(
            "SELECT request_hash, session_id, environment_fingerprint \
             FROM plugin_resource_reads \
             WHERE project_id = $1 AND client_request_id = $2",
        )
        .bind(project_id)
        .bind(request.client_request_id)
        .fetch_one(pool)
        .await?;
        if saved.0 != request_hash || saved.1 != session_id || saved.2 != environment_fingerprint {
            return Err(AppError::conflict(
                "idempotency_key_reused",
                "clientRequestId 已用于不同的插件上下文读取",
            ));
        }
    }
    Ok(ReadPluginContextResponse {
        schema_version: 1,
        replayed: inserted.is_none(),
        project_id,
        session_id,
        environment_fingerprint,
        plugins,
        requested_resources,
    })
}

fn plugin_resource_content(
    plugin: ResolvedPluginRef,
    row: &PluginResourceRow,
) -> AppResult<PluginResourceContent> {
    let textual = row.media_type.starts_with("text/")
        || row.media_type == "application/json"
        || row.media_type.ends_with("+json");
    let text = if textual {
        Some(String::from_utf8(row.content.clone()).map_err(|_| {
            AppError::conflict(
                "plugin_resource_corrupt",
                format!("文本插件资源 {} 不是 UTF-8", row.path),
            )
        })?)
    } else {
        None
    };
    Ok(PluginResourceContent {
        plugin,
        path: row.path.clone(),
        media_type: row.media_type.clone(),
        content_digest: row.content_digest.clone(),
        byte_length: row.content.len() as u32,
        encoding: "base64",
        content_base64: STANDARD.encode(&row.content),
        text,
    })
}

fn default_true() -> bool {
    true
}

pub async fn create_plugin_install_request(
    pool: &PgPool,
    project_id: Uuid,
    session_id: Uuid,
    mut request: CreatePluginInstallRequest,
) -> AppResult<PluginInstallRequestRecord> {
    let selector = PluginSelector {
        plugin_id: request.plugin_id,
        version: request.version_requirement,
    }
    .normalize()?;
    request.plugin_id = selector.plugin_id;
    request.version_requirement = selector.version;
    request.capability = request.capability.trim().to_owned();
    request.reason = request.reason.trim().to_owned();
    if request.capability.is_empty()
        || request.capability.chars().count() > 200
        || request.reason.is_empty()
        || request.reason.chars().count() > 2_000
    {
        return Err(AppError::bad_request(
            "invalid_plugin_install_request",
            "安装请求的能力或原因为空或过长",
        ));
    }
    let identity = CommandReceiptIdentity::from_input("plugin.install_request", &request)?;
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    if let Some(existing) = sqlx::query_as::<_, PluginInstallRequestRecord>(
        "SELECT * FROM plugin_install_requests \
         WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.request_hash != identity.input_hash {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "同一 clientRequestId 已用于不同插件安装请求",
            ));
        }
        transaction.commit().await?;
        return Ok(existing);
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
            "只有 running Session 可以提出插件安装请求",
        ));
    }
    let record = sqlx::query_as::<_, PluginInstallRequestRecord>(
        "INSERT INTO plugin_install_requests \
         (id, project_id, goal_branch_id, session_id, client_request_id, request_hash, \
          plugin_id, version_requirement, capability, reason) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(session.goal_branch_id)
    .bind(session_id)
    .bind(request.client_request_id)
    .bind(&identity.input_hash)
    .bind(&request.plugin_id)
    .bind(&request.version_requirement)
    .bind(&request.capability)
    .bind(&request.reason)
    .fetch_one(&mut *transaction)
    .await?;
    insert_tool_event(
        &mut transaction,
        project_id,
        record.id,
        "plugin.install_requested",
        request.client_request_id,
        json!({
            "sessionId": session_id,
            "goalBranchId": session.goal_branch_id,
            "pluginId": request.plugin_id,
            "versionRequirement": request.version_requirement,
            "capability": request.capability,
            "reasonHash": crate::goal_domain::canonical_json_sha256(&request.reason)?,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(record)
}

pub async fn prepare_real_tool(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    session_id: Uuid,
    request: PrepareRealToolRequest,
) -> AppResult<PrepareRealToolResponse> {
    let request_identity = CommandReceiptIdentity::from_input("tool.execute.real", &request)?;
    let requested_plugin = request.plugin.clone().validate()?;
    let session: SessionToolState = sqlx::query_as(
        "SELECT goal_branch_id, status, environment_fingerprint \
         FROM goal_sessions WHERE id = $1 AND project_id = $2",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("Agent Session 不存在"))?;
    if SessionStatus::try_from(session.status.as_str())? != SessionStatus::Running {
        return Err(AppError::conflict(
            "invalid_state_transition",
            "只有 running Session 可以调用真实插件",
        ));
    }
    let environment_fingerprint = session.environment_fingerprint.clone().ok_or_else(|| {
        AppError::conflict(
            "environment_not_bound",
            "Session 尚未固定 EnvironmentManifest",
        )
    })?;
    let environment: EnvironmentRecord = sqlx::query_as(
        "SELECT e.* FROM session_environment_bindings b \
         JOIN environment_manifests e ON e.id = b.environment_manifest_id \
         WHERE b.session_id = $1 AND b.environment_fingerprint = $2",
    )
    .bind(session_id)
    .bind(&environment_fingerprint)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::conflict(
            "environment_fingerprint_mismatch",
            "Session 环境指纹没有对应的不可变 Manifest",
        )
    })?;
    validate_environment_record(&environment)?;
    let environment_manifest =
        serde_json::from_value::<EnvironmentManifest>(environment.manifest.0.clone())?
            .normalize()?;
    if environment_manifest.fingerprint()? != environment_fingerprint {
        return Err(AppError::conflict(
            "environment_fingerprint_mismatch",
            "持久化 EnvironmentManifest 与 Session 指纹不一致",
        ));
    }
    if environment_manifest.base_runtime.kind != "fudian-runner"
        || environment_manifest.base_runtime.digest != config.runner_runtime_digest
    {
        return Err(AppError::conflict(
            "runner_digest_mismatch",
            "真实工具环境没有固定当前 Runner Runtime 摘要",
        ));
    }
    if environment_manifest.network_policy != "denied" {
        return Err(AppError::forbidden(
            "network_adapter_unavailable",
            "当前真实插件阶段只执行完全断网环境",
        ));
    }
    if !environment_manifest.plugins.contains(&requested_plugin) {
        return Err(AppError::conflict(
            "tool_not_allowed",
            "Session 的固定环境没有包含该插件版本",
        ));
    }

    let (package_id, manifest_json): (Uuid, Json<Value>) = sqlx::query_as(
        "SELECT id, manifest FROM plugin_packages \
         WHERE plugin_id = $1 AND version = $2 AND content_digest = $3",
    )
    .bind(&requested_plugin.plugin_id)
    .bind(&requested_plugin.version)
    .bind(&requested_plugin.content_digest)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("固定插件包不存在"))?;
    let manifest = serde_json::from_value::<PluginManifest>(manifest_json.0)?.verify_seal()?;
    let installation =
        load_active_installation(pool, package_id, &manifest, &config.runner_runtime_digest)
            .await?;
    if manifest.runtime.kind != "oci" {
        return Err(AppError::conflict(
            "invalid_plugin_runtime",
            "真实 Tool Broker 只接受已签名 OCI Runtime",
        ));
    }
    if manifest.permissions.network != "denied" || manifest.permissions.external_writes {
        return Err(AppError::forbidden(
            "plugin_permission_unavailable",
            "当前 Worker 不提供联网或外部写适配器",
        ));
    }
    if !manifest
        .permissions
        .workspace_read
        .iter()
        .any(|pattern| pattern == "**")
    {
        return Err(AppError::forbidden(
            "narrow_read_adapter_unavailable",
            "当前 Worker 的只读输入挂载按整个 worktree 授权，不能冒充更窄的读取边界",
        ));
    }
    ensure_resource_policy_fits(
        &manifest.resource_hints,
        &environment_manifest.resource_policy,
    )?;
    if request.timeout_seconds > environment_manifest.resource_policy.timeout_seconds {
        return Err(AppError::bad_request(
            "invalid_tool_timeout",
            "ToolCall 超时超过固定环境上限",
        ));
    }
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
    }
    .validate(&manifest)?;
    let descriptor = manifest
        .tools
        .iter()
        .find(|tool| tool.name == call.tool_name)
        .ok_or_else(|| AppError::bad_request("tool_not_found", "插件没有声明这个工具"))?;
    validate_tool_input_schema(&descriptor.input_schema, &call.input)?;

    if let Some(existing_hash) = sqlx::query_scalar::<_, String>(
        "SELECT request_hash FROM tool_calls WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .fetch_optional(pool)
    .await?
    {
        if existing_hash == request_identity.input_hash {
            return Err(AppError::conflict(
                "tool_call_already_completed",
                "这个真实工具请求已经完成，请读取原 ToolCall",
            ));
        }
        return Err(AppError::conflict(
            "idempotency_conflict",
            "同一 clientRequestId 已用于不同 ToolCall",
        ));
    }

    let input_json = serde_json::to_string(&call.input)?;
    let runner_request = PrepareRunnerJobRequest {
        client_request_id: request.client_request_id,
        base_workspace_snapshot: call.base_workspace_snapshot.clone(),
        runtime_entry_digest: Some(installation.runtime_entry_digest.clone()),
        allowed_writes: call.allowed_writes.clone(),
        delete_paths: request.delete_paths.clone(),
        capabilities: RunnerCapabilities {
            network: "denied".to_owned(),
            external_writes: Vec::new(),
            account_references: Vec::new(),
            paid_operations: false,
            deployment: false,
        },
        resources: RunnerResourceLimits {
            cpu_millis: manifest.resource_hints.cpu_millis,
            memory_mi_b: manifest.resource_hints.memory_mi_b,
            disk_mi_b: manifest.resource_hints.disk_mi_b,
            pids: manifest.resource_hints.pids,
            timeout_seconds: call.timeout_seconds,
            stdout_bytes: manifest.resource_hints.stdout_bytes,
            stderr_bytes: manifest.resource_hints.stderr_bytes,
        },
        command: RunnerCommand {
            program: manifest.runtime.entrypoint.clone(),
            args: vec![
                "execute".to_owned(),
                manifest.plugin_id.clone(),
                manifest.version.clone(),
                call.tool_name.clone(),
                input_json,
            ],
            environment: Default::default(),
        },
    };
    let runner =
        workspaces::prepare_runner_job(pool, config, project_id, session_id, runner_request)
            .await?;

    let execution_id = Uuid::new_v4();
    let inserted: AppResult<(Uuid, bool)> = async {
        let mut transaction = pool.begin().await?;
        let trust: Option<(String, String)> = sqlx::query_as(
            "SELECT i.status, publisher.status FROM plugin_installations i \
             JOIN plugin_publishers publisher ON publisher.publisher_id = i.publisher_id \
             WHERE i.id = $1 FOR UPDATE OF i, publisher",
        )
        .bind(installation.id)
        .fetch_optional(&mut *transaction)
        .await?;
        if trust
            .as_ref()
            .is_none_or(|(installation_status, publisher_status)| {
                installation_status != "installed" || publisher_status != "active"
            })
        {
            return Err(AppError::forbidden(
                "plugin_revoked",
                "插件或发布者在 RunnerJob 准备期间被撤销",
            ));
        }
        let inserted_id: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO tool_execution_requests \
             (id, project_id, goal_branch_id, session_id, runner_job_id, \
              plugin_installation_id, client_request_id, request_hash, plugin_id, \
              plugin_version, plugin_digest, runtime_image_digest, runtime_entry_digest, \
              tool_name, input, environment_fingerprint, base_workspace_snapshot) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, \
                     $14, $15, $16, $17) \
             ON CONFLICT (project_id, client_request_id) DO NOTHING RETURNING id",
        )
        .bind(execution_id)
        .bind(project_id)
        .bind(session.goal_branch_id)
        .bind(session_id)
        .bind(runner.job_id)
        .bind(installation.id)
        .bind(request.client_request_id)
        .bind(&request_identity.input_hash)
        .bind(&requested_plugin.plugin_id)
        .bind(&requested_plugin.version)
        .bind(&requested_plugin.content_digest)
        .bind(&installation.runtime_image_digest)
        .bind(&installation.runtime_entry_digest)
        .bind(&call.tool_name)
        .bind(Json(call.input.clone()))
        .bind(&environment_fingerprint)
        .bind(&call.base_workspace_snapshot)
        .fetch_optional(&mut *transaction)
        .await?;
        let (persisted_id, replayed) = if let Some(id) = inserted_id {
            insert_tool_event(
                &mut transaction,
                project_id,
                id,
                "tool.execution_prepared",
                request.client_request_id,
                json!({
                    "sessionId": session_id,
                    "goalBranchId": session.goal_branch_id,
                    "runnerJobId": runner.job_id,
                    "pluginInstallationId": installation.id,
                    "plugin": requested_plugin,
                    "runtimeImageDigest": installation.runtime_image_digest,
                    "runtimeEntryDigest": installation.runtime_entry_digest,
                    "environmentFingerprint": environment_fingerprint,
                    "baseWorkspaceSnapshot": call.base_workspace_snapshot,
                }),
            )
            .await?;
            (id, false)
        } else {
            let existing: ToolExecutionRecord = sqlx::query_as(
                "SELECT * FROM tool_execution_requests \
                 WHERE project_id = $1 AND client_request_id = $2",
            )
            .bind(project_id)
            .bind(request.client_request_id)
            .fetch_one(&mut *transaction)
            .await?;
            if existing.request_hash != request_identity.input_hash
                || existing.runner_job_id != runner.job_id
                || existing.plugin_installation_id != installation.id
            {
                return Err(AppError::conflict(
                    "idempotency_conflict",
                    "同一 clientRequestId 已用于不同真实 ToolCall",
                ));
            }
            (existing.id, true)
        };
        transaction.commit().await?;
        Ok((persisted_id, replayed))
    }
    .await;
    let (execution_id, execution_replayed) = match inserted {
        Ok(value) => value,
        Err(error) => {
            if let Some(lease_token) = runner.lease_token.clone() {
                let _ = workspaces::fail_runner_job(
                    pool,
                    project_id,
                    session_id,
                    runner.job_id,
                    FailRunnerJobRequest {
                        lease_token,
                        failure_kind: "cancelled".to_owned(),
                        summary: "真实插件执行记录未能安全持久化".to_owned(),
                    },
                )
                .await;
            }
            return Err(error);
        }
    };
    Ok(PrepareRealToolResponse {
        replayed: runner.replayed || execution_replayed,
        execution_id,
        runtime_image_digest: installation.runtime_image_digest,
        runtime_entry_digest: installation.runtime_entry_digest,
        runner,
    })
}

pub async fn finalize_real_tool(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    session_id: Uuid,
    execution_id: Uuid,
    job_id: Uuid,
    request: FinalizeRealToolRequest,
) -> AppResult<FinalizeRealToolResponse> {
    let execution: ToolExecutionRecord = sqlx::query_as(
        "SELECT * FROM tool_execution_requests \
         WHERE id = $1 AND project_id = $2 AND session_id = $3 AND runner_job_id = $4",
    )
    .bind(execution_id)
    .bind(project_id)
    .bind(session_id)
    .bind(job_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("真实 ToolExecution 不存在"))?;
    if execution.project_id != project_id
        || execution.session_id != session_id
        || execution.runner_job_id != job_id
    {
        return Err(AppError::conflict(
            "tool_execution_scope_mismatch",
            "ToolExecution、Session 与 RunnerJob 作用域不一致",
        ));
    }

    let runner_status: String = sqlx::query_scalar("SELECT status FROM runner_jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?;
    if !is_terminal_runner_status(&runner_status) {
        let proof = load_installation_by_id(
            pool,
            execution.plugin_installation_id,
            &config.runner_runtime_digest,
            true,
        )
        .await;
        if let Err(error) = proof {
            let _ = workspaces::fail_runner_job(
                pool,
                project_id,
                session_id,
                job_id,
                FailRunnerJobRequest {
                    lease_token: request.lease_token.clone(),
                    failure_kind: "cancelled".to_owned(),
                    summary: "插件或发布者在结果回写前已撤销".to_owned(),
                },
            )
            .await;
            return Err(error);
        }
    }

    let runner = workspaces::finalize_runner_job(
        pool,
        config,
        project_id,
        session_id,
        job_id,
        FinalizeRunnerJobRequest {
            lease_token: request.lease_token,
            result: request.result,
        },
    )
    .await?;

    if let Some(saved) = load_saved_real_tool_call(pool, project_id, job_id).await? {
        return Ok(FinalizeRealToolResponse {
            replayed: true,
            execution_id,
            runner,
            tool_call: ExecuteToolResponse {
                replayed: true,
                ..saved
            },
        });
    }

    let runner_audit: RunnerAuditRecord = sqlx::query_as(
        "SELECT status, spec, result, created_at, started_at, completed_at \
         FROM runner_jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_one(pool)
    .await?;
    let spec: RunnerJobSpec = serde_json::from_value(runner_audit.spec.0)?;
    let stored_result: RunnerExecutionResult = serde_json::from_value(
        runner_audit
            .result
            .ok_or_else(|| AppError::conflict("runner_result_missing", "Runner 终态缺少结果证明"))?
            .0,
    )?;
    if stored_result.job_id != job_id
        || spec.runtime_entry_digest.as_deref() != Some(execution.runtime_entry_digest.as_str())
    {
        return Err(AppError::conflict(
            "runner_result_identity_mismatch",
            "Runner 终态没有绑定准确 ToolExecution 或 Runtime 入口",
        ));
    }
    let (manifest, installation) = load_installation_by_id(
        pool,
        execution.plugin_installation_id,
        &spec.runtime_digest,
        false,
    )
    .await?;
    if manifest.resolved_ref()
        != (ResolvedPluginRef {
            plugin_id: execution.plugin_id.clone(),
            version: execution.plugin_version.clone(),
            content_digest: execution.plugin_digest.clone(),
        })
        || installation.runtime_image_digest != execution.runtime_image_digest
    {
        return Err(AppError::conflict(
            "plugin_installation_conflict",
            "ToolExecution 与不可变插件安装证明不一致",
        ));
    }
    let descriptor = manifest
        .tools
        .iter()
        .find(|tool| tool.name == execution.tool_name)
        .ok_or_else(|| AppError::conflict("tool_not_found", "持久化工具入口不在插件 Manifest"))?;
    let started_at = runner_audit.started_at.unwrap_or(runner_audit.created_at);
    let completed_at = runner_audit.completed_at.unwrap_or_else(Utc::now);
    let changes = stored_result
        .files
        .iter()
        .map(|file| {
            json!({
                "kind": "write",
                "path": file.path,
                "sha256": file.sha256,
                "sizeBytes": file.size_bytes,
                "executable": file.executable,
            })
        })
        .chain(
            spec.delete_paths
                .iter()
                .map(|path| json!({ "kind": "delete", "path": path })),
        )
        .collect::<Vec<_>>();
    let tool_result = ToolResult {
        call_id: execution.id,
        status: runner_audit.status.clone(),
        output: json!({
            "runnerJobId": job_id,
            "headCommit": runner.head_commit,
            "specHash": stored_result.spec_hash,
            "outputManifestHash": stored_result.output_manifest_hash,
            "files": stored_result.files,
            "exitCode": stored_result.exit_code,
            "diagnostics": stored_result.diagnostics,
        }),
        base_workspace_snapshot: execution.base_workspace_snapshot.clone(),
        result_workspace_snapshot: runner.workspace_snapshot.clone(),
        environment_fingerprint: execution.environment_fingerprint.clone(),
        change_set: changes,
        artifacts: Vec::new(),
        evidence: vec![json!({
            "kind": "signed_oci_tool_execution",
            "pluginInstallationId": installation.id,
            "publisherId": installation.publisher_id,
            "runtimeImageDigest": execution.runtime_image_digest,
            "runtimeEntryDigest": execution.runtime_entry_digest,
            "runnerDigest": spec.runtime_digest,
            "runnerJobId": job_id,
            "headCommit": runner.head_commit,
            "workspaceSnapshot": runner.workspace_snapshot,
            "outputManifestHash": stored_result.output_manifest_hash,
        })],
        log_reference: Some(format!("runner-job:{job_id}")),
        retry_safety: descriptor.idempotency.clone(),
        started_at,
        completed_at,
    };
    let plugin = manifest.resolved_ref();
    let response = ExecuteToolResponse {
        replayed: false,
        project_id,
        goal_branch_id: execution.goal_branch_id,
        session_id,
        plugin,
        environment_fingerprint: execution.environment_fingerprint.clone(),
        result: tool_result,
    };
    let response_json = serde_json::to_value(&response)?;
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await?;
    if let Some(saved) = sqlx::query_scalar::<_, Json<Value>>(
        "SELECT result FROM tool_calls WHERE project_id = $1 AND runner_job_id = $2",
    )
    .bind(project_id)
    .bind(job_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        let saved: ExecuteToolResponse = serde_json::from_value(saved.0)?;
        transaction.commit().await?;
        return Ok(FinalizeRealToolResponse {
            replayed: true,
            execution_id,
            runner,
            tool_call: ExecuteToolResponse {
                replayed: true,
                ..saved
            },
        });
    }
    sqlx::query(
        "INSERT INTO tool_calls \
         (id, project_id, goal_branch_id, session_id, runner_job_id, client_request_id, \
          request_hash, plugin_id, plugin_version, plugin_digest, tool_name, input, \
          environment_fingerprint, base_workspace_snapshot, allowed_writes, timeout_seconds, \
          status, result, started_at, completed_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, \
                 $15, $16, $17, $18, $19, $20)",
    )
    .bind(execution.id)
    .bind(project_id)
    .bind(execution.goal_branch_id)
    .bind(session_id)
    .bind(job_id)
    .bind(execution.client_request_id)
    .bind(&execution.request_hash)
    .bind(&execution.plugin_id)
    .bind(&execution.plugin_version)
    .bind(&execution.plugin_digest)
    .bind(&execution.tool_name)
    .bind(execution.input.clone())
    .bind(&execution.environment_fingerprint)
    .bind(&execution.base_workspace_snapshot)
    .bind(Json(spec.allowed_writes.clone()))
    .bind(
        i32::try_from(spec.resources.timeout_seconds)
            .map_err(|_| AppError::bad_request("invalid_tool_timeout", "工具超时超出数据库范围"))?,
    )
    .bind(&runner_audit.status)
    .bind(Json(response_json))
    .bind(started_at)
    .bind(completed_at)
    .execute(&mut *transaction)
    .await?;
    insert_tool_event(
        &mut transaction,
        project_id,
        execution.id,
        "tool.call_completed",
        execution.client_request_id,
        json!({
            "sessionId": session_id,
            "goalBranchId": execution.goal_branch_id,
            "runnerJobId": job_id,
            "pluginInstallationId": execution.plugin_installation_id,
            "plugin": manifest.resolved_ref(),
            "runtimeImageDigest": execution.runtime_image_digest,
            "runtimeEntryDigest": execution.runtime_entry_digest,
            "environmentFingerprint": execution.environment_fingerprint,
            "baseWorkspaceSnapshot": execution.base_workspace_snapshot,
            "resultWorkspaceSnapshot": runner.workspace_snapshot,
            "status": runner_audit.status,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(FinalizeRealToolResponse {
        replayed: false,
        execution_id,
        runner,
        tool_call: response,
    })
}

async fn load_active_installation(
    pool: &PgPool,
    package_id: Uuid,
    manifest: &PluginManifest,
    expected_runner_digest: &str,
) -> AppResult<PluginInstallationRecord> {
    let installation = sqlx::query_as::<_, PluginInstallationRecord>(
        "SELECT * FROM plugin_installations WHERE plugin_package_id = $1",
    )
    .bind(package_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| {
        AppError::forbidden("plugin_not_installed", "OCI 插件没有受信任的签名安装证明")
    })?;
    let (_, installation) = validate_installation(
        pool,
        manifest.clone(),
        installation,
        expected_runner_digest,
        true,
    )
    .await?;
    Ok(installation)
}

pub(crate) async fn load_installation_by_id(
    pool: &PgPool,
    installation_id: Uuid,
    expected_runner_digest: &str,
    require_active: bool,
) -> AppResult<(PluginManifest, PluginInstallationRecord)> {
    let installation = sqlx::query_as::<_, PluginInstallationRecord>(
        "SELECT * FROM plugin_installations WHERE id = $1",
    )
    .bind(installation_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("插件安装记录不存在"))?;
    let manifest_json: Json<Value> =
        sqlx::query_scalar("SELECT manifest FROM plugin_packages WHERE id = $1")
            .bind(installation.plugin_package_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::conflict("plugin_package_missing", "插件包不存在"))?;
    let manifest = serde_json::from_value::<PluginManifest>(manifest_json.0)?.verify_seal()?;
    validate_installation(
        pool,
        manifest,
        installation,
        expected_runner_digest,
        require_active,
    )
    .await
}

async fn validate_installation(
    pool: &PgPool,
    manifest: PluginManifest,
    installation: PluginInstallationRecord,
    expected_runner_digest: &str,
    require_active: bool,
) -> AppResult<(PluginManifest, PluginInstallationRecord)> {
    let publisher: PluginPublisherRecord =
        sqlx::query_as("SELECT * FROM plugin_publishers WHERE publisher_id = $1")
            .bind(&installation.publisher_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::conflict("publisher_missing", "插件安装证明的发布者不存在"))?;
    if require_active && (installation.status != "installed" || publisher.status != "active") {
        return Err(AppError::forbidden(
            "plugin_revoked",
            "插件安装或发布者已经撤销",
        ));
    }
    if installation.runtime_image_digest != manifest.runtime.content_digest
        || installation.runner_digest != expected_runner_digest
    {
        return Err(AppError::conflict(
            "plugin_installation_conflict",
            "插件安装证明与 Runtime 或当前 Runner 摘要不一致",
        ));
    }
    let self_test =
        serde_json::from_value::<PluginSelfTest>(installation.self_test.0.clone())?.normalize()?;
    if self_test.digest()? != installation.self_test_digest
        || self_test.runner_digest != installation.runner_digest
        || self_test.runtime_entry_digest != installation.runtime_entry_digest
    {
        return Err(AppError::conflict(
            "plugin_self_test_corrupt",
            "插件自检内容与不可变摘要不一致",
        ));
    }
    let statement_request = PluginInstallStatementRequest {
        manifest: manifest.clone(),
        publisher_id: installation.publisher_id.clone(),
        self_test,
    }
    .normalize()?;
    let expected_statement = statement_request.statement()?;
    let expected_statement_value = serde_json::to_value(&expected_statement)?;
    if expected_statement_value != installation.statement.0
        || statement_request.statement_digest()? != installation.statement_digest
    {
        return Err(AppError::conflict(
            "plugin_statement_corrupt",
            "插件安装声明与签名摘要不一致",
        ));
    }
    verify_install_signature(
        &publisher.public_key,
        &installation.signature,
        &installation.statement_digest,
    )?;
    Ok((manifest, installation))
}

async fn load_saved_real_tool_call(
    pool: &PgPool,
    project_id: Uuid,
    job_id: Uuid,
) -> AppResult<Option<ExecuteToolResponse>> {
    let saved: Option<Json<Value>> = sqlx::query_scalar(
        "SELECT result FROM tool_calls WHERE project_id = $1 AND runner_job_id = $2",
    )
    .bind(project_id)
    .bind(job_id)
    .fetch_optional(pool)
    .await?;
    saved
        .map(|saved| serde_json::from_value(saved.0).map_err(AppError::from))
        .transpose()
}

fn ensure_resource_policy_fits(
    requested: &crate::tooling::ResourcePolicy,
    maximum: &crate::tooling::ResourcePolicy,
) -> AppResult<()> {
    if requested.cpu_millis > maximum.cpu_millis
        || requested.memory_mi_b > maximum.memory_mi_b
        || requested.disk_mi_b > maximum.disk_mi_b
        || requested.pids > maximum.pids
        || requested.timeout_seconds > maximum.timeout_seconds
        || requested.stdout_bytes > maximum.stdout_bytes
        || requested.stderr_bytes > maximum.stderr_bytes
    {
        return Err(AppError::forbidden(
            "resource_limit_denied",
            "插件资源需求超过固定 EnvironmentManifest 上限",
        ));
    }
    Ok(())
}

fn is_terminal_runner_status(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "failed" | "timed_out" | "policy_denied" | "workspace_conflict" | "cancelled"
    )
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
