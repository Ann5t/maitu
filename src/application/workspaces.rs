use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use chrono::{DateTime, Duration, Utc};
use fudian::runner_protocol::{
    RunnerCapabilities, RunnerExecutionResult, RunnerJobSpec, RunnerOutputFile,
    RunnerResourceLimits, canonical_json_sha256 as runner_canonical_json_sha256,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool, Postgres, Transaction, types::Json};
use uuid::Uuid;

use crate::{
    config::Config,
    error::{AppError, AppResult},
    goal_domain::{CommandReceiptIdentity, canonical_json_sha256},
    workspace::{
        FailRunnerJobRequest, FinalizeRunnerJobRequest, PrepareRunnerJobRequest,
        PrepareRunnerJobResponse, RunnerJobOutcome, WorkspaceCapabilityPolicy,
        normalize_relative_file_path, path_matches_pattern,
    },
};

type WorkspaceTransaction<'a> = Transaction<'a, Postgres>;

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepositoryRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub storage_key: String,
    pub default_branch: String,
    pub default_head_commit: String,
    pub object_format: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalWorkspaceRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub repository_id: Uuid,
    pub git_branch_name: String,
    pub worktree_key: String,
    pub base_commit: Option<String>,
    pub head_commit: Option<String>,
    pub tree_id: Option<String>,
    pub workspace_snapshot: Option<String>,
    pub dirty: bool,
    pub status: String,
    pub fencing_counter: i64,
    pub last_error_code: Option<String>,
    pub last_error_summary: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspacePolicyRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub source_proposal_id: Uuid,
    pub source_proposal_revision: i32,
    pub policy: Json<Value>,
    pub policy_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshotRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub workspace_id: Uuid,
    pub operation_id: Uuid,
    pub runner_job_id: Option<Uuid>,
    pub parent_snapshot_id: Option<Uuid>,
    pub head_commit: String,
    pub tree_id: String,
    pub dirty: bool,
    pub snapshot_hash: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitWorkspaceInspection {
    pub head_commit: String,
    pub tree_id: String,
    pub dirty: bool,
    pub status_digest: String,
    pub workspace_snapshot: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDetail {
    pub repository: GitRepositoryRecord,
    pub workspace: GoalWorkspaceRecord,
    pub policy: WorkspacePolicyRecord,
    pub latest_snapshot: Option<WorkspaceSnapshotRecord>,
    pub observed: GitWorkspaceInspection,
    pub matches_record: bool,
}

#[derive(Clone, Debug, FromRow)]
struct BranchProvisionState {
    id: Uuid,
    project_id: Uuid,
    creating_proposal_id: Uuid,
    parent_goal_branch_id: Option<Uuid>,
    inherited_from_session_id: Option<Uuid>,
    head_session_id: Uuid,
    git_branch_name: Option<String>,
    approved_revision: Option<i32>,
}

#[derive(Clone, Debug, FromRow)]
struct RunnerJobState {
    id: Uuid,
    project_id: Uuid,
    goal_branch_id: Uuid,
    session_id: Uuid,
    workspace_id: Uuid,
    lease_id: Uuid,
    client_request_id: Uuid,
    request_hash: String,
    status: String,
    spec: Json<Value>,
    spec_hash: String,
    runtime_digest: String,
    candidate_commit: Option<String>,
}

#[derive(Clone, Debug, FromRow)]
struct LeaseState {
    id: Uuid,
    status: String,
    fencing_token: i64,
    renewal_token_digest: String,
    base_commit: String,
    base_workspace_snapshot: String,
    allowed_writes: Json<Vec<String>>,
    delete_paths: Json<Vec<String>>,
    capabilities: Json<Value>,
    resource_policy: Json<Value>,
    output_key: String,
    hard_expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct ManagedRoots {
    repositories: PathBuf,
    worktrees: PathBuf,
    runner_outputs: PathBuf,
}

impl ManagedRoots {
    async fn from_config(config: &Config) -> AppResult<Self> {
        let repositories = canonical_root(&config.repository_root).await?;
        let worktrees = canonical_root(&config.worktree_root).await?;
        let runner_outputs = canonical_root(&config.runner_output_root).await?;
        if repositories == worktrees
            || repositories == runner_outputs
            || worktrees == runner_outputs
        {
            return Err(AppError::internal(
                "Git 仓库、worktree 与 Runner 输出必须使用不同托管根目录",
            ));
        }
        Ok(Self {
            repositories,
            worktrees,
            runner_outputs,
        })
    }
}

pub async fn provision_goal_branch(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    goal_branch_id: Uuid,
    session_id: Uuid,
    client_request_id: Uuid,
) -> AppResult<WorkspaceDetail> {
    let roots = ManagedRoots::from_config(config).await?;
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    let branch = load_branch_provision_state(&mut transaction, project_id, goal_branch_id).await?;
    if branch.project_id != project_id || branch.head_session_id != session_id {
        return Err(AppError::conflict(
            "stale_session_head",
            "只能为目标枝干当前 Session 建立 worktree",
        ));
    }
    let expected_git_branch = format!("goal/{goal_branch_id}");
    if branch.git_branch_name.as_deref() != Some(expected_git_branch.as_str()) {
        return Err(AppError::conflict(
            "git_identity_mismatch",
            "GoalBranch 的 Git branch 身份不是系统确定值",
        ));
    }

    let repository_key = format!("projects/{project_id}.git");
    let repository_path = managed_path(&roots.repositories, &repository_key, true)?;
    let repository = sqlx::query_as::<_, GitRepositoryRecord>(
        "SELECT * FROM project_git_repositories WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let repository = if let Some(repository) = repository {
        if repository.storage_key != repository_key || repository.status != "active" {
            return Err(AppError::conflict(
                "git_repository_mismatch",
                "项目托管 Git 仓库身份或状态不合法",
            ));
        }
        repository
    } else {
        let path = repository_path.clone();
        let initialized = tokio::task::spawn_blocking(move || ensure_bare_repository(&path))
            .await
            .map_err(|_| AppError::internal("初始化 Git 仓库的阻塞任务异常结束"))?
            .map_err(|error| AppError::internal(format!("初始化托管 Git 仓库失败：{error}")))?;
        sqlx::query_as::<_, GitRepositoryRecord>(
            "INSERT INTO project_git_repositories \
             (id, project_id, storage_key, default_branch, default_head_commit, object_format) \
             VALUES ($1, $2, $3, 'main', $4, $5) RETURNING *",
        )
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(&repository_key)
        .bind(initialized.head_commit)
        .bind(initialized.object_format)
        .fetch_one(&mut *transaction)
        .await?
    };

    let policy = ensure_workspace_policy(&mut transaction, &branch).await?;
    let worktree_key = format!("projects/{project_id}/goals/{goal_branch_id}");
    let worktree_path = managed_path(&roots.worktrees, &worktree_key, true)?;
    let existing = sqlx::query_as::<_, GoalWorkspaceRecord>(
        "SELECT * FROM goal_workspaces WHERE goal_branch_id = $1 FOR UPDATE",
    )
    .bind(goal_branch_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let (workspace, operation_id, base_commit) = if let Some(workspace) = existing {
        if workspace.repository_id != repository.id
            || workspace.git_branch_name != expected_git_branch
            || workspace.worktree_key != worktree_key
        {
            return Err(AppError::conflict(
                "workspace_identity_mismatch",
                "数据库中的 GoalWorkspace 身份与确定性路径不一致",
            ));
        }
        if workspace.status == "ready" {
            transaction.commit().await?;
            return verified_ready_workspace(pool, &roots, project_id, goal_branch_id).await;
        }
        if workspace.status == "error" {
            return Err(AppError::conflict(
                "workspace_requires_recovery",
                "worktree 曾在建立过程中失败，需要先执行显式恢复",
            ));
        }
        let operation_id: Uuid = sqlx::query_scalar(
            "SELECT id FROM workspace_operations \
             WHERE workspace_id = $1 AND operation_kind = 'provision' \
               AND status IN ('planned', 'applying') ORDER BY created_at LIMIT 1",
        )
        .bind(workspace.id)
        .fetch_one(&mut *transaction)
        .await?;
        let base = workspace.base_commit.clone().ok_or_else(|| {
            AppError::conflict(
                "workspace_requires_recovery",
                "provisioning worktree 缺少固定基线",
            )
        })?;
        (workspace, operation_id, base)
    } else {
        let base_commit = if let Some(parent_branch_id) = branch.parent_goal_branch_id {
            let parent: GoalWorkspaceRecord = sqlx::query_as(
                "SELECT * FROM goal_workspaces \
                 WHERE goal_branch_id = $1 AND project_id = $2 FOR UPDATE",
            )
            .bind(parent_branch_id)
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or_else(|| {
                AppError::conflict(
                    "parent_workspace_missing",
                    "子目标必须从父枝干的真实 worktree 安全点创建",
                )
            })?;
            if parent.status != "ready" || parent.dirty {
                return Err(AppError::conflict(
                    "parent_workspace_not_safe",
                    "父 worktree 不是可分枝的干净安全点",
                ));
            }
            if parent.repository_id != repository.id {
                return Err(AppError::conflict(
                    "parent_workspace_not_safe",
                    "父 worktree 不属于项目的确定性托管仓库",
                ));
            }
            let parent_has_active_lease: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM workspace_write_leases \
                 WHERE workspace_id = $1 AND status = 'active')",
            )
            .bind(parent.id)
            .fetch_one(&mut *transaction)
            .await?;
            if parent_has_active_lease {
                return Err(AppError::conflict(
                    "parent_workspace_busy",
                    "父 worktree 尚有 active 写 Lease，不能从移动中的基线分枝",
                ));
            }
            let parent_head = parent.head_commit.clone().ok_or_else(|| {
                AppError::conflict("parent_workspace_not_safe", "父 worktree 缺少 HEAD")
            })?;
            let parent_tree = parent.tree_id.clone().ok_or_else(|| {
                AppError::conflict("parent_workspace_not_safe", "父 worktree 缺少 tree ID")
            })?;
            let parent_snapshot = parent.workspace_snapshot.clone().ok_or_else(|| {
                AppError::conflict("parent_workspace_not_safe", "父 worktree 缺少快照摘要")
            })?;
            let parent_path = managed_path(&roots.worktrees, &parent.worktree_key, false)?;
            let parent_repository_path = repository_path.clone();
            let parent_branch_name = parent.git_branch_name.clone();
            let observed_parent = tokio::task::spawn_blocking(move || {
                inspect_managed_worktree(&parent_repository_path, &parent_path, &parent_branch_name)
            })
            .await
            .map_err(|_| AppError::internal("检查父 worktree 的阻塞任务异常结束"))?
            .map_err(|error| {
                AppError::conflict(
                    "parent_workspace_inspection_failed",
                    format!("无法验证父 worktree：{error}"),
                )
            })?;
            if observed_parent.dirty
                || observed_parent.head_commit != parent_head
                || observed_parent.tree_id != parent_tree
                || observed_parent.workspace_snapshot != parent_snapshot
            {
                return Err(AppError::conflict(
                    "parent_workspace_drifted",
                    "父 worktree 的磁盘现场与已记录安全点不一致，已拒绝从错误基线分枝",
                ));
            }
            parent_head
        } else {
            repository.default_head_commit.clone()
        };
        let workspace_id = Uuid::new_v4();
        let workspace = sqlx::query_as::<_, GoalWorkspaceRecord>(
            "INSERT INTO goal_workspaces \
             (id, project_id, goal_branch_id, repository_id, git_branch_name, \
              worktree_key, base_commit, status) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, 'provisioning') RETURNING *",
        )
        .bind(workspace_id)
        .bind(project_id)
        .bind(goal_branch_id)
        .bind(repository.id)
        .bind(&expected_git_branch)
        .bind(&worktree_key)
        .bind(&base_commit)
        .fetch_one(&mut *transaction)
        .await?;
        let operation_id = Uuid::new_v4();
        let request_hash = canonical_json_sha256(&json!({
            "kind": "provision",
            "projectId": project_id,
            "goalBranchId": goal_branch_id,
            "sessionId": session_id,
            "repositoryId": repository.id,
            "gitBranchName": expected_git_branch,
            "worktreeKey": worktree_key,
            "baseCommit": base_commit,
        }))?;
        sqlx::query(
            "INSERT INTO workspace_operations \
             (id, project_id, goal_branch_id, workspace_id, operation_kind, status, \
              request_hash, expected_head_commit, detail) \
             VALUES ($1, $2, $3, $4, 'provision', 'planned', $5, $6, $7)",
        )
        .bind(operation_id)
        .bind(project_id)
        .bind(goal_branch_id)
        .bind(workspace_id)
        .bind(request_hash)
        .bind(&base_commit)
        .bind(Json(json!({
            "repositoryKey": repository_key,
            "worktreeKey": worktree_key,
            "parentGoalBranchId": branch.parent_goal_branch_id,
            "inheritedFromSessionId": branch.inherited_from_session_id,
        })))
        .execute(&mut *transaction)
        .await?;
        (workspace, operation_id, base_commit)
    };
    sqlx::query(
        "UPDATE workspace_operations SET status = 'applying', updated_at = now() \
         WHERE id = $1 AND status = 'planned'",
    )
    .bind(operation_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let repository_path_for_git = repository_path.clone();
    let worktree_path_for_git = worktree_path.clone();
    let branch_name_for_git = expected_git_branch.clone();
    let base_for_git = base_commit.clone();
    let inspection = tokio::task::spawn_blocking(move || {
        ensure_goal_worktree(
            &repository_path_for_git,
            &worktree_path_for_git,
            &branch_name_for_git,
            &base_for_git,
        )
    })
    .await
    .map_err(|_| AppError::internal("建立 worktree 的阻塞任务异常结束"))?;
    let inspection = match inspection {
        Ok(inspection) => inspection,
        Err(error) => {
            mark_provision_failure(
                pool,
                project_id,
                goal_branch_id,
                session_id,
                workspace.id,
                operation_id,
                client_request_id,
                "git_worktree_provision_failed",
                &error,
            )
            .await?;
            return Err(AppError::conflict(
                "workspace_provision_failed",
                format!("Git worktree 建立失败并已安全暂停 Session：{error}"),
            ));
        }
    };
    if inspection.head_commit != base_commit || inspection.dirty {
        let summary = "新 worktree 的 HEAD/脏状态与固定基线不一致";
        mark_provision_failure(
            pool,
            project_id,
            goal_branch_id,
            session_id,
            workspace.id,
            operation_id,
            client_request_id,
            "git_worktree_baseline_mismatch",
            summary,
        )
        .await?;
        return Err(AppError::conflict("workspace_provision_failed", summary));
    }

    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    let locked: GoalWorkspaceRecord =
        sqlx::query_as("SELECT * FROM goal_workspaces WHERE id = $1 FOR UPDATE")
            .bind(workspace.id)
            .fetch_one(&mut *transaction)
            .await?;
    if locked.status != "provisioning" || locked.base_commit.as_deref() != Some(&base_commit) {
        return Err(AppError::conflict(
            "workspace_concurrent_change",
            "worktree 建立期间数据库身份发生变化",
        ));
    }
    let parent_snapshot_id: Option<Uuid> =
        if let Some(parent_session_id) = branch.inherited_from_session_id {
            sqlx::query_scalar(
                "SELECT id FROM workspace_snapshots WHERE session_id = $1 \
             ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .bind(parent_session_id)
            .fetch_optional(&mut *transaction)
            .await?
        } else {
            None
        };
    let snapshot_id = Uuid::new_v4();
    sqlx::query(
        "UPDATE goal_workspaces SET head_commit = $1, tree_id = $2, workspace_snapshot = $3, \
         dirty = false, status = 'ready', last_error_code = NULL, last_error_summary = NULL, \
         updated_at = now() WHERE id = $4",
    )
    .bind(&inspection.head_commit)
    .bind(&inspection.tree_id)
    .bind(&inspection.workspace_snapshot)
    .bind(workspace.id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE goal_branches SET worktree_path = $1, base_commit = $2, updated_at = now() \
         WHERE id = $3 AND project_id = $4",
    )
    .bind(&worktree_key)
    .bind(&base_commit)
    .bind(goal_branch_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE workspace_operations SET status = 'applied', candidate_commit = $1, \
         detail = detail || $2, updated_at = now(), completed_at = now() WHERE id = $3",
    )
    .bind(&inspection.head_commit)
    .bind(Json(json!({
        "treeId": inspection.tree_id,
        "workspaceSnapshot": inspection.workspace_snapshot,
        "dirty": false,
    })))
    .bind(operation_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO workspace_snapshots \
         (id, project_id, goal_branch_id, session_id, workspace_id, operation_id, \
          parent_snapshot_id, head_commit, tree_id, dirty, snapshot_hash) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, false, $10)",
    )
    .bind(snapshot_id)
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(session_id)
    .bind(workspace.id)
    .bind(operation_id)
    .bind(parent_snapshot_id)
    .bind(&inspection.head_commit)
    .bind(&inspection.tree_id)
    .bind(&inspection.workspace_snapshot)
    .execute(&mut *transaction)
    .await?;
    insert_workspace_event(
        &mut transaction,
        project_id,
        "goal_branch",
        goal_branch_id,
        "workspace.provisioned",
        client_request_id,
        json!({
            "workspaceId": workspace.id,
            "repositoryId": repository.id,
            "gitBranchName": expected_git_branch,
            "worktreeKey": worktree_key,
            "baseCommit": base_commit,
            "headCommit": inspection.head_commit,
            "treeId": inspection.tree_id,
            "workspaceSnapshot": inspection.workspace_snapshot,
            "policyHash": policy.policy_hash,
        }),
    )
    .await?;
    transaction.commit().await?;
    verified_ready_workspace(pool, &roots, project_id, goal_branch_id).await
}

pub async fn get_workspace(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    goal_branch_id: Uuid,
) -> AppResult<WorkspaceDetail> {
    let roots = ManagedRoots::from_config(config).await?;
    verified_workspace_detail(pool, &roots, project_id, goal_branch_id).await
}

pub async fn pause_failed_workspace_provision(
    pool: &PgPool,
    project_id: Uuid,
    goal_branch_id: Uuid,
    session_id: Uuid,
    client_request_id: Uuid,
    code: &str,
    summary: &str,
) -> AppResult<()> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    let changed = sqlx::query(
        "UPDATE goal_sessions SET status = 'exception_paused', updated_at = now() \
         WHERE id = $1 AND project_id = $2 AND goal_branch_id = $3 AND status = 'running'",
    )
    .bind(session_id)
    .bind(project_id)
    .bind(goal_branch_id)
    .execute(&mut *transaction)
    .await?
    .rows_affected();
    if changed == 0 {
        transaction.commit().await?;
        return Ok(());
    }
    sqlx::query(
        "UPDATE goal_workspaces SET status = 'error', last_error_code = $1, \
         last_error_summary = $2, updated_at = now() \
         WHERE project_id = $3 AND goal_branch_id = $4 \
           AND status IN ('provisioning', 'ready')",
    )
    .bind(code)
    .bind(summary)
    .bind(project_id)
    .bind(goal_branch_id)
    .execute(&mut *transaction)
    .await?;
    insert_exception_attention(
        &mut transaction,
        project_id,
        goal_branch_id,
        session_id,
        &format!("workspace:{goal_branch_id}:provision-orchestration"),
        "Git worktree 准备未完成",
        summary,
        "BranchProposal 已批准，但 Session 在获得任何 Runner 写权前暂停",
        "系统已停止自动准备 worktree，并保留确定性仓库与枝干身份",
        "继续执行可能建立在未验证或漂移的 Git 基线上",
        "查看异常详情并修复托管目录或父枝干现场，再显式恢复",
    )
    .await?;
    insert_workspace_event(
        &mut transaction,
        project_id,
        "session",
        session_id,
        "workspace.provision_orchestration_paused",
        client_request_id,
        json!({ "code": code, "summary": summary }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

pub async fn prepare_runner_job(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    session_id: Uuid,
    request: PrepareRunnerJobRequest,
) -> AppResult<PrepareRunnerJobResponse> {
    let request = request.normalize()?;
    let identity = CommandReceiptIdentity::from_input("runner.prepare", &request)?;
    let roots = ManagedRoots::from_config(config).await?;
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;

    if let Some(existing) = sqlx::query_as::<_, RunnerJobState>(
        "SELECT id, project_id, goal_branch_id, session_id, workspace_id, lease_id, \
                client_request_id, request_hash, status, spec, spec_hash, runtime_digest, \
                candidate_commit \
         FROM runner_jobs WHERE project_id = $1 AND client_request_id = $2",
    )
    .bind(project_id)
    .bind(request.client_request_id)
    .fetch_optional(&mut *transaction)
    .await?
    {
        if existing.request_hash != identity.input_hash {
            return Err(AppError::conflict(
                "idempotency_conflict",
                "同一 clientRequestId 已用于不同 RunnerJob 输入",
            ));
        }
        let lease: LeaseState = load_lease(&mut transaction, existing.lease_id).await?;
        let spec: RunnerJobSpec = serde_json::from_value(existing.spec.0).map_err(|_| {
            AppError::conflict("runner_spec_corrupt", "已保存 RunnerJob spec 无法解析")
        })?;
        transaction.commit().await?;
        return Ok(PrepareRunnerJobResponse {
            replayed: true,
            job_id: existing.id,
            lease_id: existing.lease_id,
            lease_token: None,
            fencing_token: lease.fencing_token,
            output_key: lease.output_key,
            spec,
            spec_hash: existing.spec_hash,
        });
    }

    let (goal_branch_id, session_status, head_session_id): (Uuid, String, Uuid) = sqlx::query_as(
        "SELECT s.goal_branch_id, s.status, b.head_session_id \
             FROM goal_sessions s JOIN goal_branches b ON b.id = s.goal_branch_id \
             WHERE s.id = $1 AND s.project_id = $2 FOR UPDATE OF s, b",
    )
    .bind(session_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| AppError::not_found("Agent Session 不存在"))?;
    if session_status != "running" || head_session_id != session_id {
        return Err(AppError::conflict(
            "session_not_writable",
            "只有目标枝干当前 running Session 可以请求 RunnerJob",
        ));
    }
    let mut workspace = sqlx::query_as::<_, GoalWorkspaceRecord>(
        "SELECT * FROM goal_workspaces \
         WHERE project_id = $1 AND goal_branch_id = $2 FOR UPDATE",
    )
    .bind(project_id)
    .bind(goal_branch_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| AppError::conflict("workspace_missing", "GoalBranch 尚无真实 worktree"))?;
    if workspace.status != "ready" || workspace.dirty {
        return Err(AppError::conflict(
            "workspace_not_ready",
            "worktree 不是可执行的干净 ready 状态",
        ));
    }

    let expired_lease: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, session_id FROM workspace_write_leases \
         WHERE workspace_id = $1 AND status = 'active' AND hard_expires_at <= now() \
         FOR UPDATE",
    )
    .bind(workspace.id)
    .fetch_optional(&mut *transaction)
    .await?;
    if let Some((lease_id, expired_session_id)) = expired_lease {
        sqlx::query(
            "UPDATE runner_jobs SET status = 'timed_out', completed_at = now(), \
             result = COALESCE(result, $1) \
             WHERE lease_id = $2 AND status IN ('prepared', 'running')",
        )
        .bind(Json(
            json!({ "reason": "lease hard expiry observed before next acquire" }),
        ))
        .bind(lease_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE workspace_write_leases SET status = 'expired', completed_at = now() \
             WHERE id = $1",
        )
        .bind(lease_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE goal_sessions SET status = 'exception_paused', updated_at = now() \
             WHERE id = $1 AND status = 'running'",
        )
        .bind(expired_session_id)
        .execute(&mut *transaction)
        .await?;
        insert_exception_attention(
            &mut transaction,
            project_id,
            goal_branch_id,
            expired_session_id,
            &format!("workspace:{}:lease-expired:{lease_id}", workspace.id),
            "Workspace 写 Lease 已过硬到期时间",
            "旧 Worker 没有在硬到期前完成；其 fencing token 已失效",
            "worktree 仍停在 Lease 的固定基线，未接受迟到输出",
            "系统在下一次抢占写权时发现过期 Lease 并封存",
            "迟到 Worker 可能仍尝试回写，但 fencing token 会拒绝它",
            "检查旧 Job 状态后显式恢复 Session，再创建新 Job",
        )
        .await?;
        transaction.commit().await?;
        return Err(AppError::conflict(
            "previous_lease_expired",
            "发现过期写 Lease，Session 已安全暂停；处理后再恢复",
        ));
    }
    let active_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM workspace_write_leases \
         WHERE workspace_id = $1 AND status = 'active')",
    )
    .bind(workspace.id)
    .fetch_one(&mut *transaction)
    .await?;
    if active_exists {
        return Err(AppError::conflict(
            "workspace_writer_exists",
            "该目标 worktree 已有一个 active 写 Lease；并行必须先拆子目标",
        ));
    }
    let policy_record = sqlx::query_as::<_, WorkspacePolicyRecord>(
        "SELECT * FROM goal_workspace_policies \
         WHERE project_id = $1 AND goal_branch_id = $2",
    )
    .bind(project_id)
    .bind(goal_branch_id)
    .fetch_one(&mut *transaction)
    .await?;
    let policy = serde_json::from_value::<WorkspaceCapabilityPolicy>(policy_record.policy.0)
        .map_err(|_| AppError::conflict("workspace_policy_corrupt", "WorkspacePolicy 无法解析"))?
        .normalize()?;
    policy.authorize(
        &request.capabilities,
        &request.allowed_writes,
        &request.resources,
    )?;
    if request.capabilities.network != "denied"
        || !request.capabilities.external_writes.is_empty()
        || !request.capabilities.account_references.is_empty()
        || request.capabilities.paid_operations
        || request.capabilities.deployment
    {
        return Err(AppError::forbidden(
            "runner_adapter_unavailable",
            "BP-04 Worker 只支持可实测的完全隔离能力；声明联网或外部副作用不能自动降级执行",
        ));
    }
    let repository: GitRepositoryRecord =
        sqlx::query_as("SELECT * FROM project_git_repositories WHERE id = $1 AND project_id = $2")
            .bind(workspace.repository_id)
            .bind(project_id)
            .fetch_one(&mut *transaction)
            .await?;
    let repository_path = managed_path(&roots.repositories, &repository.storage_key, false)?;
    let worktree_path = managed_path(&roots.worktrees, &workspace.worktree_key, false)?;
    let branch_name = workspace.git_branch_name.clone();
    let inspection_result = tokio::task::spawn_blocking(move || {
        inspect_managed_worktree(&repository_path, &worktree_path, &branch_name)
    })
    .await;
    let inspection = match inspection_result {
        Ok(Ok(inspection)) => inspection,
        Ok(Err(error)) => {
            let summary = format!("无法验证托管 Git/worktree 身份或现场：{error}");
            sqlx::query(
                "UPDATE goal_workspaces SET dirty = true, status = 'error', \
                 last_error_code = 'workspace_inspection_failed', last_error_summary = $1, \
                 updated_at = now() WHERE id = $2",
            )
            .bind(&summary)
            .bind(workspace.id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "UPDATE goal_sessions SET status = 'exception_paused', updated_at = now() \
                 WHERE id = $1 AND status = 'running'",
            )
            .bind(session_id)
            .execute(&mut *transaction)
            .await?;
            insert_exception_attention(
                &mut transaction,
                project_id,
                goal_branch_id,
                session_id,
                &format!("workspace:{}:inspection-failed", workspace.id),
                "Workspace Git 身份无法验证",
                &summary,
                "没有发放写 Lease，也没有启动 Worker",
                "已核验 .git、托管 common directory、枝干名与磁盘现场",
                "继续执行可能写入错误仓库或覆盖未审计现场",
                "恢复准确托管 Git 身份并完成 reconcile 后再显式恢复",
            )
            .await?;
            transaction.commit().await?;
            return Err(AppError::conflict(
                "workspace_inspection_failed",
                "worktree 身份无法验证，Session 已安全暂停",
            ));
        }
        Err(_) => {
            return Err(AppError::internal("检查 Runner 基线的阻塞任务异常结束"));
        }
    };
    if inspection.dirty
        || workspace.head_commit.as_deref() != Some(&inspection.head_commit)
        || workspace.workspace_snapshot.as_deref() != Some(&inspection.workspace_snapshot)
        || request.base_workspace_snapshot != inspection.workspace_snapshot
    {
        workspace.dirty = inspection.dirty;
        sqlx::query(
            "UPDATE goal_workspaces SET dirty = $1, status = 'error', \
             last_error_code = 'workspace_baseline_mismatch', \
             last_error_summary = 'Git 实测基线与数据库/请求不一致', updated_at = now() \
             WHERE id = $2",
        )
        .bind(inspection.dirty)
        .bind(workspace.id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE goal_sessions SET status = 'exception_paused', updated_at = now() \
             WHERE id = $1 AND status = 'running'",
        )
        .bind(session_id)
        .execute(&mut *transaction)
        .await?;
        insert_exception_attention(
            &mut transaction,
            project_id,
            goal_branch_id,
            session_id,
            &format!("workspace:{}:baseline-mismatch", workspace.id),
            "Workspace 基线发生未审计变化",
            "Git HEAD、脏状态、数据库 snapshot 或 Runner 请求基线不一致",
            "没有发放写 Lease，也没有启动 Worker",
            "重新读取了真实 Git HEAD/tree/status 并封存冲突",
            "继续执行会把未知宿主修改与 Agent 输出混合",
            "审查外部修改，形成新安全快照或恢复准确基线后再继续",
        )
        .await?;
        transaction.commit().await?;
        return Err(AppError::conflict(
            "workspace_baseline_mismatch",
            "Git worktree 与固定基线不一致，Session 已安全暂停",
        ));
    }

    let job_id = Uuid::new_v4();
    let lease_id = Uuid::new_v4();
    let lease_token = format!(
        "lease_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let token_digest = sha256_text(&lease_token);
    let output_key = format!("jobs/{job_id}");
    let output_path = managed_path(&roots.runner_outputs, &output_key, true)?;
    if output_path.exists() {
        return Err(AppError::conflict(
            "runner_output_collision",
            "新 RunnerJob 的确定性输出目录已存在",
        ));
    }
    tokio::fs::create_dir_all(&output_path).await?;
    let metadata = tokio::fs::symlink_metadata(&output_path).await?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::conflict(
            "runner_output_unsafe",
            "Runner 输出层不是安全目录",
        ));
    }
    let fencing_token: i64 = sqlx::query_scalar(
        "UPDATE goal_workspaces SET fencing_counter = fencing_counter + 1, updated_at = now() \
         WHERE id = $1 RETURNING fencing_counter",
    )
    .bind(workspace.id)
    .fetch_one(&mut *transaction)
    .await?;
    let spec = RunnerJobSpec {
        schema_version: 1,
        job_id,
        lease_id,
        project_id,
        goal_branch_id,
        session_id,
        fencing_token,
        base_commit: inspection.head_commit.clone(),
        base_workspace_snapshot: inspection.workspace_snapshot.clone(),
        runtime_digest: config.runner_runtime_digest.clone(),
        input_mount: "/workspace/input".to_owned(),
        output_mount: "/workspace/output".to_owned(),
        result_mount: "/workspace/result".to_owned(),
        allowed_writes: request.allowed_writes.clone(),
        delete_paths: request.delete_paths.clone(),
        capabilities: request.capabilities.clone(),
        resources: request.resources.clone(),
        command: request.command.clone(),
    };
    let spec_hash = spec
        .digest()
        .map_err(|_| AppError::internal("无法计算 RunnerJobSpec 摘要"))?;
    let now = Utc::now();
    let timeout = i64::from(request.resources.timeout_seconds);
    let soft_expires_at = now + Duration::seconds(timeout + 60);
    let hard_expires_at = now + Duration::seconds(timeout + 300);
    sqlx::query(
        "INSERT INTO workspace_write_leases \
         (id, project_id, goal_branch_id, session_id, workspace_id, client_request_id, \
          request_hash, status, fencing_token, renewal_token_digest, base_commit, \
          base_workspace_snapshot, allowed_writes, delete_paths, capabilities, resource_policy, \
          output_key, soft_expires_at, hard_expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'active', $8, $9, $10, $11, \
                 $12, $13, $14, $15, $16, $17, $18)",
    )
    .bind(lease_id)
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(session_id)
    .bind(workspace.id)
    .bind(request.client_request_id)
    .bind(&identity.input_hash)
    .bind(fencing_token)
    .bind(token_digest)
    .bind(&inspection.head_commit)
    .bind(&inspection.workspace_snapshot)
    .bind(Json(&request.allowed_writes))
    .bind(Json(&request.delete_paths))
    .bind(Json(serde_json::to_value(&request.capabilities)?))
    .bind(Json(serde_json::to_value(&request.resources)?))
    .bind(&output_key)
    .bind(soft_expires_at)
    .bind(hard_expires_at)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO runner_jobs \
         (id, project_id, goal_branch_id, session_id, workspace_id, lease_id, \
          client_request_id, request_hash, status, spec, spec_hash, runtime_digest) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'prepared', $9, $10, $11)",
    )
    .bind(job_id)
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(session_id)
    .bind(workspace.id)
    .bind(lease_id)
    .bind(request.client_request_id)
    .bind(&identity.input_hash)
    .bind(Json(serde_json::to_value(&spec)?))
    .bind(&spec_hash)
    .bind(&config.runner_runtime_digest)
    .execute(&mut *transaction)
    .await?;
    insert_workspace_event(
        &mut transaction,
        project_id,
        "tool",
        job_id,
        "runner_job.prepared",
        request.client_request_id,
        json!({
            "goalBranchId": goal_branch_id,
            "sessionId": session_id,
            "workspaceId": workspace.id,
            "leaseId": lease_id,
            "fencingToken": fencing_token,
            "baseCommit": inspection.head_commit,
            "baseWorkspaceSnapshot": inspection.workspace_snapshot,
            "specHash": spec_hash,
            "runtimeDigest": config.runner_runtime_digest,
            "repositoryKey": repository.storage_key,
            "worktreeKey": workspace.worktree_key,
            "outputKey": output_key,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(PrepareRunnerJobResponse {
        replayed: false,
        job_id,
        lease_id,
        lease_token: Some(lease_token),
        fencing_token,
        output_key,
        spec,
        spec_hash,
    })
}

pub async fn finalize_runner_job(
    pool: &PgPool,
    config: &Config,
    project_id: Uuid,
    session_id: Uuid,
    job_id: Uuid,
    request: FinalizeRunnerJobRequest,
) -> AppResult<RunnerJobOutcome> {
    let roots = ManagedRoots::from_config(config).await?;
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    let job = load_runner_job(&mut transaction, project_id, session_id, job_id).await?;
    let lease = load_lease(&mut transaction, job.lease_id).await?;
    validate_lease_token(&lease, &request.lease_token)?;
    if is_terminal_job(&job.status) {
        let outcome = current_job_outcome(&mut transaction, &job, true).await?;
        transaction.commit().await?;
        return Ok(outcome);
    }
    if lease.status != "active"
        || lease.fencing_token != request.result.fencing_token
        || lease.hard_expires_at <= Utc::now()
    {
        return Err(AppError::conflict(
            "stale_fencing_token",
            "RunnerJob 的 Lease 已失效、过期或 fencing token 不匹配",
        ));
    }
    let workspace: GoalWorkspaceRecord =
        sqlx::query_as("SELECT * FROM goal_workspaces WHERE id = $1 FOR UPDATE")
            .bind(job.workspace_id)
            .fetch_one(&mut *transaction)
            .await?;
    if workspace.fencing_counter != lease.fencing_token {
        return Err(AppError::conflict(
            "stale_fencing_token",
            "已有更新的写 Lease，迟到 Worker 不能回写",
        ));
    }
    let spec: RunnerJobSpec = serde_json::from_value(job.spec.0.clone())
        .map_err(|_| AppError::conflict("runner_spec_corrupt", "RunnerJob spec 无法解析"))?;
    validate_runner_result(&spec, &job, &lease, &request.result)?;
    if request.result.status != "succeeded" {
        let status = match request.result.status.as_str() {
            "timed_out" => "timed_out",
            "policy_denied" => "policy_denied",
            _ => "failed",
        };
        let summary = format!(
            "隔离 Worker 返回 {}；exit={:?}",
            request.result.status, request.result.exit_code
        );
        let result_value = serde_json::to_value(&request.result)?;
        complete_runner_failure_in_transaction(
            &mut transaction,
            &job,
            &lease,
            status,
            &summary,
            Some(result_value),
        )
        .await?;
        let outcome = current_job_outcome(&mut transaction, &job, false).await?;
        transaction.commit().await?;
        return Ok(outcome);
    }
    let output_path = managed_path(&roots.runner_outputs, &lease.output_key, false)?;
    let output_files = tokio::task::spawn_blocking(move || scan_runner_output(&output_path))
        .await
        .map_err(|_| AppError::internal("扫描 Runner 输出层的阻塞任务异常结束"))?
        .map_err(|error| AppError::bad_request("unsafe_runner_output", error))?;
    validate_output_manifest(&spec, &request.result, &output_files)?;

    let result_value = serde_json::to_value(&request.result)?;
    let operation_id = if job.status == "applying" {
        sqlx::query_scalar(
            "SELECT id FROM workspace_operations WHERE runner_job_id = $1 \
             AND operation_kind = 'apply' AND status IN ('planned', 'applying')",
        )
        .bind(job.id)
        .fetch_one(&mut *transaction)
        .await?
    } else {
        if job.status != "prepared" && job.status != "running" {
            return Err(AppError::conflict(
                "runner_job_not_finalizable",
                "RunnerJob 当前状态不能进入回写",
            ));
        }
        let operation_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workspace_operations \
             (id, project_id, goal_branch_id, workspace_id, runner_job_id, operation_kind, \
              status, request_hash, expected_head_commit, detail) \
             VALUES ($1, $2, $3, $4, $5, 'apply', 'planned', $6, $7, $8)",
        )
        .bind(operation_id)
        .bind(project_id)
        .bind(job.goal_branch_id)
        .bind(job.workspace_id)
        .bind(job.id)
        .bind(canonical_json_sha256(&result_value)?)
        .bind(&lease.base_commit)
        .bind(Json(json!({
            "specHash": job.spec_hash,
            "outputManifestHash": request.result.output_manifest_hash,
            "fencingToken": lease.fencing_token,
        })))
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE runner_jobs SET status = 'applying', result = $1, \
             output_manifest_hash = $2, started_at = COALESCE(started_at, now()) \
             WHERE id = $3",
        )
        .bind(Json(result_value.clone()))
        .bind(&request.result.output_manifest_hash)
        .bind(job.id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE goal_workspaces SET status = 'applying', updated_at = now() WHERE id = $1",
        )
        .bind(job.workspace_id)
        .execute(&mut *transaction)
        .await?;
        operation_id
    };
    transaction.commit().await?;

    let repository: GitRepositoryRecord = sqlx::query_as(
        "SELECT r.* FROM project_git_repositories r \
         JOIN goal_workspaces w ON w.repository_id = r.id WHERE w.id = $1",
    )
    .bind(job.workspace_id)
    .fetch_one(pool)
    .await?;
    let repository_path = managed_path(&roots.repositories, &repository.storage_key, false)?;
    let live_worktree_path = managed_path(&roots.worktrees, &workspace.worktree_key, false)?;
    let output_path = managed_path(&roots.runner_outputs, &lease.output_key, false)?;
    let apply_key = format!("projects/{project_id}/apply/{job_id}");
    let apply_path = managed_path(&roots.worktrees, &apply_key, true)?;
    let branch_ref = format!("refs/heads/{}", workspace.git_branch_name);
    let existing_candidate = job.candidate_commit.clone();
    let base_commit = lease.base_commit.clone();
    let files_for_apply = output_files.clone();
    let deletes_for_apply = spec.delete_paths.clone();
    let repository_for_build = repository_path.clone();
    let apply_for_build = apply_path.clone();
    let output_for_build = output_path.clone();
    let job_for_build = job.id;
    let candidate = if let Some(candidate) = existing_candidate {
        candidate
    } else {
        let build_result = tokio::task::spawn_blocking(move || {
            build_candidate_commit(
                &repository_for_build,
                &apply_for_build,
                &output_for_build,
                &base_commit,
                job_for_build,
                &files_for_apply,
                &deletes_for_apply,
            )
        })
        .await
        .unwrap_or_else(|_| Err("构建候选 Git commit 的阻塞任务异常结束".to_owned()));
        let candidate = match build_result {
            Ok(candidate) => candidate,
            Err(error) => {
                let mut transaction = pool.begin().await?;
                let locked =
                    load_runner_job(&mut transaction, project_id, session_id, job_id).await?;
                let locked_lease = load_lease(&mut transaction, locked.lease_id).await?;
                validate_lease_token(&locked_lease, &request.lease_token)?;
                complete_runner_failure_in_transaction(
                    &mut transaction,
                    &locked,
                    &locked_lease,
                    "failed",
                    &error,
                    Some(result_value.clone()),
                )
                .await?;
                sqlx::query(
                    "UPDATE workspace_operations SET status = 'failed', \
                     error_code = 'candidate_commit_failed', error_summary = $1, \
                     updated_at = now(), completed_at = now() WHERE id = $2",
                )
                .bind(&error)
                .bind(operation_id)
                .execute(&mut *transaction)
                .await?;
                transaction.commit().await?;
                return Err(AppError::conflict("candidate_commit_failed", error));
            }
        };
        let mut transaction = pool.begin().await?;
        let locked = load_runner_job(&mut transaction, project_id, session_id, job_id).await?;
        let locked_lease = load_lease(&mut transaction, locked.lease_id).await?;
        validate_lease_token(&locked_lease, &request.lease_token)?;
        if locked.status != "applying"
            || locked_lease.status != "active"
            || locked_lease.fencing_token != lease.fencing_token
        {
            return Err(AppError::conflict(
                "stale_fencing_token",
                "候选 commit 形成后 Lease 已不再有效",
            ));
        }
        sqlx::query("UPDATE runner_jobs SET candidate_commit = $1 WHERE id = $2")
            .bind(&candidate)
            .bind(job_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE workspace_operations SET status = 'applying', candidate_commit = $1, \
             updated_at = now() WHERE id = $2",
        )
        .bind(&candidate)
        .bind(operation_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        candidate
    };

    let repository_for_publish = repository_path.clone();
    let live_for_publish = live_worktree_path.clone();
    let apply_for_publish = apply_path.clone();
    let branch_ref_for_publish = branch_ref.clone();
    let base_for_publish = lease.base_commit.clone();
    let candidate_for_publish = candidate.clone();
    let inspection = tokio::task::spawn_blocking(move || {
        publish_candidate_commit(
            &repository_for_publish,
            &live_for_publish,
            &apply_for_publish,
            &branch_ref_for_publish,
            &base_for_publish,
            &candidate_for_publish,
        )
    })
    .await
    .map_err(|_| AppError::internal("发布候选 Git commit 的阻塞任务异常结束"))?;
    let inspection = match inspection {
        Ok(inspection) => inspection,
        Err(error) => {
            let mut transaction = pool.begin().await?;
            let locked = load_runner_job(&mut transaction, project_id, session_id, job_id).await?;
            let locked_lease = load_lease(&mut transaction, locked.lease_id).await?;
            let terminal = if error.contains("compare-and-swap") {
                "workspace_conflict"
            } else {
                "failed"
            };
            complete_runner_failure_in_transaction(
                &mut transaction,
                &locked,
                &locked_lease,
                terminal,
                &error,
                Some(result_value),
            )
            .await?;
            sqlx::query(
                "UPDATE workspace_operations SET status = 'failed', error_code = $1, \
                 error_summary = $2, updated_at = now(), completed_at = now() WHERE id = $3",
            )
            .bind(terminal)
            .bind(&error)
            .bind(operation_id)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Err(AppError::conflict(terminal, error));
        }
    };
    if inspection.head_commit != candidate || inspection.dirty {
        return Err(AppError::conflict(
            "workspace_publish_mismatch",
            "Git compare-and-swap 后 worktree 没有落在候选干净 HEAD",
        ));
    }

    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    let locked = load_runner_job(&mut transaction, project_id, session_id, job_id).await?;
    let locked_lease = load_lease(&mut transaction, locked.lease_id).await?;
    validate_lease_token(&locked_lease, &request.lease_token)?;
    let locked_workspace: GoalWorkspaceRecord =
        sqlx::query_as("SELECT * FROM goal_workspaces WHERE id = $1 FOR UPDATE")
            .bind(locked.workspace_id)
            .fetch_one(&mut *transaction)
            .await?;
    if locked.status != "applying"
        || locked.candidate_commit.as_deref() != Some(candidate.as_str())
        || locked_lease.status != "active"
        || locked_workspace.fencing_counter != locked_lease.fencing_token
    {
        return Err(AppError::conflict(
            "stale_fencing_token",
            "Git 发布完成后数据库 fencing 状态已变化，等待 reconcile",
        ));
    }
    let parent_snapshot_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM workspace_snapshots WHERE workspace_id = $1 \
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(locked.workspace_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let snapshot_id = Uuid::new_v4();
    sqlx::query(
        "UPDATE goal_workspaces SET head_commit = $1, tree_id = $2, workspace_snapshot = $3, \
         dirty = false, status = 'ready', last_error_code = NULL, last_error_summary = NULL, \
         updated_at = now() WHERE id = $4",
    )
    .bind(&inspection.head_commit)
    .bind(&inspection.tree_id)
    .bind(&inspection.workspace_snapshot)
    .bind(locked.workspace_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query("UPDATE runner_jobs SET status = 'succeeded', completed_at = now() WHERE id = $1")
        .bind(job_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "UPDATE workspace_write_leases SET status = 'released', completed_at = now() \
         WHERE id = $1",
    )
    .bind(locked.lease_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE workspace_operations SET status = 'applied', candidate_commit = $1, \
         detail = detail || $2, updated_at = now(), completed_at = now() WHERE id = $3",
    )
    .bind(&inspection.head_commit)
    .bind(Json(json!({
        "treeId": inspection.tree_id,
        "workspaceSnapshot": inspection.workspace_snapshot,
    })))
    .bind(operation_id)
    .execute(&mut *transaction)
    .await?;
    for file in &output_files {
        sqlx::query(
            "INSERT INTO runner_job_files \
             (runner_job_id, project_id, path, sha256, size_bytes, executable) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(job_id)
        .bind(project_id)
        .bind(&file.path)
        .bind(&file.sha256)
        .bind(i64::try_from(file.size_bytes).map_err(|_| {
            AppError::bad_request("runner_output_too_large", "输出文件大小超出数据库范围")
        })?)
        .bind(file.executable)
        .execute(&mut *transaction)
        .await?;
    }
    sqlx::query(
        "INSERT INTO workspace_snapshots \
         (id, project_id, goal_branch_id, session_id, workspace_id, operation_id, runner_job_id, \
          parent_snapshot_id, head_commit, tree_id, dirty, snapshot_hash) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, false, $11)",
    )
    .bind(snapshot_id)
    .bind(project_id)
    .bind(locked.goal_branch_id)
    .bind(session_id)
    .bind(locked.workspace_id)
    .bind(operation_id)
    .bind(job_id)
    .bind(parent_snapshot_id)
    .bind(&inspection.head_commit)
    .bind(&inspection.tree_id)
    .bind(&inspection.workspace_snapshot)
    .execute(&mut *transaction)
    .await?;
    let contribution_id = Uuid::new_v4();
    let contribution_title = format!("RunnerJob {} 的 Git 变更", job_id);
    let contribution_body = serde_json::to_string_pretty(&json!({
        "schemaVersion": 1,
        "runnerJobId": job_id,
        "workspaceId": locked.workspace_id,
        "leaseId": locked.lease_id,
        "fencingToken": locked_lease.fencing_token,
        "specHash": locked.spec_hash,
        "runtimeDigest": locked.runtime_digest,
        "baseCommit": locked_lease.base_commit,
        "headCommit": inspection.head_commit,
        "treeId": inspection.tree_id,
        "workspaceSnapshot": inspection.workspace_snapshot,
        "outputManifestHash": request.result.output_manifest_hash,
        "files": output_files,
        "deletedPaths": &spec.delete_paths,
    }))?;
    let evidence_refs = json!([{
        "kind": "runner_job",
        "runnerJobId": job_id,
        "headCommit": inspection.head_commit,
        "workspaceSnapshot": inspection.workspace_snapshot,
        "outputManifestHash": request.result.output_manifest_hash,
    }]);
    let contribution_hash = canonical_json_sha256(&json!({
        "kind": "code_change",
        "title": contribution_title,
        "body": contribution_body,
        "artifactId": Value::Null,
        "evidenceRefs": evidence_refs,
        "evidenceIds": Vec::<Uuid>::new(),
        "supersedesId": Value::Null,
    }))?;
    sqlx::query(
        "INSERT INTO goal_contributions \
         (id, project_id, goal_branch_id, session_id, kind, title, body, \
          evidence_refs, runner_job_id, content_hash) \
         VALUES ($1, $2, $3, $4, 'code_change', $5, $6, $7, $8, $9)",
    )
    .bind(contribution_id)
    .bind(project_id)
    .bind(locked.goal_branch_id)
    .bind(session_id)
    .bind(&contribution_title)
    .bind(&contribution_body)
    .bind(Json(evidence_refs))
    .bind(job_id)
    .bind(&contribution_hash)
    .execute(&mut *transaction)
    .await?;
    insert_workspace_event(
        &mut transaction,
        project_id,
        "tool",
        job_id,
        "runner_job.applied",
        locked.client_request_id,
        json!({
            "workspaceId": locked.workspace_id,
            "leaseId": locked.lease_id,
            "fencingToken": locked_lease.fencing_token,
            "baseCommit": locked_lease.base_commit,
            "headCommit": inspection.head_commit,
            "treeId": inspection.tree_id,
            "workspaceSnapshot": inspection.workspace_snapshot,
            "outputManifestHash": request.result.output_manifest_hash,
            "fileCount": output_files.len(),
            "deleteCount": spec.delete_paths.len(),
            "contributionId": contribution_id,
            "contributionHash": contribution_hash,
        }),
    )
    .await?;
    transaction.commit().await?;
    Ok(RunnerJobOutcome {
        replayed: false,
        job_id,
        status: "succeeded".to_owned(),
        workspace_snapshot: inspection.workspace_snapshot,
        head_commit: inspection.head_commit,
        session_status: "running".to_owned(),
    })
}

pub async fn fail_runner_job(
    pool: &PgPool,
    project_id: Uuid,
    session_id: Uuid,
    job_id: Uuid,
    mut request: FailRunnerJobRequest,
) -> AppResult<RunnerJobOutcome> {
    request.failure_kind = request.failure_kind.trim().to_ascii_lowercase();
    if !matches!(
        request.failure_kind.as_str(),
        "timed_out" | "resource_exhausted" | "runner_failed" | "cancelled"
    ) {
        return Err(AppError::bad_request(
            "invalid_runner_failure",
            "未知 Runner 失败类型",
        ));
    }
    request.summary = request.summary.trim().to_owned();
    if request.summary.is_empty() || request.summary.chars().count() > 4_000 {
        return Err(AppError::bad_request(
            "invalid_runner_failure",
            "Runner 失败摘要不能为空或超过 4000 字符",
        ));
    }
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| AppError::not_found("项目不存在"))?;
    let job = load_runner_job(&mut transaction, project_id, session_id, job_id).await?;
    let lease = load_lease(&mut transaction, job.lease_id).await?;
    validate_lease_token(&lease, &request.lease_token)?;
    if is_terminal_job(&job.status) {
        let outcome = current_job_outcome(&mut transaction, &job, true).await?;
        transaction.commit().await?;
        return Ok(outcome);
    }
    if !matches!(job.status.as_str(), "prepared" | "running") || lease.status != "active" {
        return Err(AppError::conflict(
            "runner_job_not_fail_safe",
            "RunnerJob 已进入回写，不能再用 Worker 启动失败覆盖状态",
        ));
    }
    let status = match request.failure_kind.as_str() {
        "timed_out" => "timed_out",
        "cancelled" => "cancelled",
        _ => "failed",
    };
    complete_runner_failure_in_transaction(
        &mut transaction,
        &job,
        &lease,
        status,
        &request.summary,
        Some(json!({
            "failureKind": request.failure_kind,
            "summaryHash": sha256_text(&request.summary),
        })),
    )
    .await?;
    let outcome = current_job_outcome(&mut transaction, &job, false).await?;
    transaction.commit().await?;
    Ok(outcome)
}

async fn load_runner_job(
    transaction: &mut WorkspaceTransaction<'_>,
    project_id: Uuid,
    session_id: Uuid,
    job_id: Uuid,
) -> AppResult<RunnerJobState> {
    sqlx::query_as::<_, RunnerJobState>(
        "SELECT id, project_id, goal_branch_id, session_id, workspace_id, lease_id, \
                client_request_id, request_hash, status, spec, spec_hash, runtime_digest, \
                candidate_commit \
         FROM runner_jobs WHERE id = $1 AND project_id = $2 AND session_id = $3 FOR UPDATE",
    )
    .bind(job_id)
    .bind(project_id)
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("RunnerJob 不存在"))
}

async fn load_lease(
    transaction: &mut WorkspaceTransaction<'_>,
    lease_id: Uuid,
) -> AppResult<LeaseState> {
    sqlx::query_as::<_, LeaseState>(
        "SELECT id, status, fencing_token, renewal_token_digest, base_commit, \
                base_workspace_snapshot, allowed_writes, delete_paths, capabilities, \
                resource_policy, output_key, hard_expires_at \
         FROM workspace_write_leases WHERE id = $1 FOR UPDATE",
    )
    .bind(lease_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("Workspace Lease 不存在"))
}

fn validate_lease_token(lease: &LeaseState, token: &str) -> AppResult<()> {
    if token.len() != 70
        || !token.starts_with("lease_")
        || !token[6..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        || sha256_text(token) != lease.renewal_token_digest
    {
        return Err(AppError::forbidden(
            "invalid_lease_token",
            "Workspace Lease token 不匹配",
        ));
    }
    Ok(())
}

fn validate_runner_result(
    spec: &RunnerJobSpec,
    job: &RunnerJobState,
    lease: &LeaseState,
    result: &RunnerExecutionResult,
) -> AppResult<()> {
    if result.schema_version != 1
        || result.job_id != job.id
        || result.lease_id != lease.id
        || result.fencing_token != lease.fencing_token
        || result.spec_hash != job.spec_hash
        || spec
            .digest()
            .map_err(|_| AppError::internal("无法复算 RunnerJobSpec 摘要"))?
            != job.spec_hash
        || spec.runtime_digest != job.runtime_digest
        || spec.base_commit != lease.base_commit
        || spec.base_workspace_snapshot != lease.base_workspace_snapshot
    {
        return Err(AppError::conflict(
            "runner_result_identity_mismatch",
            "RunnerResult 没有绑定准确 Job、Lease、fencing token、Runtime 或基线",
        ));
    }
    let lease_capabilities: RunnerCapabilities =
        serde_json::from_value(lease.capabilities.0.clone())
            .map_err(|_| AppError::conflict("workspace_lease_corrupt", "Lease 能力快照无法解析"))?;
    let lease_resources: RunnerResourceLimits =
        serde_json::from_value(lease.resource_policy.0.clone())
            .map_err(|_| AppError::conflict("workspace_lease_corrupt", "Lease 资源快照无法解析"))?;
    if spec.allowed_writes != lease.allowed_writes.0
        || spec.delete_paths != lease.delete_paths.0
        || spec.capabilities != lease_capabilities
        || spec.resources != lease_resources
    {
        return Err(AppError::conflict(
            "runner_spec_lease_mismatch",
            "RunnerJobSpec 与不可变 Lease 权限/资源快照不一致",
        ));
    }
    if !matches!(
        result.status.as_str(),
        "succeeded" | "failed" | "timed_out" | "policy_denied"
    ) {
        return Err(AppError::bad_request(
            "invalid_runner_result",
            "RunnerResult 状态不合法",
        ));
    }
    if result.status == "succeeded" && result.exit_code != Some(0) {
        return Err(AppError::bad_request(
            "invalid_runner_result",
            "成功 RunnerResult 必须具有零退出码",
        ));
    }
    let mut sorted = result.files.clone();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    if sorted != result.files || sorted.windows(2).any(|pair| pair[0].path == pair[1].path) {
        return Err(AppError::bad_request(
            "invalid_runner_manifest",
            "Runner 输出清单必须按路径稳定排序且不能重复",
        ));
    }
    let mut case_folded_paths = BTreeSet::new();
    let delete_paths = spec
        .delete_paths
        .iter()
        .map(|path| path.to_lowercase())
        .collect::<BTreeSet<_>>();
    for file in &result.files {
        normalize_relative_file_path(&file.path)?;
        if !case_folded_paths.insert(file.path.to_lowercase()) {
            return Err(AppError::bad_request(
                "workspace_case_collision",
                "Runner 输出包含在大小写不敏感文件系统中冲突的路径",
            ));
        }
        if delete_paths.contains(&file.path.to_lowercase()) {
            return Err(AppError::bad_request(
                "runner_output_delete_collision",
                "同一路径不能在一个 RunnerJob 中同时输出和删除",
            ));
        }
        validate_prefixed_sha256("Runner 输出摘要", &file.sha256)?;
    }
    let manifest_hash = runner_canonical_json_sha256(&result.files)
        .map_err(|_| AppError::internal("无法复算 Runner 输出清单摘要"))?;
    if manifest_hash != result.output_manifest_hash {
        return Err(AppError::conflict(
            "runner_manifest_hash_mismatch",
            "Runner 输出清单与摘要不一致",
        ));
    }
    validate_prefixed_sha256("Runner stdout 摘要", &result.stdout_sha256)?;
    validate_prefixed_sha256("Runner stderr 摘要", &result.stderr_sha256)?;
    if result.stdout_bytes > spec.resources.stdout_bytes
        || result.stderr_bytes > spec.resources.stderr_bytes
    {
        return Err(AppError::bad_request(
            "runner_log_limit_exceeded",
            "Runner 日志超过固定字节上限",
        ));
    }
    if result.status != "policy_denied" {
        let isolation = &result.isolation;
        if isolation.runtime_digest != job.runtime_digest
            || !isolation.network_isolated
            || isolation
                .visible_network_interfaces
                .iter()
                .any(|name| name != "lo")
            || !isolation.no_new_privileges
            || !isolation
                .effective_capabilities_hex
                .chars()
                .all(|character| character == '0')
            || !isolation.root_read_only
            || !isolation.input_read_only
            || !isolation.output_writable
            || !isolation.docker_socket_absent
            || !isolation.host_home_absent
            || isolation
                .observed_cpu_millis
                .is_none_or(|value| value > spec.resources.cpu_millis)
            || isolation
                .observed_memory_mi_b
                .is_none_or(|value| value > spec.resources.memory_mi_b)
            || isolation
                .observed_pids
                .is_none_or(|value| value > spec.resources.pids)
        {
            return Err(AppError::forbidden(
                "unsafe_runner_attestation",
                "Worker 隔离证明不满足断网、无能力、只读输入/根文件系统或 cgroup 上限",
            ));
        }
    }
    Ok(())
}

fn validate_output_manifest(
    spec: &RunnerJobSpec,
    result: &RunnerExecutionResult,
    observed: &[RunnerOutputFile],
) -> AppResult<()> {
    if observed != result.files {
        return Err(AppError::conflict(
            "runner_output_mismatch",
            "共享输出层中的文件、权限、大小或摘要与 RunnerResult 不一致",
        ));
    }
    let total = observed
        .iter()
        .try_fold(0_u64, |sum, file| sum.checked_add(file.size_bytes))
        .ok_or_else(|| AppError::bad_request("runner_output_too_large", "Runner 输出大小溢出"))?;
    if total > u64::from(spec.resources.disk_mi_b) * 1024 * 1024 {
        return Err(AppError::bad_request(
            "runner_output_too_large",
            "Runner 输出超过固定磁盘上限",
        ));
    }
    for file in observed {
        if !spec
            .allowed_writes
            .iter()
            .any(|pattern| path_matches_pattern(pattern, &file.path))
        {
            return Err(AppError::forbidden(
                "workspace_write_denied",
                format!("Runner 输出路径 {} 未获授权", file.path),
            ));
        }
    }
    Ok(())
}

fn scan_runner_output(root: &Path) -> Result<Vec<RunnerOutputFile>, String> {
    if fs::symlink_metadata(root)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Runner 输出根目录不能是符号链接".to_owned());
    }
    let mut files = Vec::new();
    scan_runner_output_directory(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn scan_runner_output_directory(
    root: &Path,
    directory: &Path,
    files: &mut Vec<RunnerOutputFile>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err("Runner 输出中含有符号链接".to_owned());
        }
        if metadata.is_dir() {
            scan_runner_output_directory(root, &path, files)?;
            continue;
        }
        if !metadata.is_file() {
            return Err("Runner 输出中含有非普通文件".to_owned());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "Runner 输出路径逃逸根目录".to_owned())?;
        let relative = relative
            .components()
            .map(|component| match component {
                Component::Normal(value) => value
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "Runner 输出路径不是 UTF-8".to_owned()),
                _ => Err("Runner 输出路径不是规范相对路径".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()?
            .join("/");
        normalize_relative_file_path(&relative).map_err(|error| error.public_message())?;
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        files.push(RunnerOutputFile {
            path: relative,
            sha256: format!("sha256:{}", hex::encode(Sha256::digest(&bytes))),
            size_bytes: bytes.len() as u64,
            executable: metadata.permissions().mode() & 0o111 != 0,
        });
    }
    Ok(())
}

fn build_candidate_commit(
    repository_path: &Path,
    apply_path: &Path,
    output_path: &Path,
    base_commit: &str,
    job_id: Uuid,
    files: &[RunnerOutputFile],
    delete_paths: &[String],
) -> Result<String, String> {
    validate_git_oid(base_commit)?;
    if apply_path.exists() {
        let current = inspect_worktree(apply_path);
        if let Ok(current) = current
            && current.head_commit != base_commit
            && !current.dirty
        {
            let message = run_git([
                OsStr::new("-C"),
                apply_path.as_os_str(),
                OsStr::new("show"),
                OsStr::new("-s"),
                OsStr::new("--format=%B"),
                OsStr::new("HEAD"),
            ])?;
            if message.contains(&format!("Fudian-Runner-Job: {job_id}")) {
                return Ok(current.head_commit);
            }
        }
        run_git([
            OsStr::new("--git-dir"),
            repository_path.as_os_str(),
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            apply_path.as_os_str(),
        ])?;
    }
    if let Some(parent) = apply_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    run_git([
        OsStr::new("--git-dir"),
        repository_path.as_os_str(),
        OsStr::new("worktree"),
        OsStr::new("add"),
        OsStr::new("--detach"),
        apply_path.as_os_str(),
        OsStr::new(base_commit),
    ])?;
    for file in files {
        let relative =
            normalize_relative_file_path(&file.path).map_err(|error| error.public_message())?;
        let source = output_path.join(&relative);
        let destination = apply_path.join(&relative);
        if let Some(parent) = destination.parent() {
            create_directories_without_symlinks(apply_path, parent)?;
        }
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                fs::remove_file(&destination).map_err(|error| error.to_string())?;
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!("候选输出路径 {relative} 不是普通文件"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        fs::copy(&source, &destination).map_err(|error| error.to_string())?;
        let mode = if file.executable { 0o755 } else { 0o644 };
        fs::set_permissions(&destination, fs::Permissions::from_mode(mode))
            .map_err(|error| error.to_string())?;
    }
    for delete_path in delete_paths {
        let relative =
            normalize_relative_file_path(delete_path).map_err(|error| error.public_message())?;
        let destination = apply_path.join(&relative);
        let parent = destination
            .parent()
            .ok_or_else(|| "删除路径缺少 worktree 父目录".to_owned())?;
        assert_no_symlink_below(apply_path, parent)?;
        run_git_owned(&[
            OsString::from("-C"),
            apply_path.as_os_str().to_os_string(),
            OsString::from("ls-files"),
            OsString::from("--error-unmatch"),
            OsString::from("--"),
            OsString::from(&relative),
        ])
        .map_err(|_| format!("删除路径 {relative} 不是基线中的受跟踪文件"))?;
        let metadata = fs::symlink_metadata(&destination)
            .map_err(|_| format!("删除路径 {relative} 在基线 worktree 中不存在"))?;
        if !metadata.is_file() && !metadata.file_type().is_symlink() {
            return Err(format!("删除路径 {relative} 不是普通文件或符号链接"));
        }
        fs::remove_file(&destination).map_err(|error| format!("无法删除 {relative}: {error}"))?;
    }
    let mut add_args = vec![
        OsString::from("-C"),
        apply_path.as_os_str().to_os_string(),
        OsString::from("add"),
        OsString::from("--all"),
        OsString::from("--"),
    ];
    add_args.extend(files.iter().map(|file| OsString::from(&file.path)));
    add_args.extend(delete_paths.iter().map(OsString::from));
    run_git_owned(&add_args)?;
    let diff_status = run_git_exit(&[
        OsString::from("-C"),
        apply_path.as_os_str().to_os_string(),
        OsString::from("diff"),
        OsString::from("--cached"),
        OsString::from("--quiet"),
        OsString::from("--exit-code"),
    ])?;
    if diff_status == 0 {
        return Ok(base_commit.to_owned());
    }
    if diff_status != 1 {
        return Err("Git 无法判断候选变更".to_owned());
    }
    run_git_authored(&[
        OsString::from("-C"),
        apply_path.as_os_str().to_os_string(),
        OsString::from("commit"),
        OsString::from("--no-gpg-sign"),
        OsString::from("-m"),
        OsString::from(format!("fudian: apply isolated runner job {job_id}")),
        OsString::from("-m"),
        OsString::from(format!("Fudian-Runner-Job: {job_id}")),
    ])?;
    let candidate = run_git([
        OsStr::new("-C"),
        apply_path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("HEAD"),
    ])?;
    validate_git_oid(&candidate)?;
    Ok(candidate)
}

fn create_directories_without_symlinks(root: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "候选输出父目录逃逸 apply worktree".to_owned())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err("候选输出父目录不规范".to_owned());
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err("apply worktree 的输出父路径不是安全目录".to_owned());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| error.to_string())?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

fn publish_candidate_commit(
    repository_path: &Path,
    live_worktree_path: &Path,
    apply_path: &Path,
    branch_ref: &str,
    base_commit: &str,
    candidate_commit: &str,
) -> Result<GitWorkspaceInspection, String> {
    validate_git_oid(base_commit)?;
    validate_git_oid(candidate_commit)?;
    let branch_name = branch_ref
        .strip_prefix("refs/heads/")
        .ok_or_else(|| "Git branch ref 不是托管 heads 引用".to_owned())?;
    let current_ref = run_git([
        OsStr::new("--git-dir"),
        repository_path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("--verify"),
        OsStr::new(branch_ref),
    ])?;
    let needs_reset = if current_ref == base_commit {
        let before = inspect_managed_worktree(repository_path, live_worktree_path, branch_name)?;
        if before.head_commit != base_commit || before.dirty {
            return Err(
                "Git compare-and-swap conflict: live worktree no longer matches the clean base"
                    .to_owned(),
            );
        }
        if candidate_commit != base_commit {
            run_git([
                OsStr::new("--git-dir"),
                repository_path.as_os_str(),
                OsStr::new("update-ref"),
                OsStr::new(branch_ref),
                OsStr::new(candidate_commit),
                OsStr::new(base_commit),
            ])
            .map_err(|error| format!("Git compare-and-swap failed: {error}"))?;
        }
        candidate_commit != base_commit
    } else if current_ref == candidate_commit {
        let observed = inspect_managed_worktree(repository_path, live_worktree_path, branch_name)?;
        if !observed.dirty && observed.head_commit == candidate_commit {
            false
        } else if worktree_contents_match_commit(live_worktree_path, base_commit)? {
            true
        } else {
            return Err(
                "Git compare-and-swap conflict: recovery worktree matches neither base nor candidate"
                    .to_owned(),
            );
        }
    } else {
        return Err(format!(
            "Git compare-and-swap conflict: expected {base_commit}, observed {current_ref}"
        ));
    };
    if needs_reset {
        validate_managed_worktree_identity(repository_path, live_worktree_path, branch_name)?;
        if !worktree_contents_match_commit(live_worktree_path, base_commit)? {
            return Err(
                "Git compare-and-swap conflict: live files changed outside the active Lease"
                    .to_owned(),
            );
        }
        run_git([
            OsStr::new("-C"),
            live_worktree_path.as_os_str(),
            OsStr::new("reset"),
            OsStr::new("--hard"),
            OsStr::new(candidate_commit),
        ])?;
    }
    let inspection = inspect_managed_worktree(repository_path, live_worktree_path, branch_name)?;
    if apply_path.exists() {
        let _ = run_git([
            OsStr::new("--git-dir"),
            repository_path.as_os_str(),
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            apply_path.as_os_str(),
        ]);
    }
    Ok(inspection)
}

fn assert_no_symlink_below(root: &Path, target: &Path) -> Result<(), String> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| "候选输出路径逃逸 apply worktree".to_owned())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err("候选输出路径不规范".to_owned());
        };
        current.push(component);
        if fs::symlink_metadata(&current)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("apply worktree 目标路径含符号链接".to_owned());
        }
    }
    Ok(())
}

async fn complete_runner_failure_in_transaction(
    transaction: &mut WorkspaceTransaction<'_>,
    job: &RunnerJobState,
    lease: &LeaseState,
    status: &str,
    summary: &str,
    result: Option<Value>,
) -> AppResult<()> {
    let terminal = if job.status == "applying" && status != "workspace_conflict" {
        "failed"
    } else {
        status
    };
    sqlx::query(
        "UPDATE runner_jobs SET status = $1, result = COALESCE($2, result), completed_at = now() \
         WHERE id = $3",
    )
    .bind(terminal)
    .bind(result.map(Json))
    .bind(job.id)
    .execute(&mut **transaction)
    .await?;
    let lease_status = if terminal == "cancelled" {
        "cancelled"
    } else if terminal == "timed_out" {
        "expired"
    } else {
        "failed"
    };
    sqlx::query(
        "UPDATE workspace_write_leases SET status = $1, completed_at = now() WHERE id = $2",
    )
    .bind(lease_status)
    .bind(lease.id)
    .execute(&mut **transaction)
    .await?;
    if terminal == "workspace_conflict" {
        sqlx::query(
            "UPDATE goal_workspaces SET status = 'error', \
             last_error_code = 'workspace_conflict', last_error_summary = $1, \
             updated_at = now() WHERE id = $2",
        )
        .bind(summary)
        .bind(job.workspace_id)
        .execute(&mut **transaction)
        .await?;
    } else {
        sqlx::query(
            "UPDATE goal_workspaces SET status = 'ready', updated_at = now() \
             WHERE id = $1 AND status = 'applying'",
        )
        .bind(job.workspace_id)
        .execute(&mut **transaction)
        .await?;
    }
    sqlx::query(
        "UPDATE goal_sessions SET status = 'exception_paused', updated_at = now() \
         WHERE id = $1 AND status = 'running'",
    )
    .bind(job.session_id)
    .execute(&mut **transaction)
    .await?;
    insert_exception_attention(
        transaction,
        job.project_id,
        job.goal_branch_id,
        job.session_id,
        &format!("runner-job:{}:failure", job.id),
        "隔离 RunnerJob 未能安全完成",
        summary,
        if terminal == "workspace_conflict" {
            "数据库没有接受候选；检测到的旁路 Git/文件现场原样保留，等待 reconcile"
        } else {
            "worktree 保持在固定基线；失败输出没有进入 Git branch"
        },
        "Worker 结果和摘要已记录；可变输出层保留供只读诊断",
        "重试前需要判断失败是否来自资源、插件、恶意输出或环境",
        "检查诊断后显式恢复同一个 Session；需要更大资源时先修订契约/权限",
    )
    .await?;
    insert_workspace_event(
        transaction,
        job.project_id,
        "tool",
        job.id,
        "runner_job.failed",
        job.client_request_id,
        json!({
            "status": terminal,
            "summaryHash": sha256_text(summary),
            "workspaceId": job.workspace_id,
            "leaseId": lease.id,
            "fencingToken": lease.fencing_token,
            "baseCommit": lease.base_commit,
        }),
    )
    .await?;
    Ok(())
}

async fn current_job_outcome(
    transaction: &mut WorkspaceTransaction<'_>,
    job: &RunnerJobState,
    replayed: bool,
) -> AppResult<RunnerJobOutcome> {
    let (status, workspace_snapshot, head_commit, session_status): (
        String,
        Option<String>,
        Option<String>,
        String,
    ) = sqlx::query_as(
        "SELECT j.status, w.workspace_snapshot, w.head_commit, s.status \
         FROM runner_jobs j JOIN goal_workspaces w ON w.id = j.workspace_id \
         JOIN goal_sessions s ON s.id = j.session_id WHERE j.id = $1",
    )
    .bind(job.id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(RunnerJobOutcome {
        replayed,
        job_id: job.id,
        status,
        workspace_snapshot: workspace_snapshot.ok_or_else(|| {
            AppError::conflict("workspace_snapshot_missing", "GoalWorkspace 缺少 snapshot")
        })?,
        head_commit: head_commit.ok_or_else(|| {
            AppError::conflict("workspace_head_missing", "GoalWorkspace 缺少 HEAD")
        })?,
        session_status,
    })
}

fn is_terminal_job(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "failed" | "timed_out" | "policy_denied" | "workspace_conflict" | "cancelled"
    )
}

fn run_git_owned(args: &[OsString]) -> Result<String, String> {
    run_git(args.iter().map(OsString::as_os_str))
}

fn run_git_authored(args: &[OsString]) -> Result<String, String> {
    let bytes = run_git_command(args.iter().map(OsString::as_os_str), None, true)?;
    String::from_utf8(bytes)
        .map(|value| value.trim().to_owned())
        .map_err(|_| "Git 输出不是 UTF-8".to_owned())
}

fn run_git_exit(args: &[OsString]) -> Result<i32, String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("commit.gpgSign=false")
        .arg("-c")
        .arg("protocol.file.allow=never")
        .args(args)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| error.to_string())?;
    Ok(output.status.code().unwrap_or(128))
}

fn validate_prefixed_sha256(label: &str, value: &str) -> AppResult<()> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    {
        return Err(AppError::bad_request(
            "invalid_digest",
            format!("{label}不是规范 SHA-256 摘要"),
        ));
    }
    Ok(())
}

fn sha256_text(value: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(value.as_bytes())))
}

async fn verified_workspace_detail(
    pool: &PgPool,
    roots: &ManagedRoots,
    project_id: Uuid,
    goal_branch_id: Uuid,
) -> AppResult<WorkspaceDetail> {
    let workspace = sqlx::query_as::<_, GoalWorkspaceRecord>(
        "SELECT * FROM goal_workspaces WHERE project_id = $1 AND goal_branch_id = $2",
    )
    .bind(project_id)
    .bind(goal_branch_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::not_found("GoalBranch 尚未建立 worktree"))?;
    let repository = sqlx::query_as::<_, GitRepositoryRecord>(
        "SELECT * FROM project_git_repositories WHERE id = $1 AND project_id = $2",
    )
    .bind(workspace.repository_id)
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let policy = sqlx::query_as::<_, WorkspacePolicyRecord>(
        "SELECT * FROM goal_workspace_policies WHERE goal_branch_id = $1 AND project_id = $2",
    )
    .bind(goal_branch_id)
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    let latest_snapshot = sqlx::query_as::<_, WorkspaceSnapshotRecord>(
        "SELECT * FROM workspace_snapshots WHERE workspace_id = $1 \
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(workspace.id)
    .fetch_optional(pool)
    .await?;
    if workspace.status != "ready" {
        return Err(AppError::conflict(
            "workspace_not_ready",
            "GoalBranch worktree 尚未处于 ready 状态",
        ));
    }
    let path = managed_path(&roots.worktrees, &workspace.worktree_key, false)?;
    let repository_path = managed_path(&roots.repositories, &repository.storage_key, false)?;
    let branch_name = workspace.git_branch_name.clone();
    let observed = tokio::task::spawn_blocking(move || {
        inspect_managed_worktree(&repository_path, &path, &branch_name)
    })
    .await
    .map_err(|_| AppError::internal("检查 worktree 的阻塞任务异常结束"))?
    .map_err(|error| AppError::conflict("workspace_inspection_failed", error))?;
    let matches_record = workspace.head_commit.as_deref() == Some(&observed.head_commit)
        && workspace.tree_id.as_deref() == Some(&observed.tree_id)
        && workspace.workspace_snapshot.as_deref() == Some(&observed.workspace_snapshot)
        && workspace.dirty == observed.dirty;
    Ok(WorkspaceDetail {
        repository,
        workspace,
        policy,
        latest_snapshot,
        observed,
        matches_record,
    })
}

async fn verified_ready_workspace(
    pool: &PgPool,
    roots: &ManagedRoots,
    project_id: Uuid,
    goal_branch_id: Uuid,
) -> AppResult<WorkspaceDetail> {
    let detail = verified_workspace_detail(pool, roots, project_id, goal_branch_id).await?;
    if !detail.matches_record || detail.observed.dirty {
        return Err(AppError::conflict(
            "workspace_record_drifted",
            "worktree 的磁盘现场与已记录安全点不一致",
        ));
    }
    Ok(detail)
}

async fn ensure_workspace_policy(
    transaction: &mut WorkspaceTransaction<'_>,
    branch: &BranchProvisionState,
) -> AppResult<WorkspacePolicyRecord> {
    if let Some(policy) = sqlx::query_as::<_, WorkspacePolicyRecord>(
        "SELECT * FROM goal_workspace_policies WHERE goal_branch_id = $1",
    )
    .bind(branch.id)
    .fetch_optional(&mut **transaction)
    .await?
    {
        let parsed = serde_json::from_value::<WorkspaceCapabilityPolicy>(policy.policy.0.clone())
            .map_err(|_| {
                AppError::conflict("workspace_policy_corrupt", "WorkspacePolicy 无法解析")
            })?
            .normalize()?;
        let observed_hash = canonical_json_sha256(&serde_json::to_value(parsed)?)?;
        if observed_hash != policy.policy_hash {
            return Err(AppError::conflict(
                "workspace_policy_hash_mismatch",
                "WorkspacePolicy 内容与固定摘要不一致",
            ));
        }
        return Ok(policy);
    }
    let revision = branch.approved_revision.ok_or_else(|| {
        AppError::conflict(
            "workspace_policy_source_missing",
            "GoalBranch 的已批准 Proposal 缺少准确修订",
        )
    })?;
    let value: Json<Value> = sqlx::query_scalar(
        "SELECT capability_policy FROM goal_branch_proposal_revisions \
         WHERE proposal_id = $1 AND revision = $2",
    )
    .bind(branch.creating_proposal_id)
    .bind(revision)
    .fetch_one(&mut **transaction)
    .await?;
    let parsed = serde_json::from_value::<WorkspaceCapabilityPolicy>(value.0)
        .map_err(|_| {
            AppError::conflict(
                "workspace_policy_source_invalid",
                "BranchProposal 的权限策略无法解析",
            )
        })?
        .normalize()?;
    let normalized = serde_json::to_value(parsed)?;
    let hash = canonical_json_sha256(&normalized)?;
    sqlx::query_as::<_, WorkspacePolicyRecord>(
        "INSERT INTO goal_workspace_policies \
         (id, project_id, goal_branch_id, source_proposal_id, source_proposal_revision, \
          policy, policy_hash) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
    )
    .bind(Uuid::new_v4())
    .bind(branch.project_id)
    .bind(branch.id)
    .bind(branch.creating_proposal_id)
    .bind(revision)
    .bind(Json(normalized))
    .bind(hash)
    .fetch_one(&mut **transaction)
    .await
    .map_err(AppError::from)
}

async fn load_branch_provision_state(
    transaction: &mut WorkspaceTransaction<'_>,
    project_id: Uuid,
    goal_branch_id: Uuid,
) -> AppResult<BranchProvisionState> {
    sqlx::query_as::<_, BranchProvisionState>(
        "SELECT b.id, b.project_id, b.creating_proposal_id, b.parent_goal_branch_id, \
                b.inherited_from_session_id, b.head_session_id, b.git_branch_name, \
                p.approved_revision \
         FROM goal_branches b JOIN goal_branch_proposals p ON p.id = b.creating_proposal_id \
         WHERE b.id = $1 AND b.project_id = $2 FOR UPDATE OF b, p",
    )
    .bind(goal_branch_id)
    .bind(project_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| AppError::not_found("GoalBranch 不存在"))
}

#[allow(clippy::too_many_arguments)]
async fn mark_provision_failure(
    pool: &PgPool,
    project_id: Uuid,
    goal_branch_id: Uuid,
    session_id: Uuid,
    workspace_id: Uuid,
    operation_id: Uuid,
    client_request_id: Uuid,
    code: &str,
    summary: &str,
) -> AppResult<()> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM projects WHERE id = $1 FOR UPDATE")
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "UPDATE goal_workspaces SET status = 'error', last_error_code = $1, \
         last_error_summary = $2, updated_at = now() WHERE id = $3",
    )
    .bind(code)
    .bind(summary)
    .bind(workspace_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE workspace_operations SET status = 'failed', error_code = $1, \
         error_summary = $2, updated_at = now(), completed_at = now() WHERE id = $3",
    )
    .bind(code)
    .bind(summary)
    .bind(operation_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE goal_sessions SET status = 'exception_paused', updated_at = now() \
         WHERE id = $1 AND project_id = $2 AND status = 'running'",
    )
    .bind(session_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    insert_exception_attention(
        &mut transaction,
        project_id,
        goal_branch_id,
        session_id,
        &format!("workspace:{workspace_id}:provision"),
        "Git worktree 建立失败",
        summary,
        "数据库已保留确定性 repository/worktree key；失败现场未授权任何 Runner 写入",
        "检查托管目录权限、Git 元数据和残留 worktree，不要直接覆盖目录",
        "错误恢复前 Session 不再具有写权",
        "修复根因后执行显式 workspace reconcile，再恢复同一 Session",
    )
    .await?;
    insert_workspace_event(
        &mut transaction,
        project_id,
        "session",
        session_id,
        "workspace.provision_failed",
        client_request_id,
        json!({ "workspaceId": workspace_id, "code": code, "summary": summary }),
    )
    .await?;
    transaction.commit().await?;
    Ok(())
}

#[derive(Debug)]
struct InitializedRepository {
    head_commit: String,
    object_format: String,
}

fn ensure_bare_repository(path: &Path) -> Result<InitializedRepository, String> {
    if path.exists() {
        if fs::symlink_metadata(path)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("托管 Git 仓库路径不能是符号链接".to_owned());
        }
    } else {
        fs::create_dir_all(path).map_err(|error| error.to_string())?;
    }
    let bare = run_git([
        OsStr::new("--git-dir"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("--is-bare-repository"),
    ]);
    if bare.as_deref() != Ok("true") {
        if fs::read_dir(path)
            .map_err(|error| error.to_string())?
            .next()
            .is_some()
        {
            return Err("托管仓库路径已存在但不是可识别的 bare repository".to_owned());
        }
        run_git([
            OsStr::new("init"),
            OsStr::new("--bare"),
            OsStr::new("--initial-branch=main"),
            path.as_os_str(),
        ])?;
    }
    let head = run_git([
        OsStr::new("--git-dir"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("--verify"),
        OsStr::new("refs/heads/main"),
    ]);
    let head_commit = match head {
        Ok(head) => head,
        Err(_) => {
            let empty_tree = run_git_with_input(
                [
                    OsStr::new("--git-dir"),
                    path.as_os_str(),
                    OsStr::new("hash-object"),
                    OsStr::new("-t"),
                    OsStr::new("tree"),
                    OsStr::new("--stdin"),
                ],
                b"",
                false,
            )?;
            let commit = run_git_with_input(
                [
                    OsStr::new("--git-dir"),
                    path.as_os_str(),
                    OsStr::new("commit-tree"),
                    OsStr::new("-m"),
                    OsStr::new("fudian: initialize managed project repository"),
                    OsStr::new(&empty_tree),
                ],
                b"",
                true,
            )?;
            run_git([
                OsStr::new("--git-dir"),
                path.as_os_str(),
                OsStr::new("update-ref"),
                OsStr::new("refs/heads/main"),
                OsStr::new(&commit),
            ])?;
            commit
        }
    };
    let object_format = run_git([
        OsStr::new("--git-dir"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("--show-object-format"),
    ])?;
    validate_git_oid(&head_commit)?;
    if !matches!(object_format.as_str(), "sha1" | "sha256") {
        return Err("Git 仓库使用了未知对象格式".to_owned());
    }
    Ok(InitializedRepository {
        head_commit,
        object_format,
    })
}

fn ensure_goal_worktree(
    repository_path: &Path,
    worktree_path: &Path,
    branch_name: &str,
    base_commit: &str,
) -> Result<GitWorkspaceInspection, String> {
    validate_git_oid(base_commit)?;
    if worktree_path.exists() {
        if fs::symlink_metadata(worktree_path)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("worktree 路径不能是符号链接".to_owned());
        }
    } else {
        let parent = worktree_path
            .parent()
            .ok_or_else(|| "worktree 缺少父目录".to_owned())?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        let branch_ref = format!("refs/heads/{branch_name}");
        let branch_exists = run_git([
            OsStr::new("--git-dir"),
            repository_path.as_os_str(),
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new(&branch_ref),
        ])
        .is_ok();
        if branch_exists {
            run_git([
                OsStr::new("--git-dir"),
                repository_path.as_os_str(),
                OsStr::new("worktree"),
                OsStr::new("add"),
                worktree_path.as_os_str(),
                OsStr::new(branch_name),
            ])?;
        } else {
            run_git([
                OsStr::new("--git-dir"),
                repository_path.as_os_str(),
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("-b"),
                OsStr::new(branch_name),
                worktree_path.as_os_str(),
                OsStr::new(base_commit),
            ])?;
        }
    }
    inspect_managed_worktree(repository_path, worktree_path, branch_name)
}

fn inspect_managed_worktree(
    repository_path: &Path,
    worktree_path: &Path,
    branch_name: &str,
) -> Result<GitWorkspaceInspection, String> {
    validate_managed_worktree_identity(repository_path, worktree_path, branch_name)?;
    inspect_worktree(worktree_path)
}

fn validate_managed_worktree_identity(
    repository_path: &Path,
    worktree_path: &Path,
    branch_name: &str,
) -> Result<(), String> {
    let git_file = worktree_path.join(".git");
    let metadata = fs::symlink_metadata(&git_file).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("托管 worktree 的 .git 必须是普通链接文件".to_owned());
    }
    let observed_branch = run_git([
        OsStr::new("-C"),
        worktree_path.as_os_str(),
        OsStr::new("symbolic-ref"),
        OsStr::new("--short"),
        OsStr::new("HEAD"),
    ])?;
    if observed_branch != branch_name {
        return Err("worktree 当前 branch 与 GoalBranch 身份不一致".to_owned());
    }
    let common_directory = run_git([
        OsStr::new("-C"),
        worktree_path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("--path-format=absolute"),
        OsStr::new("--git-common-dir"),
    ])?;
    let observed_repository =
        fs::canonicalize(&common_directory).map_err(|error| error.to_string())?;
    let expected_repository =
        fs::canonicalize(repository_path).map_err(|error| error.to_string())?;
    if observed_repository != expected_repository {
        return Err("worktree 的 Git common directory 不属于记录的托管仓库".to_owned());
    }
    Ok(())
}

fn worktree_contents_match_commit(worktree_path: &Path, commit: &str) -> Result<bool, String> {
    validate_git_oid(commit)?;
    let diff_status = run_git_exit(&[
        OsString::from("-C"),
        worktree_path.as_os_str().to_os_string(),
        OsString::from("diff"),
        OsString::from("--quiet"),
        OsString::from("--exit-code"),
        OsString::from(commit),
        OsString::from("--"),
    ])?;
    if diff_status != 0 {
        return Ok(false);
    }
    let untracked = run_git_bytes([
        OsStr::new("-C"),
        worktree_path.as_os_str(),
        OsStr::new("ls-files"),
        OsStr::new("--others"),
        OsStr::new("--exclude-standard"),
        OsStr::new("-z"),
    ])?;
    Ok(untracked.is_empty())
}

fn inspect_worktree(path: &Path) -> Result<GitWorkspaceInspection, String> {
    let head_commit = run_git([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("HEAD"),
    ])?;
    let tree_id = run_git([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("HEAD^{tree}"),
    ])?;
    validate_git_oid(&head_commit)?;
    validate_git_oid(&tree_id)?;
    let status = run_git_bytes([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("status"),
        OsStr::new("--porcelain=v1"),
        OsStr::new("-z"),
        OsStr::new("--untracked-files=all"),
    ])?;
    let dirty = !status.is_empty();
    let status_digest = format!("sha256:{}", hex::encode(Sha256::digest(&status)));
    let workspace_snapshot = runner_canonical_json_sha256(&json!({
        "schemaVersion": 1,
        "headCommit": head_commit,
        "treeId": tree_id,
        "dirty": dirty,
        "statusDigest": status_digest,
    }))
    .map_err(|error| error.to_string())?;
    Ok(GitWorkspaceInspection {
        head_commit,
        tree_id,
        dirty,
        status_digest,
        workspace_snapshot,
    })
}

fn run_git<'a>(args: impl IntoIterator<Item = &'a OsStr>) -> Result<String, String> {
    let bytes = run_git_bytes(args)?;
    String::from_utf8(bytes)
        .map(|value| value.trim().to_owned())
        .map_err(|_| "Git 输出不是 UTF-8".to_owned())
}

fn run_git_bytes<'a>(args: impl IntoIterator<Item = &'a OsStr>) -> Result<Vec<u8>, String> {
    run_git_command(args, None, false)
}

fn run_git_with_input<'a>(
    args: impl IntoIterator<Item = &'a OsStr>,
    input: &[u8],
    author: bool,
) -> Result<String, String> {
    let bytes = run_git_command(args, Some(input), author)?;
    String::from_utf8(bytes)
        .map(|value| value.trim().to_owned())
        .map_err(|_| "Git 输出不是 UTF-8".to_owned())
}

fn run_git_command<'a>(
    args: impl IntoIterator<Item = &'a OsStr>,
    input: Option<&[u8]>,
    author: bool,
) -> Result<Vec<u8>, String> {
    let mut command = Command::new("git");
    command
        .arg("-c")
        .arg("core.hooksPath=/dev/null")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("commit.gpgSign=false")
        .arg("-c")
        .arg("protocol.file.allow=never")
        .args(args)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if author {
        command
            .env("GIT_AUTHOR_NAME", "Fudian Runner")
            .env("GIT_AUTHOR_EMAIL", "runner@fudian.invalid")
            .env("GIT_COMMITTER_NAME", "Fudian Runner")
            .env("GIT_COMMITTER_EMAIL", "runner@fudian.invalid");
    }
    if input.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    if let Some(input) = input {
        child
            .stdin
            .take()
            .ok_or_else(|| "Git stdin 不可用".to_owned())?
            .write_all(input)
            .map_err(|error| error.to_string())?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.chars().take(1_000).collect::<String>();
        return Err(format!("Git 返回 {:?}：{stderr}", output.status.code()));
    }
    Ok(output.stdout)
}

fn validate_git_oid(value: &str) -> Result<(), String> {
    if matches!(value.len(), 40 | 64)
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err("Git object ID 不是规范小写十六进制".to_owned())
    }
}

async fn canonical_root(root: &Path) -> AppResult<PathBuf> {
    tokio::fs::create_dir_all(root).await?;
    let metadata = tokio::fs::symlink_metadata(root).await?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::internal("托管根目录必须是真实目录而不是符号链接"));
    }
    tokio::fs::canonicalize(root).await.map_err(AppError::from)
}

fn managed_path(root: &Path, key: &str, leaf_may_be_missing: bool) -> AppResult<PathBuf> {
    let relative = Path::new(key);
    if relative.is_absolute() || key.contains('\0') || key.contains('\\') {
        return Err(AppError::internal("托管存储 key 不是规范相对路径"));
    }
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err(AppError::internal("托管存储 key 为空"));
    }
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            return Err(AppError::internal("托管存储 key 包含越界片段"));
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::conflict(
                    "managed_path_symlink",
                    "托管路径中发现符号链接，拒绝继续",
                ));
            }
            Ok(_) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && (leaf_may_be_missing || index + 1 < components.len()) => {}
            Err(error) => return Err(AppError::Io(error)),
        }
    }
    if !current.starts_with(root) {
        return Err(AppError::internal("托管路径逃逸根目录"));
    }
    Ok(current)
}

async fn insert_workspace_event(
    transaction: &mut WorkspaceTransaction<'_>,
    project_id: Uuid,
    aggregate_type: &str,
    aggregate_id: Uuid,
    event_type: &str,
    client_request_id: Uuid,
    payload: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO goal_events \
         (id, project_id, aggregate_type, aggregate_id, event_type, actor_type, \
          client_request_id, payload) VALUES ($1, $2, $3, $4, $5, 'system', $6, $7)",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(aggregate_type)
    .bind(aggregate_id)
    .bind(event_type)
    .bind(client_request_id)
    .bind(Json(payload))
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_exception_attention(
    transaction: &mut WorkspaceTransaction<'_>,
    project_id: Uuid,
    goal_branch_id: Uuid,
    session_id: Uuid,
    dedupe_key: &str,
    title: &str,
    reason: &str,
    safe_checkpoint: &str,
    attempted: &str,
    risk: &str,
    user_action: &str,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO goal_attention_items \
         (id, project_id, goal_branch_id, session_id, kind, status, dedupe_key, title, \
          reason, safe_checkpoint, attempted, risk, user_action, recommendation) \
         VALUES ($1, $2, $3, $4, 'exception', 'open', $5, $6, $7, $8, $9, $10, $11, $12) \
         ON CONFLICT (project_id, dedupe_key) WHERE status = 'open' DO NOTHING",
    )
    .bind(Uuid::new_v4())
    .bind(project_id)
    .bind(goal_branch_id)
    .bind(session_id)
    .bind(dedupe_key)
    .bind(title)
    .bind(reason)
    .bind(safe_checkpoint)
    .bind(attempted)
    .bind(risk)
    .bind(user_action)
    .bind("处理后显式恢复同一个 Session；不要绕过 Lease 直接改 worktree")
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
