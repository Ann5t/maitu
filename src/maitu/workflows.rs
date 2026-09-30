use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, types::Json};
use tokio::{sync::watch, task::JoinSet};
use tracing::error;
use uuid::Uuid;

use crate::{
    artifacts::ArtifactStore,
    error::{AppError, AppResult},
    models::Project,
    web::AppState,
};

use super::provider::{self, ProviderConfig, ProviderStore};

const MAX_SOURCE_BYTES: usize = 256 * 1024;
const MAX_INPUT_BYTES: usize = 512 * 1024;
const WORKER_LOCK: i64 = 6_401_997_015;

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub instruction: String,
    pub output_filename: String,
    pub source_ids: Json<Vec<Uuid>>,
    pub status: String,
    pub wait_reason: Option<String>,
    pub latest_attempt_id: Option<Uuid>,
    pub accepted_attempt_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRecord {
    pub id: Uuid,
    pub task_id: Uuid,
    pub number: i32,
    pub status: String,
    pub input_snapshot: Option<Json<Value>>,
    pub provider_base_url: Option<String>,
    pub model: Option<String>,
    pub artifact_id: Option<Uuid>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub usage: Option<Json<Value>>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub request_started_at: Option<DateTime<Utc>>,
    pub response_received_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub filename: String,
    pub content: String,
    pub sha256: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub id: Uuid,
    pub filename: String,
    pub sha256: String,
    pub size_bytes: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub task_id: Uuid,
    pub parent_task_id: Uuid,
}

#[derive(Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptEvent {
    pub id: i64,
    pub phase: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSnapshot {
    pub project: Project,
    pub tasks: Vec<TaskRecord>,
    pub dependencies: Vec<Dependency>,
    pub sources: Vec<SourceSummary>,
    pub provider: super::provider::ProviderView,
    pub active_tasks: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub task: TaskRecord,
    pub attempts: Vec<AttemptRecord>,
    pub events: HashMap<Uuid, Vec<AttemptEvent>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub request_id: Uuid,
    pub title: String,
    pub instruction: String,
    pub output_filename: String,
    #[serde(default)]
    pub source_ids: Vec<Uuid>,
    #[serde(default)]
    pub dependency_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddSourceRequest {
    pub filename: String,
    pub content: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub request_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptRequest {
    pub attempt_id: Uuid,
}

pub async fn project(pool: &PgPool, id: Uuid) -> AppResult<Project> {
    sqlx::query_as("SELECT * FROM projects WHERE id=$1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))
}

pub async fn snapshot(
    pool: &PgPool,
    providers: &ProviderStore,
    id: Uuid,
) -> AppResult<WorkflowSnapshot> {
    Ok(WorkflowSnapshot {
        project: project(pool, id).await?,
        tasks: sqlx::query_as("SELECT * FROM maitu_tasks WHERE project_id=$1 ORDER BY created_at,id")
            .bind(id).fetch_all(pool).await?,
        dependencies: sqlx::query_as("SELECT task_id,parent_task_id FROM maitu_task_dependencies WHERE project_id=$1")
            .bind(id).fetch_all(pool).await?,
        sources: sqlx::query_as("SELECT id,filename,sha256,octet_length(content) AS size_bytes,created_at FROM maitu_sources WHERE project_id=$1 ORDER BY created_at,id")
            .bind(id).fetch_all(pool).await?,
        provider: providers.get().await.view(),
        active_tasks: sqlx::query_scalar("SELECT count(*) FROM maitu_tasks WHERE status='running'").fetch_one(pool).await?,
    })
}

pub async fn task(pool: &PgPool, id: Uuid) -> AppResult<TaskRecord> {
    sqlx::query_as("SELECT * FROM maitu_tasks WHERE id=$1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("任务不存在"))
}

pub async fn detail(pool: &PgPool, id: Uuid) -> AppResult<TaskDetail> {
    let task = task(pool, id).await?;
    let attempts: Vec<AttemptRecord> =
        sqlx::query_as("SELECT * FROM maitu_attempts WHERE task_id=$1 ORDER BY number DESC")
            .bind(id)
            .fetch_all(pool)
            .await?;
    let mut events = HashMap::new();
    for attempt in &attempts {
        events.insert(attempt.id, sqlx::query_as("SELECT id,phase,message,created_at FROM maitu_attempt_events WHERE attempt_id=$1 ORDER BY id")
            .bind(attempt.id).fetch_all(pool).await?);
    }
    Ok(TaskDetail {
        task,
        attempts,
        events,
    })
}

fn validate_filename(name: &str) -> AppResult<()> {
    if name.trim().is_empty()
        || name.len() > 200
        || name.starts_with('.')
        || name.ends_with(['.', ' '])
        || !name.chars().all(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
    {
        return Err(AppError::bad_request(
            "invalid_filename",
            "文件名不能包含路径、控制字符或系统特殊字符",
        ));
    }
    Ok(())
}

pub async fn add_source(
    pool: &PgPool,
    project_id: Uuid,
    input: AddSourceRequest,
) -> AppResult<SourceRecord> {
    validate_filename(&input.filename)?;
    if input.content.is_empty()
        || input.content.len() > MAX_SOURCE_BYTES
        || input.content.contains('\0')
    {
        return Err(AppError::bad_request(
            "invalid_source",
            "请提供不超过 256 KiB 的 UTF-8 文本资料",
        ));
    }
    project(pool, project_id).await?;
    let id = Uuid::new_v4();
    let hash = hex::encode(Sha256::digest(input.content.as_bytes()));
    Ok(sqlx::query_as("INSERT INTO maitu_sources(id,project_id,filename,content,sha256) VALUES($1,$2,$3,$4,$5) RETURNING *")
        .bind(id).bind(project_id).bind(input.filename).bind(input.content).bind(hash)
        .fetch_one(pool).await?)
}

pub async fn source(pool: &PgPool, id: Uuid) -> AppResult<SourceRecord> {
    sqlx::query_as("SELECT * FROM maitu_sources WHERE id=$1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("资料不存在"))
}

pub async fn create_task(
    pool: &PgPool,
    project_id: Uuid,
    mut input: CreateTaskRequest,
) -> AppResult<TaskRecord> {
    input.title = input.title.trim().into();
    input.instruction = input.instruction.trim().into();
    validate_filename(&input.output_filename)?;
    if input.title.is_empty()
        || input.title.len() > 400
        || input.instruction.is_empty()
        || input.instruction.len() > 32 * 1024
        || input.source_ids.len() > 64
        || input.dependency_ids.len() > 32
    {
        return Err(AppError::bad_request(
            "invalid_task",
            "请填写任务名称、具体要求和有效的资料范围",
        ));
    }
    input.source_ids.sort_unstable();
    input.source_ids.dedup();
    input.dependency_ids.sort_unstable();
    input.dependency_ids.dedup();
    let mut tx = pool.begin().await?;
    // Lock the project to serialize creation and idempotent request replays.
    let found: Option<Uuid> = sqlx::query_scalar("SELECT id FROM projects WHERE id=$1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *tx)
        .await?;
    if found.is_none() {
        return Err(AppError::not_found("项目不存在"));
    }
    let existing: Option<TaskRecord> = sqlx::query_as("SELECT * FROM maitu_tasks WHERE id=$1")
        .bind(input.request_id)
        .fetch_optional(&mut *tx)
        .await?;
    if let Some(existing) = existing {
        let mut parents: Vec<Uuid> = sqlx::query_scalar(
            "SELECT parent_task_id FROM maitu_task_dependencies WHERE task_id=$1",
        )
        .bind(existing.id)
        .fetch_all(&mut *tx)
        .await?;
        parents.sort_unstable();
        if existing.project_id != project_id
            || existing.title != input.title
            || existing.instruction != input.instruction
            || existing.output_filename != input.output_filename
            || existing.source_ids.0 != input.source_ids
            || parents != input.dependency_ids
        {
            return Err(AppError::conflict(
                "request_reused",
                "此请求编号已用于另一项任务",
            ));
        }
        return Ok(existing);
    }
    let sources: i64 =
        sqlx::query_scalar("SELECT count(*) FROM maitu_sources WHERE project_id=$1 AND id=ANY($2)")
            .bind(project_id)
            .bind(&input.source_ids)
            .fetch_one(&mut *tx)
            .await?;
    let parents: i64 =
        sqlx::query_scalar("SELECT count(*) FROM maitu_tasks WHERE project_id=$1 AND id=ANY($2)")
            .bind(project_id)
            .bind(&input.dependency_ids)
            .fetch_one(&mut *tx)
            .await?;
    if sources != input.source_ids.len() as i64 || parents != input.dependency_ids.len() as i64 {
        return Err(AppError::bad_request(
            "invalid_task_inputs",
            "资料与前序任务必须来自当前项目",
        ));
    }
    let task = sqlx::query_as("INSERT INTO maitu_tasks(id,project_id,title,instruction,output_filename,source_ids) VALUES($1,$2,$3,$4,$5,$6) RETURNING *")
        .bind(input.request_id).bind(project_id).bind(input.title).bind(input.instruction).bind(input.output_filename)
        .bind(Json(input.source_ids)).fetch_one(&mut *tx).await?;
    for parent in input.dependency_ids {
        sqlx::query("INSERT INTO maitu_task_dependencies(project_id,task_id,parent_task_id) VALUES($1,$2,$3)")
            .bind(project_id).bind(input.request_id).bind(parent).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(task)
}

pub async fn start(pool: &PgPool, task_id: Uuid, request_id: Uuid) -> AppResult<AttemptRecord> {
    let mut tx = pool.begin().await?;
    let current: TaskRecord = sqlx::query_as("SELECT * FROM maitu_tasks WHERE id=$1 FOR UPDATE")
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("任务不存在"))?;
    let existing: Option<AttemptRecord> =
        sqlx::query_as("SELECT * FROM maitu_attempts WHERE id=$1")
            .bind(request_id)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some(existing) = existing {
        if existing.task_id != task_id {
            return Err(AppError::conflict(
                "request_reused",
                "此请求编号已用于另一项执行",
            ));
        }
        return Ok(existing);
    }
    if matches!(current.status.as_str(), "queued" | "running") {
        return Ok(sqlx::query_as("SELECT * FROM maitu_attempts WHERE id=$1")
            .bind(current.latest_attempt_id)
            .fetch_one(&mut *tx)
            .await?);
    }
    let sources: Vec<SourceRecord> = sqlx::query_as("SELECT * FROM maitu_sources WHERE project_id=$1 AND (cardinality($2::uuid[])=0 OR id=ANY($2)) ORDER BY created_at,id")
        .bind(current.project_id).bind(&current.source_ids.0).fetch_all(&mut *tx).await?;
    let snapshot = json!({"title":current.title,"instruction":current.instruction,"outputFilename":current.output_filename,"sources":sources,"upstream":[]});
    if snapshot.to_string().len() > MAX_INPUT_BYTES {
        return Err(AppError::bad_request(
            "input_limit",
            "本轮资料总量超过 512 KiB，请选择较少的资料",
        ));
    }
    let number: i32 =
        sqlx::query_scalar("SELECT COALESCE(MAX(number),0)+1 FROM maitu_attempts WHERE task_id=$1")
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
    let attempt = sqlx::query_as("INSERT INTO maitu_attempts(id,task_id,number,status,input_snapshot) VALUES($1,$2,$3,'queued',$4) RETURNING *")
        .bind(request_id).bind(task_id).bind(number).bind(Json(snapshot)).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO maitu_attempt_dependencies(attempt_id,parent_task_id,source_attempt_id) SELECT $1,d.parent_task_id,p.accepted_attempt_id FROM maitu_task_dependencies d JOIN maitu_tasks p ON p.id=d.parent_task_id WHERE d.task_id=$2")
        .bind(request_id).bind(task_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'queued','任务已排队，已固定当前资料')")
        .bind(request_id).execute(&mut *tx).await?;
    sqlx::query("UPDATE maitu_tasks SET status='queued',latest_attempt_id=$2,wait_reason='等待执行空位或前序成果',updated_at=now() WHERE id=$1")
        .bind(task_id).bind(request_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(attempt)
}

pub async fn accept(pool: &PgPool, task_id: Uuid, attempt_id: Uuid) -> AppResult<TaskRecord> {
    let mut tx = pool.begin().await?;
    let current: TaskRecord = sqlx::query_as("SELECT * FROM maitu_tasks WHERE id=$1 FOR UPDATE")
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("任务不存在"))?;
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM maitu_attempts WHERE id=$1 AND task_id=$2 AND status='produced' AND artifact_id IS NOT NULL)")
        .bind(attempt_id).bind(task_id).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(AppError::conflict(
            "output_unavailable",
            "只能采用此任务已经保存的成果",
        ));
    }
    if current.accepted_attempt_id != Some(attempt_id) {
        sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'accepted','你采用了这次成果，可供后续任务引用')")
            .bind(attempt_id).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE artifacts SET status='approved',approved_at=COALESCE(approved_at,now()) WHERE id=(SELECT artifact_id FROM maitu_attempts WHERE id=$1)")
        .bind(attempt_id).execute(&mut *tx).await?;
    let result = sqlx::query_as(
        "UPDATE maitu_tasks SET accepted_attempt_id=$2,updated_at=now() WHERE id=$1 RETURNING *",
    )
    .bind(task_id)
    .bind(attempt_id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(result)
}

async fn event(pool: &PgPool, attempt: Uuid, phase: &str, message: &str) -> AppResult<()> {
    sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,$2,$3)")
        .bind(attempt)
        .bind(phase)
        .bind(message)
        .execute(pool)
        .await?;
    Ok(())
}

async fn wait_reason(pool: &PgPool, task: Uuid, reason: &str) -> AppResult<()> {
    sqlx::query("UPDATE maitu_tasks SET wait_reason=$2 WHERE id=$1 AND status='queued' AND wait_reason IS DISTINCT FROM $2")
        .bind(task).bind(reason).execute(pool).await?;
    Ok(())
}

async fn claim(
    state: &AppState,
    config: &ProviderConfig,
) -> AppResult<Option<(TaskRecord, AttemptRecord)>> {
    // Pin available parents even when another parent is still missing. Blocked jobs
    // must not hide ready jobs beyond the first page of the queue.
    sqlx::query("UPDATE maitu_attempt_dependencies d SET source_attempt_id=p.accepted_attempt_id FROM maitu_tasks p,maitu_tasks t WHERE d.attempt_id=t.latest_attempt_id AND t.status='queued' AND p.id=d.parent_task_id AND d.source_attempt_id IS NULL AND p.accepted_attempt_id IS NOT NULL")
        .execute(&state.pool).await?;
    sqlx::query("UPDATE maitu_tasks t SET wait_reason='等待前序任务的成果被采用；可先推进其他任务' WHERE t.status='queued' AND EXISTS(SELECT 1 FROM maitu_attempt_dependencies d WHERE d.attempt_id=t.latest_attempt_id AND d.source_attempt_id IS NULL) AND t.wait_reason IS DISTINCT FROM '等待前序任务的成果被采用；可先推进其他任务'")
        .execute(&state.pool).await?;
    let queued: Vec<TaskRecord> = sqlx::query_as(
        "SELECT * FROM maitu_tasks t WHERE t.status='queued' AND NOT EXISTS(SELECT 1 FROM maitu_attempt_dependencies d WHERE d.attempt_id=t.latest_attempt_id AND d.source_attempt_id IS NULL) ORDER BY t.created_at,t.id LIMIT 128",
    )
    .fetch_all(&state.pool)
    .await?;
    'candidates: for candidate in queued {
        let mut tx = state.pool.begin().await?;
        let current: Option<TaskRecord> = sqlx::query_as(
            "SELECT * FROM maitu_tasks WHERE id=$1 AND status='queued' FOR UPDATE SKIP LOCKED",
        )
        .bind(candidate.id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(current) = current else { continue };
        let Some(attempt_id) = current.latest_attempt_id else {
            continue;
        };
        // Bind each available upstream version once. Later adoption cannot silently change it.
        sqlx::query("UPDATE maitu_attempt_dependencies d SET source_attempt_id=p.accepted_attempt_id FROM maitu_tasks p WHERE d.attempt_id=$1 AND p.id=d.parent_task_id AND d.source_attempt_id IS NULL AND p.accepted_attempt_id IS NOT NULL")
            .bind(attempt_id).execute(&mut *tx).await?;
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM maitu_attempt_dependencies WHERE attempt_id=$1 AND source_attempt_id IS NULL")
            .bind(attempt_id).fetch_one(&mut *tx).await?;
        if pending > 0 {
            tx.commit().await?;
            wait_reason(
                &state.pool,
                current.id,
                "等待前序任务的成果被采用；可先推进其他任务",
            )
            .await?;
            continue;
        }
        let upstream: Vec<(Uuid, Uuid, Uuid, String, String, String, String)> = sqlx::query_as(
            "SELECT d.parent_task_id,d.source_attempt_id,a.artifact_id,t.title,t.output_filename,f.storage_path,f.sha256 FROM maitu_attempt_dependencies d JOIN maitu_attempts a ON a.id=d.source_attempt_id JOIN maitu_tasks t ON t.id=d.parent_task_id JOIN artifacts f ON f.id=a.artifact_id WHERE d.attempt_id=$1 ORDER BY d.parent_task_id"
        ).bind(attempt_id).fetch_all(&mut *tx).await?;
        let mut snapshot: Json<Value> = sqlx::query_scalar(
            "SELECT input_snapshot FROM maitu_attempts WHERE id=$1 AND status='queued'",
        )
        .bind(attempt_id)
        .fetch_one(&mut *tx)
        .await?;
        let store = ArtifactStore::new(state.config.artifact_root.clone());
        let mut outputs = Vec::new();
        for (parent, source_attempt, artifact_id, title, filename, storage_path, hash) in upstream {
            let content = match store.read(&storage_path).await {
                Ok(content) => content,
                Err(_) => {
                    tx.commit().await?;
                    finish_failed(
                        &state.pool,
                        current.id,
                        attempt_id,
                        "upstream_missing",
                        "前序成果文件无法读取，请检查来源记录",
                    )
                    .await?;
                    continue 'candidates;
                }
            };
            if hex::encode(Sha256::digest(&content)) != hash {
                tx.commit().await?;
                finish_failed(
                    &state.pool,
                    current.id,
                    attempt_id,
                    "upstream_changed",
                    "前序成果文件与记录的校验值不一致",
                )
                .await?;
                continue 'candidates;
            }
            let content = match String::from_utf8(content) {
                Ok(content) => content,
                Err(_) => {
                    tx.commit().await?;
                    finish_failed(
                        &state.pool,
                        current.id,
                        attempt_id,
                        "upstream_not_text",
                        "前序成果不是文本文件",
                    )
                    .await?;
                    continue 'candidates;
                }
            };
            outputs.push(json!({"taskId":parent,"attemptId":source_attempt,"artifactId":artifact_id,"title":title,"filename":filename,"sha256":hash,"content":content}));
        }
        snapshot.0["upstream"] = json!(outputs);
        snapshot.0["provider"] = json!(config.view());
        if snapshot.0.to_string().len() > MAX_INPUT_BYTES {
            tx.commit().await?;
            finish_failed(
                &state.pool,
                current.id,
                attempt_id,
                "input_limit",
                "加入前序成果后，资料超过 512 KiB 的本轮限制",
            )
            .await?;
            continue;
        }
        let attempt = sqlx::query_as("UPDATE maitu_attempts SET status='running',started_at=now(),input_snapshot=$2,provider_base_url=$3,model=$4 WHERE id=$1 AND status='queued' RETURNING *")
            .bind(attempt_id).bind(snapshot).bind(&config.base_url).bind(&config.model).fetch_one(&mut *tx).await?;
        sqlx::query(
            "UPDATE maitu_tasks SET status='running',wait_reason=NULL,updated_at=now() WHERE id=$1",
        )
        .bind(current.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'started','输入与前序成果版本已固定，开始独立执行')")
            .bind(attempt_id).execute(&mut *tx).await?;
        tx.commit().await?;
        return Ok(Some((current, attempt)));
    }
    Ok(None)
}

async fn finish_failed(
    pool: &PgPool,
    task: Uuid,
    attempt: Uuid,
    code: &str,
    message: &str,
) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE maitu_attempts SET status='failed',error_code=$2,error_message=$3,completed_at=now() WHERE id=$1 AND status IN ('queued','running')")
        .bind(attempt).bind(code).bind(message).execute(&mut *tx).await?;
    sqlx::query("UPDATE maitu_tasks SET status='failed',wait_reason=NULL,updated_at=now() WHERE id=$1 AND latest_attempt_id=$2")
        .bind(task).bind(attempt).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'failed',$2)",
    )
    .bind(attempt)
    .bind(message)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

fn prompt(snapshot: &Value) -> String {
    let mut text = format!(
        "任务：{}\n要求：{}\n成果文件：{}\n\n",
        snapshot["title"].as_str().unwrap_or(""),
        snapshot["instruction"].as_str().unwrap_or(""),
        snapshot["outputFilename"].as_str().unwrap_or("")
    );
    for (field, label) in [("sources", "项目资料"), ("upstream", "已采用的前序成果")] {
        if let Some(items) = snapshot[field].as_array() {
            for item in items {
                text.push_str(&format!(
                    "\n--- {label}：{} ---\n{}\n",
                    item["filename"].as_str().unwrap_or(""),
                    item["content"].as_str().unwrap_or("")
                ));
            }
        }
    }
    text
}

async fn execute(
    state: Arc<AppState>,
    config: ProviderConfig,
    task: TaskRecord,
    attempt: AttemptRecord,
) -> AppResult<()> {
    event(
        &state.pool,
        attempt.id,
        "request",
        "开始向模型服务发起独立请求",
    )
    .await?;
    sqlx::query("UPDATE maitu_attempts SET request_started_at=now() WHERE id=$1")
        .bind(attempt.id)
        .execute(&state.pool)
        .await?;
    let response = provider::complete(
        &config,
        &prompt(
            &attempt
                .input_snapshot
                .as_ref()
                .expect("claimed input snapshot")
                .0,
        ),
    )
    .await;
    if response.is_ok() {
        sqlx::query("UPDATE maitu_attempts SET response_received_at=now() WHERE id=$1")
            .bind(attempt.id)
            .execute(&state.pool)
            .await?;
    }
    let response = match response {
        Ok(response) => response,
        Err(failure) => {
            return finish_failed(
                &state.pool,
                task.id,
                attempt.id,
                failure.code,
                &failure.message,
            )
            .await;
        }
    };
    event(
        &state.pool,
        attempt.id,
        "response",
        "已收到完整模型结果，正在保存文件",
    )
    .await?;
    let artifact_id = Uuid::new_v4();
    let stored = ArtifactStore::new(state.config.artifact_root.clone())
        .write_text(
            task.project_id,
            artifact_id,
            &task.output_filename,
            &response.content,
        )
        .await?;
    let mut tx = state.pool.begin().await?;
    sqlx::query("INSERT INTO artifacts(id,project_id,title,kind,storage_path,media_type,sha256,version,status) VALUES($1,$2,$3,'ai_file',$4,'text/plain; charset=utf-8',$5,$6,'review')")
        .bind(artifact_id).bind(task.project_id).bind(&task.output_filename).bind(stored.storage_path)
        .bind(stored.sha256).bind(attempt.number).execute(&mut *tx).await?;
    let changed = sqlx::query("UPDATE maitu_attempts SET status='produced',artifact_id=$2,usage=$3,completed_at=now() WHERE id=$1 AND status='running'")
        .bind(attempt.id).bind(artifact_id).bind(Json(response.usage)).execute(&mut *tx).await?;
    if changed.rows_affected() != 1 {
        return Err(AppError::conflict(
            "attempt_changed",
            "此执行已结束，不能覆盖记录",
        ));
    }
    sqlx::query("UPDATE maitu_tasks SET status='produced',wait_reason=NULL,updated_at=now() WHERE id=$1 AND latest_attempt_id=$2")
        .bind(task.id).bind(attempt.id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'produced','成果文件已保存，可查看、下载或采用')")
        .bind(attempt.id).execute(&mut *tx).await?;
    sqlx::query("UPDATE projects SET updated_at=now() WHERE id=$1")
        .bind(task.project_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn interrupt_running(pool: &PgPool) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) SELECT id,'interrupted','执行进程已停止，服务端结果可能仍已产生；请检查后决定是否重试' FROM maitu_attempts WHERE status='running'")
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE maitu_attempts SET status='interrupted',error_code='process_interrupted',error_message='执行进程已停止，模型侧是否完成尚不确定',completed_at=now() WHERE status='running'")
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE maitu_tasks SET status='interrupted',wait_reason=NULL,updated_at=now() WHERE status='running'")
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub async fn run_worker(
    state: Arc<AppState>,
    mut shutdown: watch::Receiver<bool>,
) -> AppResult<()> {
    // A connection-scoped leader lock is released by PostgreSQL when this process dies.
    // It prevents another app instance from marking a live worker's requests interrupted.
    let mut leader = loop {
        let mut connection = state.pool.acquire().await?;
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
            .bind(WORKER_LOCK)
            .fetch_one(&mut *connection)
            .await?;
        if acquired {
            // Own the connection so cancellation closes the session instead of returning
            // an advisory-locked session to the shared pool.
            break connection.detach();
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {},
            _ = shutdown.changed() => return Ok(()),
        }
    };
    interrupt_running(&state.pool).await?;
    let mut jobs: JoinSet<(Uuid, Uuid, AppResult<()>)> = JoinSet::new();
    let mut active = HashMap::new();
    let result = async {
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                Some(joined) = jobs.join_next_with_id(), if !jobs.is_empty() => {
                    match joined {
                        Ok((id, (task, attempt, outcome))) => {
                            active.remove(&id);
                            if let Err(failure) = outcome {
                                error!(code=failure.code(), "资料任务执行失败");
                                finish_failed(&state.pool, task, attempt, failure.code(), &failure.public_message()).await?;
                            }
                        }
                        Err(failure) => {
                            if let Some((task, attempt)) = active.remove(&failure.id()) {
                                finish_failed(&state.pool, task, attempt, "worker_interrupted", "此任务执行中断，可查看输入后重试").await?;
                            }
                        }
                    }
                }
                _ = tick.tick() => {
                    // Keep the leader session alive and fail closed if its lock is lost.
                    sqlx::query("SELECT 1").execute(&mut leader).await?;
                    let config = state.providers.get().await;
                    if config.api_key.is_empty() {
                        sqlx::query("UPDATE maitu_tasks SET wait_reason='请先配置 DeepSeek 连接' WHERE status='queued' AND wait_reason IS DISTINCT FROM '请先配置 DeepSeek 连接'").execute(&state.pool).await?;
                        continue;
                    }
                    while jobs.len() < config.concurrency {
                        let Some((task, attempt)) = claim(&state, &config).await? else { break };
                        let task_id = task.id;
                        let attempt_id = attempt.id;
                        let worker_state = state.clone();
                        let worker_config = config.clone();
                        let handle = jobs.spawn(async move {
                            let result = execute(worker_state, worker_config, task, attempt).await;
                            (task_id, attempt_id, result)
                        });
                        active.insert(handle.id(), (task_id, attempt_id));
                    }
                }
            }
        }
        Ok::<_, AppError>(())
    }.await;
    jobs.abort_all();
    while jobs.join_next().await.is_some() {}
    let interrupted = interrupt_running(&state.pool).await;
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(WORKER_LOCK)
        .execute(&mut leader)
        .await;
    result?;
    interrupted?;
    Ok(())
}

pub async fn serve_worker(state: Arc<AppState>, mut shutdown: watch::Receiver<bool>) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        if let Err(failure) = run_worker(state.clone(), shutdown.clone()).await {
            error!(code = failure.code(), "资料任务执行进程中断，正在恢复连接");
        }
        if *shutdown.borrow() {
            return;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {},
            _ = shutdown.changed() => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_cannot_choose_host_paths() {
        for name in ["../secret", "/tmp/file", "C:\\key", ".env", "result\n.md"] {
            assert!(validate_filename(name).is_err());
        }
        assert!(validate_filename("项目结论.md").is_ok());
        assert!(validate_filename("01 项目需求（草稿）.md").is_ok());
    }

    #[test]
    fn prompt_uses_captured_materials_and_pinned_outputs() {
        let text = prompt(
            &json!({"title":"计划","instruction":"列出步骤","outputFilename":"plan.md","sources":[{"filename":"brief.txt","content":"captured-source"}],"upstream":[{"filename":"analysis.md","content":"pinned-result"}]}),
        );
        assert!(text.contains("captured-source"));
        assert!(text.contains("pinned-result"));
    }
}
