use std::collections::HashSet;

use serde_json::{Value, json};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::{
    application::projects::{insert_edge, insert_event},
    domain::{
        AppendProgressInput, CreateBranchInput, GraphActionRequest, IntegrateBranchInput,
        ParkBranchInput, can_append_to_branch, can_integrate_branch,
    },
    error::{AppError, AppResult},
    models::{ProjectBranch, ProjectContribution, ProjectNode},
};

const BRANCH_COLORS: &[&str] = &[
    "#6f7cff", "#ef7f5a", "#3ba77a", "#b06fc7", "#cf9d2e", "#4389c9",
];

pub async fn run_graph_action(
    pool: &PgPool,
    project_id: Uuid,
    request: GraphActionRequest,
) -> AppResult<()> {
    match request.action.as_str() {
        "create_branch" => {
            let input: CreateBranchInput = decode(request.payload, "新分支参数不完整")?;
            create_branch(pool, project_id, input.validate()?).await
        }
        "append_progress" => {
            let input: AppendProgressInput = decode(request.payload, "进展参数不完整")?;
            append_progress(pool, project_id, input.validate()?).await
        }
        "integrate_branch" => {
            let input: IntegrateBranchInput = decode(request.payload, "合流参数不完整")?;
            integrate_branch(pool, project_id, input.validate()?).await
        }
        "park_branch" => {
            let input: ParkBranchInput = decode(request.payload, "暂停分支参数不完整")?;
            park_branch(pool, project_id, input.validate()?).await
        }
        _ => Err(AppError::bad_request(
            "unsupported_graph_action",
            "不支持的项目脉络动作",
        )),
    }
}

fn decode<T: serde::de::DeserializeOwned>(value: Value, message: &str) -> AppResult<T> {
    serde_json::from_value(value).map_err(|_| AppError::bad_request("invalid_graph_input", message))
}

pub async fn create_branch(
    pool: &PgPool,
    project_id: Uuid,
    input: CreateBranchInput,
) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM project_branches WHERE client_request_id = $1)",
    )
    .bind(input.client_request_id)
    .fetch_one(pool)
    .await?;
    if exists {
        return Ok(());
    }
    let source = find_node(pool, project_id, input.from_node_id)
        .await?
        .ok_or_else(|| {
            AppError::bad_request("invalid_fork_node", "只能从当前项目已有的节点开分支")
        })?;
    let branch_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM project_branches WHERE project_id = $1")
            .bind(project_id)
            .fetch_one(pool)
            .await?;
    let color_index = (branch_count.max(1) as usize - 1) % BRANCH_COLORS.len();
    let branch_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO project_branches \
         (id, project_id, name, purpose, status, is_main, color, forked_from_node_id, client_request_id) \
         VALUES ($1, $2, $3, $4, 'active', 0, $5, $6, $7)",
    )
    .bind(branch_id)
    .bind(project_id)
    .bind(&input.name)
    .bind(&input.purpose)
    .bind(BRANCH_COLORS[color_index])
    .bind(source.id)
    .bind(input.client_request_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO project_nodes \
         (id, project_id, branch_id, kind, title, summary, outcome, actor_type) \
         VALUES ($1, $2, $3, 'work', $4, $5, 'open', 'human')",
    )
    .bind(node_id)
    .bind(project_id)
    .bind(branch_id)
    .bind(&input.name)
    .bind(&input.purpose)
    .execute(&mut *transaction)
    .await?;
    insert_edge(&mut transaction, project_id, source.id, node_id, "fork").await?;
    sqlx::query("UPDATE project_branches SET head_node_id = $1, updated_at = now() WHERE id = $2")
        .bind(node_id)
        .bind(branch_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("UPDATE projects SET current_focus = $1, updated_at = now() WHERE id = $2")
        .bind(&input.purpose)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    insert_event(
        &mut transaction,
        project_id,
        "graph.branch.created",
        "human",
        json!({
            "branchId": branch_id,
            "nodeId": node_id,
            "fromNodeId": source.id,
            "name": input.name,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn append_progress(
    pool: &PgPool,
    project_id: Uuid,
    input: AppendProgressInput,
) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM project_nodes WHERE client_request_id = $1)",
    )
    .bind(input.client_request_id)
    .fetch_one(pool)
    .await?;
    if exists {
        return Ok(());
    }
    let branch = find_branch(pool, project_id, input.branch_id)
        .await?
        .filter(|branch| can_append_to_branch(&branch.status) && branch.head_node_id.is_some())
        .ok_or_else(|| {
            AppError::conflict("branch_not_appendable", "这条分支当前不能继续追加进展")
        })?;
    let branch_head = branch.head_node_id.expect("checked above");
    let node_id = Uuid::new_v4();
    let contribution_id = Uuid::new_v4();
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO project_nodes \
         (id, project_id, branch_id, kind, title, summary, outcome, actor_type, \
          client_request_id, resolved_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'human', $8, \
          CASE WHEN $7 = 'open' THEN NULL ELSE now() END)",
    )
    .bind(node_id)
    .bind(project_id)
    .bind(branch.id)
    .bind(if input.outcome == "open" {
        "work"
    } else {
        "result"
    })
    .bind(&input.title)
    .bind(&input.summary)
    .bind(&input.outcome)
    .bind(input.client_request_id)
    .execute(&mut *transaction)
    .await?;
    insert_edge(
        &mut transaction,
        project_id,
        branch_head,
        node_id,
        "continue",
    )
    .await?;
    sqlx::query(
        "INSERT INTO project_contributions \
         (id, project_id, node_id, kind, title, body, reference_uri, scope, reopen_when, \
          status, accepted_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, \
          CASE WHEN $10 = 'accepted' THEN now() ELSE NULL END)",
    )
    .bind(contribution_id)
    .bind(project_id)
    .bind(node_id)
    .bind(&input.contribution.kind)
    .bind(&input.contribution.title)
    .bind(&input.contribution.body)
    .bind(&input.contribution.reference_uri)
    .bind(&input.contribution.scope)
    .bind(&input.contribution.reopen_when)
    .bind(if branch.is_main == 1 {
        "accepted"
    } else {
        "candidate"
    })
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE project_branches SET head_node_id = $1, status = 'active', \
         updated_at = now() WHERE id = $2",
    )
    .bind(node_id)
    .bind(branch.id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE projects SET current_focus = $1, updated_at = now() WHERE id = $2")
        .bind(&input.summary)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    insert_event(
        &mut transaction,
        project_id,
        "graph.node.created",
        "human",
        json!({
            "branchId": branch.id,
            "nodeId": node_id,
            "outcome": input.outcome,
            "contributionId": contribution_id,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn integrate_branch(
    pool: &PgPool,
    project_id: Uuid,
    input: IntegrateBranchInput,
) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM branch_merges WHERE client_request_id = $1)",
    )
    .bind(input.client_request_id)
    .fetch_one(pool)
    .await?;
    if exists {
        return Ok(());
    }
    let source = find_branch(pool, project_id, input.source_branch_id)
        .await?
        .ok_or_else(|| AppError::not_found("来源分支不存在"))?;
    let target = sqlx::query_as::<_, ProjectBranch>(
        "SELECT * FROM project_branches WHERE project_id = $1 AND is_main = 1 LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::internal("项目主线不存在"))?;
    if source.head_node_id.is_none()
        || target.head_node_id.is_none()
        || !can_integrate_branch(&source.status, source.is_main == 1)
    {
        return Err(AppError::conflict(
            "branch_not_integratable",
            "这条分支当前不能合回主线",
        ));
    }
    let source_head = source.head_node_id.expect("checked above");
    let target_head = target.head_node_id.expect("checked above");
    let source_node_ids =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM project_nodes WHERE branch_id = $1")
            .bind(source.id)
            .fetch_all(pool)
            .await?;
    let contributions = if source_node_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, ProjectContribution>(
            "SELECT * FROM project_contributions WHERE node_id = ANY($1)",
        )
        .bind(&source_node_ids)
        .fetch_all(pool)
        .await?
    };
    if contributions.is_empty() {
        return Err(AppError::conflict(
            "branch_has_no_contribution",
            "先记录这条分支实际产生的文件、结论、证据或条件，再带回主线",
        ));
    }
    let available: HashSet<Uuid> = contributions.iter().map(|item| item.id).collect();
    if input
        .accepted_contribution_ids
        .iter()
        .any(|id| !available.contains(id))
    {
        return Err(AppError::bad_request(
            "invalid_merge_contribution",
            "只能带回这条来源分支实际产生的内容",
        ));
    }
    let source_outcome =
        sqlx::query_scalar::<_, String>("SELECT outcome FROM project_nodes WHERE id = $1")
            .bind(source_head)
            .fetch_optional(pool)
            .await?
            .unwrap_or_else(|| "useful".into());
    let merge_outcome = if source_outcome == "open" {
        "useful"
    } else {
        &source_outcome
    };
    let merge_node_id = Uuid::new_v4();
    let merge_id = Uuid::new_v4();
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO project_nodes \
         (id, project_id, branch_id, kind, title, summary, outcome, actor_type, \
          client_request_id, resolved_at) \
         VALUES ($1, $2, $3, 'merge', $4, $5, $6, 'human', $7, now())",
    )
    .bind(merge_node_id)
    .bind(project_id)
    .bind(target.id)
    .bind(format!("吸收「{}」", source.name))
    .bind(&input.summary)
    .bind(merge_outcome)
    .bind(input.client_request_id)
    .execute(&mut *transaction)
    .await?;
    insert_edge(
        &mut transaction,
        project_id,
        target_head,
        merge_node_id,
        "continue",
    )
    .await?;
    insert_edge(
        &mut transaction,
        project_id,
        source_head,
        merge_node_id,
        "merge",
    )
    .await?;
    sqlx::query(
        "INSERT INTO branch_merges \
         (id, project_id, source_branch_id, target_branch_id, result_node_id, summary, \
          accepted_contribution_ids, client_request_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(merge_id)
    .bind(project_id)
    .bind(source.id)
    .bind(target.id)
    .bind(merge_node_id)
    .bind(&input.summary)
    .bind(Json(&input.accepted_contribution_ids))
    .bind(input.client_request_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO project_contributions \
         (id, project_id, node_id, kind, title, body, status, accepted_at) \
         VALUES ($1, $2, $3, 'decision', $4, $5, 'accepted', now())",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(merge_node_id)
    .bind(format!("主线已吸收「{}」的结果", source.name))
    .bind(&input.summary)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE project_contributions SET status = 'accepted', accepted_at = now() \
         WHERE id = ANY($1)",
    )
    .bind(&input.accepted_contribution_ids)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE project_branches SET head_node_id = $1, updated_at = now() WHERE id = $2")
        .bind(merge_node_id)
        .bind(target.id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "UPDATE project_branches SET status = 'integrated', closed_at = now(), \
         updated_at = now() WHERE id = $1",
    )
    .bind(source.id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE projects SET current_focus = $1, updated_at = now() WHERE id = $2")
        .bind(&input.summary)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    insert_event(
        &mut transaction,
        project_id,
        "graph.branch.integrated",
        "human",
        json!({
            "sourceBranchId": source.id,
            "targetBranchId": target.id,
            "mergeNodeId": merge_node_id,
            "mergeId": merge_id,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn park_branch(pool: &PgPool, project_id: Uuid, input: ParkBranchInput) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM project_nodes WHERE client_request_id = $1)",
    )
    .bind(input.client_request_id)
    .fetch_one(pool)
    .await?;
    if exists {
        return Ok(());
    }
    let branch = find_branch(pool, project_id, input.branch_id)
        .await?
        .filter(|branch| {
            branch.is_main != 1
                && branch.head_node_id.is_some()
                && can_append_to_branch(&branch.status)
        })
        .ok_or_else(|| AppError::conflict("branch_not_parkable", "这条分支当前不能暂停"))?;
    let branch_head = branch.head_node_id.expect("checked above");
    let node_id = Uuid::new_v4();
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "INSERT INTO project_nodes \
         (id, project_id, branch_id, kind, title, summary, outcome, actor_type, \
          client_request_id, resolved_at) \
         VALUES ($1, $2, $3, 'decision', $4, $5, 'blocked', 'human', $6, now())",
    )
    .bind(node_id)
    .bind(project_id)
    .bind(branch.id)
    .bind(format!("暂停「{}」", branch.name))
    .bind(&input.reason)
    .bind(input.client_request_id)
    .execute(&mut *transaction)
    .await?;
    insert_edge(
        &mut transaction,
        project_id,
        branch_head,
        node_id,
        "continue",
    )
    .await?;
    sqlx::query(
        "INSERT INTO project_contributions \
         (id, project_id, node_id, kind, title, body, reopen_when, status) \
         VALUES ($1, $2, $3, 'condition', '当前无法继续的条件', $4, $5, 'candidate')",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(node_id)
    .bind(&input.reason)
    .bind(&input.reopen_when)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE project_branches SET head_node_id = $1, status = 'waiting', \
         updated_at = now() WHERE id = $2",
    )
    .bind(node_id)
    .bind(branch.id)
    .execute(&mut *transaction)
    .await?;
    insert_event(
        &mut transaction,
        project_id,
        "graph.branch.waiting",
        "human",
        json!({
            "branchId": branch.id,
            "nodeId": node_id,
            "reopenWhen": input.reopen_when,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn find_node(
    pool: &PgPool,
    project_id: Uuid,
    node_id: Uuid,
) -> AppResult<Option<ProjectNode>> {
    Ok(sqlx::query_as::<_, ProjectNode>(
        "SELECT * FROM project_nodes WHERE id = $1 AND project_id = $2",
    )
    .bind(node_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await?)
}

async fn find_branch(
    pool: &PgPool,
    project_id: Uuid,
    branch_id: Uuid,
) -> AppResult<Option<ProjectBranch>> {
    Ok(sqlx::query_as::<_, ProjectBranch>(
        "SELECT * FROM project_branches WHERE id = $1 AND project_id = $2",
    )
    .bind(branch_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await?)
}
