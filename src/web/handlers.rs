use std::sync::Arc;

use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
};
use chrono::Utc;
use serde_json::{Value, json};
use tracing::error;
use uuid::Uuid;

use crate::{
    application::{
        context_memory, goal_branches, graph, ideas, inputs, plugins, projects,
        scheduler as scheduler_app, workspaces,
    },
    artifacts::ArtifactStore,
    domain::{GraphActionRequest, ProjectActionRequest, ProjectIntake},
    error::{AppError, AppResult},
    idea_domain::{AttachIdeaSourceQuery, IdeaCommandRequest},
    input_artifacts::{BeginInputArtifact, ChunkQuery, FinishInputArtifact, ImportInputArtifact},
    scheduler::{
        AcknowledgeToolCleanupRequest, ActivateToolLeaseRequest, CancelActionRunRequest,
        ClaimActionRunRequest, CompleteActionRunRequest, CreateToolLeaseRequest,
        EnqueueActionRunRequest, FailActionRunRequest, FinishToolLeaseRequest,
        HeartbeatActionRunRequest, MarkNotificationReadRequest, ReconcileActionRunsRequest,
        RegisterWorkerRequest, RequestToolLeaseStopRequest, ResumeActionRunRequest,
    },
    tooling::{
        EnvironmentManifest, PluginInstallStatementRequest, PluginManifestDraft,
        PluginPublisherDraft, PluginSelector, SignedPluginInstallRequest,
    },
    workspace::{
        FailRunnerJobRequest, FinalizeIntegrationRequest, FinalizeRunnerJobRequest,
        PrepareIntegrationRequest, PrepareRunnerJobRequest,
    },
};

use super::AppState;

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
        HeaderValue::from_str(&if artifact.kind == "ai_file" {
            format!(
                "inline; filename*=UTF-8''{}",
                urlencoding::encode(&artifact.title)
            )
        } else {
            format!("inline; filename=artifact-{}.md", artifact.id)
        })
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
        "name": "fudian",
        "time": Utc::now(),
    })))
}

pub async fn api_list_projects(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let projects = projects::list_projects(&state.pool).await?;
    Ok(Json(json!({ "projects": projects })))
}

pub async fn api_list_ideas(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let ideas = ideas::list_ideas(&state.pool).await?;
    Ok(Json(
        json!({ "modelVersion": "idea-project/v1", "ideas": ideas }),
    ))
}

pub async fn api_create_idea(
    State(state): State<Arc<AppState>>,
    Json(request): Json<IdeaCommandRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    if request.action != "idea.create" {
        return Err(AppError::bad_request(
            "invalid_create_idea_action",
            "创建想法接口只接受 idea.create",
        ));
    }
    let response = ideas::run_command(&state.pool, None, request).await?;
    let status = if response.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(serde_json::to_value(response)?)))
}

pub async fn api_idea_snapshot(
    State(state): State<Arc<AppState>>,
    Path(idea_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let snapshot = ideas::get_snapshot(&state.pool, idea_id).await?;
    Ok(Json(serde_json::to_value(snapshot)?))
}

pub async fn api_attach_idea_source(
    State(state): State<Arc<AppState>>,
    Path(idea_id): Path<Uuid>,
    Query(query): Query<AttachIdeaSourceQuery>,
    body: Body,
) -> AppResult<Json<Value>> {
    let limit = usize::try_from(state.config.input_max_bytes).unwrap_or(usize::MAX);
    let bytes = to_bytes(body, limit).await.map_err(|_| {
        AppError::bad_request(
            "upload_too_large",
            format!("单个想法来源不能超过 {} 字节", state.config.input_max_bytes),
        )
    })?;
    let response = ideas::attach_source(
        &state.pool,
        &state.config.artifact_root,
        state.config.input_max_bytes,
        idea_id,
        query,
        bytes.to_vec(),
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_download_idea_source(
    State(state): State<Arc<AppState>>,
    Path((idea_id, source_id)): Path<(Uuid, Uuid)>,
) -> AppResult<(HeaderMap, Vec<u8>)> {
    let (source, bytes) =
        ideas::read_source(&state.pool, &state.config.artifact_root, idea_id, source_id).await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&source.trusted_media_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "inline; filename*=UTF-8''{}",
            urlencoding::encode(&source.display_name)
        ))
        .map_err(|_| AppError::internal("想法来源文件名无法用于响应"))?,
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("\"{}\"", source.sha256))
            .map_err(|_| AppError::internal("想法来源摘要不合法"))?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok((headers, bytes))
}

pub async fn api_idea_command(
    State(state): State<Arc<AppState>>,
    Path(idea_id): Path<Uuid>,
    Json(request): Json<IdeaCommandRequest>,
) -> AppResult<Json<Value>> {
    if request.action.starts_with("project_proposal.")
        && request.action != "project_proposal.create"
    {
        return Err(AppError::bad_request(
            "wrong_command_endpoint",
            "ProjectProposal 后续命令应发送到它自己的命令地址",
        ));
    }
    let response = ideas::run_command(&state.pool, Some(idea_id), request).await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_project_proposal_command(
    State(state): State<Arc<AppState>>,
    Path(proposal_id): Path<Uuid>,
    Json(request): Json<IdeaCommandRequest>,
) -> AppResult<Json<Value>> {
    if !request.action.starts_with("project_proposal.")
        || request.action == "project_proposal.create"
    {
        return Err(AppError::bad_request(
            "wrong_command_endpoint",
            "该地址只接受已存在 ProjectProposal 的命令",
        ));
    }
    let response = ideas::run_command(&state.pool, Some(proposal_id), request).await?;
    Ok(Json(serde_json::to_value(response)?))
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

pub async fn api_goal_snapshot(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let snapshot = goal_branches::get_snapshot(&state.pool, project_id).await?;
    Ok(Json(serde_json::to_value(snapshot)?))
}

pub async fn api_goal_command(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Json(request): Json<goal_branches::GoalCommandRequest>,
) -> AppResult<Json<Value>> {
    let response = run_goal_command_with_workspace(&state, project_id, request).await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_goal_workspace(
    State(state): State<Arc<AppState>>,
    Path((project_id, goal_branch_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let detail =
        workspaces::get_workspace(&state.pool, &state.config, project_id, goal_branch_id).await?;
    Ok(Json(serde_json::to_value(detail)?))
}

pub async fn api_prepare_runner_job(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<PrepareRunnerJobRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let response =
        workspaces::prepare_runner_job(&state.pool, &state.config, project_id, session_id, request)
            .await?;
    let status = if response.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(serde_json::to_value(response)?)))
}

pub async fn api_finalize_runner_job(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id, job_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<FinalizeRunnerJobRequest>,
) -> AppResult<Json<Value>> {
    let outcome = workspaces::finalize_runner_job(
        &state.pool,
        &state.config,
        project_id,
        session_id,
        job_id,
        request,
    )
    .await?;
    Ok(Json(serde_json::to_value(outcome)?))
}

pub async fn api_fail_runner_job(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id, job_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(request): Json<FailRunnerJobRequest>,
) -> AppResult<Json<Value>> {
    let outcome =
        workspaces::fail_runner_job(&state.pool, project_id, session_id, job_id, request).await?;
    Ok(Json(serde_json::to_value(outcome)?))
}

pub async fn api_register_scheduler_worker(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<RegisterWorkerRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let bootstrap = headers
        .get("x-fudian-worker-bootstrap")
        .and_then(|value| value.to_str().ok());
    let response =
        scheduler_app::register_worker(&state.pool, &state.config, bootstrap, request).await?;
    let status = if response.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(serde_json::to_value(response)?)))
}

pub async fn api_enqueue_action_run(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<EnqueueActionRunRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let (replayed, action) =
        scheduler_app::enqueue_action_run(&state.pool, project_id, session_id, request).await?;
    let status = if replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        Json(json!({ "replayed": replayed, "action": action })),
    ))
}

pub async fn api_list_action_runs(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let actions = scheduler_app::list_action_runs(&state.pool, project_id, session_id).await?;
    Ok(Json(json!({ "actions": actions })))
}

pub async fn api_get_action_run(
    State(state): State<Arc<AppState>>,
    Path((project_id, action_run_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let action = scheduler_app::get_action_run(&state.pool, project_id, action_run_id).await?;
    Ok(Json(serde_json::to_value(action)?))
}

pub async fn api_cancel_action_run(
    State(state): State<Arc<AppState>>,
    Path((project_id, action_run_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CancelActionRunRequest>,
) -> AppResult<Json<Value>> {
    let (replayed, action) =
        scheduler_app::cancel_action_run(&state.pool, project_id, action_run_id, request).await?;
    Ok(Json(json!({ "replayed": replayed, "action": action })))
}

pub async fn api_resume_action_run(
    State(state): State<Arc<AppState>>,
    Path((project_id, action_run_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<ResumeActionRunRequest>,
) -> AppResult<Json<Value>> {
    let (replayed, action) =
        scheduler_app::resume_action_run(&state.pool, project_id, action_run_id, request).await?;
    Ok(Json(json!({ "replayed": replayed, "action": action })))
}

pub async fn api_claim_action_run(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ClaimActionRunRequest>,
) -> AppResult<Json<Value>> {
    let response = scheduler_app::claim_action_run(&state.pool, request).await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_heartbeat_action_run(
    State(state): State<Arc<AppState>>,
    Path(action_run_id): Path<Uuid>,
    Json(request): Json<HeartbeatActionRunRequest>,
) -> AppResult<Json<Value>> {
    let response = scheduler_app::heartbeat_action_run(&state.pool, action_run_id, request).await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_complete_action_run(
    State(state): State<Arc<AppState>>,
    Path(action_run_id): Path<Uuid>,
    Json(request): Json<CompleteActionRunRequest>,
) -> AppResult<Json<Value>> {
    let action = scheduler_app::complete_action_run(&state.pool, action_run_id, request).await?;
    Ok(Json(serde_json::to_value(action)?))
}

pub async fn api_fail_action_run(
    State(state): State<Arc<AppState>>,
    Path(action_run_id): Path<Uuid>,
    Json(request): Json<FailActionRunRequest>,
) -> AppResult<Json<Value>> {
    let action = scheduler_app::fail_action_run(&state.pool, action_run_id, request).await?;
    Ok(Json(serde_json::to_value(action)?))
}

pub async fn api_prepare_integration(
    State(state): State<Arc<AppState>>,
    Path((action_run_id, integration_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<PrepareIntegrationRequest>,
) -> AppResult<Json<Value>> {
    let project_id: Uuid = sqlx::query_scalar(
        "SELECT project_id FROM goal_action_runs WHERE id = $1 AND subject_kind = 'integration'",
    )
    .bind(action_run_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Integration ActionRun 不存在"))?;
    let response = workspaces::prepare_integration(
        &state.pool,
        &state.config,
        project_id,
        action_run_id,
        integration_id,
        request,
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_finalize_integration(
    State(state): State<Arc<AppState>>,
    Path((action_run_id, integration_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<FinalizeIntegrationRequest>,
) -> AppResult<Json<Value>> {
    let project_id: Uuid = sqlx::query_scalar(
        "SELECT project_id FROM goal_action_runs WHERE id = $1 AND subject_kind = 'integration'",
    )
    .bind(action_run_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::not_found("Integration ActionRun 不存在"))?;
    let response = workspaces::finalize_integration(
        &state.pool,
        &state.config,
        project_id,
        action_run_id,
        integration_id,
        request,
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_reconcile_action_runs(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ReconcileActionRunsRequest>,
) -> AppResult<Json<Value>> {
    let response = scheduler_app::reconcile_action_runs(&state.pool, request).await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_list_notifications(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let notifications = scheduler_app::list_notifications(&state.pool, project_id).await?;
    Ok(Json(json!({ "notifications": notifications })))
}

pub async fn api_mark_notification_read(
    State(state): State<Arc<AppState>>,
    Path((project_id, notification_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<MarkNotificationReadRequest>,
) -> AppResult<Json<Value>> {
    let (replayed, notification) =
        scheduler_app::mark_notification_read(&state.pool, project_id, notification_id, request)
            .await?;
    Ok(Json(json!({
        "replayed": replayed,
        "notification": notification,
    })))
}

pub async fn api_create_tool_lease(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<CreateToolLeaseRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let response = scheduler_app::create_tool_lease(
        &state.pool,
        &state.config,
        project_id,
        session_id,
        request,
    )
    .await?;
    let status = if response.replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(serde_json::to_value(response)?)))
}

pub async fn api_get_tool_lease(
    State(state): State<Arc<AppState>>,
    Path((project_id, tool_lease_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let lease = scheduler_app::get_tool_lease(&state.pool, project_id, tool_lease_id).await?;
    Ok(Json(serde_json::to_value(lease)?))
}

pub async fn api_activate_tool_lease(
    State(state): State<Arc<AppState>>,
    Path(action_run_id): Path<Uuid>,
    Json(request): Json<ActivateToolLeaseRequest>,
) -> AppResult<Json<Value>> {
    let lease = scheduler_app::activate_tool_lease(&state.pool, action_run_id, request).await?;
    Ok(Json(serde_json::to_value(lease)?))
}

pub async fn api_finish_tool_lease(
    State(state): State<Arc<AppState>>,
    Path(action_run_id): Path<Uuid>,
    Json(request): Json<FinishToolLeaseRequest>,
) -> AppResult<Json<Value>> {
    let response = scheduler_app::finish_tool_lease(&state.pool, action_run_id, request).await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_request_tool_lease_stop(
    State(state): State<Arc<AppState>>,
    Path((project_id, tool_lease_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<RequestToolLeaseStopRequest>,
) -> AppResult<Json<Value>> {
    let (replayed, response) =
        scheduler_app::request_tool_lease_stop(&state.pool, project_id, tool_lease_id, request)
            .await?;
    Ok(Json(json!({ "replayed": replayed, "result": response })))
}

pub async fn api_acknowledge_tool_cleanup(
    State(state): State<Arc<AppState>>,
    Path(tool_lease_id): Path<Uuid>,
    Json(request): Json<AcknowledgeToolCleanupRequest>,
) -> AppResult<Json<Value>> {
    let lease =
        scheduler_app::acknowledge_tool_cleanup(&state.pool, tool_lease_id, request).await?;
    Ok(Json(serde_json::to_value(lease)?))
}

async fn run_goal_command_with_workspace(
    state: &AppState,
    project_id: Uuid,
    mut request: goal_branches::GoalCommandRequest,
) -> AppResult<goal_branches::GoalCommandResponse> {
    let action = request.action.clone();
    let client_request_id = request.client_request_id;
    if action == "merge.propose" {
        let session_id = request
            .payload
            .get("sessionId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| AppError::bad_request("invalid_input", "拟合并缺少 Session ID"))?;
        let binding = workspaces::review_workspace_binding(
            &state.pool,
            &state.config,
            project_id,
            session_id,
        )
        .await?;
        let candidate = request
            .payload
            .get_mut("candidate")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| AppError::bad_request("invalid_input", "拟合并缺少候选对象"))?;
        candidate.insert("gitBaseCommit".into(), json!(binding.base_commit));
        candidate.insert("gitHeadCommit".into(), json!(binding.head_commit));
        candidate.insert("gitDirty".into(), json!(false));
        candidate.insert("treeId".into(), json!(binding.tree_id));
        candidate.insert(
            "workspaceSnapshot".into(),
            json!(binding.workspace_snapshot),
        );
        candidate.insert(
            "environmentFingerprint".into(),
            json!(binding.environment_fingerprint),
        );
    }
    let mut response = goal_branches::run_command(&state.pool, project_id, request).await?;
    if action == "proposal.approve" {
        let goal_branch_id = response
            .result
            .get("goalBranchId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| AppError::internal("Proposal 批准结果缺少 GoalBranch ID"))?;
        let session_id = response
            .result
            .get("sessionId")
            .and_then(Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| AppError::internal("Proposal 批准结果缺少 Session ID"))?;
        let workspace = match workspaces::provision_goal_branch(
            &state.pool,
            &state.config,
            project_id,
            goal_branch_id,
            session_id,
            client_request_id,
        )
        .await
        {
            Ok(workspace) => workspace,
            Err(provision_error) => {
                if let Err(pause_error) = workspaces::pause_failed_workspace_provision(
                    &state.pool,
                    project_id,
                    goal_branch_id,
                    session_id,
                    client_request_id,
                    provision_error.code(),
                    &provision_error.public_message(),
                )
                .await
                {
                    error!(
                        error = %pause_error,
                        goal_branch_id = %goal_branch_id,
                        session_id = %session_id,
                        "worktree 准备失败后无法持久化安全暂停"
                    );
                }
                return Err(provision_error);
            }
        };
        response
            .result
            .as_object_mut()
            .ok_or_else(|| AppError::internal("Proposal 批准结果不是可扩展的 JSON 对象"))?
            .insert("workspace".to_owned(), serde_json::to_value(workspace)?);
    }
    Ok(response)
}

pub async fn api_session_context(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let context = context_memory::get_context(&state.pool, project_id, session_id).await?;
    Ok(Json(serde_json::to_value(context)?))
}

pub async fn api_session_context_catalog(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Query(query): Query<context_memory::ContextCatalogQuery>,
) -> AppResult<Json<Value>> {
    let page =
        context_memory::list_context_catalog(&state.pool, project_id, session_id, query).await?;
    Ok(Json(serde_json::to_value(page)?))
}

pub async fn api_read_session_context(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<context_memory::ContextReadRequest>,
) -> AppResult<Json<Value>> {
    let response = context_memory::read_context(
        &state.pool,
        &state.config.artifact_root,
        project_id,
        session_id,
        request,
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_rebuild_session_context(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<context_memory::RebuildContextRequest>,
) -> AppResult<Json<Value>> {
    let response = context_memory::rebuild_context(
        &state.pool,
        &state.config.artifact_root,
        project_id,
        session_id,
        request,
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_plugin_catalog(State(state): State<Arc<AppState>>) -> AppResult<Json<Value>> {
    let plugins = plugins::list_catalog(&state.pool).await?;
    Ok(Json(json!({ "plugins": plugins })))
}

pub async fn api_register_plugin(
    State(state): State<Arc<AppState>>,
    Json(draft): Json<PluginManifestDraft>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let manifest = plugins::register_plugin(&state.pool, draft).await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(manifest)?)))
}

pub async fn api_seal_plugin(Json(draft): Json<PluginManifestDraft>) -> AppResult<Json<Value>> {
    Ok(Json(serde_json::to_value(plugins::seal_plugin(draft)?)?))
}

pub async fn api_register_plugin_publisher(
    State(state): State<Arc<AppState>>,
    Json(draft): Json<PluginPublisherDraft>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let publisher = plugins::register_publisher(&state.pool, draft).await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(publisher)?)))
}

pub async fn api_revoke_plugin_publisher(
    State(state): State<Arc<AppState>>,
    Path(publisher_id): Path<String>,
    Json(request): Json<plugins::RevokePluginPublisherRequest>,
) -> AppResult<Json<Value>> {
    let publisher = plugins::revoke_plugin_publisher(&state.pool, &publisher_id, request).await?;
    Ok(Json(serde_json::to_value(publisher)?))
}

pub async fn api_plugin_install_statement(
    Json(request): Json<PluginInstallStatementRequest>,
) -> AppResult<Json<Value>> {
    Ok(Json(plugins::preview_install_statement(request)?))
}

pub async fn api_install_signed_plugin(
    State(state): State<Arc<AppState>>,
    Json(request): Json<SignedPluginInstallRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let installation =
        plugins::install_signed_plugin(&state.pool, &state.config.runner_runtime_digest, request)
            .await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(installation)?),
    ))
}

pub async fn api_revoke_plugin_installation(
    State(state): State<Arc<AppState>>,
    Path(installation_id): Path<Uuid>,
    Json(request): Json<plugins::RevokePluginInstallationRequest>,
) -> AppResult<Json<Value>> {
    let installation =
        plugins::revoke_plugin_installation(&state.pool, installation_id, request).await?;
    Ok(Json(serde_json::to_value(installation)?))
}

pub async fn api_plugin_detail(
    State(state): State<Arc<AppState>>,
    Path((plugin_id, version)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    let manifest = plugins::get_plugin(&state.pool, &plugin_id, &version).await?;
    Ok(Json(serde_json::to_value(manifest)?))
}

pub async fn api_plugin_install_proof(
    State(state): State<Arc<AppState>>,
    Path((plugin_id, version)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    let detail = plugins::get_plugin_detail(&state.pool, &plugin_id, &version).await?;
    Ok(Json(serde_json::to_value(detail)?))
}

pub async fn api_resolve_plugin(
    State(state): State<Arc<AppState>>,
    Json(selector): Json<PluginSelector>,
) -> AppResult<Json<Value>> {
    let plugin = plugins::resolve_plugin(&state.pool, selector).await?;
    Ok(Json(serde_json::to_value(plugin)?))
}

pub async fn api_create_environment(
    State(state): State<Arc<AppState>>,
    Json(manifest): Json<EnvironmentManifest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let environment = plugins::create_environment(&state.pool, manifest).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(environment)?),
    ))
}

pub async fn api_environment_detail(
    State(state): State<Arc<AppState>>,
    Path(environment_id): Path<Uuid>,
) -> AppResult<Json<Value>> {
    let environment = plugins::get_environment(&state.pool, environment_id).await?;
    Ok(Json(serde_json::to_value(environment)?))
}

pub async fn api_read_plugin_context(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<plugins::ReadPluginContextRequest>,
) -> AppResult<Json<Value>> {
    let response = plugins::read_plugin_context(
        &state.pool,
        &state.config.runner_runtime_digest,
        project_id,
        session_id,
        request,
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_bind_environment(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<plugins::BindEnvironmentRequest>,
) -> AppResult<Json<Value>> {
    let response = plugins::bind_environment(&state.pool, project_id, session_id, request).await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_execute_tool(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<plugins::ExecuteToolRequest>,
) -> AppResult<Json<Value>> {
    let response = plugins::execute_tool(&state.pool, project_id, session_id, request).await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_create_plugin_install_request(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<plugins::CreatePluginInstallRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let record =
        plugins::create_plugin_install_request(&state.pool, project_id, session_id, request)
            .await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(record)?)))
}

pub async fn api_prepare_real_tool(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<plugins::PrepareRealToolRequest>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let response =
        plugins::prepare_real_tool(&state.pool, &state.config, project_id, session_id, request)
            .await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(response)?)))
}

pub async fn api_finalize_real_tool(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id, execution_id, job_id)): Path<(Uuid, Uuid, Uuid, Uuid)>,
    Json(request): Json<plugins::FinalizeRealToolRequest>,
) -> AppResult<Json<Value>> {
    let response = plugins::finalize_real_tool(
        &state.pool,
        &state.config,
        project_id,
        session_id,
        execution_id,
        job_id,
        request,
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_list_inputs(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let records = inputs::list_inputs(&state.pool, project_id, session_id).await?;
    Ok(Json(json!({ "inputs": records })))
}

pub async fn api_begin_input(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<BeginInputArtifact>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let response = inputs::begin_input(
        &state.pool,
        &state.config.artifact_root,
        state.config.input_max_bytes,
        project_id,
        session_id,
        input,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(serde_json::to_value(response)?)))
}

pub async fn api_append_input_chunk(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id, input_id)): Path<(Uuid, Uuid, Uuid)>,
    Query(query): Query<ChunkQuery>,
    body: Body,
) -> AppResult<Json<Value>> {
    let bytes = to_bytes(body, state.config.input_chunk_max_bytes)
        .await
        .map_err(|_| {
            AppError::bad_request("upload_chunk_too_large", "请求体超过单个上传分段的大小限制")
        })?;
    let response = inputs::append_chunk(
        &state.pool,
        &state.config.artifact_root,
        state.config.input_chunk_max_bytes,
        inputs::InputLocation {
            project_id,
            session_id,
            input_id,
        },
        query,
        bytes.to_vec(),
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_finish_input(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id, input_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<FinishInputArtifact>,
) -> AppResult<Json<Value>> {
    let response = inputs::finish_input(
        &state.pool,
        &state.config.artifact_root,
        project_id,
        session_id,
        input_id,
        input,
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_import_input(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id, input_id)): Path<(Uuid, Uuid, Uuid)>,
    Json(input): Json<ImportInputArtifact>,
) -> AppResult<Json<Value>> {
    let response = inputs::import_input(
        &state.pool,
        &state.config.artifact_root,
        state.config.input_inbox_copy_max_bytes,
        project_id,
        session_id,
        input_id,
        input,
    )
    .await?;
    Ok(Json(serde_json::to_value(response)?))
}

pub async fn api_download_input(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id, input_id)): Path<(Uuid, Uuid, Uuid)>,
) -> AppResult<(HeaderMap, Vec<u8>)> {
    let (record, bytes) = inputs::read_input(
        &state.pool,
        &state.config.artifact_root,
        project_id,
        session_id,
        input_id,
    )
    .await?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(
            record
                .trusted_media_type
                .as_deref()
                .unwrap_or("application/octet-stream"),
        )
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename*=UTF-8''{}",
            urlencoding::encode(&record.display_name)
        ))
        .map_err(|_| AppError::internal("InputArtifact 文件名无法用于下载响应"))?,
    );
    headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!(
            "\"{}\"",
            record.sha256.as_deref().unwrap_or_default()
        ))
        .map_err(|_| AppError::internal("InputArtifact 摘要不合法"))?,
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok((headers, bytes))
}

