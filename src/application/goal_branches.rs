use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    goal_domain::{
        BranchProposalRevisionDraft, CandidateSnapshot, CommandReceiptIdentity, GoalActor,
        GoalBranchStatus, GoalContractDraft, ProposalStatus, ReviewDecisionKind, ReviewGateStatus,
        SessionStatus, canonical_json_sha256, validate_new_running_session,
    },
    goal_models::{
        GoalAttentionRecord, GoalBranchRecord, GoalContractVersionRecord, GoalContributionRecord,
        GoalEventRecord, GoalGraphSnapshot, GoalIntegrationContributionRecord,
        GoalIntegrationRecord, GoalProposalRecord, GoalProposalRevisionRecord,
        GoalReviewDecisionRecord, GoalReviewGateRecord, GoalSessionRecord,
    },
    models::Project,
};

type GoalTransaction<'a> = Transaction<'a, Postgres>;

const CONTRIBUTION_KINDS: &[&str] = &[
    "artifact",
    "finding",
    "evidence",
    "decision",
    "condition",
    "code_change",
    "other",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalCommandRequest {
    pub client_request_id: Uuid,
    pub action: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalCommandResponse {
    pub replayed: bool,
    pub result: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateProposalInput {
    revision: BranchProposalRevisionDraft,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviseProposalInput {
    proposal_id: Uuid,
    expected_revision: i32,
    revision: BranchProposalRevisionDraft,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposalVersionInput {
    proposal_id: Uuid,
    expected_revision: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CancelProposalInput {
    proposal_id: Uuid,
    reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApproveProposalInput {
    proposal_id: Uuid,
    expected_revision: i32,
    branch_name: String,
    assignment: String,
    agent_identity: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposeChildInput {
    parent_session_id: Uuid,
    revision: BranchProposalRevisionDraft,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AddContributionInput {
    session_id: Uuid,
    kind: String,
    title: String,
    body: String,
    artifact_id: Option<Uuid>,
    #[serde(default)]
    evidence_refs: Value,
    supersedes_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestJudgmentInput {
    session_id: Uuid,
    question: String,
    #[serde(default)]
    candidates: Vec<String>,
    evidence: Option<String>,
    recommendation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PauseExceptionInput {
    session_id: Uuid,
    reason: String,
    safe_checkpoint: String,
    attempted: String,
    risk: String,
    user_action: String,
    recommendation: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PauseManualInput {
    session_id: Uuid,
    reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResumeSessionInput {
    session_id: Uuid,
    resolution: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartNextSessionInput {
    goal_branch_id: Uuid,
    previous_session_id: Uuid,
    assignment: String,
    agent_identity: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProposeMergeInput {
    session_id: Uuid,
    candidate: CandidateSnapshot,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AiReviewInput {
    review_gate_id: Uuid,
    reviewer_identity: String,
    decision: ReviewDecisionKind,
    rationale: String,
    #[serde(default = "empty_object")]
    contract_check: Value,
    #[serde(default)]
    retest_evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct HumanReviewInput {
    review_gate_id: Uuid,
    decision: ReviewDecisionKind,
    rationale: String,
    #[serde(default)]
    selected_contribution_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, FromRow)]
struct ProposalStateRow {
    id: Uuid,
    parent_goal_branch_id: Option<Uuid>,
    parent_session_id: Option<Uuid>,
    status: String,
    current_revision: i32,
}

#[derive(Clone, Debug, FromRow)]
struct ProposalRevisionRow {
    why_needed: String,
    contract: Json<Value>,
    expected_contributions: Json<Value>,
    context_inheritance: Json<Value>,
}

#[derive(Clone, Debug, FromRow)]
struct SessionStateRow {
    id: Uuid,
    goal_branch_id: Uuid,
    session_number: i32,
    status: String,
    assignment: String,
    agent_identity: Option<String>,
    contract_version_id: Uuid,
    environment_fingerprint: Option<String>,
}

#[derive(Clone, Debug, FromRow)]
struct BranchStateRow {
    id: Uuid,
    parent_goal_branch_id: Option<Uuid>,
    inherited_from_session_id: Option<Uuid>,
    status: String,
    current_contract_version_id: Uuid,
    head_session_id: Uuid,
    environment_fingerprint: Option<String>,
}

#[derive(Clone, Debug, FromRow)]
struct GateStateRow {
    id: Uuid,
    goal_branch_id: Uuid,
    session_id: Uuid,
    status: String,
    candidate_hash: String,
    candidate_snapshot: Json<Value>,
}

pub async fn run_command(
    pool: &PgPool,
    project_id: Uuid,
    request: GoalCommandRequest,
) -> AppResult<GoalCommandResponse> {
    let action = clean_text("动作", request.action, 120)?;
    let identity = CommandReceiptIdentity::from_input(&action, &request.payload)?;
    let mut transaction = pool.begin().await?;

    let project_exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?;
    if project_exists.is_none() {
        return Err(AppError::not_found("项目不存在"));
    }

    let existing: Option<(String, String, Json<Value>)> = sqlx::query_as(
        "SELECT command_kind, input_hash, result FROM goal_command_receipts \
         WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((command_kind, input_hash, result)) = existing {
        CommandReceiptIdentity {
            command_kind,
            input_hash,
        }
        .ensure_replay_matches(&identity)?;
        transaction.commit().await?;
        return Ok(GoalCommandResponse {
            replayed: true,
            result: result.0,
        });
    }

    let result = match action.as_str() {
        "proposal.create" => {
            let input: CreateProposalInput = decode_payload(request.payload)?;
            create_root_proposal(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "proposal.revise" => {
            let input: ReviseProposalInput = decode_payload(request.payload)?;
            revise_proposal(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "proposal.submit" => {
            let input: ProposalVersionInput = decode_payload(request.payload)?;
            submit_proposal(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "proposal.cancel" => {
            let input: CancelProposalInput = decode_payload(request.payload)?;
            cancel_proposal(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "proposal.approve" => {
            let input: ApproveProposalInput = decode_payload(request.payload)?;
            approve_proposal(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "session.propose_child" => {
            let input: ProposeChildInput = decode_payload(request.payload)?;
            propose_child(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "session.add_contribution" => {
            let input: AddContributionInput = decode_payload(request.payload)?;
            add_contribution(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "session.request_judgment" => {
            let input: RequestJudgmentInput = decode_payload(request.payload)?;
            request_judgment(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "session.pause_exception" => {
            let input: PauseExceptionInput = decode_payload(request.payload)?;
            pause_exception(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "session.pause_manual" => {
            let input: PauseManualInput = decode_payload(request.payload)?;
            pause_manual(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "session.resume" => {
            let input: ResumeSessionInput = decode_payload(request.payload)?;
            resume_session(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "session.start_next" => {
            let input: StartNextSessionInput = decode_payload(request.payload)?;
            start_next_session(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "merge.propose" => {
            let input: ProposeMergeInput = decode_payload(request.payload)?;
            propose_merge(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "review.ai_record" => {
            let input: AiReviewInput = decode_payload(request.payload)?;
            record_ai_review(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        "review.human_decide" => {
            let input: HumanReviewInput = decode_payload(request.payload)?;
            record_human_review(
                &mut transaction,
                project_id,
                request.client_request_id,
                input,
            )
            .await?
        }
        _ => {
            return Err(AppError::bad_request(
                "unsupported_goal_action",
                "不支持的目标枝干动作",
            ));
        }
    };

    sqlx::query(
        "INSERT INTO goal_command_receipts \
         (project_id, client_request_id, command_kind, input_hash, result) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .bind(&identity.command_kind)
    .bind(&identity.input_hash)
    .bind(Json(result.clone()))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(GoalCommandResponse {
        replayed: false,
        result,
    })
}

pub async fn get_snapshot(pool: &PgPool, project_id: Uuid) -> AppResult<GoalGraphSnapshot> {
    let project = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    let proposals = sqlx::query_as::<_, GoalProposalRecord>(
        "SELECT * FROM goal_branch_proposals WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let proposal_revisions = sqlx::query_as::<_, GoalProposalRevisionRecord>(
        "SELECT r.* FROM goal_branch_proposal_revisions r \
         JOIN goal_branch_proposals p ON p.id = r.proposal_id \
         WHERE p.project_id = $1 ORDER BY r.created_at, r.revision",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let contracts = sqlx::query_as::<_, GoalContractVersionRecord>(
        "SELECT * FROM goal_contract_versions WHERE project_id = $1 \
         ORDER BY created_at, version",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let branches = sqlx::query_as::<_, GoalBranchRecord>(
        "SELECT * FROM goal_branches WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let sessions = sqlx::query_as::<_, GoalSessionRecord>(
        "SELECT * FROM goal_sessions WHERE project_id = $1 \
         ORDER BY started_at, session_number",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let contributions = sqlx::query_as::<_, GoalContributionRecord>(
        "SELECT * FROM goal_contributions WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let review_gates = sqlx::query_as::<_, GoalReviewGateRecord>(
        "SELECT * FROM goal_review_gates WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let review_decisions = sqlx::query_as::<_, GoalReviewDecisionRecord>(
        "SELECT * FROM goal_review_decisions WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let integrations = sqlx::query_as::<_, GoalIntegrationRecord>(
        "SELECT * FROM goal_integrations WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let integration_contributions = sqlx::query_as::<_, GoalIntegrationContributionRecord>(
        "SELECT c.* FROM goal_integration_contributions c \
         JOIN goal_integrations i ON i.id = c.integration_id \
         WHERE i.project_id = $1 ORDER BY c.created_at, c.contribution_id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let attention_items = sqlx::query_as::<_, GoalAttentionRecord>(
        "SELECT * FROM goal_attention_items WHERE project_id = $1 ORDER BY created_at, id",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let events = sqlx::query_as::<_, GoalEventRecord>(
        "SELECT * FROM goal_events WHERE project_id = $1 ORDER BY sequence",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    Ok(GoalGraphSnapshot {
        model_version: "goal-branch-v1",
        project,
        proposals,
        proposal_revisions,
        contracts,
        branches,
        sessions,
        contributions,
        review_gates,
        review_decisions,
        integrations,
        integration_contributions,
        attention_items,
        events,
    })
}

fn decode_payload<T: DeserializeOwned>(value: Value) -> AppResult<T> {
    serde_json::from_value(value).map_err(|error| {
        AppError::bad_request("invalid_goal_input", format!("目标枝干参数不完整：{error}"))
    })
}

fn empty_object() -> Value {
    json!({})
}

async fn create_root_proposal(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: CreateProposalInput,
) -> AppResult<Value> {
    let revision = input.revision.validate(false)?;
    let root_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_branches \
         WHERE project_id = $1 AND parent_goal_branch_id IS NULL)",
    )
    .bind(project_id)
    .fetch_one(&mut **transaction)
    .await?;
    if root_exists {
        return Err(AppError::conflict(
            "root_goal_exists",
            "该项目已经有根目标枝干，请从当前 Session 提出子目标",
        ));
    }

    let proposal_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO goal_branch_proposals \
         (id, project_id, status, current_revision, created_by) \
         VALUES ($1, $2, 'draft', 1, 'agent')",
    )
    .bind(proposal_id)
    .bind(project_id)
    .execute(&mut **transaction)
    .await?;
    insert_proposal_revision(transaction, proposal_id, 1, &revision, "agent").await?;
    insert_goal_event(
        transaction,
        project_id,
        "proposal",
        proposal_id,
        "proposal.created",
        "agent",
        None,
        client_request_id,
        json!({ "revision": 1, "root": true }),
    )
    .await?;
    Ok(json!({
        "proposalId": proposal_id,
        "revision": 1,
        "status": ProposalStatus::Draft.as_str(),
    }))
}

async fn revise_proposal(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: ReviseProposalInput,
) -> AppResult<Value> {
    let revision = input.revision.validate(false)?;
    let proposal = load_proposal_for_update(transaction, project_id, input.proposal_id).await?;
    if proposal.current_revision != input.expected_revision {
        return Err(AppError::conflict(
            "stale_revision",
            "BranchProposal 已有更新，请基于最新修订继续",
        ));
    }
    let status = ProposalStatus::try_from(proposal.status.as_str())?.revise()?;
    let next_revision = proposal.current_revision + 1;
    insert_proposal_revision(transaction, proposal.id, next_revision, &revision, "agent").await?;
    sqlx::query(
        "UPDATE goal_branch_proposals \
         SET status = $1, current_revision = $2, updated_at = now() WHERE id = $3",
    )
    .bind(status.as_str())
    .bind(next_revision)
    .bind(proposal.id)
    .execute(&mut **transaction)
    .await?;
    resolve_attention_by_key(
        transaction,
        project_id,
        &format!("proposal:{}:review", proposal.id),
        "Proposal 已进入新修订",
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "proposal",
        proposal.id,
        "proposal.revised",
        "agent",
        None,
        client_request_id,
        json!({
            "fromRevision": proposal.current_revision,
            "revision": next_revision,
        }),
    )
    .await?;
    Ok(json!({
        "proposalId": proposal.id,
        "revision": next_revision,
        "status": status.as_str(),
    }))
}

async fn submit_proposal(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: ProposalVersionInput,
) -> AppResult<Value> {
    let proposal = load_proposal_for_update(transaction, project_id, input.proposal_id).await?;
    if proposal.current_revision != input.expected_revision {
        return Err(AppError::conflict(
            "stale_revision",
            "BranchProposal 已有更新，请先查看最新修订",
        ));
    }
    let revision =
        load_proposal_revision(transaction, proposal.id, proposal.current_revision).await?;
    let _: GoalContractDraft =
        serde_json::from_value::<GoalContractDraft>(revision.contract.0.clone())
            .map_err(|_| AppError::bad_request("invalid_goal_contract", "目标契约结构不合法"))?
            .validate_for_approval()?;
    let status = ProposalStatus::try_from(proposal.status.as_str())?.submit()?;
    sqlx::query("UPDATE goal_branch_proposals SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status.as_str())
        .bind(proposal.id)
        .execute(&mut **transaction)
        .await?;
    insert_attention(
        transaction,
        project_id,
        proposal.parent_goal_branch_id,
        proposal.parent_session_id,
        "branch_review",
        &format!("proposal:{}:review", proposal.id),
        "BranchProposal 等待批准",
        &revision.why_needed,
        None,
        None,
        None,
        Some("审核目标方向、关键边界与未知项"),
        Some("批准、退回修订或取消 Proposal"),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "proposal",
        proposal.id,
        "proposal.submitted",
        "agent",
        None,
        client_request_id,
        json!({ "revision": proposal.current_revision }),
    )
    .await?;
    Ok(json!({
        "proposalId": proposal.id,
        "revision": proposal.current_revision,
        "status": status.as_str(),
    }))
}

async fn cancel_proposal(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: CancelProposalInput,
) -> AppResult<Value> {
    let reason = clean_text("取消理由", input.reason, 4_000)?;
    let proposal = load_proposal_for_update(transaction, project_id, input.proposal_id).await?;
    let status = ProposalStatus::try_from(proposal.status.as_str())?.cancel(GoalActor::Human)?;
    sqlx::query(
        "UPDATE goal_branch_proposals \
         SET status = $1, updated_at = now(), decided_at = now() WHERE id = $2",
    )
    .bind(status.as_str())
    .bind(proposal.id)
    .execute(&mut **transaction)
    .await?;
    resolve_attention_by_key(
        transaction,
        project_id,
        &format!("proposal:{}:review", proposal.id),
        &reason,
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "proposal",
        proposal.id,
        "proposal.cancelled",
        "human",
        None,
        client_request_id,
        json!({ "reason": reason }),
    )
    .await?;
    Ok(json!({
        "proposalId": proposal.id,
        "status": status.as_str(),
        "parentSessionRemainsPaused": proposal.parent_session_id.is_some(),
    }))
}

async fn propose_child(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: ProposeChildInput,
) -> AppResult<Value> {
    let revision = input.revision.validate(true)?;
    let parent = load_session_for_update(transaction, project_id, input.parent_session_id).await?;
    let next_status = SessionStatus::try_from(parent.status.as_str())?.propose_child()?;
    let open_proposal_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_branch_proposals \
         WHERE parent_session_id = $1 AND status IN ('draft', 'awaiting_approval'))",
    )
    .bind(parent.id)
    .fetch_one(&mut **transaction)
    .await?;
    if open_proposal_exists {
        return Err(AppError::conflict(
            "proposal_already_pending",
            "当前 Session 已有一个尚未决定的子目标 Proposal",
        ));
    }

    let proposal_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO goal_branch_proposals \
         (id, project_id, parent_goal_branch_id, parent_session_id, status, \
          current_revision, created_by) \
         VALUES ($1, $2, $3, $4, 'awaiting_approval', 1, 'agent')",
    )
    .bind(proposal_id)
    .bind(project_id)
    .bind(parent.goal_branch_id)
    .bind(parent.id)
    .execute(&mut **transaction)
    .await?;
    insert_proposal_revision(transaction, proposal_id, 1, &revision, "agent").await?;
    sqlx::query("UPDATE goal_sessions SET status = $1, updated_at = now() WHERE id = $2")
        .bind(next_status.as_str())
        .bind(parent.id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("UPDATE goal_branches SET status = 'waiting', updated_at = now() WHERE id = $1")
        .bind(parent.goal_branch_id)
        .execute(&mut **transaction)
        .await?;
    insert_attention(
        transaction,
        project_id,
        Some(parent.goal_branch_id),
        Some(parent.id),
        "branch_review",
        &format!("proposal:{proposal_id}:review"),
        "Agent 建议拆出子目标枝干",
        &revision.why_needed,
        Some("父 Session 已在最近一次安全写入后暂停"),
        None,
        Some("批准会创建独立枝干；退回不会改动父 worktree"),
        Some("判断这个目标是否值得独立并行或探索"),
        Some("审核简要契约；需要时要求 AI 修订"),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "session",
        parent.id,
        "session.child_branch_proposed",
        "agent",
        parent.agent_identity.as_deref(),
        client_request_id,
        json!({ "proposalId": proposal_id, "revision": 1 }),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "proposal",
        proposal_id,
        "proposal.submitted",
        "agent",
        parent.agent_identity.as_deref(),
        client_request_id,
        json!({ "revision": 1, "parentSessionId": parent.id }),
    )
    .await?;
    Ok(json!({
        "proposalId": proposal_id,
        "revision": 1,
        "status": ProposalStatus::AwaitingApproval.as_str(),
        "parentSessionStatus": next_status.as_str(),
    }))
}

async fn approve_proposal(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: ApproveProposalInput,
) -> AppResult<Value> {
    let branch_name = clean_text("目标枝干名称", input.branch_name, 120)?;
    let assignment = clean_text("Session 分配说明", input.assignment, 4_000)?;
    let agent_identity = clean_optional_text("Agent 身份", input.agent_identity, 200)?;
    let proposal = load_proposal_for_update(transaction, project_id, input.proposal_id).await?;
    if proposal.current_revision != input.expected_revision {
        return Err(AppError::conflict(
            "stale_revision",
            "BranchProposal 已有更新，不能批准旧修订",
        ));
    }
    let proposal_status =
        ProposalStatus::try_from(proposal.status.as_str())?.approve(GoalActor::Human)?;
    let revision =
        load_proposal_revision(transaction, proposal.id, proposal.current_revision).await?;
    let mut contract: GoalContractDraft =
        serde_json::from_value::<GoalContractDraft>(revision.contract.0.clone())
            .map_err(|_| AppError::bad_request("invalid_goal_contract", "目标契约结构不合法"))?
            .validate_for_approval()?;
    let proposal_contributions =
        value_to_string_vec("期望回流贡献", revision.expected_contributions.0.clone())?;
    contract
        .expected_contributions
        .extend(proposal_contributions);
    contract.expected_contributions.sort();
    contract.expected_contributions.dedup();

    if proposal.parent_goal_branch_id.is_none() {
        let root_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM goal_branches \
             WHERE project_id = $1 AND parent_goal_branch_id IS NULL)",
        )
        .bind(project_id)
        .fetch_one(&mut **transaction)
        .await?;
        if root_exists {
            return Err(AppError::conflict(
                "root_goal_exists",
                "该项目已经有根目标枝干",
            ));
        }
    }

    let inherited_environment = if let Some(parent_session_id) = proposal.parent_session_id {
        let parent = load_session_for_update(transaction, project_id, parent_session_id).await?;
        if parent.goal_branch_id
            != proposal
                .parent_goal_branch_id
                .expect("paired parent fields")
            || SessionStatus::try_from(parent.status.as_str())?
                != SessionStatus::WaitingBranchReview
        {
            return Err(AppError::conflict(
                "invalid_state_transition",
                "父 Session 已不再等待这个 Proposal",
            ));
        }
        let binding: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT environment_manifest_id, environment_fingerprint \
             FROM session_environment_bindings WHERE session_id = $1",
        )
        .bind(parent_session_id)
        .fetch_optional(&mut **transaction)
        .await?;
        if parent.environment_fingerprint.is_some() && binding.is_none() {
            return Err(AppError::conflict(
                "environment_fingerprint_mismatch",
                "父 Session 有环境指纹但缺少不可变绑定",
            ));
        }
        binding
    } else {
        None
    };

    let goal_branch_id = Uuid::new_v4();
    let contract_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let git_branch_name = format!("goal/{goal_branch_id}");
    sqlx::query(
        "INSERT INTO goal_branches \
         (id, project_id, creating_proposal_id, parent_goal_branch_id, \
          inherited_from_session_id, name, status, current_contract_version_id, \
          head_session_id, git_branch_name, environment_fingerprint) \
         VALUES ($1, $2, $3, $4, $5, $6, 'active', $7, $8, $9, $10)",
    )
    .bind(goal_branch_id)
    .bind(project_id)
    .bind(proposal.id)
    .bind(proposal.parent_goal_branch_id)
    .bind(proposal.parent_session_id)
    .bind(&branch_name)
    .bind(contract_id)
    .bind(session_id)
    .bind(&git_branch_name)
    .bind(inherited_environment.as_ref().map(|binding| &binding.1))
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO goal_contract_versions \
         (id, project_id, goal_branch_id, version, desired_outcome, hard_constraints, \
          subjective_preferences, unknowns, non_goals, validation_plan, judgment_triggers, \
          stop_conditions, expected_contributions, source_proposal_id, created_by) \
         VALUES ($1, $2, $3, 1, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, 'human')",
    )
    .bind(contract_id)
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(&contract.desired_outcome)
    .bind(Json(contract.hard_constraints))
    .bind(Json(contract.subjective_preferences))
    .bind(Json(contract.unknowns))
    .bind(Json(contract.non_goals))
    .bind(Json(contract.validation_plan))
    .bind(Json(contract.judgment_triggers))
    .bind(Json(contract.stop_conditions))
    .bind(Json(contract.expected_contributions))
    .bind(proposal.id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO goal_sessions \
         (id, project_id, goal_branch_id, session_number, status, assignment, \
          agent_identity, contract_version_id, environment_fingerprint, inherited_context) \
         VALUES ($1, $2, $3, 1, 'running', $4, $5, $6, $7, $8)",
    )
    .bind(session_id)
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(&assignment)
    .bind(&agent_identity)
    .bind(contract_id)
    .bind(inherited_environment.as_ref().map(|binding| &binding.1))
    .bind(revision.context_inheritance)
    .execute(&mut **transaction)
    .await?;
    if let (Some(parent_session_id), Some((environment_id, fingerprint))) =
        (proposal.parent_session_id, &inherited_environment)
    {
        sqlx::query(
            "INSERT INTO session_environment_bindings \
             (session_id, project_id, goal_branch_id, environment_manifest_id, \
              environment_fingerprint, inherited_from_session_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(session_id)
        .bind(project_id)
        .bind(goal_branch_id)
        .bind(environment_id)
        .bind(fingerprint)
        .bind(parent_session_id)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        "UPDATE goal_branch_proposals SET status = $1, approved_revision = $2, \
         approved_goal_branch_id = $3, updated_at = now(), decided_at = now() WHERE id = $4",
    )
    .bind(proposal_status.as_str())
    .bind(proposal.current_revision)
    .bind(goal_branch_id)
    .bind(proposal.id)
    .execute(&mut **transaction)
    .await?;

    resolve_attention_by_key(
        transaction,
        project_id,
        &format!("proposal:{}:review", proposal.id),
        "用户已批准 Proposal",
    )
    .await?;
    if let Some(parent_session_id) = proposal.parent_session_id {
        sqlx::query(
            "UPDATE goal_sessions SET status = 'waiting_dependency', updated_at = now() \
             WHERE id = $1",
        )
        .bind(parent_session_id)
        .execute(&mut **transaction)
        .await?;
        insert_attention(
            transaction,
            project_id,
            proposal.parent_goal_branch_id,
            Some(parent_session_id),
            "dependency",
            &format!("session:{parent_session_id}:dependency:{goal_branch_id}"),
            "父 Session 等待子目标回流",
            &format!("子目标“{branch_name}”已经开始独立推进"),
            Some("父枝干 worktree 保持在提出 Proposal 时的安全状态"),
            None,
            Some("子枝干未经用户接受不会影响父枝干"),
            Some("等待子枝干接受、停止或被取消"),
            Some("子目标产生审核结论后再决定如何恢复父 Session"),
        )
        .await?;
    } else {
        sqlx::query(
            "UPDATE projects SET state = 'active', current_focus = $1, updated_at = now() \
             WHERE id = $2 AND state IN ('shaping', 'waiting', 'paused')",
        )
        .bind(&contract.desired_outcome)
        .bind(project_id)
        .execute(&mut **transaction)
        .await?;
    }

    insert_goal_event(
        transaction,
        project_id,
        "proposal",
        proposal.id,
        "proposal.approved",
        "human",
        None,
        client_request_id,
        json!({
            "revision": proposal.current_revision,
            "goalBranchId": goal_branch_id,
        }),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "goal_branch",
        goal_branch_id,
        "goal_branch.created",
        "system",
        None,
        client_request_id,
        json!({
            "proposalId": proposal.id,
            "parentGoalBranchId": proposal.parent_goal_branch_id,
            "contractVersionId": contract_id,
            "gitBranchName": git_branch_name,
        }),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "session",
        session_id,
        "session.started",
        "system",
        agent_identity.as_deref(),
        client_request_id,
        json!({
            "goalBranchId": goal_branch_id,
            "sessionNumber": 1,
            "contractVersionId": contract_id,
        }),
    )
    .await?;
    Ok(json!({
        "proposalId": proposal.id,
        "goalBranchId": goal_branch_id,
        "contractVersionId": contract_id,
        "sessionId": session_id,
        "status": proposal_status.as_str(),
    }))
}

async fn add_contribution(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    mut input: AddContributionInput,
) -> AppResult<Value> {
    let session = load_session_for_update(transaction, project_id, input.session_id).await?;
    if !SessionStatus::try_from(session.status.as_str())?.is_writable() {
        return Err(AppError::conflict(
            "candidate_frozen",
            "只有 running Session 可以新增 Contribution",
        ));
    }
    input.kind = clean_text("Contribution 类型", input.kind, 80)?;
    if !CONTRIBUTION_KINDS.contains(&input.kind.as_str()) {
        return Err(AppError::bad_request(
            "invalid_contribution_kind",
            "未知的 Contribution 类型",
        ));
    }
    input.title = clean_text("Contribution 标题", input.title, 200)?;
    input.body = clean_text("Contribution 内容", input.body, 16_000)?;
    if !input.evidence_refs.is_array() && !input.evidence_refs.is_null() {
        return Err(AppError::bad_request(
            "invalid_evidence_refs",
            "证据引用必须是数组",
        ));
    }
    if input.evidence_refs.is_null() {
        input.evidence_refs = json!([]);
    }
    if let Some(artifact_id) = input.artifact_id {
        let belongs: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM artifacts WHERE id = $1 AND project_id = $2)",
        )
        .bind(artifact_id)
        .bind(project_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !belongs {
            return Err(AppError::bad_request(
                "cross_project_reference",
                "Artifact 不属于当前项目",
            ));
        }
    }
    if let Some(supersedes_id) = input.supersedes_id {
        let belongs: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM goal_contributions \
             WHERE id = $1 AND project_id = $2 AND goal_branch_id = $3)",
        )
        .bind(supersedes_id)
        .bind(project_id)
        .bind(session.goal_branch_id)
        .fetch_one(&mut **transaction)
        .await?;
        if !belongs {
            return Err(AppError::bad_request(
                "cross_project_reference",
                "被替代的 Contribution 不属于当前目标枝干",
            ));
        }
    }

    let contribution_id = Uuid::new_v4();
    let content_hash = canonical_json_sha256(&json!({
        "kind": input.kind,
        "title": input.title,
        "body": input.body,
        "artifactId": input.artifact_id,
        "evidenceRefs": input.evidence_refs,
        "supersedesId": input.supersedes_id,
    }))?;
    sqlx::query(
        "INSERT INTO goal_contributions \
         (id, project_id, goal_branch_id, session_id, kind, title, body, artifact_id, \
          evidence_refs, supersedes_id, content_hash) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(contribution_id)
    .bind(project_id)
    .bind(session.goal_branch_id)
    .bind(session.id)
    .bind(&input.kind)
    .bind(&input.title)
    .bind(&input.body)
    .bind(input.artifact_id)
    .bind(Json(input.evidence_refs))
    .bind(input.supersedes_id)
    .bind(&content_hash)
    .execute(&mut **transaction)
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "session",
        session.id,
        "session.contribution_added",
        "agent",
        session.agent_identity.as_deref(),
        client_request_id,
        json!({
            "goalBranchId": session.goal_branch_id,
            "contributionId": contribution_id,
            "kind": input.kind,
            "contentHash": content_hash,
        }),
    )
    .await?;
    Ok(json!({
        "contributionId": contribution_id,
        "contentHash": content_hash,
    }))
}

async fn request_judgment(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    mut input: RequestJudgmentInput,
) -> AppResult<Value> {
    input.question = clean_text("判断问题", input.question, 4_000)?;
    normalize_text_list("候选方案", &mut input.candidates, 50, 2_000)?;
    input.evidence = clean_optional_text("判断证据", input.evidence, 8_000)?;
    input.recommendation = clean_optional_text("Agent 建议", input.recommendation, 4_000)?;
    let session = load_session_for_update(transaction, project_id, input.session_id).await?;
    let status = SessionStatus::try_from(session.status.as_str())?.request_judgment()?;
    pause_session_and_branch(transaction, &session, status).await?;
    insert_attention(
        transaction,
        project_id,
        Some(session.goal_branch_id),
        Some(session.id),
        "judgment",
        &format!("session:{}:judgment", session.id),
        "Session 等待你的判断",
        &input.question,
        Some("Agent 已保存当前工作现场"),
        input.evidence.as_deref(),
        Some("在得到方向判断前继续可能造成无效探索"),
        Some("体验候选并给出方向、品味或科研判断"),
        input.recommendation.as_deref(),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "session",
        session.id,
        "session.judgment_requested",
        "agent",
        session.agent_identity.as_deref(),
        client_request_id,
        json!({
            "question": input.question,
            "candidates": input.candidates,
            "evidence": input.evidence,
            "recommendation": input.recommendation,
        }),
    )
    .await?;
    Ok(json!({ "sessionId": session.id, "status": status.as_str() }))
}

async fn pause_exception(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: PauseExceptionInput,
) -> AppResult<Value> {
    let reason = clean_text("异常原因", input.reason, 4_000)?;
    let safe_checkpoint = clean_text("安全检查点", input.safe_checkpoint, 4_000)?;
    let attempted = clean_text("已尝试办法", input.attempted, 8_000)?;
    let risk = clean_text("风险", input.risk, 4_000)?;
    let user_action = clean_text("用户所需动作", input.user_action, 4_000)?;
    let recommendation = clean_text("建议处理方式", input.recommendation, 4_000)?;
    let session = load_session_for_update(transaction, project_id, input.session_id).await?;
    let status = SessionStatus::try_from(session.status.as_str())?.pause_exception()?;
    pause_session_and_branch(transaction, &session, status).await?;
    insert_attention(
        transaction,
        project_id,
        Some(session.goal_branch_id),
        Some(session.id),
        "exception",
        &format!("session:{}:exception", session.id),
        "Session 异常暂停",
        &reason,
        Some(&safe_checkpoint),
        Some(&attempted),
        Some(&risk),
        Some(&user_action),
        Some(&recommendation),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "session",
        session.id,
        "session.exception_paused",
        "agent",
        session.agent_identity.as_deref(),
        client_request_id,
        json!({
            "reason": reason,
            "safeCheckpoint": safe_checkpoint,
            "attempted": attempted,
            "risk": risk,
            "userAction": user_action,
            "recommendation": recommendation,
        }),
    )
    .await?;
    Ok(json!({ "sessionId": session.id, "status": status.as_str() }))
}

async fn pause_manual(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: PauseManualInput,
) -> AppResult<Value> {
    let reason = clean_text("暂停原因", input.reason, 4_000)?;
    let session = load_session_for_update(transaction, project_id, input.session_id).await?;
    let status =
        SessionStatus::try_from(session.status.as_str())?.pause_manual(GoalActor::Human)?;
    pause_session_and_branch(transaction, &session, status).await?;
    insert_attention(
        transaction,
        project_id,
        Some(session.goal_branch_id),
        Some(session.id),
        "manual_pause",
        &format!("session:{}:manual", session.id),
        "Session 已手动暂停",
        &reason,
        Some("系统保存了暂停前的领域状态"),
        None,
        None,
        Some("准备好后填写恢复说明"),
        Some("先检查工作现场，再显式恢复"),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "session",
        session.id,
        "session.manual_paused",
        "human",
        None,
        client_request_id,
        json!({ "reason": reason }),
    )
    .await?;
    Ok(json!({ "sessionId": session.id, "status": status.as_str() }))
}

async fn resume_session(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: ResumeSessionInput,
) -> AppResult<Value> {
    let resolution = clean_text("恢复说明", input.resolution, 8_000)?;
    let session = load_session_for_update(transaction, project_id, input.session_id).await?;
    let current_status = SessionStatus::try_from(session.status.as_str())?;
    if current_status == SessionStatus::WaitingBranchReview {
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM goal_branch_proposals \
             WHERE parent_session_id = $1 AND status IN ('draft', 'awaiting_approval'))",
        )
        .bind(session.id)
        .fetch_one(&mut **transaction)
        .await?;
        if pending {
            return Err(AppError::conflict(
                "unresolved_attention",
                "子目标 Proposal 尚未决定，不能恢复父 Session",
            ));
        }
    }
    if current_status == SessionStatus::WaitingDependency {
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM goal_branches \
             WHERE inherited_from_session_id = $1 \
             AND status IN ('active', 'waiting', 'review_pending'))",
        )
        .bind(session.id)
        .fetch_one(&mut **transaction)
        .await?;
        if pending {
            return Err(AppError::conflict(
                "unresolved_attention",
                "仍有子目标枝干在推进或审核，不能恢复父 Session",
            ));
        }
    }
    let status = current_status.resume(false)?;
    sqlx::query("UPDATE goal_sessions SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status.as_str())
        .bind(session.id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("UPDATE goal_branches SET status = 'active', updated_at = now() WHERE id = $1")
        .bind(session.goal_branch_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "UPDATE goal_attention_items SET status = 'resolved', resolution = $1, resolved_at = now() \
         WHERE project_id = $2 AND session_id = $3 AND status = 'open'",
    )
    .bind(&resolution)
    .bind(project_id)
    .bind(session.id)
    .execute(&mut **transaction)
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "session",
        session.id,
        "session.resumed",
        "human",
        None,
        client_request_id,
        json!({ "resolution": resolution }),
    )
    .await?;
    Ok(json!({ "sessionId": session.id, "status": status.as_str() }))
}

async fn start_next_session(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: StartNextSessionInput,
) -> AppResult<Value> {
    let assignment = clean_text("下一 Session 分配说明", input.assignment, 4_000)?;
    let agent_identity = clean_optional_text("Agent 身份", input.agent_identity, 200)?;
    let branch = load_branch_for_update(transaction, project_id, input.goal_branch_id).await?;
    if branch.head_session_id != input.previous_session_id {
        return Err(AppError::conflict(
            "stale_session_head",
            "目标枝干已经有更新的 Session 节点",
        ));
    }
    let previous =
        load_session_for_update(transaction, project_id, input.previous_session_id).await?;
    if previous.goal_branch_id != branch.id {
        return Err(AppError::bad_request(
            "cross_project_reference",
            "上一 Session 不属于该目标枝干",
        ));
    }
    let running_writer_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_sessions \
         WHERE goal_branch_id = $1 AND status = 'running')",
    )
    .bind(branch.id)
    .fetch_one(&mut **transaction)
    .await?;
    let pending_review_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_review_gates \
         WHERE goal_branch_id = $1 AND status IN ('pending_ai_review', 'pending_human_review'))",
    )
    .bind(branch.id)
    .fetch_one(&mut **transaction)
    .await?;
    validate_new_running_session(
        GoalBranchStatus::try_from(branch.status.as_str())?,
        Some(SessionStatus::try_from(previous.status.as_str())?),
        running_writer_exists,
        pending_review_exists,
    )?;

    let session_id = Uuid::new_v4();
    let next_number = previous.session_number + 1;
    sqlx::query(
        "INSERT INTO goal_sessions \
         (id, project_id, goal_branch_id, session_number, status, assignment, \
          agent_identity, contract_version_id, environment_fingerprint, inherited_context) \
         VALUES ($1, $2, $3, $4, 'running', $5, $6, $7, $8, $9)",
    )
    .bind(session_id)
    .bind(project_id)
    .bind(branch.id)
    .bind(next_number)
    .bind(&assignment)
    .bind(&agent_identity)
    .bind(branch.current_contract_version_id)
    .bind(&branch.environment_fingerprint)
    .bind(Json(json!({
        "previousSessionId": previous.id,
        "previousAssignment": previous.assignment,
    })))
    .execute(&mut **transaction)
    .await?;
    if let Some(fingerprint) = &branch.environment_fingerprint {
        let environment_id: Option<Uuid> = sqlx::query_scalar(
            "SELECT environment_manifest_id FROM session_environment_bindings \
             WHERE session_id = $1 AND environment_fingerprint = $2",
        )
        .bind(previous.id)
        .bind(fingerprint)
        .fetch_optional(&mut **transaction)
        .await?;
        let environment_id = environment_id.ok_or_else(|| {
            AppError::conflict(
                "environment_fingerprint_mismatch",
                "上一 Session 环境没有对应的不可变绑定",
            )
        })?;
        sqlx::query(
            "INSERT INTO session_environment_bindings \
             (session_id, project_id, goal_branch_id, environment_manifest_id, \
              environment_fingerprint, inherited_from_session_id) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(session_id)
        .bind(project_id)
        .bind(branch.id)
        .bind(environment_id)
        .bind(fingerprint)
        .bind(previous.id)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        "UPDATE goal_branches SET head_session_id = $1, status = 'active', updated_at = now() \
         WHERE id = $2",
    )
    .bind(session_id)
    .bind(branch.id)
    .execute(&mut **transaction)
    .await?;
    resolve_attention_by_key(
        transaction,
        project_id,
        &format!("branch:{}:continuation", branch.id),
        "用户已分配下一轮 Session",
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "session",
        session_id,
        "session.started",
        "human",
        None,
        client_request_id,
        json!({
            "goalBranchId": branch.id,
            "sessionNumber": next_number,
            "previousSessionId": previous.id,
            "contractVersionId": branch.current_contract_version_id,
        }),
    )
    .await?;
    Ok(json!({
        "sessionId": session_id,
        "sessionNumber": next_number,
        "status": SessionStatus::Running.as_str(),
    }))
}

async fn pause_session_and_branch(
    transaction: &mut GoalTransaction<'_>,
    session: &SessionStateRow,
    status: SessionStatus,
) -> AppResult<()> {
    sqlx::query("UPDATE goal_sessions SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status.as_str())
        .bind(session.id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("UPDATE goal_branches SET status = 'waiting', updated_at = now() WHERE id = $1")
        .bind(session.goal_branch_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn propose_merge(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    input: ProposeMergeInput,
) -> AppResult<Value> {
    let candidate = input.candidate.validate()?;
    let session = load_session_for_update(transaction, project_id, input.session_id).await?;
    let next_session_status = SessionStatus::try_from(session.status.as_str())?.propose_merge()?;
    if candidate.contract_version_id != session.contract_version_id {
        return Err(AppError::conflict(
            "stale_revision",
            "拟合并必须绑定 Session 正在执行的目标契约版本",
        ));
    }
    if candidate.environment_fingerprint != session.environment_fingerprint {
        return Err(AppError::conflict(
            "environment_fingerprint_mismatch",
            "拟合并候选必须绑定 Session 的准确环境指纹",
        ));
    }

    let matching_contributions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM goal_contributions \
         WHERE project_id = $1 AND goal_branch_id = $2 AND id = ANY($3)",
    )
    .bind(project_id)
    .bind(session.goal_branch_id)
    .bind(&candidate.contribution_ids)
    .fetch_one(&mut **transaction)
    .await?;
    if matching_contributions != candidate.contribution_ids.len() as i64 {
        return Err(AppError::bad_request(
            "cross_project_reference",
            "候选 Contribution 必须全部属于当前目标枝干",
        ));
    }
    let current_session_has_contribution: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_contributions \
         WHERE session_id = $1 AND id = ANY($2))",
    )
    .bind(session.id)
    .bind(&candidate.contribution_ids)
    .fetch_one(&mut **transaction)
    .await?;
    if !current_session_has_contribution {
        return Err(AppError::bad_request(
            "missing_current_session_contribution",
            "候选至少要包含当前 Session 新形成的一项 Contribution",
        ));
    }
    let unresolved_child: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM goal_branches \
         WHERE parent_goal_branch_id = $1 \
         AND status IN ('active', 'waiting', 'review_pending'))",
    )
    .bind(session.goal_branch_id)
    .fetch_one(&mut **transaction)
    .await?;
    if unresolved_child {
        return Err(AppError::conflict(
            "unresolved_child_goal",
            "仍有必需子目标在推进或审核，整条枝干还不能拟合并",
        ));
    }

    let review_gate_id = Uuid::new_v4();
    let candidate_hash = candidate.fingerprint()?;
    let candidate_json = serde_json::to_value(&candidate)?;
    sqlx::query(
        "INSERT INTO goal_review_gates \
         (id, project_id, goal_branch_id, session_id, contract_version_id, status, \
          candidate_snapshot, candidate_hash, git_base_commit, git_head_commit, git_dirty, \
          environment_fingerprint, test_evidence, risks, self_check) \
         VALUES ($1, $2, $3, $4, $5, 'pending_ai_review', $6, $7, $8, $9, $10, $11, $12, $13, $14)",
    )
    .bind(review_gate_id)
    .bind(project_id)
    .bind(session.goal_branch_id)
    .bind(session.id)
    .bind(session.contract_version_id)
    .bind(Json(candidate_json))
    .bind(&candidate_hash)
    .bind(&candidate.git_base_commit)
    .bind(&candidate.git_head_commit)
    .bind(candidate.git_dirty)
    .bind(&candidate.environment_fingerprint)
    .bind(Json(candidate.test_evidence.clone()))
    .bind(Json(candidate.risks.clone()))
    .bind(Json(json!({ "summary": candidate.self_check })))
    .execute(&mut **transaction)
    .await?;
    for contribution_id in &candidate.contribution_ids {
        sqlx::query(
            "INSERT INTO goal_review_gate_contributions \
             (review_gate_id, contribution_id) VALUES ($1, $2)",
        )
        .bind(review_gate_id)
        .bind(contribution_id)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query("UPDATE goal_sessions SET status = $1, updated_at = now() WHERE id = $2")
        .bind(next_session_status.as_str())
        .bind(session.id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "UPDATE goal_branches SET status = 'review_pending', updated_at = now() WHERE id = $1",
    )
    .bind(session.goal_branch_id)
    .execute(&mut **transaction)
    .await?;
    insert_attention(
        transaction,
        project_id,
        Some(session.goal_branch_id),
        Some(session.id),
        "merge_review",
        &format!("gate:{review_gate_id}:review"),
        "目标枝干等待拟合并审核",
        "工作 Agent 声称整条目标枝干已经达成；候选现场已冻结",
        Some("Git、环境、测试证据和 Contribution 集已绑定候选哈希"),
        Some("下一步由独立审核 AI 检查契约、遗漏和反例"),
        Some("未经用户接受，候选不会影响父枝干"),
        Some("先完成独立 AI 审核，再由用户最终决定"),
        Some("检查目标契约、差异、测试证据、风险与未解决项"),
    )
    .await?;
    insert_goal_event(
        transaction,
        project_id,
        "review_gate",
        review_gate_id,
        "merge.proposed",
        "agent",
        session.agent_identity.as_deref(),
        client_request_id,
        json!({
            "goalBranchId": session.goal_branch_id,
            "sessionId": session.id,
            "candidateHash": candidate_hash,
            "contributionIds": candidate.contribution_ids,
        }),
    )
    .await?;
    Ok(json!({
        "reviewGateId": review_gate_id,
        "candidateHash": candidate_hash,
        "status": ReviewGateStatus::PendingAiReview.as_str(),
    }))
}

async fn record_ai_review(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    mut input: AiReviewInput,
) -> AppResult<Value> {
    input.reviewer_identity = clean_text("审核 AI 身份", input.reviewer_identity, 200)?;
    input.rationale = clean_text("审核理由", input.rationale, 12_000)?;
    normalize_text_list("复验证据", &mut input.retest_evidence, 100, 4_000)?;
    if !input.contract_check.is_object() {
        return Err(AppError::bad_request(
            "invalid_contract_check",
            "契约检查必须是 JSON 对象",
        ));
    }
    let gate = load_gate_for_update(transaction, project_id, input.review_gate_id).await?;
    ensure_gate_hash(&gate)?;
    let session = load_session_for_update(transaction, project_id, gate.session_id).await?;
    if session.agent_identity.as_deref() == Some(input.reviewer_identity.as_str()) {
        return Err(AppError::conflict(
            "independent_reviewer_required",
            "审核 AI 不能与工作 Agent 使用同一身份",
        ));
    }
    let status = ReviewGateStatus::try_from(gate.status.as_str())?
        .record_ai_review(GoalActor::ReviewAi, input.decision)?;
    let decision_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO goal_review_decisions \
         (id, project_id, review_gate_id, actor_role, actor_identity, decision, rationale, \
          contract_check, retest_evidence, selected_contribution_ids) \
         VALUES ($1, $2, $3, 'review_ai', $4, $5, $6, $7, $8, '[]'::jsonb)",
    )
    .bind(decision_id)
    .bind(project_id)
    .bind(gate.id)
    .bind(&input.reviewer_identity)
    .bind(input.decision.as_str())
    .bind(&input.rationale)
    .bind(Json(input.contract_check.clone()))
    .bind(Json(input.retest_evidence.clone()))
    .execute(&mut **transaction)
    .await?;
    sqlx::query("UPDATE goal_review_gates SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status.as_str())
        .bind(gate.id)
        .execute(&mut **transaction)
        .await?;
    insert_goal_event(
        transaction,
        project_id,
        "review_gate",
        gate.id,
        "review.ai_completed",
        "review_ai",
        Some(&input.reviewer_identity),
        client_request_id,
        json!({
            "decisionId": decision_id,
            "decision": input.decision.as_str(),
            "candidateHash": gate.candidate_hash,
        }),
    )
    .await?;
    Ok(json!({
        "reviewGateId": gate.id,
        "decisionId": decision_id,
        "status": status.as_str(),
    }))
}

async fn record_human_review(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    client_request_id: Uuid,
    mut input: HumanReviewInput,
) -> AppResult<Value> {
    input.rationale = clean_text("用户审核理由", input.rationale, 12_000)?;
    input.selected_contribution_ids.sort_unstable();
    let selection_len = input.selected_contribution_ids.len();
    input.selected_contribution_ids.dedup();
    if selection_len != input.selected_contribution_ids.len() {
        return Err(AppError::bad_request(
            "duplicate_contribution",
            "不能重复选择同一 Contribution",
        ));
    }
    let gate = load_gate_for_update(transaction, project_id, input.review_gate_id).await?;
    ensure_gate_hash(&gate)?;
    let status = ReviewGateStatus::try_from(gate.status.as_str())?
        .record_human_decision(GoalActor::Human, input.decision)?;
    let session = load_session_for_update(transaction, project_id, gate.session_id).await?;
    let branch = load_branch_for_update(transaction, project_id, gate.goal_branch_id).await?;
    if SessionStatus::try_from(session.status.as_str())? != SessionStatus::AwaitingMergeReview
        || GoalBranchStatus::try_from(branch.status.as_str())? != GoalBranchStatus::ReviewPending
    {
        return Err(AppError::conflict(
            "invalid_state_transition",
            "Session 或目标枝干已不再等待该审核",
        ));
    }
    let mut candidate_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT contribution_id FROM goal_review_gate_contributions \
         WHERE review_gate_id = $1 ORDER BY contribution_id",
    )
    .bind(gate.id)
    .fetch_all(&mut **transaction)
    .await?;
    candidate_ids.sort_unstable();

    match input.decision {
        ReviewDecisionKind::Accept => {
            if input.selected_contribution_ids != candidate_ids {
                return Err(AppError::bad_request(
                    "invalid_contribution_selection",
                    "完整接受必须选择冻结候选中的全部 Contribution",
                ));
            }
        }
        ReviewDecisionKind::PartialAccept => {
            if input.selected_contribution_ids.is_empty()
                || input.selected_contribution_ids.len() >= candidate_ids.len()
                || input
                    .selected_contribution_ids
                    .iter()
                    .any(|id| !candidate_ids.contains(id))
            {
                return Err(AppError::bad_request(
                    "invalid_contribution_selection",
                    "部分接受必须选择候选中的非空真子集",
                ));
            }
        }
        ReviewDecisionKind::Reject | ReviewDecisionKind::Abandon => {
            if !input.selected_contribution_ids.is_empty() {
                return Err(AppError::bad_request(
                    "invalid_contribution_selection",
                    "退回或放弃不能同时回流 Contribution",
                ));
            }
        }
        _ => unreachable!("domain validation rejected non-human decisions"),
    }

    let decision_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO goal_review_decisions \
         (id, project_id, review_gate_id, actor_role, decision, rationale, \
          selected_contribution_ids) \
         VALUES ($1, $2, $3, 'human', $4, $5, $6)",
    )
    .bind(decision_id)
    .bind(project_id)
    .bind(gate.id)
    .bind(input.decision.as_str())
    .bind(&input.rationale)
    .bind(Json(input.selected_contribution_ids.clone()))
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE goal_review_gates SET status = $1, updated_at = now(), resolved_at = now() \
         WHERE id = $2",
    )
    .bind(status.as_str())
    .bind(gate.id)
    .execute(&mut **transaction)
    .await?;
    resolve_attention_by_key(
        transaction,
        project_id,
        &format!("gate:{}:review", gate.id),
        &input.rationale,
    )
    .await?;

    let integration_id = match input.decision {
        ReviewDecisionKind::Accept | ReviewDecisionKind::PartialAccept => {
            let integration_id = Uuid::new_v4();
            let integration_kind = if input.decision == ReviewDecisionKind::Accept {
                "full"
            } else {
                "partial"
            };
            sqlx::query(
                "INSERT INTO goal_integrations \
                 (id, project_id, source_goal_branch_id, target_goal_branch_id, review_gate_id, \
                  kind, summary, git_integration_status) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, 'not_attempted')",
            )
            .bind(integration_id)
            .bind(project_id)
            .bind(branch.id)
            .bind(branch.parent_goal_branch_id)
            .bind(gate.id)
            .bind(integration_kind)
            .bind(&input.rationale)
            .execute(&mut **transaction)
            .await?;
            for contribution_id in &input.selected_contribution_ids {
                sqlx::query(
                    "INSERT INTO goal_integration_contributions \
                     (integration_id, contribution_id) VALUES ($1, $2)",
                )
                .bind(integration_id)
                .bind(contribution_id)
                .execute(&mut **transaction)
                .await?;
            }
            Some(integration_id)
        }
        _ => None,
    };

    match input.decision {
        ReviewDecisionKind::Accept => {
            let session_status =
                SessionStatus::try_from(session.status.as_str())?.mark_accepted()?;
            sqlx::query(
                "UPDATE goal_sessions SET status = $1, updated_at = now(), ended_at = now() \
                 WHERE id = $2",
            )
            .bind(session_status.as_str())
            .bind(session.id)
            .execute(&mut **transaction)
            .await?;
            if branch.parent_goal_branch_id.is_some() {
                sqlx::query(
                    "UPDATE goal_branches SET status = 'integrated', updated_at = now(), \
                     completed_at = now() WHERE id = $1",
                )
                .bind(branch.id)
                .execute(&mut **transaction)
                .await?;
                mark_child_result_ready(transaction, project_id, &branch, &input.rationale).await?;
            } else {
                sqlx::query(
                    "UPDATE goal_branches SET status = 'completed', updated_at = now(), \
                     completed_at = now() WHERE id = $1",
                )
                .bind(branch.id)
                .execute(&mut **transaction)
                .await?;
                sqlx::query(
                    "UPDATE projects SET state = 'completed', completion_reason = $1, \
                     current_focus = NULL, updated_at = now() WHERE id = $2",
                )
                .bind(&input.rationale)
                .bind(project_id)
                .execute(&mut **transaction)
                .await?;
            }
        }
        ReviewDecisionKind::PartialAccept => {
            sqlx::query(
                "UPDATE goal_sessions SET status = 'accepted', updated_at = now(), ended_at = now() \
                 WHERE id = $1",
            )
            .bind(session.id)
            .execute(&mut **transaction)
            .await?;
            sqlx::query(
                "UPDATE goal_branches SET status = 'stopped', updated_at = now(), stopped_at = now() \
                 WHERE id = $1",
            )
            .bind(branch.id)
            .execute(&mut **transaction)
            .await?;
            if branch.parent_goal_branch_id.is_some() {
                mark_child_result_ready(transaction, project_id, &branch, &input.rationale).await?;
            } else {
                sqlx::query(
                    "UPDATE projects SET state = 'stopped', completion_reason = $1, \
                     current_focus = NULL, updated_at = now() WHERE id = $2",
                )
                .bind(&input.rationale)
                .bind(project_id)
                .execute(&mut **transaction)
                .await?;
            }
        }
        ReviewDecisionKind::Reject => {
            let session_status =
                SessionStatus::try_from(session.status.as_str())?.mark_review_rejected()?;
            sqlx::query(
                "UPDATE goal_sessions SET status = $1, updated_at = now(), ended_at = now() \
                 WHERE id = $2",
            )
            .bind(session_status.as_str())
            .bind(session.id)
            .execute(&mut **transaction)
            .await?;
            sqlx::query(
                "UPDATE goal_branches SET status = 'active', updated_at = now() WHERE id = $1",
            )
            .bind(branch.id)
            .execute(&mut **transaction)
            .await?;
            insert_attention(
                transaction,
                project_id,
                Some(branch.id),
                Some(session.id),
                "continuation_required",
                &format!("branch:{}:continuation", branch.id),
                "拟合并被退回，需要下一 Session",
                &input.rationale,
                Some("被退回的候选现场继续保持冻结，不在原 Session 上修改"),
                None,
                Some("直接复用旧 Session 会破坏审核证据"),
                Some("讨论下一轮目标与验收后创建新 Session"),
                Some("保留同一目标枝干，在新 Session 中修正"),
            )
            .await?;
        }
        ReviewDecisionKind::Abandon => {
            sqlx::query(
                "UPDATE goal_sessions SET status = 'stopped', updated_at = now(), ended_at = now() \
                 WHERE id = $1",
            )
            .bind(session.id)
            .execute(&mut **transaction)
            .await?;
            sqlx::query(
                "UPDATE goal_branches SET status = 'stopped', updated_at = now(), stopped_at = now() \
                 WHERE id = $1",
            )
            .bind(branch.id)
            .execute(&mut **transaction)
            .await?;
            if branch.parent_goal_branch_id.is_none() {
                sqlx::query(
                    "UPDATE projects SET state = 'stopped', completion_reason = $1, \
                     current_focus = NULL, updated_at = now() WHERE id = $2",
                )
                .bind(&input.rationale)
                .bind(project_id)
                .execute(&mut **transaction)
                .await?;
            }
        }
        _ => unreachable!("domain validation rejected non-human decisions"),
    }

    insert_goal_event(
        transaction,
        project_id,
        "review_gate",
        gate.id,
        match input.decision {
            ReviewDecisionKind::Accept => "review.accepted",
            ReviewDecisionKind::PartialAccept => "review.partially_accepted",
            ReviewDecisionKind::Reject => "review.rejected",
            ReviewDecisionKind::Abandon => "review.abandoned",
            _ => unreachable!("domain validation rejected non-human decisions"),
        },
        "human",
        None,
        client_request_id,
        json!({
            "decisionId": decision_id,
            "decision": input.decision.as_str(),
            "candidateHash": gate.candidate_hash,
            "selectedContributionIds": input.selected_contribution_ids,
            "integrationId": integration_id,
            "gitIntegrationStatus": integration_id.map(|_| "not_attempted"),
        }),
    )
    .await?;
    Ok(json!({
        "reviewGateId": gate.id,
        "decisionId": decision_id,
        "integrationId": integration_id,
        "status": status.as_str(),
        "goalBranchStatus": match input.decision {
            ReviewDecisionKind::Accept if branch.parent_goal_branch_id.is_some() => "integrated",
            ReviewDecisionKind::Accept => "completed",
            ReviewDecisionKind::PartialAccept | ReviewDecisionKind::Abandon => "stopped",
            ReviewDecisionKind::Reject => "active",
            _ => unreachable!("domain validation rejected non-human decisions"),
        },
    }))
}

async fn mark_child_result_ready(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    branch: &BranchStateRow,
    resolution: &str,
) -> AppResult<()> {
    let Some(parent_session_id) = branch.inherited_from_session_id else {
        return Ok(());
    };
    resolve_attention_by_key(
        transaction,
        project_id,
        &format!("session:{parent_session_id}:dependency:{}", branch.id),
        resolution,
    )
    .await?;
    insert_attention(
        transaction,
        project_id,
        branch.parent_goal_branch_id,
        Some(parent_session_id),
        "child_result_ready",
        &format!("session:{parent_session_id}:child-result:{}", branch.id),
        "子目标已有用户审核结论",
        resolution,
        Some("选中的 Contribution 已进入父枝干可见上下文；Git 集成仍明确标为未执行"),
        None,
        Some("子目标通过不等于父目标自动通过"),
        Some("检查回流内容并显式恢复父 Session"),
        Some("恢复后执行父目标的整合验证"),
    )
    .await?;
    Ok(())
}

async fn load_proposal_for_update(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    proposal_id: Uuid,
) -> AppResult<ProposalStateRow> {
    sqlx::query_as::<_, ProposalStateRow>(
        "SELECT id, parent_goal_branch_id, parent_session_id, status, current_revision \
         FROM goal_branch_proposals WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(proposal_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("BranchProposal 不存在"))
}

async fn load_proposal_revision(
    transaction: &mut GoalTransaction<'_>,
    proposal_id: Uuid,
    revision: i32,
) -> AppResult<ProposalRevisionRow> {
    sqlx::query_as::<_, ProposalRevisionRow>(
        "SELECT why_needed, contract, expected_contributions, context_inheritance \
         FROM goal_branch_proposal_revisions \
         WHERE proposal_id = $1 AND revision = $2",
    )
    .bind(proposal_id)
    .bind(revision)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("BranchProposal 修订不存在"))
}

async fn load_session_for_update(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    session_id: Uuid,
) -> AppResult<SessionStateRow> {
    sqlx::query_as::<_, SessionStateRow>(
        "SELECT id, goal_branch_id, session_number, status, assignment, agent_identity, \
         contract_version_id, environment_fingerprint \
         FROM goal_sessions WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("Agent Session 不存在"))
}

async fn load_branch_for_update(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    branch_id: Uuid,
) -> AppResult<BranchStateRow> {
    sqlx::query_as::<_, BranchStateRow>(
        "SELECT id, parent_goal_branch_id, inherited_from_session_id, status, \
         current_contract_version_id, head_session_id, environment_fingerprint \
         FROM goal_branches WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(branch_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("目标枝干不存在"))
}

async fn load_gate_for_update(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    review_gate_id: Uuid,
) -> AppResult<GateStateRow> {
    sqlx::query_as::<_, GateStateRow>(
        "SELECT id, goal_branch_id, session_id, status, candidate_hash, candidate_snapshot \
         FROM goal_review_gates WHERE id = $1 AND project_id = $2 FOR UPDATE",
    )
    .bind(review_gate_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("ReviewGate 不存在"))
}

async fn insert_proposal_revision(
    transaction: &mut GoalTransaction<'_>,
    proposal_id: Uuid,
    revision_number: i32,
    revision: &BranchProposalRevisionDraft,
    created_by: &str,
) -> AppResult<Uuid> {
    let revision_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO goal_branch_proposal_revisions \
         (id, proposal_id, revision, why_needed, contract, expected_contributions, \
          exploration_plan, context_inheritance, tool_requirements, inferences, \
          revision_reason, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(revision_id)
    .bind(proposal_id)
    .bind(revision_number)
    .bind(&revision.why_needed)
    .bind(Json(serde_json::to_value(&revision.contract)?))
    .bind(Json(serde_json::to_value(
        &revision.expected_contributions,
    )?))
    .bind(Json(serde_json::to_value(&revision.exploration_plan)?))
    .bind(Json(revision.context_inheritance.clone()))
    .bind(Json(serde_json::to_value(&revision.tool_requirements)?))
    .bind(Json(serde_json::to_value(&revision.inferences)?))
    .bind(&revision.revision_reason)
    .bind(created_by)
    .execute(&mut **transaction)
    .await?;
    Ok(revision_id)
}

#[allow(clippy::too_many_arguments)]
async fn insert_goal_event(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    aggregate_type: &str,
    aggregate_id: Uuid,
    event_type: &str,
    actor_type: &str,
    actor_identity: Option<&str>,
    client_request_id: Uuid,
    payload: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO goal_events \
         (id, project_id, aggregate_type, aggregate_id, event_type, actor_type, \
          actor_identity, client_request_id, payload) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(aggregate_type)
    .bind(aggregate_id)
    .bind(event_type)
    .bind(actor_type)
    .bind(actor_identity)
    .bind(client_request_id)
    .bind(Json(payload))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_attention(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    goal_branch_id: Option<Uuid>,
    session_id: Option<Uuid>,
    kind: &str,
    dedupe_key: &str,
    title: &str,
    reason: &str,
    safe_checkpoint: Option<&str>,
    attempted: Option<&str>,
    risk: Option<&str>,
    user_action: Option<&str>,
    recommendation: Option<&str>,
) -> AppResult<Uuid> {
    let attention_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO goal_attention_items \
         (id, project_id, goal_branch_id, session_id, kind, status, dedupe_key, title, \
          reason, safe_checkpoint, attempted, risk, user_action, recommendation) \
         VALUES ($1, $2, $3, $4, $5, 'open', $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(attention_id)
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(session_id)
    .bind(kind)
    .bind(dedupe_key)
    .bind(title)
    .bind(reason)
    .bind(safe_checkpoint)
    .bind(attempted)
    .bind(risk)
    .bind(user_action)
    .bind(recommendation)
    .execute(&mut **transaction)
    .await?;
    Ok(attention_id)
}

async fn resolve_attention_by_key(
    transaction: &mut GoalTransaction<'_>,
    project_id: Uuid,
    dedupe_key: &str,
    resolution: &str,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE goal_attention_items SET status = 'resolved', resolution = $1, \
         resolved_at = now() WHERE project_id = $2 AND dedupe_key = $3 AND status = 'open'",
    )
    .bind(resolution)
    .bind(project_id)
    .bind(dedupe_key)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn ensure_gate_hash(gate: &GateStateRow) -> AppResult<()> {
    let observed = canonical_json_sha256(&gate.candidate_snapshot.0)?;
    if observed == gate.candidate_hash {
        return Ok(());
    }
    Err(AppError::conflict(
        "candidate_frozen",
        "拟合并候选内容与冻结哈希不一致",
    ))
}

fn clean_text(label: &str, value: String, max: usize) -> AppResult<String> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::bad_request(
            "invalid_input",
            format!("{label}不能为空"),
        ));
    }
    if value.chars().count() > max {
        return Err(AppError::bad_request(
            "invalid_input",
            format!("{label}过长"),
        ));
    }
    Ok(value)
}

fn clean_optional_text(
    label: &str,
    value: Option<String>,
    max: usize,
) -> AppResult<Option<String>> {
    value
        .map(|value| {
            let value = value.trim().to_owned();
            if value.is_empty() {
                Ok(None)
            } else if value.chars().count() > max {
                Err(AppError::bad_request(
                    "invalid_input",
                    format!("{label}过长"),
                ))
            } else {
                Ok(Some(value))
            }
        })
        .unwrap_or(Ok(None))
}

fn normalize_text_list(
    label: &str,
    values: &mut Vec<String>,
    max_items: usize,
    max_chars: usize,
) -> AppResult<()> {
    if values.len() > max_items {
        return Err(AppError::bad_request(
            "too_many_items",
            format!("{label}条目过多"),
        ));
    }
    for value in values.iter_mut() {
        *value = clean_text(label, std::mem::take(value), max_chars)?;
    }
    values.sort();
    values.dedup();
    Ok(())
}

fn value_to_string_vec(label: &str, value: Value) -> AppResult<Vec<String>> {
    let mut values: Vec<String> = serde_json::from_value(value).map_err(|_| {
        AppError::bad_request("invalid_goal_input", format!("{label}必须是字符串数组"))
    })?;
    normalize_text_list(label, &mut values, 100, 2_000)?;
    Ok(values)
}
