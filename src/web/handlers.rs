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
use tracing::error;
use uuid::Uuid;

use crate::{
    application::{
        context_memory, goal_branches, graph, ideas, inputs, plugins, projects,
        scheduler as scheduler_app, workbench, workspaces,
    },
    artifacts::ArtifactStore,
    domain::{
        AppendProgressInput, ContributionInput, CreateBranchInput, GraphActionRequest,
        IntegrateBranchInput, ParkBranchInput, ProjectActionRequest, ProjectIntake,
        contribution_kind_label,
    },
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

#[derive(Debug, Default, Deserialize)]
pub struct IdeaPageQuery {
    pub view: Option<String>,
    pub notice: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct IdeaCommandForm {
    pub client_request_id: Option<Uuid>,
    pub subject_id: Option<Uuid>,
    pub return_idea_id: Option<Uuid>,
    pub action: String,
    pub title: Option<String>,
    pub body: Option<String>,
    pub source_kind: Option<String>,
    pub source_ref: Option<String>,
    pub revision_reason: Option<String>,
    pub expected_revision: Option<i32>,
    pub target_idea_id: Option<Uuid>,
    pub target_idea_ref: Option<String>,
    pub target_revision: Option<i32>,
    pub relation: Option<String>,
    pub rationale: Option<String>,
    pub project_intent: Option<String>,
    pub why_now: Option<String>,
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
    pub exploration_mode: Option<String>,
    pub exploration_budgets: Option<String>,
    pub exploration_candidates: Option<String>,
    pub uncertainty_reduction: Option<String>,
    pub tool_requirements: Option<String>,
    pub inferences: Option<String>,
    pub retained_notes: Option<String>,
    pub omitted_notes: Option<String>,
    pub source_idea_id: Option<Uuid>,
    pub source_idea_revision: Option<i32>,
    pub additional_sources: Option<String>,
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
    pub revision_request_id: Option<Uuid>,
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
    pub exploration_mode: Option<String>,
    pub exploration_budgets: Option<String>,
    pub exploration_candidates: Option<String>,
    pub uncertainty_reduction: Option<String>,
    pub tool_requirements: Option<String>,
    pub inferences: Option<String>,
    pub revision_reason: Option<String>,
    pub branch_name: Option<String>,
    pub assignment: Option<String>,
    pub agent_identity: Option<String>,
    pub contribution_kind: Option<String>,
    pub evidence_kind: Option<String>,
    pub evidence_stance: Option<String>,
    pub verification_status: Option<String>,
    pub evidence_ids: Option<String>,
    pub claim: Option<String>,
    pub observation: Option<String>,
    pub source_uri: Option<String>,
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
    pub new_evidence: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PluginInstallRequestForm {
    pub client_request_id: Uuid,
    pub plugin_id: String,
    pub version_requirement: String,
    pub capability: String,
    pub reason: String,
}

pub async fn dashboard(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DashboardQuery>,
) -> AppResult<Markup> {
    let projects = projects::list_projects(&state.pool).await?;
    Ok(views::dashboard(&projects, query.view.as_deref()))
}

pub async fn ideas_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<IdeaPageQuery>,
) -> AppResult<Markup> {
    let (ideas, links) = tokio::try_join!(
        ideas::list_ideas(&state.pool),
        ideas::list_links(&state.pool),
    )?;
    Ok(views::ideas(&ideas, &links, &query))
}

pub async fn new_idea_page(Query(query): Query<IdeaPageQuery>) -> Markup {
    views::new_idea(query.error.as_deref())
}

pub async fn idea_page(
    State(state): State<Arc<AppState>>,
    Path(idea_id): Path<Uuid>,
    Query(query): Query<IdeaPageQuery>,
) -> AppResult<Markup> {
    let (snapshot, all_ideas) = tokio::try_join!(
        ideas::get_snapshot(&state.pool, idea_id),
        ideas::list_ideas(&state.pool),
    )?;
    Ok(views::idea(&snapshot, &all_ideas, &query))
}

pub async fn idea_command_form(
    State(state): State<Arc<AppState>>,
    Form(form): Form<IdeaCommandForm>,
) -> Response {
    let request_id = form.client_request_id.unwrap_or_else(Uuid::new_v4);
    let subject_id = form.subject_id;
    let return_idea_id = form.return_idea_id.or_else(|| {
        if form.action.starts_with("idea.") && form.action != "idea.create" {
            subject_id
        } else {
            None
        }
    });
    let payload = idea_form_payload(&form);
    let result = match payload {
        Ok(payload) => {
            ideas::run_command(
                &state.pool,
                subject_id,
                IdeaCommandRequest {
                    client_request_id: request_id,
                    action: form.action.clone(),
                    payload,
                },
            )
            .await
        }
        Err(error) => Err(error),
    };
    match result {
        Ok(response) => {
            if let Some(project_id) = response
                .result
                .get("projectId")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
            {
                return Redirect::to(&format!(
                    "/projects/{project_id}?notice={}",
                    urlencoding::encode("ProjectProposal 已批准；根目标契约草案等待审核")
                ))
                .into_response();
            }
            let target = response
                .result
                .get("ideaId")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .or(return_idea_id);
            if let Some(idea_id) = target {
                Redirect::to(&format!(
                    "/ideas/{idea_id}?notice={}",
                    urlencoding::encode("想法空间已更新")
                ))
                .into_response()
            } else {
                Redirect::to("/ideas").into_response()
            }
        }
        Err(error) => {
            let target = return_idea_id
                .map(|idea_id| format!("/ideas/{idea_id}"))
                .unwrap_or_else(|| "/ideas/new".into());
            Redirect::to(&format!(
                "{target}?error={}",
                urlencoding::encode(&error.public_message())
            ))
            .into_response()
        }
    }
}

fn idea_form_payload(form: &IdeaCommandForm) -> AppResult<Value> {
    let lines = |value: &Option<String>| split_lines(value.as_deref());
    let revision = || {
        json!({
            "title": form.title.clone().unwrap_or_default(),
            "body": form.body.clone().unwrap_or_default(),
            "sourceKind": form.source_kind.clone().unwrap_or_else(|| "text".into()),
            "sourceRef": form.source_ref.clone().filter(|value| !value.trim().is_empty()),
            "revisionReason": form.revision_reason.clone().filter(|value| !value.trim().is_empty()),
        })
    };
    let proposal_revision = || -> AppResult<Value> {
        let idea_id = form
            .source_idea_id
            .or(form.return_idea_id)
            .ok_or_else(|| AppError::bad_request("missing_idea_source", "缺少起始想法"))?;
        let idea_revision = form
            .source_idea_revision
            .ok_or_else(|| AppError::bad_request("missing_idea_revision", "缺少起始想法版本"))?;
        let mut sources = vec![json!({
            "ideaId": idea_id,
            "ideaRevision": idea_revision,
            "role": "source",
            "rationale": "当前想法是立项来源",
        })];
        if let Some(encoded) = form
            .additional_sources
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            let references: Vec<String> = serde_json::from_str(encoded).map_err(|_| {
                AppError::bad_request("invalid_additional_sources", "附加想法来源格式不正确")
            })?;
            for reference in references {
                let (source_id, source_revision) = reference.split_once('@').ok_or_else(|| {
                    AppError::bad_request("invalid_additional_sources", "附加想法来源缺少版本")
                })?;
                let source_id = Uuid::parse_str(source_id).map_err(|_| {
                    AppError::bad_request("invalid_additional_sources", "附加想法 ID 不正确")
                })?;
                let source_revision = source_revision.parse::<i32>().map_err(|_| {
                    AppError::bad_request("invalid_additional_sources", "附加想法版本不正确")
                })?;
                if source_id != idea_id {
                    sources.push(json!({
                        "ideaId": source_id,
                        "ideaRevision": source_revision,
                        "role": "supporting",
                        "rationale": "用户在立项编辑器中选为支持来源",
                    }));
                }
            }
        }
        Ok(json!({
            "title": form.title.clone().unwrap_or_default(),
            "projectIntent": form.project_intent.clone().unwrap_or_default(),
            "whyNow": form.why_now.clone().unwrap_or_default(),
            "rootGoal": {
                "whyNeeded": form.why_now.clone().unwrap_or_default(),
                "contract": {
                    "desiredOutcome": form.desired_outcome.clone().unwrap_or_default(),
                    "hardConstraints": lines(&form.hard_constraints),
                    "subjectivePreferences": lines(&form.subjective_preferences),
                    "unknowns": lines(&form.unknowns),
                    "nonGoals": lines(&form.non_goals),
                    "validationPlan": lines(&form.validation_plan),
                    "judgmentTriggers": lines(&form.judgment_triggers),
                    "stopConditions": lines(&form.stop_conditions),
                    "expectedContributions": lines(&form.expected_contributions),
                    "exploration": {
                        "mode": form.exploration_mode.clone().unwrap_or_else(|| "delivery".into()),
                        "budgets": lines(&form.exploration_budgets),
                        "candidateOutputs": lines(&form.exploration_candidates),
                        "uncertaintyReduction": lines(&form.uncertainty_reduction),
                    },
                },
                "expectedContributions": lines(&form.expected_contributions),
                "explorationPlan": lines(&form.exploration_plan),
                "contextInheritance": {},
                "toolRequirements": lines(&form.tool_requirements),
                "inferences": lines(&form.inferences),
                "revisionReason": form.revision_reason.clone().filter(|value| !value.trim().is_empty()),
            },
            "retainedNotes": lines(&form.retained_notes),
            "omittedNotes": lines(&form.omitted_notes),
            "sources": sources,
            "revisionReason": form.revision_reason.clone().filter(|value| !value.trim().is_empty()),
        }))
    };
    match form.action.as_str() {
        "idea.create" => Ok(json!({ "revision": revision() })),
        "idea.revise" => Ok(json!({
            "expectedRevision": require_form_value(form.expected_revision, "缺少想法版本")?,
            "revision": revision(),
        })),
        "idea.link" => {
            let target_from_ref = form.target_idea_ref.as_deref().and_then(|value| {
                let (id, revision) = value.split_once('@')?;
                Some((Uuid::parse_str(id).ok()?, revision.parse::<i32>().ok()?))
            });
            let target_idea_id = form
                .target_idea_id
                .or_else(|| target_from_ref.map(|item| item.0));
            let target_revision = form
                .target_revision
                .or_else(|| target_from_ref.map(|item| item.1));
            Ok(json!({
            "expectedSourceRevision": require_form_value(form.expected_revision, "缺少当前想法版本")?,
            "targetIdeaId": require_form_value(target_idea_id, "请选择关联想法")?,
            "expectedTargetRevision": require_form_value(target_revision, "缺少目标想法版本")?,
            "relation": form.relation.clone().unwrap_or_default(),
            "rationale": form.rationale.clone().unwrap_or_default(),
            }))
        }
        "idea.archive" => Ok(json!({ "reason": form.rationale.clone().unwrap_or_default() })),
        "project_proposal.create" => Ok(json!({ "revision": proposal_revision()? })),
        "project_proposal.revise" => Ok(json!({
            "expectedRevision": require_form_value(form.expected_revision, "缺少提案版本")?,
            "revision": proposal_revision()?,
        })),
        "project_proposal.submit" | "project_proposal.approve" => Ok(json!({
            "expectedRevision": require_form_value(form.expected_revision, "缺少提案版本")?,
        })),
        "project_proposal.reject" | "project_proposal.cancel" => Ok(json!({
            "rationale": form.rationale.clone().unwrap_or_default(),
        })),
        _ => Err(AppError::bad_request(
            "unsupported_idea_action",
            "不支持的想法动作",
        )),
    }
}

fn require_form_value<T>(value: Option<T>, message: &'static str) -> AppResult<T> {
    value.ok_or_else(|| AppError::bad_request("missing_form_value", message))
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
            run_goal_command_with_workspace(
                &state,
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

pub async fn plugin_install_request_form(
    State(state): State<Arc<AppState>>,
    Path((project_id, session_id)): Path<(Uuid, Uuid)>,
    Form(form): Form<PluginInstallRequestForm>,
) -> Response {
    let result = plugins::create_plugin_install_request(
        &state.pool,
        project_id,
        session_id,
        plugins::CreatePluginInstallRequest {
            client_request_id: form.client_request_id,
            plugin_id: form.plugin_id,
            version_requirement: form.version_requirement,
            capability: form.capability,
            reason: form.reason,
        },
    )
    .await;
    let location = match result {
        Ok(_) => format!(
            "/projects/{project_id}?tab=goals&notice={}&session={session_id}#worksite-plugins",
            urlencoding::encode("插件安装请求已记录；签名安装完成前不会假装可用"),
        ),
        Err(error) => format!(
            "/projects/{project_id}?tab=goals&error={}&session={session_id}#worksite-plugins",
            urlencoding::encode(&error.public_message()),
        ),
    };
    Redirect::to(&location).into_response()
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
                "exploration": {
                    "mode": form.exploration_mode.clone().unwrap_or_else(|| "delivery".into()),
                    "budgets": split_lines(form.exploration_budgets.as_deref()),
                    "candidateOutputs": split_lines(form.exploration_candidates.as_deref()),
                    "uncertaintyReduction": split_lines(form.uncertainty_reduction.as_deref()),
                },
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
            "evidenceIds": parse_uuid_list(form.evidence_ids.as_deref())?,
            "supersedesId": null,
        })),
        "session.add_evidence" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "kind": form.evidence_kind.clone().unwrap_or_else(|| "observation".into()),
            "stance": form.evidence_stance.clone().unwrap_or_else(|| "supports".into()),
            "claim": form.claim.clone().unwrap_or_default(),
            "observation": form.observation.clone().unwrap_or_default(),
            "sourceUri": optional_form_text(form.source_uri.as_deref()),
            "artifactId": null,
            "toolCallId": null,
            "verificationStatus": form.verification_status.clone().unwrap_or_else(|| "unverified".into()),
        })),
        "contract.propose_revision" => Ok(json!({
            "goalBranchId": required_uuid(form.goal_branch_id, "目标枝干")?,
            "expectedContractVersionId": required_uuid(form.contract_version_id, "当前契约版本")?,
            "proposedBySessionId": form.session_id,
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
                "exploration": {
                    "mode": form.exploration_mode.clone().unwrap_or_else(|| "delivery".into()),
                    "budgets": split_lines(form.exploration_budgets.as_deref()),
                    "candidateOutputs": split_lines(form.exploration_candidates.as_deref()),
                    "uncertaintyReduction": split_lines(form.uncertainty_reduction.as_deref()),
                },
            },
            "reason": form.reason.clone().unwrap_or_default(),
            "sourceAnnotations": [{
                "fieldPath": "/",
                "sourceKind": "human_input",
                "sourceRef": null,
                "note": form.reason.clone().unwrap_or_else(|| "用户从工作台提出契约调整".into()),
            }],
        })),
        "contract.accept_revision" | "contract.reject_revision" => Ok(json!({
            "revisionRequestId": required_uuid(form.revision_request_id, "契约修订")?,
            "rationale": form.rationale.clone().unwrap_or_default(),
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
        "session.stop" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "reason": form.reason.clone().unwrap_or_default(),
        })),
        "merge.propose" => Ok(json!({
            "sessionId": required_uuid(form.session_id, "Session")?,
            "candidate": {
                "contributionIds": parse_uuid_list(form.contribution_ids.as_deref())?,
                "evidenceIds": parse_uuid_list(form.evidence_ids.as_deref())?,
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
        "merge.withdraw" => Ok(json!({
            "reviewGateId": required_uuid(form.review_gate_id, "ReviewGate")?,
            "reason": form.reason.clone().unwrap_or_default(),
            "newEvidence": split_lines(form.new_evidence.as_deref()),
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
        "goal_branch.archive" => Ok(json!({
            "goalBranchId": required_uuid(form.goal_branch_id, "目标枝干")?,
            "reason": form.reason.clone().unwrap_or_default(),
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
