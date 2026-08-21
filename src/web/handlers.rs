use std::{collections::HashMap, sync::Arc};

use axum::{
    Form, Json,
    body::{Body, to_bytes},
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
    application::{goal_branches, graph, inputs, plugins, projects, workbench},
    artifacts::ArtifactStore,
    domain::{
        AppendProgressInput, ContributionInput, CreateBranchInput, GraphActionRequest,
        IntegrateBranchInput, ParkBranchInput, ProjectActionRequest, ProjectIntake,
        contribution_kind_label,
    },
    error::{AppError, AppResult},
    input_artifacts::{BeginInputArtifact, ChunkQuery, FinishInputArtifact, ImportInputArtifact},
    tooling::{EnvironmentManifest, PluginManifestDraft, PluginSelector},
};

use super::{AppState, views};

#[derive(Debug, Default, Deserialize)]
pub struct ProjectPageQuery {
    pub tab: Option<String>,
    pub node: Option<Uuid>,
    pub session: Option<Uuid>,
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

#[derive(Debug, Default, Deserialize)]
pub struct GoalCommandForm {
    pub client_request_id: Option<Uuid>,
    pub action: String,
    pub payload: Option<String>,
    pub return_session_id: Option<Uuid>,
    pub proposal_id: Option<Uuid>,
    pub expected_revision: Option<i32>,
    pub parent_session_id: Option<Uuid>,
    pub goal_branch_id: Option<Uuid>,
    pub previous_session_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub contract_version_id: Option<Uuid>,
    pub review_gate_id: Option<Uuid>,
    pub why_needed: Option<String>,
    pub desired_outcome: Option<String>,
    pub hard_constraints: Option<String>,
    pub subjective_preferences: Option<String>,
    pub unknowns: Option<String>,
    pub non_goals: Option<String>,
    pub validation_plan: Option<String>,
    pub judgment_triggers: Option<String>,
    pub stop_conditions: Option<String>,
    pub expected_contributions: Option<String>,
    pub exploration_plan: Option<String>,
    pub tool_requirements: Option<String>,
    pub inferences: Option<String>,
    pub revision_reason: Option<String>,
    pub branch_name: Option<String>,
    pub assignment: Option<String>,
    pub agent_identity: Option<String>,
    pub contribution_kind: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub question: Option<String>,
    pub candidates: Option<String>,
    pub evidence: Option<String>,
    pub recommendation: Option<String>,
    pub reason: Option<String>,
    pub safe_checkpoint: Option<String>,
    pub attempted: Option<String>,
    pub risk: Option<String>,
    pub user_action: Option<String>,
    pub resolution: Option<String>,
    pub contribution_ids: Option<String>,
    pub test_evidence: Option<String>,
    pub risks: Option<String>,
    pub self_check: Option<String>,
    pub reviewer_identity: Option<String>,
    pub review_decision: Option<String>,
    pub rationale: Option<String>,
    pub selected_contribution_ids: Option<String>,
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
    let (snapshot, goal_snapshot, activity) = tokio::try_join!(
        projects::get_snapshot(&state.pool, project_id),
        goal_branches::get_snapshot(&state.pool, project_id),
        workbench::get_activity(&state.pool, project_id),
    )?;
    Ok(views::project(&snapshot, &goal_snapshot, &activity, &query))
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

pub async fn goal_command_form(
    State(state): State<Arc<AppState>>,
    Path(project_id): Path<Uuid>,
    Form(form): Form<GoalCommandForm>,
) -> Response {
    let session_hint = form
        .return_session_id
        .or(form.session_id)
        .or(form.parent_session_id)
        .or(form.previous_session_id);
    let payload = goal_form_payload(&form);
    let result = match payload {
        Ok(payload) => {
            goal_branches::run_command(
                &state.pool,
                project_id,
                goal_branches::GoalCommandRequest {
                    client_request_id: form.client_request_id.unwrap_or_else(Uuid::new_v4),
                    action: form.action.clone(),
                    payload,
                },
            )
            .await
        }
        Err(error) => Err(error),
    };
    redirect_goal_result(project_id, result, session_hint)
}

fn goal_form_payload(form: &GoalCommandForm) -> AppResult<Value> {
    if let Some(payload) = form
        .payload
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return serde_json::from_str(payload).map_err(|_| {
            AppError::bad_request("invalid_goal_input", "表单中的目标枝干参数不是有效 JSON")
        });
    }
    let required_uuid = |value: Option<Uuid>, label: &'static str| {
        value.ok_or_else(|| AppError::bad_request("invalid_goal_input", format!("缺少{label}标识")))
    };
    let revision = || {
        json!({
            "whyNeeded": form.why_needed.clone().unwrap_or_default(),
            "contract": {
                "desiredOutcome": form.desired_outcome.clone().unwrap_or_default(),
                "hardConstraints": split_lines(form.hard_constraints.as_deref()),
                "subjectivePreferences": split_lines(form.subjective_preferences.as_deref()),
                "unknowns": split_lines(form.unknowns.as_deref()),
                "nonGoals": split_lines(form.non_goals.as_deref()),
                "validationPlan": split_lines(form.validation_plan.as_deref()),
                "judgmentTriggers": split_lines(form.judgment_triggers.as_deref()),
                "stopConditions": split_lines(form.stop_conditions.as_deref()),
                "expectedContributions": split_lines(form.expected_contributions.as_deref()),
            },
            "expectedContributions": split_lines(form.expected_contributions.as_deref()),
            "explorationPlan": split_lines(form.exploration_plan.as_deref()),
            "contextInheritance": {},
            "toolRequirements": split_lines(form.tool_requirements.as_deref()),
            "inferences": split_lines(form.inferences.as_deref()),
            "revisionReason": optional_form_text(form.revision_reason.as_deref()),
        })
    };

    match form.action.as_str() {
        "proposal.create" => Ok(json!({ "revision": revision() })),
        "proposal.revise" => Ok(json!({
            "proposalId": required_uuid(form.proposal_id, "Proposal")?,
            "expectedRevision": form.expected_revision.unwrap_or_default(),
            "revision": revision(),
        })),
        "proposal.submit" => Ok(json!({
            "proposalId": required_uuid(form.proposal_id, "Proposal")?,
            "expectedRevision": form.expected_revision.unwrap_or_default(),
        })),
        "proposal.cancel" => Ok(json!({
            "proposalId": required_uuid(form.proposal_id, "Proposal")?,
            "reason": form.reason.clone().unwrap_or_default(),
        })),
        "proposal.approve" => Ok(json!({
            "proposalId": required_uuid(form.proposal_id, "Proposal")?,
            "expectedRevision": form.expected_revision.unwrap_or_default(),
            "branchName": form.branch_name.clone().unwrap_or_default(),
            "assignment": form.assignment.clone().unwrap_or_default(),
            "agentIdentity": optional_form_text(form.agent_identity.as_deref()),
        })),
        "session.propose_child" => Ok(json!({
            "parentSessionId": required_uuid(form.parent_session_id, "父 Session")?,
            "revision": revision(),
        })),
        "session.add_contribution" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "kind": form.contribution_kind.clone().unwrap_or_else(|| "finding".into()),
            "title": form.title.clone().unwrap_or_default(),
            "body": form.body.clone().unwrap_or_default(),
            "artifactId": null,
            "evidenceRefs": [],
            "supersedesId": null,
        })),
        "session.request_judgment" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "question": form.question.clone().unwrap_or_default(),
            "candidates": split_lines(form.candidates.as_deref()),
            "evidence": optional_form_text(form.evidence.as_deref()),
            "recommendation": optional_form_text(form.recommendation.as_deref()),
        })),
        "session.pause_exception" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "reason": form.reason.clone().unwrap_or_default(),
            "safeCheckpoint": form.safe_checkpoint.clone().unwrap_or_default(),
            "attempted": form.attempted.clone().unwrap_or_default(),
            "risk": form.risk.clone().unwrap_or_default(),
            "userAction": form.user_action.clone().unwrap_or_default(),
            "recommendation": form.recommendation.clone().unwrap_or_default(),
        })),
        "session.pause_manual" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "reason": form.reason.clone().unwrap_or_default(),
        })),
        "session.resume" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "resolution": form.resolution.clone().unwrap_or_default(),
        })),
        "session.start_next" => Ok(json!({
            "goalBranchId": required_uuid(form.goal_branch_id, "目标枝干")?,
            "previousSessionId": required_uuid(form.previous_session_id, "上一 Session")?,
            "assignment": form.assignment.clone().unwrap_or_default(),
            "agentIdentity": optional_form_text(form.agent_identity.as_deref()),
        })),
        "merge.propose" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "candidate": {
                "contributionIds": parse_uuid_list(form.contribution_ids.as_deref())?,
                "contractVersionId": required_uuid(form.contract_version_id, "契约版本")?,
                "gitBaseCommit": null,
                "gitHeadCommit": null,
                "gitDirty": false,
                "environmentFingerprint": null,
                "testEvidence": split_lines(form.test_evidence.as_deref()),
                "risks": split_lines(form.risks.as_deref()),
                "selfCheck": form.self_check.clone().unwrap_or_default(),
            }
        })),
        "review.ai_record" => Ok(json!({
            "reviewGateId": required_uuid(form.review_gate_id, "ReviewGate")?,
            "reviewerIdentity": form.reviewer_identity.clone().unwrap_or_default(),
            "decision": form.review_decision.clone().unwrap_or_default(),
            "rationale": form.rationale.clone().unwrap_or_default(),
            "contractCheck": { "recordedFrom": "workbench" },
            "retestEvidence": split_lines(form.test_evidence.as_deref()),
        })),
        "review.human_decide" => Ok(json!({
            "reviewGateId": required_uuid(form.review_gate_id, "ReviewGate")?,
            "decision": form.review_decision.clone().unwrap_or_default(),
            "rationale": form.rationale.clone().unwrap_or_default(),
            "selectedContributionIds": parse_uuid_list(form.selected_contribution_ids.as_deref())?,
        })),
        _ => Err(AppError::bad_request(
            "unsupported_goal_action",
            "不支持的目标枝干表单动作",
        )),
    }
}

fn split_lines(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect()
}

fn optional_form_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_uuid_list(value: Option<&str>) -> AppResult<Vec<Uuid>> {
    value
        .unwrap_or_default()
        .split([',', '\n', '\r'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            Uuid::parse_str(item).map_err(|_| {
                AppError::bad_request("invalid_contribution", "Contribution 标识不合法")
            })
        })
        .collect()
}

fn redirect_goal_result(
    project_id: Uuid,
    result: AppResult<goal_branches::GoalCommandResponse>,
    session_id: Option<Uuid>,
) -> Response {
    let session = session_id
        .map(|session_id| format!("&session={session_id}"))
        .unwrap_or_default();
    let location = match result {
        Ok(_) => format!(
            "/projects/{project_id}?tab=goals&notice={}{}#goal-workbench",
            urlencoding::encode("目标枝干已更新"),
            session,
        ),
        Err(error) => format!(
            "/projects/{project_id}?tab=goals&error={}{}#goal-workbench",
            urlencoding::encode(&error.public_message()),
            session,
        ),
    };
    Redirect::to(&location).into_response()
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
    let response = goal_branches::run_command(&state.pool, project_id, request).await?;
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

pub async fn api_plugin_detail(
    State(state): State<Arc<AppState>>,
    Path((plugin_id, version)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    let manifest = plugins::get_plugin(&state.pool, &plugin_id, &version).await?;
    Ok(Json(serde_json::to_value(manifest)?))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_goal_form_keeps_unknowns_without_requiring_json() {
        let form = GoalCommandForm {
            action: "proposal.create".into(),
            why_needed: Some("需要探索".into()),
            desired_outcome: Some("找到可行方案".into()),
            unknowns: Some("性能上限\n\n最终手感 ".into()),
            validation_plan: Some("实际测试".into()),
            stop_conditions: Some("用户确认".into()),
            ..GoalCommandForm::default()
        };
        let payload = goal_form_payload(&form).unwrap();
        assert_eq!(
            payload["revision"]["contract"]["unknowns"],
            json!(["性能上限", "最终手感"])
        );
        assert_eq!(
            payload["revision"]["contract"]["desiredOutcome"],
            "找到可行方案"
        );
    }

    #[test]
    fn generic_json_form_remains_a_compatible_escape_hatch() {
        let form = GoalCommandForm {
            action: "proposal.submit".into(),
            payload: Some(r#"{"expectedRevision":2}"#.into()),
            ..GoalCommandForm::default()
        };
        assert_eq!(goal_form_payload(&form).unwrap()["expectedRevision"], 2);
    }

    #[test]
    fn contribution_identifier_lists_are_strict() {
        assert!(parse_uuid_list(Some("not-a-uuid")).is_err());
        let id = Uuid::new_v4();
        assert_eq!(parse_uuid_list(Some(&id.to_string())).unwrap(), vec![id]);
    }
}
