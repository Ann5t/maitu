use std::path::PathBuf;

use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction, types::Json};
use uuid::Uuid;

use crate::{
    artifacts::ArtifactStore,
    domain::{
        OutcomeConfirmation, OutcomeContractDraft, ProjectActionRequest, ProjectIntake,
        create_project_brief_markdown, draft_project_from_intent, normalize_intent,
        validate_outcome_confirmation,
    },
    error::{AppError, AppResult},
    models::{
        ActionRun, Artifact, BranchMerge, DecisionCheckpoint, Evidence, OutcomeContract, Project,
        ProjectBranch, ProjectContribution, ProjectEvent, ProjectNode, ProjectNodeEdge,
        ProjectSnapshot, ProjectSummary, QualityGate,
    },
};

pub async fn list_projects(pool: &PgPool) -> AppResult<Vec<ProjectSummary>> {
    Ok(sqlx::query_as::<_, ProjectSummary>(
        "SELECT p.id, p.title, p.intent, p.state, p.current_focus, p.updated_at, \
         c.status AS contract_status, \
         (SELECT count(*) FROM action_runs a WHERE a.project_id = p.id \
          AND a.status IN ('proposed', 'ready', 'running', 'blocked')) AS attention_count, \
         (SELECT count(*) FROM artifacts f WHERE f.project_id = p.id) AS artifact_count \
         FROM projects p \
         LEFT JOIN outcome_contracts c ON c.project_id = p.id \
         ORDER BY p.updated_at DESC",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn get_snapshot(pool: &PgPool, project_id: Uuid) -> AppResult<ProjectSnapshot> {
    let project = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = $1")
        .bind(project_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    let contract = sqlx::query_as::<_, OutcomeContract>(
        "SELECT * FROM outcome_contracts WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;
    let actions = sqlx::query_as::<_, ActionRun>(
        "SELECT * FROM action_runs WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let artifacts = sqlx::query_as::<_, Artifact>(
        "SELECT * FROM artifacts WHERE project_id = $1 ORDER BY created_at DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let quality_gates = sqlx::query_as::<_, QualityGate>(
        "SELECT * FROM quality_gates WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let evidence = sqlx::query_as::<_, Evidence>(
        "SELECT * FROM evidence WHERE project_id = $1 ORDER BY observed_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let decisions = sqlx::query_as::<_, DecisionCheckpoint>(
        "SELECT * FROM decision_checkpoints WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let branches = sqlx::query_as::<_, ProjectBranch>(
        "SELECT * FROM project_branches WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let nodes = sqlx::query_as::<_, ProjectNode>(
        "SELECT * FROM project_nodes WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let edges = sqlx::query_as::<_, ProjectNodeEdge>(
        "SELECT * FROM project_node_edges WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let contributions = sqlx::query_as::<_, ProjectContribution>(
        "SELECT * FROM project_contributions WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let merges = sqlx::query_as::<_, BranchMerge>(
        "SELECT * FROM branch_merges WHERE project_id = $1 ORDER BY created_at",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    let events = sqlx::query_as::<_, ProjectEvent>(
        "SELECT * FROM project_events WHERE project_id = $1 ORDER BY created_at DESC LIMIT 50",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    Ok(ProjectSnapshot {
        project,
        contract,
        actions,
        artifacts,
        quality_gates,
        evidence,
        decisions,
        branches,
        nodes,
        edges,
        contributions,
        merges,
        events,
    })
}

pub async fn create_project(pool: &PgPool, input: ProjectIntake) -> AppResult<Uuid> {
    let draft = draft_project_from_intent(&input.intent)?;
    let project_id = Uuid::new_v4();
    let contract_id = Uuid::new_v4();
    let action_id = Uuid::new_v4();
    let gate_id = Uuid::new_v4();
    let main_branch_id = Uuid::new_v4();
    let origin_node_id = Uuid::new_v4();
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO projects (id, title, intent, state, current_focus) \
         VALUES ($1, $2, $3, 'shaping', $4)",
    )
    .bind(project_id)
    .bind(&draft.title)
    .bind(&draft.intent)
    .bind("确认项目何时才算真正结束")
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "INSERT INTO project_branches \
         (id, project_id, name, purpose, status, is_main, color) \
         VALUES ($1, $2, '主线', $3, 'active', 1, '#202925')",
    )
    .bind(main_branch_id)
    .bind(project_id)
    .bind("保存当前已经接受、可以继续依赖的项目状态")
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "INSERT INTO project_nodes \
         (id, project_id, branch_id, kind, title, summary, outcome, actor_type, resolved_at) \
         VALUES ($1, $2, $3, 'origin', '项目起点', $4, 'useful', 'human', now())",
    )
    .bind(origin_node_id)
    .bind(project_id)
    .bind(main_branch_id)
    .bind(&draft.intent)
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "UPDATE project_branches SET forked_from_node_id = $1, head_node_id = $1 WHERE id = $2",
    )
    .bind(origin_node_id)
    .bind(main_branch_id)
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "INSERT INTO outcome_contracts \
         (id, project_id, desired_outcome, success_evidence, constraints, non_goals, \
          confirmation_question, contradictions, status) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'draft')",
    )
    .bind(contract_id)
    .bind(project_id)
    .bind(&draft.outcome_contract.desired_outcome)
    .bind(Json(&draft.outcome_contract.success_evidence))
    .bind(Json(&draft.outcome_contract.constraints))
    .bind(Json(&draft.outcome_contract.non_goals))
    .bind(&draft.confirmation_question)
    .bind(Json(&draft.contradictions))
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "INSERT INTO action_runs \
         (id, project_id, node_id, kind, title, owner, status, expected_signal, requires_approval) \
         VALUES ($1, $2, $3, 'confirm_outcome', $4, 'human', 'ready', $5, 1)",
    )
    .bind(action_id)
    .bind(project_id)
    .bind(origin_node_id)
    .bind(if draft.contradictions.is_empty() {
        "确认成果契约"
    } else {
        "澄清目标中的关键矛盾"
    })
    .bind("用户确认什么现实结果代表项目成功")
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "INSERT INTO quality_gates \
         (id, project_id, node_id, title, criteria, status, required) \
         VALUES ($1, $2, $3, '成果契约已确认', $4, 'pending', 1)",
    )
    .bind(gate_id)
    .bind(project_id)
    .bind(origin_node_id)
    .bind(Json(vec![
        "成功必须由外部事实验证",
        "硬约束与不做事项已经明确",
    ]))
    .execute(&mut *transaction)
    .await?;

    insert_event(
        &mut transaction,
        project_id,
        "project.created",
        "human",
        json!({ "contradictions": draft.contradictions.len() }),
    )
    .await?;
    transaction.commit().await?;
    Ok(project_id)
}

pub async fn run_action(
    pool: &PgPool,
    artifact_root: PathBuf,
    project_id: Uuid,
    request: ProjectActionRequest,
) -> AppResult<()> {
    match request.action.as_str() {
        "confirm_outcome" => {
            let input: OutcomeConfirmation = serde_json::from_value(request.payload)
                .map_err(|_| AppError::bad_request("invalid_input", "请填写可核验的完成标准"))?;
            confirm_outcome(pool, project_id, input).await
        }
        "generate_brief" => generate_brief(pool, artifact_root, project_id).await,
        "approve_brief" => approve_brief(pool, project_id).await,
        "revise_intent" => {
            let input: ProjectIntake = serde_json::from_value(request.payload)
                .map_err(|_| AppError::bad_request("invalid_input", "请填写项目意图"))?;
            revise_intent(pool, project_id, input).await
        }
        _ => Err(AppError::bad_request(
            "unsupported_action",
            "不支持的项目动作",
        )),
    }
}

pub async fn revise_intent(pool: &PgPool, project_id: Uuid, input: ProjectIntake) -> AppResult<()> {
    let intent = normalize_intent(&input.intent)?;
    let draft = draft_project_from_intent(&intent)?;
    let snapshot = get_snapshot(pool, project_id).await?;
    let contract = snapshot
        .contract
        .ok_or_else(|| AppError::not_found("成果契约不存在"))?;
    if contract.status != "draft" {
        return Err(AppError::conflict(
            "contract_locked",
            "已确认的成果契约需要通过正式修订流程更新",
        ));
    }
    let current_action = snapshot
        .actions
        .iter()
        .find(|action| action.kind == "confirm_outcome" && action.status != "completed");
    let origin_node = snapshot.nodes.iter().find(|node| node.kind == "origin");
    let mut transaction = pool.begin().await?;

    sqlx::query("UPDATE projects SET title = $1, intent = $2, updated_at = now() WHERE id = $3")
        .bind(&draft.title)
        .bind(&draft.intent)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "UPDATE outcome_contracts SET desired_outcome = $1, success_evidence = $2, \
         constraints = $3, non_goals = $4, confirmation_question = $5, contradictions = $6, \
         version = version + 1, updated_at = now() WHERE project_id = $7",
    )
    .bind(&draft.outcome_contract.desired_outcome)
    .bind(Json(&draft.outcome_contract.success_evidence))
    .bind(Json(&draft.outcome_contract.constraints))
    .bind(Json(&draft.outcome_contract.non_goals))
    .bind(&draft.confirmation_question)
    .bind(Json(&draft.contradictions))
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    if let Some(node) = origin_node {
        sqlx::query("UPDATE project_nodes SET title = '项目起点', summary = $1 WHERE id = $2")
            .bind(&draft.intent)
            .bind(node.id)
            .execute(&mut *transaction)
            .await?;
    }
    if let Some(action) = current_action {
        sqlx::query("UPDATE action_runs SET title = $1, updated_at = now() WHERE id = $2")
            .bind(if draft.contradictions.is_empty() {
                "确认成果契约"
            } else {
                "澄清目标中的关键矛盾"
            })
            .bind(action.id)
            .execute(&mut *transaction)
            .await?;
    }
    insert_event(
        &mut transaction,
        project_id,
        "project.intent.revised",
        "human",
        json!({
            "contractVersion": contract.version + 1,
            "contradictions": draft.contradictions.len(),
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn confirm_outcome(
    pool: &PgPool,
    project_id: Uuid,
    input: OutcomeConfirmation,
) -> AppResult<()> {
    let completion_evidence = validate_outcome_confirmation(input)?;
    let snapshot = get_snapshot(pool, project_id).await?;
    let contract = snapshot
        .contract
        .as_ref()
        .ok_or_else(|| AppError::not_found("成果契约不存在"))?;
    if contract.status == "confirmed" {
        return Ok(());
    }
    if !contract.contradictions.0.is_empty() {
        return Err(AppError::conflict(
            "unresolved_contradiction",
            "成果描述仍有关键矛盾，请先修正原始意图",
        ));
    }
    let current_action = snapshot
        .actions
        .iter()
        .find(|action| action.kind == "confirm_outcome" && action.status != "completed");
    let contract_gate = snapshot
        .quality_gates
        .iter()
        .find(|gate| gate.title == "成果契约已确认");
    let main_branch = snapshot
        .branches
        .iter()
        .find(|branch| branch.is_main == 1)
        .ok_or_else(|| AppError::internal("项目主线尚未初始化"))?;
    let main_head = main_branch
        .head_node_id
        .ok_or_else(|| AppError::internal("项目主线没有当前节点"))?;
    let confirmation_node_id = Uuid::new_v4();
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO project_nodes \
         (id, project_id, branch_id, kind, title, summary, outcome, actor_type, resolved_at) \
         VALUES ($1, $2, $3, 'decision', '确认项目完成标准', $4, 'useful', 'human', now())",
    )
    .bind(confirmation_node_id)
    .bind(project_id)
    .bind(main_branch.id)
    .bind(&completion_evidence)
    .execute(&mut *transaction)
    .await?;
    insert_edge(
        &mut transaction,
        project_id,
        main_head,
        confirmation_node_id,
        "continue",
    )
    .await?;
    sqlx::query(
        "INSERT INTO project_contributions \
         (id, project_id, node_id, kind, title, body, status, accepted_at) \
         VALUES ($1, $2, $3, 'decision', '可核验的完成标准', $4, 'accepted', now())",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(confirmation_node_id)
    .bind(&completion_evidence)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE project_branches SET head_node_id = $1, updated_at = now() WHERE id = $2")
        .bind(confirmation_node_id)
        .bind(main_branch.id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "UPDATE outcome_contracts SET success_evidence = $1, status = 'confirmed', \
         confirmed_at = now(), updated_at = now() WHERE project_id = $2",
    )
    .bind(Json(vec![completion_evidence.clone()]))
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    if let Some(action) = current_action {
        complete_action(&mut transaction, action.id).await?;
    }
    if let Some(gate) = contract_gate {
        sqlx::query(
            "UPDATE quality_gates SET status = 'passed', resolved_at = now() WHERE id = $1",
        )
        .bind(gate.id)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO action_runs \
         (id, project_id, node_id, kind, title, owner, status, expected_signal, requires_approval) \
         VALUES ($1, $2, $3, 'draft_project_brief', '生成第一版项目启动说明', \
         'agent', 'ready', $4, 0)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(confirmation_node_id)
    .bind("生成一份带成果标准、约束和第一项外部证据行动的版本化文件")
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE projects SET state = 'active', current_focus = $1, updated_at = now() WHERE id = $2",
    )
    .bind("生成并审阅第一版项目启动说明")
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO decision_checkpoints \
         (id, project_id, node_id, decision, rationale, confidence) \
         VALUES ($1, $2, $3, 'continue', $4, 100)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(confirmation_node_id)
    .bind("成果契约已经由用户确认，可以开始生成第一项正式产物。")
    .execute(&mut *transaction)
    .await?;
    insert_event(
        &mut transaction,
        project_id,
        "outcome_contract.confirmed",
        "human",
        json!({ "version": contract.version, "successEvidence": completion_evidence }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn generate_brief(
    pool: &PgPool,
    artifact_root: PathBuf,
    project_id: Uuid,
) -> AppResult<()> {
    let snapshot = get_snapshot(pool, project_id).await?;
    let contract = snapshot
        .contract
        .as_ref()
        .ok_or_else(|| AppError::not_found("成果契约不存在"))?;
    if contract.status != "confirmed" {
        return Err(AppError::conflict(
            "contract_not_confirmed",
            "请先确认成果契约",
        ));
    }
    if snapshot
        .artifacts
        .iter()
        .any(|artifact| artifact.kind == "project_brief")
    {
        return Ok(());
    }
    let action = snapshot
        .actions
        .iter()
        .find(|action| action.kind == "draft_project_brief" && action.status != "completed")
        .ok_or_else(|| AppError::conflict("action_not_ready", "没有可执行的启动说明任务"))?;
    let main_branch = snapshot
        .branches
        .iter()
        .find(|branch| branch.is_main == 1)
        .ok_or_else(|| AppError::internal("项目主线尚未初始化"))?;
    let main_head = main_branch
        .head_node_id
        .ok_or_else(|| AppError::internal("项目主线没有当前节点"))?;
    sqlx::query("UPDATE action_runs SET status = 'running', updated_at = now() WHERE id = $1")
        .bind(action.id)
        .execute(pool)
        .await?;

    let artifact_id = Uuid::new_v4();
    let result_node_id = Uuid::new_v4();
    let contract_draft = OutcomeContractDraft {
        desired_outcome: contract.desired_outcome.clone(),
        success_evidence: contract.success_evidence.0.clone(),
        constraints: contract.constraints.0.clone(),
        non_goals: contract.non_goals.0.clone(),
    };
    let content = create_project_brief_markdown(
        &snapshot.project.title,
        &snapshot.project.intent,
        &contract_draft,
    );
    let stored = ArtifactStore::new(artifact_root)
        .write_text(project_id, artifact_id, "project-brief.md", &content)
        .await?;
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO project_nodes \
         (id, project_id, branch_id, kind, title, summary, outcome, actor_type, resolved_at) \
         VALUES ($1, $2, $3, 'result', '形成项目启动说明', $4, 'useful', 'agent', now())",
    )
    .bind(result_node_id)
    .bind(project_id)
    .bind(main_branch.id)
    .bind("生成一份可下载、可校验并等待人工审阅的项目启动说明。")
    .execute(&mut *transaction)
    .await?;
    insert_edge(
        &mut transaction,
        project_id,
        main_head,
        result_node_id,
        "continue",
    )
    .await?;
    sqlx::query("UPDATE project_branches SET head_node_id = $1, updated_at = now() WHERE id = $2")
        .bind(result_node_id)
        .bind(main_branch.id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO artifacts \
         (id, project_id, action_run_id, node_id, title, kind, storage_path, media_type, sha256, version, status) \
         VALUES ($1, $2, $3, $4, '项目启动说明', 'project_brief', $5, 'text/markdown', $6, 1, 'review')",
    )
    .bind(artifact_id)
    .bind(project_id)
    .bind(action.id)
    .bind(result_node_id)
    .bind(&stored.storage_path)
    .bind(&stored.sha256)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE action_runs SET status = 'completed', output_summary = $1, \
         completed_at = now(), updated_at = now() WHERE id = $2",
    )
    .bind("已生成项目启动说明 v1，等待用户审阅。")
    .bind(action.id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO action_runs \
         (id, project_id, node_id, kind, title, owner, status, expected_signal, requires_approval) \
         VALUES ($1, $2, $3, 'review_project_brief', '审阅项目启动说明', \
         'human', 'ready', $4, 1)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(result_node_id)
    .bind("用户确认启动说明足以指导第一项现实行动")
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO quality_gates \
         (id, project_id, node_id, title, criteria, status, required) \
         VALUES ($1, $2, $3, '项目启动说明已审阅', $4, 'pending', 1)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(result_node_id)
    .bind(Json(vec![
        "成功证据可验证",
        "约束没有被遗漏",
        "第一项行动能够产生外部证据",
    ]))
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO evidence \
         (id, project_id, action_run_id, artifact_id, node_id, kind, summary, source_uri, stance, confidence) \
         VALUES ($1, $2, $3, $4, $5, 'artifact_created', $6, $7, 'observes', 100)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(action.id)
    .bind(artifact_id)
    .bind(result_node_id)
    .bind("项目启动说明已作为实际文件保存，并记录 SHA-256。")
    .bind(&stored.storage_path)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE projects SET state = 'waiting', current_focus = '审阅项目启动说明', \
         updated_at = now() WHERE id = $1",
    )
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    insert_event(
        &mut transaction,
        project_id,
        "artifact.project_brief.created",
        "agent",
        json!({ "artifactId": artifact_id, "sha256": stored.sha256 }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn approve_brief(pool: &PgPool, project_id: Uuid) -> AppResult<()> {
    let snapshot = get_snapshot(pool, project_id).await?;
    let artifact = match snapshot
        .artifacts
        .iter()
        .find(|artifact| artifact.kind == "project_brief" && artifact.status == "review")
    {
        Some(artifact) => artifact,
        None if snapshot
            .artifacts
            .iter()
            .any(|artifact| artifact.kind == "project_brief" && artifact.status == "approved") =>
        {
            return Ok(());
        }
        None => {
            return Err(AppError::conflict(
                "artifact_not_ready",
                "没有等待审阅的项目启动说明",
            ));
        }
    };
    let review_action = snapshot
        .actions
        .iter()
        .find(|action| action.kind == "review_project_brief" && action.status != "completed");
    let review_gate = snapshot
        .quality_gates
        .iter()
        .find(|gate| gate.title == "项目启动说明已审阅" && gate.status == "pending");
    let main_branch = snapshot
        .branches
        .iter()
        .find(|branch| branch.is_main == 1)
        .ok_or_else(|| AppError::internal("项目主线尚未初始化"))?;
    let main_head = main_branch
        .head_node_id
        .ok_or_else(|| AppError::internal("项目主线没有当前节点"))?;
    let decision_node_id = Uuid::new_v4();
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO project_nodes \
         (id, project_id, branch_id, kind, title, summary, outcome, actor_type, resolved_at) \
         VALUES ($1, $2, $3, 'decision', '接受项目启动说明', $4, 'useful', 'human', now())",
    )
    .bind(decision_node_id)
    .bind(project_id)
    .bind(main_branch.id)
    .bind("启动说明已经通过人工审阅，可以据此开展下一项现实行动。")
    .execute(&mut *transaction)
    .await?;
    insert_edge(
        &mut transaction,
        project_id,
        main_head,
        decision_node_id,
        "continue",
    )
    .await?;
    sqlx::query(
        "INSERT INTO project_contributions \
         (id, project_id, node_id, kind, title, body, status, accepted_at) \
         VALUES ($1, $2, $3, 'decision', '启动说明通过审阅', $4, 'accepted', now())",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(decision_node_id)
    .bind("该版本已经被用户接受，下一步需要从图上的当前节点展开实际尝试。")
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE project_branches SET head_node_id = $1, updated_at = now() WHERE id = $2")
        .bind(decision_node_id)
        .bind(main_branch.id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE artifacts SET status = 'approved', approved_at = now() WHERE id = $1")
        .bind(artifact.id)
        .execute(&mut *transaction)
        .await?;
    if let Some(action) = review_action {
        complete_action(&mut transaction, action.id).await?;
    }
    if let Some(gate) = review_gate {
        sqlx::query(
            "UPDATE quality_gates SET status = 'passed', resolved_at = now() WHERE id = $1",
        )
        .bind(gate.id)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO action_runs \
         (id, project_id, node_id, kind, title, owner, status, expected_signal, requires_approval) \
         VALUES ($1, $2, $3, 'select_first_evidence_action', $4, 'human', 'ready', $5, 1)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(decision_node_id)
    .bind("确认第一项可产生外部证据的行动")
    .bind("选定一项现实行动及其预期反馈，而不是继续生成计划")
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE projects SET state = 'active', current_focus = $1, updated_at = now() WHERE id = $2",
    )
    .bind("确认第一项能够实际推进项目的行动")
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO decision_checkpoints \
         (id, project_id, node_id, decision, rationale, confidence) \
         VALUES ($1, $2, $3, 'continue', $4, 100)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(decision_node_id)
    .bind("项目启动说明已通过人工审阅，下一步应产生外部证据。")
    .execute(&mut *transaction)
    .await?;
    insert_event(
        &mut transaction,
        project_id,
        "artifact.project_brief.approved",
        "human",
        json!({ "artifactId": artifact.id }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn find_artifact(pool: &PgPool, artifact_id: Uuid) -> AppResult<Artifact> {
    sqlx::query_as::<_, Artifact>("SELECT * FROM artifacts WHERE id = $1")
        .bind(artifact_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::not_found("产物不存在"))
}

async fn complete_action(
    transaction: &mut Transaction<'_, Postgres>,
    action_id: Uuid,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE action_runs SET status = 'completed', completed_at = now(), \
         updated_at = now() WHERE id = $1",
    )
    .bind(action_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn insert_edge(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    parent_node_id: Uuid,
    child_node_id: Uuid,
    relation: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO project_node_edges \
         (project_id, parent_node_id, child_node_id, relation) VALUES ($1, $2, $3, $4)",
    )
    .bind(project_id)
    .bind(parent_node_id)
    .bind(child_node_id)
    .bind(relation)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn insert_event(
    transaction: &mut Transaction<'_, Postgres>,
    project_id: Uuid,
    event_type: &str,
    actor_type: &str,
    payload: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO project_events (id, project_id, event_type, actor_type, payload) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(event_type)
    .bind(actor_type)
    .bind(Json(payload))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
