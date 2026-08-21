use std::{collections::HashMap, sync::Arc};

use axum::{
    Form, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use chrono::Utc;
use maud::Markup;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    application::{graph, projects},
    artifacts::ArtifactStore,
    domain::{
        AppendProgressInput, ContributionInput, CreateBranchInput, GraphActionRequest,
        IntegrateBranchInput, ParkBranchInput, ProjectActionRequest, ProjectIntake,
        contribution_kind_label,
    },
    error::{AppError, AppResult},
};

use super::{AppState, views};

#[derive(Debug, Default, Deserialize)]
pub struct ProjectPageQuery {
    pub tab: Option<String>,
    pub node: Option<Uuid>,
    pub notice: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct DashboardQuery {
    pub view: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectActionForm {
    pub action: String,
    pub completion_evidence: Option<String>,
    pub intent: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GraphForm {
    pub action: String,
    pub node_id: Option<Uuid>,
    pub branch_id: Option<Uuid>,
    pub purpose: Option<String>,
    pub result: Option<String>,
    pub outcome: Option<String>,
    pub kind: Option<String>,
    pub reference_uri: Option<String>,
    pub scope: Option<String>,
    pub reopen_when: Option<String>,
    pub reason: Option<String>,
    pub summary: Option<String>,
    pub accepted_ids: Option<String>,
}

pub async fn dashboard(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DashboardQuery>,
) -> AppResult<Markup> {
    let projects = projects::list_projects(&state.pool).await?;
    Ok(views::dashboard(&projects, query.view.as_deref()))
}

pub async fn new_project_page(Query(query): Query<HashMap<String, String>>) -> Markup {
    views::new_project(query.get("error").map(String::as_str))
}

pub async fn create_project_form(
    State(state): State<Arc<AppState>>,
    Form(input): Form<ProjectIntake>,
) -> Response {
    match projects::create_project(&state.pool, input).await {
        Ok(project_id) => Redirect::to(&format!(
            "/projects/{project_id}?notice={}",
            urlencoding::encode("项目已创建，先确认什么事实代表完成")
        ))
        .into_response(),
        Err(error) => Redirect::to(&format!(
            "/new?error={}",
            urlencoding::encode(&error.public_message())
        ))
        .into_response(),
    }
}

pub async fn project_page(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Query(query): Query<ProjectPageQuery>,
) -> AppResult<Markup> {
    let snapshot = projects::get_snapshot(&state.pool, project_id).await?;
    Ok(views::project(&snapshot, &query))
}

pub async fn project_action_form(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Form(form): Form<ProjectActionForm>,
) -> Response {
    let payload = match form.action.as_str() {
        "confirm_outcome" => json!({
            "completionEvidence": form.completion_evidence.unwrap_or_default(),
        }),
        "revise_intent" => json!({ "intent": form.intent.unwrap_or_default() }),
        _ => Value::Null,
    };
    let request = ProjectActionRequest {
        action: form.action,
        payload,
    };
    let result = projects::run_action(
        &state.pool,
        state.config.artifact_root.clone(),
        project_id,
        request,
    )
    .await;
    redirect_project_result(project_id, result, "项目状态已更新", None)
}

pub async fn graph_action_form(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Form(form): Form<GraphForm>,
) -> Response {
    let node_hint = form.node_id;
    let result = graph_form_request(form);
    let result = match result {
        Ok(request) => graph::run_graph_action(&state.pool, project_id, request).await,
        Err(error) => Err(error),
    };
    redirect_project_result(project_id, result, "项目脉络已更新", node_hint)
}

fn graph_form_request(form: GraphForm) -> AppResult<GraphActionRequest> {
    let client_request_id = Uuid::new_v4();
    let payload = match form.action.as_str() {
        "create_branch" => {
            let purpose = form.purpose.unwrap_or_default();
            serde_json::to_value(CreateBranchInput {
                client_request_id,
                from_node_id: form
                    .node_id
                    .ok_or_else(|| AppError::bad_request("invalid_fork_node", "请选择分支起点"))?,
                name: short_title(&purpose, 24),
                purpose,
            })?
        }
        "append_progress" => {
            let summary = form.result.unwrap_or_default();
            let kind = form.kind.unwrap_or_else(|| "finding".into());
            serde_json::to_value(AppendProgressInput {
                client_request_id,
                branch_id: form
                    .branch_id
                    .ok_or_else(|| AppError::bad_request("invalid_branch", "请选择要推进的分支"))?,
                title: short_title(&summary, 28),
                summary: summary.clone(),
                outcome: form.outcome.unwrap_or_else(|| "useful".into()),
                contribution: ContributionInput {
                    title: format!(
                        "{}：{}",
                        contribution_kind_label(&kind),
                        short_title(&summary, 36)
                    ),
                    kind,
                    body: summary,
                    reference_uri: form.reference_uri,
                    scope: form.scope,
                    reopen_when: form.reopen_when,
                },
            })?
        }
        "integrate_branch" => {
            let accepted_contribution_ids = form
                .accepted_ids
                .unwrap_or_default()
                .split(',')
                .filter(|item| !item.is_empty())
                .map(|item| {
                    Uuid::parse_str(item).map_err(|_| {
                        AppError::bad_request("invalid_contribution", "待合流的产出标识不合法")
                    })
                })
                .collect::<AppResult<Vec<_>>>()?;
            serde_json::to_value(IntegrateBranchInput {
                client_request_id,
                source_branch_id: form
                    .branch_id
                    .ok_or_else(|| AppError::bad_request("invalid_branch", "请选择要合流的分支"))?,
                summary: form.summary.unwrap_or_default(),
                accepted_contribution_ids,
            })?
        }
        "park_branch" => serde_json::to_value(ParkBranchInput {
            client_request_id,
            branch_id: form
                .branch_id
                .ok_or_else(|| AppError::bad_request("invalid_branch", "请选择要暂停的分支"))?,
            reason: form.reason.unwrap_or_default(),
            reopen_when: form.reopen_when,
        })?,
        _ => {
            return Err(AppError::bad_request(
                "unsupported_graph_action",
                "不支持的项目脉络动作",
            ));
        }
    };
    Ok(GraphActionRequest {
        action: form.action,
        payload,
    })
}

fn redirect_project_result(
    project_id: Uuid,
    result: AppResult<()>,
    success: &str,
    node: Option<Uuid>,
) -> Response {
    let anchor = node.map(|id| format!("&node={id}")).unwrap_or_default();
    let location = match result {
        Ok(()) => format!(
            "/projects/{project_id}?notice={}{}#graph-workspace",
            urlencoding::encode(success),
            anchor,
        ),
        Err(error) => format!(
            "/projects/{project_id}?error={}{}#graph-workspace",
            urlencoding::encode(&error.public_message()),
            anchor,
        ),
    };
    Redirect::to(&location).into_response()
}

fn short_title(value: &str, max: usize) -> String {
    let first = value
        .trim()
        .split(['。', '！', '？', '!', '?', '\n'])
        .next()
        .unwrap_or("")
        .trim();
    if first.chars().count() <= max {
        return first.to_owned();
    }
    first
        .chars()
        .take(max.saturating_sub(1))
        .chain(std::iter::once('…'))
        .collect()
}

pub async fn artifact_download(
    State(state): State<Arc<AppState>>,
    Path(artifact_id): Path<Uuid>,
) -> AppResult<(HeaderMap, Vec<u8>)> {
    let artifact = projects::find_artifact(&state.pool, artifact_id).await?;
    let bytes = ArtifactStore::new(state.config.artifact_root.clone())
        .read(&artifact.storage_path)
        .await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&artifact.media_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("inline; filename=artifact-{}.md", artifact.id))
            .expect("ASCII content disposition"),
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"{}\"", artifact.sha256))
            .map_err(|_| AppError::internal("产物哈希不合法"))?,
    );
    Ok((headers, bytes))
}

pub async fn api_health(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let _: i32 = sqlx::query_scalar("SELECT 1")
        .fetch_one(&state.pool)
        .await?;
    Ok(Json(json!({
        "ok": true,
        "name": "fudian-rust",
        "time": Utc::now(),
    })))
}

pub async fn api_list_projects(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let projects = projects::list_projects(&state.pool).await?;
    Ok(Json(json!({ "projects": projects })))
}

pub async fn api_create_project(
    State(state): State<Arc<AppState>>,
    Json(input): Json<ProjectIntake>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let id = projects::create_project(&state.pool, input).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn api_project_snapshot(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let snapshot = projects::get_snapshot(&state.pool, project_id).await?;
    Ok(Json(serde_json::to_value(snapshot)?))
}

pub async fn api_project_action(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<ProjectActionRequest>,
) -> AppResult<Json<Value>> {
    projects::run_action(
        &state.pool,
        state.config.artifact_root.clone(),
        project_id,
        request,
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn api_graph_action(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<GraphActionRequest>,
) -> AppResult<Json<Value>> {
    graph::run_graph_action(&state.pool, project_id, request).await?;
    Ok(Json(json!({ "ok": true })))
}
