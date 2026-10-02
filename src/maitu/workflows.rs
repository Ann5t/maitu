use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, types::Json};
use tokio::{
    sync::{Mutex, watch},
    task::JoinSet,
};
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
const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const WORKER_LOCK: i64 = 6_401_997_015;

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
    pub instruction: String,
    pub output_filename: String,
    pub task_kind: String,
    pub acceptance_criteria: String,
    pub source_ids: Json<Vec<Uuid>>,
    pub connection_key: String,
    pub status: String,
    pub wait_reason: Option<String>,
    pub latest_attempt_id: Option<Uuid>,
    pub accepted_attempt_id: Option<Uuid>,
    pub accept_note: String,
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
    pub connection_key: String,
    pub provider_base_url: Option<String>,
    pub model: Option<String>,
    pub artifact_id: Option<Uuid>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub cancel_requested: bool,
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

#[derive(Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionLoad {
    pub key: String,
    pub running: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowSnapshot {
    pub project: Project,
    pub tasks: Vec<TaskRecord>,
    pub dependencies: Vec<Dependency>,
    pub sources: Vec<SourceSummary>,
    pub connections: Vec<super::provider::ProviderView>,
    pub connection_load: Vec<ConnectionLoad>,
    pub active_tasks: i64,
    pub code_project: Option<super::code::CodeProject>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub task: TaskRecord,
    pub attempts: Vec<AttemptRecord>,
    pub events: HashMap<Uuid, Vec<AttemptEvent>>,
    pub plans: Vec<super::plans::PlanRecord>,
    pub code_attempts: Vec<super::code::CodeAttempt>,
    pub operations: Vec<super::code::Operation>,
    /// Attempt id -> why its fixed inputs no longer reflect the project.
    pub stale_inputs: HashMap<Uuid, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub request_id: Uuid,
    pub title: String,
    pub instruction: String,
    pub output_filename: String,
    #[serde(default = "file_kind")]
    pub task_kind: String,
    #[serde(default)]
    pub acceptance_criteria: String,
    #[serde(default)]
    pub source_ids: Vec<Uuid>,
    #[serde(default)]
    pub dependency_ids: Vec<Uuid>,
    /// Empty means automatic scheduling across usable connections. A non-empty
    /// value pins the task to one named connection.
    #[serde(default)]
    pub connection_key: String,
}

fn file_kind() -> String {
    "file".into()
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
    #[serde(default)]
    pub additional_instruction: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptRequest {
    pub attempt_id: Uuid,
    /// Why this version is adopted; kept with the decision. Optional.
    #[serde(default)]
    pub reason: String,
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
    let load: Vec<ConnectionLoad> = sqlx::query_as(
        "SELECT connection_key AS key,count(*) AS running FROM maitu_attempts WHERE status='running' GROUP BY connection_key",
    )
    .fetch_all(pool)
    .await?;
    Ok(WorkflowSnapshot {
        project: project(pool, id).await?,
        tasks: sqlx::query_as("SELECT * FROM maitu_tasks WHERE project_id=$1 ORDER BY created_at,id")
            .bind(id).fetch_all(pool).await?,
        dependencies: sqlx::query_as("SELECT task_id,parent_task_id FROM maitu_task_dependencies WHERE project_id=$1")
            .bind(id).fetch_all(pool).await?,
        sources: sqlx::query_as("SELECT id,filename,sha256,octet_length(content) AS size_bytes,created_at FROM maitu_sources WHERE project_id=$1 ORDER BY created_at,id")
            .bind(id).fetch_all(pool).await?,
        connections: providers
            .list()
            .await
            .iter()
            .map(ProviderConfig::view)
            .collect(),
        connection_load: load,
        active_tasks: sqlx::query_scalar("SELECT count(*) FROM maitu_tasks WHERE status='running'").fetch_one(pool).await?,
        code_project: super::code::project(pool,id).await?,
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
    let stale_inputs = stale_input_reasons(pool, &task, &attempts).await?;
    Ok(TaskDetail {
        task,
        attempts,
        events,
        plans: super::plans::for_task(pool, id).await?,
        code_attempts: super::code::attempts(pool, id).await?,
        operations: super::code::operations(pool, id).await?,
        stale_inputs,
    })
}

/// An attempt fixed its inputs when it started. Sources edited or removed
/// afterwards, and code-base advancement for coding tasks, mean the recorded
/// inputs no longer describe what the project currently offers. Additions do
/// not stale an attempt: later material simply belongs to later attempts.
async fn stale_input_reasons(
    pool: &PgPool,
    task: &TaskRecord,
    attempts: &[AttemptRecord],
) -> AppResult<HashMap<Uuid, String>> {
    let mut reasons = HashMap::new();
    for attempt in attempts {
        let Some(snapshot) = attempt.input_snapshot.as_ref() else {
            continue;
        };
        let snapshot = &snapshot.0;
        let mut reason = String::new();
        if let Some(sources) = snapshot["sources"].as_array() {
            for source in sources {
                let Some(source_id) = source["id"].as_str().and_then(|v| Uuid::parse_str(v).ok())
                else {
                    continue;
                };
                let current_sha: Option<String> = sqlx::query_scalar(
                    "SELECT sha256 FROM maitu_sources WHERE id=$1 AND project_id=$2",
                )
                .bind(source_id)
                .bind(task.project_id)
                .fetch_optional(pool)
                .await?;
                match current_sha {
                    None => {
                        reason =
                            "本次输入引用的资料已被删除；记录保留，可按原输入理解这次成果".into();
                    }
                    Some(sha) if sha != source["sha256"].as_str().unwrap_or_default() => {
                        reason = "本次输入固定后，引用的资料内容已被更新；记录保留".into();
                    }
                    Some(_) => {}
                }
                if !reason.is_empty() {
                    break;
                }
            }
        }
        if reason.is_empty()
            && !snapshot["codeProject"].is_null()
            && let Some(base) = snapshot["codeProject"]["baseCommit"].as_str()
        {
            let accepted: Option<String> = sqlx::query_scalar(
                "SELECT accepted_commit FROM maitu_code_projects WHERE project_id=$1",
            )
            .bind(task.project_id)
            .fetch_optional(pool)
            .await?;
            if accepted.is_some_and(|commit| commit != base) {
                reason = "本次尝试的代码基线已不是项目当前采用版本；差异仍按当时基线记录".into();
            }
        }
        if !reason.is_empty() {
            reasons.insert(attempt.id, reason);
        }
    }
    Ok(reasons)
}

pub(super) fn validate_filename(name: &str) -> AppResult<()> {
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
    providers: &ProviderStore,
    project_id: Uuid,
    mut input: CreateTaskRequest,
) -> AppResult<TaskRecord> {
    input.title = input.title.trim().into();
    input.instruction = input.instruction.trim().into();
    input.connection_key = input.connection_key.trim().into();
    if !input.connection_key.is_empty() && providers.get(&input.connection_key).await.is_none() {
        return Err(AppError::bad_request(
            "invalid_connection",
            "任务指定的模型连接不存在",
        ));
    }
    validate_filename(&input.output_filename)?;
    if input.title.is_empty()
        || input.title.len() > 400
        || input.instruction.is_empty()
        || input.instruction.len() > 32 * 1024
        || input.source_ids.len() > 64
        || input.dependency_ids.len() > 32
        || !matches!(input.task_kind.as_str(), "file" | "plan" | "code")
        || input.acceptance_criteria.len() > 8000
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
            || existing.task_kind != input.task_kind
            || existing.acceptance_criteria != input.acceptance_criteria
            || existing.connection_key != input.connection_key
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
    let task = sqlx::query_as("INSERT INTO maitu_tasks(id,project_id,title,instruction,output_filename,source_ids,task_kind,acceptance_criteria,connection_key) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING *")
        .bind(input.request_id).bind(project_id).bind(input.title).bind(input.instruction).bind(input.output_filename)
        .bind(Json(input.source_ids)).bind(&input.task_kind).bind(&input.acceptance_criteria).bind(&input.connection_key).fetch_one(&mut *tx).await?;
    for parent in input.dependency_ids {
        sqlx::query("INSERT INTO maitu_task_dependencies(project_id,task_id,parent_task_id) VALUES($1,$2,$3)")
            .bind(project_id).bind(input.request_id).bind(parent).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(task)
}

pub async fn start(pool: &PgPool, task_id: Uuid, request_id: Uuid) -> AppResult<AttemptRecord> {
    start_with_instruction(pool, task_id, request_id, "").await
}

pub async fn start_with_instruction(
    pool: &PgPool,
    task_id: Uuid,
    request_id: Uuid,
    additional: &str,
) -> AppResult<AttemptRecord> {
    let additional = additional.trim();
    if additional.len() > 8000 {
        return Err(AppError::bad_request(
            "instruction_limit",
            "补充要求不能超过 8,000 字节",
        ));
    }
    // 编码任务在入队前同步确认检查服务可用：点击执行立即得到环境结论，
    // 不依赖连接冷却或调度节奏，也保证失败的尝试确实没有调用模型。
    let worker_ready: AppResult<()> =
        match sqlx::query_scalar::<_, String>("SELECT task_kind FROM maitu_tasks WHERE id=$1")
            .bind(task_id)
            .fetch_optional(pool)
            .await?
        {
            Some(kind) if kind == "code" => super::code::ensure_worker_ready().await,
            _ => Ok(()),
        };
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
        if existing
            .input_snapshot
            .as_ref()
            .and_then(|value| value.0["additionalInstruction"].as_str())
            .unwrap_or("")
            != additional
        {
            return Err(AppError::conflict(
                "request_reused",
                "此执行编号已用于另一份要求，请新建一次尝试",
            ));
        }
        return Ok(existing);
    }
    if matches!(current.status.as_str(), "queued" | "running") {
        if !additional.is_empty() {
            return Err(AppError::conflict(
                "task_already_started",
                "任务已经在等待或执行，补充要求请用于下一次尝试",
            ));
        }
        return Ok(sqlx::query_as("SELECT * FROM maitu_attempts WHERE id=$1")
            .bind(current.latest_attempt_id)
            .fetch_one(&mut *tx)
            .await?);
    }
    let sources: Vec<SourceRecord> = sqlx::query_as("SELECT * FROM maitu_sources WHERE project_id=$1 AND (cardinality($2::uuid[])=0 OR id=ANY($2)) ORDER BY created_at,id")
        .bind(current.project_id).bind(&current.source_ids.0).fetch_all(&mut *tx).await?;
    let code_project: Option<Json<Value>> = sqlx::query_scalar("SELECT jsonb_build_object('baseCommit',accepted_commit,'checks',checks,'sourceName',source_name,'fileCount',file_count) FROM maitu_code_projects WHERE project_id=$1")
        .bind(current.project_id).fetch_optional(&mut *tx).await?;
    if current.task_kind == "code" && code_project.is_none() {
        return Err(AppError::conflict(
            "code_project_required",
            "请先导入代码项目及检查方式，再启动编码任务",
        ));
    }
    let snapshot = json!({"title":current.title,"instruction":current.instruction,"outputFilename":current.output_filename,
        "taskKind":current.task_kind,"acceptanceCriteria":current.acceptance_criteria,"additionalInstruction":additional,"sources":sources,"upstream":[],"codeProject":code_project});
    if snapshot.to_string().len() > MAX_INPUT_BYTES {
        return Err(AppError::bad_request(
            "input_limit",
            "本次资料总量超过 8 MiB，请选择较少的资料",
        ));
    }
    let number: i32 =
        sqlx::query_scalar("SELECT COALESCE(MAX(number),0)+1 FROM maitu_attempts WHERE task_id=$1")
            .bind(task_id)
            .fetch_one(&mut *tx)
            .await?;
    if let Err(failure) = &worker_ready {
        // 预检未通过：本次尝试直接以未调用模型的状态留档，输入仍然固定可查。
        let attempt = sqlx::query_as(
            "INSERT INTO maitu_attempts(id,task_id,number,status,input_snapshot,error_code,error_message,completed_at) VALUES($1,$2,$3,'failed',$4,$5,$6,now()) RETURNING *")
            .bind(request_id)
            .bind(task_id)
            .bind(number)
            .bind(&snapshot)
            .bind(failure.code())
            .bind(failure.public_message())
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'failed',$2)",
        )
        .bind(request_id)
        .bind(failure.public_message())
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE maitu_tasks SET status='failed',latest_attempt_id=$2,wait_reason=NULL,updated_at=now() WHERE id=$1")
            .bind(task_id)
            .bind(request_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(attempt);
    }
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

pub async fn accept(
    pool: &PgPool,
    task_id: Uuid,
    attempt_id: Uuid,
    reason: &str,
) -> AppResult<TaskRecord> {
    let mut tx = pool.begin().await?;
    let current: TaskRecord = sqlx::query_as("SELECT * FROM maitu_tasks WHERE id=$1 FOR UPDATE")
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("任务不存在"))?;
    if current.task_kind == "plan" {
        return Err(AppError::conflict(
            "plan_requires_adoption",
            "请调整推进计划后，将它加入任务图",
        ));
    }
    if current.task_kind == "code" {
        return Err(AppError::conflict(
            "code_requires_integration",
            "编码成果需要经过合并和实际检查后采用",
        ));
    }
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM maitu_attempts WHERE id=$1 AND task_id=$2 AND status='produced' AND artifact_id IS NOT NULL)")
        .bind(attempt_id).bind(task_id).fetch_one(&mut *tx).await?;
    if !valid {
        return Err(AppError::conflict(
            "output_unavailable",
            "只能采用此任务已经保存的成果",
        ));
    }
    let reason = reason.trim();
    if reason.len() > 2000 {
        return Err(AppError::bad_request(
            "reason_limit",
            "采用理由不能超过 2,000 字",
        ));
    }
    if current.accepted_attempt_id != Some(attempt_id) {
        let message = if reason.is_empty() {
            "你采用了这次成果，可供后续任务引用".to_owned()
        } else {
            format!("你采用了这次成果并记录理由：{reason}")
        };
        sqlx::query(
            "INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'accepted',$2)",
        )
        .bind(attempt_id)
        .bind(message)
        .execute(&mut *tx)
        .await?;
    }
    if current.accept_note != reason {
        sqlx::query("UPDATE maitu_tasks SET accept_note=$2 WHERE id=$1")
            .bind(task_id)
            .bind(reason)
            .execute(&mut *tx)
            .await?;
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

/// Cancels a task. A queued attempt stops before any provider request; a
/// running attempt is marked so the executor stops before its next provider
/// call and never saves an output after cancellation. Records stay immutable.
pub async fn cancel(pool: &PgPool, task_id: Uuid) -> AppResult<TaskRecord> {
    let mut tx = pool.begin().await?;
    let current: TaskRecord = sqlx::query_as("SELECT * FROM maitu_tasks WHERE id=$1 FOR UPDATE")
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::not_found("任务不存在"))?;
    match current.status.as_str() {
        "cancelled" => return Ok(current),
        "queued" | "running" => {}
        other => {
            return Err(AppError::conflict(
                "not_cancellable",
                format!("任务当前为「{other}」，没有正在等待或执行的工作可取消"),
            ));
        }
    }
    let attempt_id = current.latest_attempt_id;
    if current.status == "queued" {
        if let Some(attempt_id) = attempt_id {
            sqlx::query("UPDATE maitu_attempts SET status='cancelled',error_code='user_cancelled',error_message='你在执行前取消了这次尝试；输入快照保留',completed_at=now() WHERE id=$1 AND status='queued'")
                .bind(attempt_id).execute(&mut *tx).await?;
            sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'cancelled','排队中的尝试已按你的要求取消，没有发出模型请求')")
                .bind(attempt_id).execute(&mut *tx).await?;
        }
    } else if let Some(attempt_id) = attempt_id {
        sqlx::query(
            "UPDATE maitu_attempts SET cancel_requested=true WHERE id=$1 AND status='running'",
        )
        .bind(attempt_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'cancelled','已请求取消；正在进行的模型请求不会中断，结果不会保存为成果')")
            .bind(attempt_id).execute(&mut *tx).await?;
    }
    let wait_reason = if current.status == "running" {
        Some("已请求取消；等待执行中的请求结束后生效".to_owned())
    } else {
        None
    };
    let result = sqlx::query_as(
        "UPDATE maitu_tasks SET status='cancelled',wait_reason=$2,updated_at=now() WHERE id=$1 RETURNING *",
    )
    .bind(task_id)
    .bind(wait_reason)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(result)
}

/// Applies a previously requested cancellation: the attempt ends as cancelled
/// and no artifact is saved. Provider usage already spent stays on the record.
async fn finish_cancelled(pool: &PgPool, task: Uuid, attempt: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE maitu_attempts SET status='cancelled',error_code='user_cancelled',error_message='已按取消请求停止；已发出的请求记录保留，没有保存成果',completed_at=now() WHERE id=$1 AND status='running'")
        .bind(attempt).execute(&mut *tx).await?;
    sqlx::query("UPDATE maitu_tasks SET status='cancelled',wait_reason=NULL,updated_at=now() WHERE id=$1 AND latest_attempt_id=$2")
        .bind(task).bind(attempt).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'cancelled','取消已生效：没有保存成果，之前的操作与用量记录保留')")
        .bind(attempt).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub(super) async fn event(
    pool: &PgPool,
    attempt: Uuid,
    phase: &str,
    message: &str,
) -> AppResult<()> {
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
    // must not hide ready jobs beyond the first page of the queue. Tasks pinned to
    // another connection are invisible here; automatic tasks fit any connection.
    sqlx::query("UPDATE maitu_attempt_dependencies d SET source_attempt_id=p.accepted_attempt_id FROM maitu_tasks p,maitu_tasks t WHERE d.attempt_id=t.latest_attempt_id AND t.status='queued' AND p.id=d.parent_task_id AND d.source_attempt_id IS NULL AND p.accepted_attempt_id IS NOT NULL")
        .execute(&state.pool).await?;
    sqlx::query("UPDATE maitu_tasks t SET wait_reason='等待前序任务的成果被采用；可先推进其他任务' WHERE t.status='queued' AND EXISTS(SELECT 1 FROM maitu_attempt_dependencies d WHERE d.attempt_id=t.latest_attempt_id AND d.source_attempt_id IS NULL) AND t.wait_reason IS DISTINCT FROM '等待前序任务的成果被采用；可先推进其他任务'")
        .execute(&state.pool).await?;
    let queued: Vec<TaskRecord> = sqlx::query_as(
        "SELECT * FROM maitu_tasks t WHERE t.status='queued' AND (t.connection_key='' OR t.connection_key=$1) AND NOT EXISTS(SELECT 1 FROM maitu_attempt_dependencies d WHERE d.attempt_id=t.latest_attempt_id AND d.source_attempt_id IS NULL) ORDER BY t.created_at,t.id LIMIT 128",
    )
    .bind(&config.key)
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
            let adopted_plan:Option<Json<Value>>=sqlx::query_scalar("SELECT adopted_proposal FROM maitu_plans WHERE attempt_id=$1 AND adopted_at IS NOT NULL")
                .bind(source_attempt).fetch_optional(&mut *tx).await?.flatten();
            let kind: String = sqlx::query_scalar("SELECT task_kind FROM maitu_tasks WHERE id=$1")
                .bind(parent)
                .fetch_one(&mut *tx)
                .await?;
            outputs.push(json!({"taskId":parent,"taskKind":kind,"attemptId":source_attempt,"artifactId":artifact_id,"title":title,"filename":filename,"sha256":hash,"content":content,"adoptedPlan":adopted_plan}));
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
                "加入前序成果后，资料超过 8 MiB 的文件大小限制",
            )
            .await?;
            continue;
        }
        let estimated_tokens = match provider::check_context_budget(config, &prompt(&snapshot.0)) {
            Ok(estimated) => estimated,
            Err(failure) => {
                snapshot.0["estimatedInputTokens"] =
                    json!(provider::estimate_input_tokens(&prompt(&snapshot.0)));
                sqlx::query(
                    "UPDATE maitu_attempts SET input_snapshot=$2 WHERE id=$1 AND status='queued'",
                )
                .bind(attempt_id)
                .bind(snapshot)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                finish_failed(
                    &state.pool,
                    current.id,
                    attempt_id,
                    failure.code,
                    &failure.message,
                )
                .await?;
                continue;
            }
        };
        snapshot.0["estimatedInputTokens"] = json!(estimated_tokens);
        if sqlx::query_scalar::<_, bool>("SELECT cancel_requested FROM maitu_attempts WHERE id=$1")
            .bind(attempt_id)
            .fetch_one(&mut *tx)
            .await?
        {
            tx.commit().await?;
            finish_cancelled(&state.pool, current.id, attempt_id).await?;
            continue;
        }
        let attempt = sqlx::query_as("UPDATE maitu_attempts SET status='running',started_at=now(),input_snapshot=$2,connection_key=$3,provider_base_url=$4,model=$5 WHERE id=$1 AND status='queued' RETURNING *")
            .bind(attempt_id).bind(snapshot).bind(&config.key).bind(&config.base_url).bind(&config.model).fetch_one(&mut *tx).await?;
        sqlx::query(
            "UPDATE maitu_tasks SET status='running',wait_reason=NULL,updated_at=now() WHERE id=$1",
        )
        .bind(current.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'started',$2)",
        )
        .bind(attempt_id)
        .bind(format!(
            "输入与前序成果版本已固定，使用连接「{}」（{}，{}）开始独立执行",
            config.label, config.key, config.model
        ))
        .execute(&mut *tx)
        .await?;
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

/// Provider-side codes where another try (possibly on another connection) can
/// legitimately succeed. Auth and balance failures only fail over to a
/// different connection; they never replay on the same credentials.
const TRANSIENT_CODES: [&str; 6] = [
    "provider_rate_limit",
    "provider_timeout",
    "provider_unavailable",
    "provider_network",
    "provider_response",
    "provider_http",
];
const MAX_AUTO_RETRIES: i32 = 3;

/// Records the failure and, within bounds, requeues one fresh attempt so a
/// failing or throttled connection does not leave the task blocked while other
/// connections can serve it. Interrupted attempts are never replayed here: the
/// provider may have finished the original request.
async fn fail_or_retry(
    state: &AppState,
    task: &TaskRecord,
    attempt: &AttemptRecord,
    code: &str,
    message: &str,
) -> AppResult<()> {
    let transient = TRANSIENT_CODES.contains(&code);
    // A pinned task retries only on transient provider trouble with its own
    // connection. An automatic task may also switch away from a connection
    // whose credentials or balance were rejected, but only when another usable
    // connection actually exists.
    let eligible = if task.connection_key.is_empty() {
        transient
            || state
                .providers
                .active()
                .await
                .iter()
                .any(|connection| connection.key != attempt.connection_key)
    } else {
        transient
    };
    if !eligible || attempt.number > MAX_AUTO_RETRIES {
        return finish_failed(&state.pool, task.id, attempt.id, code, message).await;
    }
    if transient {
        cooldowns().penalize(&attempt.connection_key).await;
    }
    let mut tx = state.pool.begin().await?;
    // Only the task's latest attempt may be retried; an older attempt's failure
    // was already superseded by a newer one.
    let still_latest: bool =
        sqlx::query_scalar("SELECT latest_attempt_id=$2 FROM maitu_tasks WHERE id=$1 FOR UPDATE")
            .bind(task.id)
            .bind(attempt.id)
            .fetch_optional(&mut *tx)
            .await?
            .unwrap_or(false);
    if !still_latest {
        tx.commit().await?;
        return finish_failed(&state.pool, task.id, attempt.id, code, message).await;
    }
    sqlx::query("UPDATE maitu_attempts SET status='failed',error_code=$2,error_message=$3,completed_at=now() WHERE id=$1 AND status IN ('queued','running')")
        .bind(attempt.id).bind(code).bind(message).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'failed',$2)",
    )
    .bind(attempt.id)
    .bind(message)
    .execute(&mut *tx)
    .await?;
    let number: i32 = attempt.number + 1;
    let retry_id = Uuid::new_v4();
    let snapshot = attempt
        .input_snapshot
        .as_ref()
        .map(|value| {
            let mut snapshot = value.0.clone();
            snapshot["upstream"] = json!([]);
            if let Some(object) = snapshot.as_object_mut() {
                object.remove("provider");
                object.remove("estimatedInputTokens");
            }
            snapshot
        })
        .unwrap_or_else(|| json!({"additionalInstruction":""}));
    sqlx::query("INSERT INTO maitu_attempts(id,task_id,number,status,input_snapshot) VALUES($1,$2,$3,'queued',$4)")
        .bind(retry_id).bind(task.id).bind(number).bind(Json(&snapshot)).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO maitu_attempt_dependencies(attempt_id,parent_task_id) SELECT $1,parent_task_id FROM maitu_attempt_dependencies WHERE attempt_id=$2")
        .bind(retry_id).bind(attempt.id).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO maitu_attempt_events(attempt_id,phase,message) VALUES($1,'queued',$2)",
    )
    .bind(retry_id)
    .bind(format!(
        "第 {number} 次尝试已自动排队：{}；等待可用连接，已完成的记录保留",
        if task.connection_key.is_empty() && !transient {
            "此连接暂时不可用，将优先改用其他连接"
        } else {
            "连接暂时不可用，稍后重试"
        }
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE maitu_tasks SET status='queued',latest_attempt_id=$2,wait_reason='连接暂时不可用，自动重试已排队；其他任务继续执行',updated_at=now() WHERE id=$1")
        .bind(task.id).bind(retry_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

pub(super) fn prompt(snapshot: &Value) -> String {
    let mut text = format!(
        "任务：{}\n要求：{}\n成果文件：{}\n\n",
        snapshot["title"].as_str().unwrap_or(""),
        snapshot["instruction"].as_str().unwrap_or(""),
        snapshot["outputFilename"].as_str().unwrap_or("")
    );
    if let Some(additional) = snapshot["additionalInstruction"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        text.push_str(&format!("本次补充要求：{additional}\n"));
    }
    if let Some(criteria) = snapshot["acceptanceCriteria"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        text.push_str(&format!("验收要求：{criteria}\n"));
    }
    for (field, label) in [("sources", "项目资料"), ("upstream", "已采用的前序成果")] {
        if let Some(items) = snapshot[field].as_array() {
            for item in items {
                text.push_str(&format!(
                    "\n--- {label}：{} ---\n{}\n",
                    item["filename"].as_str().unwrap_or(""),
                    item["content"].as_str().unwrap_or("")
                ));
                if !item["adoptedPlan"].is_null() {
                    text.push_str(&format!(
                        "\n此计划已由用户调整并采用，以以下版本及当前任务要求为准：\n{}\n",
                        item["adoptedPlan"]
                    ));
                }
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
    if task.task_kind != "code" {
        let cancelled: bool =
            sqlx::query_scalar("SELECT cancel_requested FROM maitu_attempts WHERE id=$1")
                .bind(attempt.id)
                .fetch_one(&state.pool)
                .await?;
        if cancelled {
            return finish_cancelled(&state.pool, task.id, attempt.id).await;
        }
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
    }
    let input = &attempt
        .input_snapshot
        .as_ref()
        .expect("claimed input snapshot")
        .0;
    let response = if task.task_kind == "code" {
        let result = super::agent::execute(&state, &config, &task, &attempt).await;
        match result {
            Ok(response) => Ok(response),
            Err(error) => return Err(error),
        }
    } else if task.task_kind == "plan" {
        let mut input = input.clone();
        if let Some(base) = input["codeProject"]["baseCommit"].as_str() {
            input["codeContext"] =
                super::code::planning_context(&state, task.project_id, base).await?;
        }
        sqlx::query("UPDATE maitu_attempts SET input_snapshot=$2 WHERE id=$1")
            .bind(attempt.id)
            .bind(Json(&input))
            .execute(&state.pool)
            .await?;
        provider::complete_with_system(
            &config,
            super::plans::SYSTEM_PROMPT,
            &super::plans::prompt_context(&input),
            true,
        )
        .await
    } else {
        provider::complete(&config, &prompt(input)).await
    };
    if response.is_ok() {
        sqlx::query("UPDATE maitu_attempts SET response_received_at=now() WHERE id=$1")
            .bind(attempt.id)
            .execute(&state.pool)
            .await?;
        // A served request proves the connection works again.
        cooldowns().clear(&attempt.connection_key).await;
    }
    if sqlx::query_scalar::<_, bool>("SELECT cancel_requested FROM maitu_attempts WHERE id=$1")
        .bind(attempt.id)
        .fetch_one(&state.pool)
        .await?
    {
        // The request completed, but the user cancelled meanwhile: keep the
        // records, never save an adopted-candidate output against that wish.
        return finish_cancelled(&state.pool, task.id, attempt.id).await;
    }
    let response = match response {
        Ok(response) => response,
        Err(failure) => {
            return fail_or_retry(&state, &task, &attempt, failure.code, &failure.message).await;
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
    if task.task_kind == "plan" {
        let parsed = super::plans::Plan::parse(&response.content).and_then(|plan| {
            plan.validate(!input["codeProject"].is_null())?;
            Ok(plan)
        });
        match parsed {
            Ok(plan) => {
                sqlx::query(
                    "INSERT INTO maitu_plans(attempt_id,project_id,proposal) VALUES($1,$2,$3)",
                )
                .bind(attempt.id)
                .bind(task.project_id)
                .bind(Json(plan))
                .execute(&mut *tx)
                .await?;
            }
            Err(failure) => {
                sqlx::query("UPDATE maitu_attempts SET artifact_id=$2,usage=$3 WHERE id=$1 AND status='running'")
                    .bind(attempt.id).bind(artifact_id).bind(Json(response.usage)).execute(&mut *tx).await?;
                tx.commit().await?;
                return finish_failed(
                    &state.pool,
                    task.id,
                    attempt.id,
                    failure.code(),
                    &failure.public_message(),
                )
                .await;
            }
        }
    }
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
    sqlx::query("UPDATE maitu_execution_operations SET status='interrupted',completed_at=now(),output=jsonb_build_object('error','执行进程中断；已有现场保留，不自动重复操作','resultUncertain',true) WHERE status='running'")
        .execute(&mut *tx).await?;
    sqlx::query("INSERT INTO maitu_attempt_events(attempt_id,phase,message) SELECT id,'interrupted','执行进程已停止，服务端结果可能仍已产生；请检查后决定是否重试' FROM maitu_attempts WHERE status='running'")
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE maitu_attempts SET status='interrupted',error_code='process_interrupted',error_message='执行进程已停止，模型侧是否完成尚不确定',completed_at=now() WHERE status='running'")
        .execute(&mut *tx).await?;
    sqlx::query("UPDATE maitu_tasks SET status='interrupted',wait_reason=NULL,updated_at=now() WHERE status='running'")
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// In-memory per-connection throttle state. A connection that just rate-limited
/// or failed us is skipped for a bounded, growing pause so retries back off
/// instead of hammering. It resets on the next success and is per process.
#[derive(Default)]
struct Cooldowns(Mutex<HashMap<String, Instant>>);

static COOLDOWNS: OnceLock<Cooldowns> = OnceLock::new();

fn cooldowns() -> &'static Cooldowns {
    COOLDOWNS.get_or_init(Cooldowns::default)
}

impl Cooldowns {
    const BASE: Duration = Duration::from_secs(1);
    const MAX: Duration = Duration::from_secs(30);

    async fn active(&self, key: &str) -> bool {
        self.0
            .lock()
            .await
            .get(key)
            .is_some_and(|until| *until > Instant::now())
    }

    async fn penalize(&self, key: &str) {
        let mut cooldowns = self.0.lock().await;
        let until = cooldowns.get(key).copied();
        let next = match until {
            Some(until) if until > Instant::now() => ((until - Instant::now()) * 2).min(Self::MAX),
            _ => Self::BASE,
        };
        cooldowns.insert(key.to_owned(), Instant::now() + next);
    }

    async fn clear(&self, key: &str) {
        self.0.lock().await.remove(key);
    }
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
    let cooldowns = cooldowns();
    let mut jobs: JoinSet<(Uuid, Uuid, String, AppResult<()>)> = JoinSet::new();
    let mut active: HashMap<tokio::task::Id, (Uuid, Uuid, String)> = HashMap::new();
    let result = async {
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                Some(joined) = jobs.join_next_with_id(), if !jobs.is_empty() => {
                    match joined {
                        Ok((id, (task, attempt, connection, outcome))) => {
                            active.remove(&id);
                            match outcome {
                                Ok(()) => {}
                                Err(failure) => {
                                    error!(code=failure.code(), "资料任务执行失败");
                                    let record = sqlx::query_as::<_, TaskRecord>("SELECT * FROM maitu_tasks WHERE id=$1")
                                        .bind(task).fetch_one(&state.pool).await?;
                                    let attempt_record = sqlx::query_as::<_, AttemptRecord>("SELECT * FROM maitu_attempts WHERE id=$1")
                                        .bind(attempt).fetch_one(&state.pool).await?;
                                    fail_or_retry(&state, &record, &attempt_record, failure.code(), &failure.public_message()).await?;
                                }
                            }
                            let _ = connection;
                        }
                        Err(failure) => {
                            if let Some((task, attempt, _)) = active.remove(&failure.id()) {
                                finish_failed(&state.pool, task, attempt, "worker_interrupted", "此任务执行中断，可查看输入后重试").await?;
                            }
                        }
                    }
                }
                _ = tick.tick() => {
                    // Keep the leader session alive and fail closed if its lock is lost.
                    sqlx::query("SELECT 1").execute(&mut leader).await?;
                    let connections = state.providers.active().await;
                    if connections.is_empty() {
                        sqlx::query("UPDATE maitu_tasks SET wait_reason='请先在设置中配置可用的模型连接' WHERE status='queued' AND wait_reason IS DISTINCT FROM '请先在设置中配置可用的模型连接'").execute(&state.pool).await?;
                        continue;
                    }
                    // Tasks pinned to a connection that is currently unusable must
                    // say so instead of appearing stuck behind invisible capacity.
                    let keys: Vec<String> = connections.iter().map(|c| c.key.clone()).collect();
                    sqlx::query("UPDATE maitu_tasks t SET wait_reason='指定的模型连接当前不可用；请在设置中检查，或将任务改为自动选择连接' WHERE t.status='queued' AND t.connection_key <> '' AND NOT (t.connection_key = ANY($1)) AND t.wait_reason IS DISTINCT FROM '指定的模型连接当前不可用；请在设置中检查，或将任务改为自动选择连接'")
                        .bind(&keys).execute(&state.pool).await?;
                    // 所有连接都在退避时，自动选择连接的排队任务也要说明实际原因，
                    // 否则只显示笼统的等待空位；退避结束后恢复常规提示。
                    let mut cooling = 0;
                    for connection in &connections {
                        if cooldowns.active(&connection.key).await {
                            cooling += 1;
                        }
                    }
                    if cooling == connections.len() {
                        sqlx::query(
                            "UPDATE maitu_tasks SET wait_reason='所有可用连接都在限流退避中，稍后自动重试' WHERE status='queued' AND connection_key='' AND wait_reason IS DISTINCT FROM '所有可用连接都在限流退避中，稍后自动重试'",
                        )
                        .execute(&state.pool)
                        .await?;
                    } else {
                        sqlx::query(
                            "UPDATE maitu_tasks SET wait_reason='等待执行空位或前序成果' WHERE status='queued' AND connection_key='' AND wait_reason='所有可用连接都在限流退避中，稍后自动重试'",
                        )
                        .execute(&state.pool)
                        .await?;
                    }
                    for connection in &connections {
                        if cooldowns.active(&connection.key).await {
                            sqlx::query("UPDATE maitu_tasks t SET wait_reason='此任务等待连接冷却后自动重试' WHERE t.status='queued' AND t.connection_key=$1 AND t.wait_reason IS DISTINCT FROM '此任务等待连接冷却后自动重试'")
                                .bind(&connection.key).execute(&state.pool).await?;
                            continue;
                        }
                        loop {
                            // The database is the single source of truth: claim
                            // commits the attempt to 'running' before returning,
                            // so spawned jobs are always counted there.
                            let running: i64 = sqlx::query_scalar("SELECT count(*) FROM maitu_attempts WHERE status='running' AND connection_key=$1")
                                .bind(&connection.key).fetch_one(&state.pool).await?;
                            if running >= connection.concurrency as i64 {
                                break;
                            }
                            let Some((task, attempt)) = claim(&state, connection).await? else { break };
                            let task_id = task.id;
                            let attempt_id = attempt.id;
                            let connection_key = connection.key.clone();
                            let worker_state = state.clone();
                            let worker_config = connection.clone();
                            let handle = jobs.spawn(async move {
                                let result = execute(worker_state, worker_config, task, attempt).await;
                                (task_id, attempt_id, connection_key, result)
                            });
                            active.insert(handle.id(), (task_id, attempt_id, connection.key.clone()));
                        }
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
