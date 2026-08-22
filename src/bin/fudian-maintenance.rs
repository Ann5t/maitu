use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, bail};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tokio::{fs, io::AsyncReadExt};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum StorageRoot {
    Artifacts,
    Repositories,
    Worktrees,
    RunnerOutputs,
}

#[derive(Clone, Debug)]
struct Roots {
    artifacts: PathBuf,
    repositories: PathBuf,
    worktrees: PathBuf,
    runner_outputs: PathBuf,
}

impl Roots {
    fn get(&self, root: StorageRoot) -> &Path {
        match root {
            StorageRoot::Artifacts => &self.artifacts,
            StorageRoot::Repositories => &self.repositories,
            StorageRoot::Worktrees => &self.worktrees,
            StorageRoot::RunnerOutputs => &self.runner_outputs,
        }
    }

    fn digest(&self) -> String {
        let mut hasher = Sha256::new();
        for (name, path) in [
            ("artifacts", &self.artifacts),
            ("repositories", &self.repositories),
            ("worktrees", &self.worktrees),
            ("runner_outputs", &self.runner_outputs),
        ] {
            hasher.update(name.as_bytes());
            hasher.update(b"\0");
            hasher.update(path.as_os_str().as_encoded_bytes());
            hasher.update(b"\0");
        }
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }
}

#[derive(Clone, Debug)]
struct Reference {
    root: StorageRoot,
    storage_class: String,
    relative_path: String,
    expected_digest: Option<String>,
    expected_size: Option<i64>,
}

#[derive(Clone, Debug)]
struct Observation {
    root: StorageRoot,
    storage_class: String,
    relative_path: String,
    state: String,
    content_digest: Option<String>,
    size_bytes: Option<i64>,
    observed_mtime: Option<DateTime<Utc>>,
    eligible_after: Option<DateTime<Utc>>,
    quarantine_path: Option<String>,
    detail: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunReport {
    run_id: Uuid,
    mode: String,
    referenced: usize,
    orphan: usize,
    missing: usize,
    quarantined: usize,
    restored: usize,
    digest_mismatches: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let database_url = secret_or_env("DATABASE_URL", "DATABASE_URL_FILE")?
        .context("必须设置 DATABASE_URL 或 DATABASE_URL_FILE")?;
    let roots = Roots {
        artifacts: required_root("ARTIFACT_ROOT")?,
        repositories: required_root("REPOSITORY_ROOT")?,
        worktrees: required_root("WORKTREE_ROOT")?,
        runner_outputs: required_root("RUNNER_OUTPUT_ROOT")?,
    };
    validate_distinct_roots(&roots).await?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .context("连接 PostgreSQL 失败")?;
    let report = match arguments.as_slice() {
        [command] if command == "scan" => run_scan(&pool, &roots, false).await?,
        [command] if command == "quarantine" => run_scan(&pool, &roots, true).await?,
        [command, run_id] if command == "restore" => {
            restore_quarantine(&pool, &roots, Uuid::parse_str(run_id)?).await?
        }
        _ => bail!("usage: fudian-maintenance scan|quarantine|restore <quarantine-run-id>"),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn run_scan(pool: &PgPool, roots: &Roots, quarantine: bool) -> anyhow::Result<RunReport> {
    let mode = if quarantine { "quarantine" } else { "scan" };
    let retention_hours = positive_i64_env("FUDIAN_QUARANTINE_RETENTION_HOURS", 24 * 7)?;
    let now = Utc::now();
    let cutoff = now - Duration::hours(retention_hours);
    let run_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO storage_reconciliation_runs \
         (id, mode, status, retention_cutoff, roots_digest) VALUES ($1,$2,'running',$3,$4)",
    )
    .bind(run_id)
    .bind(mode)
    .bind(cutoff)
    .bind(roots.digest())
    .execute(pool)
    .await?;

    let result = async {
        let references = load_references(pool).await?;
        let mut observations = observe_storage(roots, &references, retention_hours).await?;
        if quarantine {
            for observation in &mut observations {
                if observation.state != "orphan"
                    || observation
                        .eligible_after
                        .is_none_or(|eligible| eligible > now)
                {
                    continue;
                }
                let source = safe_join(roots.get(observation.root), &observation.relative_path)?;
                let quarantine_relative =
                    format!(".fudian-quarantine/{run_id}/{}", observation.relative_path);
                let target = safe_join(roots.get(observation.root), &quarantine_relative)?;
                if fs::symlink_metadata(&target).await.is_ok() {
                    bail!("quarantine 目标已经存在：{}", target.display());
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).await?;
                }
                fs::rename(&source, &target).await.with_context(|| {
                    format!("移动 orphan 到 quarantine 失败：{}", source.display())
                })?;
                observation.state = "quarantined".to_owned();
                observation.quarantine_path = Some(quarantine_relative);
            }
        }
        persist_observations(pool, run_id, &observations).await?;
        let report = report_for(run_id, mode, &observations);
        sqlx::query(
            "UPDATE storage_reconciliation_runs SET status = 'completed', summary = $1, \
             completed_at = now() WHERE id = $2 AND status = 'running'",
        )
        .bind(json!({
            "referenced": report.referenced,
            "orphan": report.orphan,
            "missing": report.missing,
            "quarantined": report.quarantined,
            "digestMismatches": report.digest_mismatches,
        }))
        .bind(run_id)
        .execute(pool)
        .await?;
        Ok::<_, anyhow::Error>(report)
    }
    .await;
    if result.is_err() {
        let _ = sqlx::query(
            "UPDATE storage_reconciliation_runs SET status = 'failed', \
             summary = jsonb_build_object('error','maintenance_failed'), completed_at = now() \
             WHERE id = $1 AND status = 'running'",
        )
        .bind(run_id)
        .execute(pool)
        .await;
    }
    result
}

async fn restore_quarantine(
    pool: &PgPool,
    roots: &Roots,
    source_run_id: Uuid,
) -> anyhow::Result<RunReport> {
    let source_mode: Option<String> = sqlx::query_scalar(
        "SELECT mode FROM storage_reconciliation_runs \
         WHERE id = $1 AND mode = 'quarantine' AND status = 'completed'",
    )
    .bind(source_run_id)
    .fetch_optional(pool)
    .await?;
    if source_mode.is_none() {
        bail!("来源不是已完成的 quarantine run");
    }
    let source_items: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT storage_class, relative_path, quarantine_path \
         FROM storage_reconciliation_items WHERE run_id = $1 AND state = 'quarantined' \
         ORDER BY storage_class, relative_path",
    )
    .bind(source_run_id)
    .fetch_all(pool)
    .await?;
    if source_items.is_empty() {
        bail!("quarantine run 没有可恢复项目");
    }
    let run_id = Uuid::new_v4();
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO storage_reconciliation_runs \
         (id, mode, status, retention_cutoff, roots_digest) VALUES ($1,'restore','running',$2,$3)",
    )
    .bind(run_id)
    .bind(now)
    .bind(roots.digest())
    .execute(pool)
    .await?;
    let result = async {
        let mut planned = Vec::with_capacity(source_items.len());
        for (storage_class, relative_path, quarantine_path) in source_items {
            validate_relative(&relative_path)?;
            validate_relative(&quarantine_path)?;
            let expected_prefix = format!(".fudian-quarantine/{source_run_id}/");
            if !quarantine_path.starts_with(&expected_prefix) {
                bail!("quarantine 路径与来源 run 不匹配");
            }
            let root = root_for_class(&storage_class)?;
            let source = safe_join(roots.get(root), &quarantine_path)?;
            let destination = safe_join(roots.get(root), &relative_path)?;
            if fs::symlink_metadata(&source).await.is_err() {
                bail!("quarantine 内容缺失：{}", source.display());
            }
            if fs::symlink_metadata(&destination).await.is_ok() {
                bail!("原路径已经被占用，拒绝覆盖：{}", destination.display());
            }
            planned.push((root, storage_class, relative_path, quarantine_path));
        }
        let mut observations = Vec::with_capacity(planned.len());
        for (root, storage_class, relative_path, quarantine_path) in planned {
            let source = safe_join(roots.get(root), &quarantine_path)?;
            let destination = safe_join(roots.get(root), &relative_path)?;
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::rename(&source, &destination).await?;
            let physical = physical_metadata(&destination).await?;
            observations.push(Observation {
                root,
                storage_class,
                relative_path,
                state: "restored".to_owned(),
                content_digest: physical.0,
                size_bytes: physical.1,
                observed_mtime: physical.2,
                eligible_after: None,
                quarantine_path: Some(quarantine_path),
                detail: json!({"sourceRunId":source_run_id}),
            });
        }
        persist_observations(pool, run_id, &observations).await?;
        let report = report_for(run_id, "restore", &observations);
        sqlx::query(
            "UPDATE storage_reconciliation_runs SET status = 'completed', summary = $1, \
             completed_at = now() WHERE id = $2 AND status = 'running'",
        )
        .bind(json!({"restored":report.restored,"sourceRunId":source_run_id}))
        .bind(run_id)
        .execute(pool)
        .await?;
        Ok::<_, anyhow::Error>(report)
    }
    .await;
    if result.is_err() {
        let _ = sqlx::query(
            "UPDATE storage_reconciliation_runs SET status = 'failed', \
             summary = jsonb_build_object('error','restore_failed'), completed_at = now() \
             WHERE id = $1 AND status = 'running'",
        )
        .bind(run_id)
        .execute(pool)
        .await;
    }
    result
}

async fn load_references(pool: &PgPool) -> anyhow::Result<Vec<Reference>> {
    let mut references = BTreeMap::<(String, String), Reference>::new();
    for (path, digest) in
        sqlx::query_as::<_, (String, String)>("SELECT storage_path, sha256 FROM artifacts")
            .fetch_all(pool)
            .await?
    {
        add_reference(
            &mut references,
            StorageRoot::Artifacts,
            "artifact",
            path,
            Some(digest),
            None,
        )?;
    }
    for (path, digest, size) in sqlx::query_as::<_, (String, String, i64)>(
        "SELECT storage_key, sha256, size_bytes FROM idea_source_objects",
    )
    .fetch_all(pool)
    .await?
    {
        add_reference(
            &mut references,
            StorageRoot::Artifacts,
            "input_object",
            path,
            Some(digest),
            Some(size),
        )?;
    }
    for (path, digest, size) in sqlx::query_as::<_, (String, Option<String>, i64)>(
        "SELECT storage_key, sha256, actual_size FROM input_artifacts \
         WHERE status <> 'rejected'",
    )
    .fetch_all(pool)
    .await?
    {
        add_reference(
            &mut references,
            StorageRoot::Artifacts,
            "input_object",
            path,
            digest,
            Some(size),
        )?;
    }
    for (path, digest, size) in sqlx::query_as::<_, (String, String, i64)>(
        "SELECT storage_key, sha256, size_bytes FROM input_artifact_chunks",
    )
    .fetch_all(pool)
    .await?
    {
        add_reference(
            &mut references,
            StorageRoot::Artifacts,
            "input_chunk",
            path,
            Some(digest),
            Some(size),
        )?;
    }
    let inboxes: Vec<String> = sqlx::query_scalar(
        "SELECT 'inputs/inboxes/' || project_id::text || '/' || goal_branch_id::text || '/' \
         || session_id::text || '/' || inbox_relative_path FROM input_artifacts \
         WHERE import_mode = 'worktree_copy' AND inbox_relative_path IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;
    for path in inboxes {
        add_reference(
            &mut references,
            StorageRoot::Artifacts,
            "input_object",
            path,
            None,
            None,
        )?;
    }
    for path in sqlx::query_scalar::<_, String>("SELECT storage_key FROM project_git_repositories")
        .fetch_all(pool)
        .await?
    {
        add_reference(
            &mut references,
            StorageRoot::Repositories,
            "repository",
            path,
            None,
            None,
        )?;
    }
    for path in sqlx::query_scalar::<_, String>("SELECT worktree_key FROM goal_workspaces")
        .fetch_all(pool)
        .await?
    {
        add_reference(
            &mut references,
            StorageRoot::Worktrees,
            "worktree",
            path,
            None,
            None,
        )?;
    }
    for path in sqlx::query_scalar::<_, String>("SELECT output_key FROM workspace_write_leases")
        .fetch_all(pool)
        .await?
    {
        add_reference(
            &mut references,
            StorageRoot::RunnerOutputs,
            "runner_output",
            path,
            None,
            None,
        )?;
    }
    for path in sqlx::query_scalar::<_, String>(
        "SELECT preparation_key FROM goal_integrations WHERE preparation_key IS NOT NULL",
    )
    .fetch_all(pool)
    .await?
    {
        add_reference(
            &mut references,
            StorageRoot::RunnerOutputs,
            "runner_output",
            path,
            None,
            None,
        )?;
    }
    Ok(references.into_values().collect())
}

fn add_reference(
    references: &mut BTreeMap<(String, String), Reference>,
    root: StorageRoot,
    storage_class: &str,
    relative_path: String,
    expected_digest: Option<String>,
    expected_size: Option<i64>,
) -> anyhow::Result<()> {
    validate_relative(&relative_path)?;
    let digest = expected_digest.map(normalize_digest).transpose()?;
    references
        .entry((storage_class.to_owned(), relative_path.clone()))
        .or_insert(Reference {
            root,
            storage_class: storage_class.to_owned(),
            relative_path,
            expected_digest: digest,
            expected_size,
        });
    Ok(())
}

async fn observe_storage(
    roots: &Roots,
    references: &[Reference],
    retention_hours: i64,
) -> anyhow::Result<Vec<Observation>> {
    let mut observations = Vec::new();
    for root_kind in [
        StorageRoot::Artifacts,
        StorageRoot::Repositories,
        StorageRoot::Worktrees,
        StorageRoot::RunnerOutputs,
    ] {
        let root_references = references
            .iter()
            .filter(|reference| reference.root == root_kind)
            .cloned()
            .collect::<Vec<_>>();
        let paths = root_references
            .iter()
            .map(|reference| reference.relative_path.clone())
            .collect::<BTreeSet<_>>();
        observe_directory(
            roots.get(root_kind),
            root_kind,
            Path::new(""),
            &paths,
            &root_references,
            retention_hours,
            &mut observations,
        )
        .await?;
        for reference in root_references {
            if observations.iter().any(|item| {
                item.storage_class == reference.storage_class
                    && item.relative_path == reference.relative_path
            }) {
                continue;
            }
            observations.push(Observation {
                root: root_kind,
                storage_class: reference.storage_class,
                relative_path: reference.relative_path,
                state: "missing".to_owned(),
                content_digest: None,
                size_bytes: None,
                observed_mtime: None,
                eligible_after: None,
                quarantine_path: None,
                detail: json!({"reason":"database_reference_missing_on_disk"}),
            });
        }
    }
    observations.sort_by(|left, right| {
        (&left.storage_class, &left.relative_path)
            .cmp(&(&right.storage_class, &right.relative_path))
    });
    Ok(observations)
}

#[allow(clippy::too_many_arguments)]
async fn observe_directory(
    absolute_root: &Path,
    root_kind: StorageRoot,
    relative_parent: &Path,
    reference_paths: &BTreeSet<String>,
    references: &[Reference],
    retention_hours: i64,
    observations: &mut Vec<Observation>,
) -> anyhow::Result<()> {
    let directory = absolute_root.join(relative_parent);
    let mut entries = fs::read_dir(&directory)
        .await
        .with_context(|| format!("读取存储根失败：{}", directory.display()))?;
    let mut children = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        children.push(entry.path());
    }
    children.sort();
    for path in children {
        let relative = path
            .strip_prefix(absolute_root)?
            .to_string_lossy()
            .replace('\\', "/");
        if relative == ".fudian-quarantine" {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).await?;
        let exact = references
            .iter()
            .filter(|reference| reference.relative_path == relative)
            .collect::<Vec<_>>();
        if !exact.is_empty() {
            for reference in exact {
                let physical = physical_metadata(&path).await?;
                let digest_mismatch = reference
                    .expected_digest
                    .as_ref()
                    .zip(physical.0.as_ref())
                    .is_some_and(|(expected, actual)| expected != actual);
                let size_mismatch = reference
                    .expected_size
                    .zip(physical.1)
                    .is_some_and(|(expected, actual)| expected != actual);
                observations.push(Observation {
                    root: root_kind,
                    storage_class: reference.storage_class.clone(),
                    relative_path: relative.clone(),
                    state: "referenced".to_owned(),
                    content_digest: physical.0,
                    size_bytes: physical.1,
                    observed_mtime: physical.2,
                    eligible_after: None,
                    quarantine_path: None,
                    detail: json!({
                        "digestMismatch": digest_mismatch,
                        "sizeMismatch": size_mismatch,
                        "unsafeSymlink": metadata.file_type().is_symlink(),
                    }),
                });
            }
            continue;
        }
        let prefix = format!("{relative}/");
        let is_reference_parent = reference_paths
            .iter()
            .any(|value| value.starts_with(&prefix));
        if metadata.is_dir() && is_reference_parent {
            Box::pin(observe_directory(
                absolute_root,
                root_kind,
                path.strip_prefix(absolute_root)?,
                reference_paths,
                references,
                retention_hours,
                observations,
            ))
            .await?;
            continue;
        }
        let physical = physical_metadata(&path).await?;
        let eligible_after = physical
            .2
            .map(|modified| modified + Duration::hours(retention_hours));
        observations.push(Observation {
            root: root_kind,
            storage_class: default_class(root_kind).to_owned(),
            relative_path: relative,
            state: "orphan".to_owned(),
            content_digest: physical.0,
            size_bytes: physical.1,
            observed_mtime: physical.2,
            eligible_after,
            quarantine_path: None,
            detail: json!({"reason":"not_referenced_by_database"}),
        });
    }
    Ok(())
}

async fn physical_metadata(
    path: &Path,
) -> anyhow::Result<(Option<String>, Option<i64>, Option<DateTime<Utc>>)> {
    let metadata = fs::symlink_metadata(path).await?;
    let modified = metadata.modified().ok().map(DateTime::<Utc>::from);
    if !metadata.is_file() {
        return Ok((None, None, modified));
    }
    let mut file = fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut size = 0_i64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size
            .checked_add(i64::try_from(read)?)
            .context("文件尺寸溢出")?;
    }
    Ok((
        Some(format!("sha256:{}", hex::encode(hasher.finalize()))),
        Some(size),
        modified,
    ))
}

async fn persist_observations(
    pool: &PgPool,
    run_id: Uuid,
    observations: &[Observation],
) -> anyhow::Result<()> {
    let mut transaction = pool.begin().await?;
    for item in observations {
        sqlx::query(
            "INSERT INTO storage_reconciliation_items \
             (id, run_id, storage_class, relative_path, state, content_digest, size_bytes, \
              observed_mtime, eligible_after, quarantine_path, detail) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(Uuid::new_v4())
        .bind(run_id)
        .bind(&item.storage_class)
        .bind(&item.relative_path)
        .bind(&item.state)
        .bind(&item.content_digest)
        .bind(item.size_bytes)
        .bind(item.observed_mtime)
        .bind(item.eligible_after)
        .bind(&item.quarantine_path)
        .bind(&item.detail)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

fn report_for(run_id: Uuid, mode: &str, observations: &[Observation]) -> RunReport {
    RunReport {
        run_id,
        mode: mode.to_owned(),
        referenced: count_state(observations, "referenced"),
        orphan: count_state(observations, "orphan"),
        missing: count_state(observations, "missing"),
        quarantined: count_state(observations, "quarantined"),
        restored: count_state(observations, "restored"),
        digest_mismatches: observations
            .iter()
            .filter(|item| item.detail.get("digestMismatch").and_then(Value::as_bool) == Some(true))
            .count(),
    }
}

fn count_state(observations: &[Observation], state: &str) -> usize {
    observations
        .iter()
        .filter(|item| item.state == state)
        .count()
}

fn default_class(root: StorageRoot) -> &'static str {
    match root {
        StorageRoot::Artifacts => "artifact",
        StorageRoot::Repositories => "repository",
        StorageRoot::Worktrees => "worktree",
        StorageRoot::RunnerOutputs => "runner_output",
    }
}

fn root_for_class(storage_class: &str) -> anyhow::Result<StorageRoot> {
    match storage_class {
        "artifact" | "input_chunk" | "input_object" => Ok(StorageRoot::Artifacts),
        "repository" => Ok(StorageRoot::Repositories),
        "worktree" => Ok(StorageRoot::Worktrees),
        "runner_output" => Ok(StorageRoot::RunnerOutputs),
        _ => bail!("未知 storage class：{storage_class}"),
    }
}

fn normalize_digest(value: String) -> anyhow::Result<String> {
    let raw = value.strip_prefix("sha256:").unwrap_or(&value);
    if raw.len() != 64 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("数据库包含无效 SHA-256 摘要");
    }
    Ok(format!("sha256:{}", raw.to_ascii_lowercase()))
}

fn validate_relative(value: &str) -> anyhow::Result<()> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || value.contains('\\')
        || value.contains("//")
    {
        bail!("数据库包含不安全存储路径：{value}");
    }
    Ok(())
}

fn safe_join(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    validate_relative(relative)?;
    Ok(root.join(relative))
}

fn required_root(name: &str) -> anyhow::Result<PathBuf> {
    env::var(name)
        .map(PathBuf::from)
        .with_context(|| format!("必须设置 {name}"))
}

async fn validate_distinct_roots(roots: &Roots) -> anyhow::Result<()> {
    let mut canonical = BTreeSet::new();
    for root in [
        &roots.artifacts,
        &roots.repositories,
        &roots.worktrees,
        &roots.runner_outputs,
    ] {
        fs::create_dir_all(root).await?;
        let resolved = fs::canonicalize(root).await?;
        if !canonical.insert(resolved) {
            bail!("存储根必须彼此独立");
        }
    }
    Ok(())
}

fn positive_i64_env(name: &str, default: i64) -> anyhow::Result<i64> {
    let value = env::var(name)
        .ok()
        .map(|value| value.parse::<i64>())
        .transpose()
        .with_context(|| format!("{name} 必须是整数"))?
        .unwrap_or(default);
    if value < 0 {
        bail!("{name} 不能小于 0");
    }
    Ok(value)
}

fn secret_or_env(value_name: &str, file_name: &str) -> anyhow::Result<Option<String>> {
    let direct = env::var(value_name).ok();
    let file = env::var(file_name).ok();
    if direct.is_some() && file.is_some() {
        bail!("{value_name} 与 {file_name} 不能同时设置");
    }
    if let Some(path) = file {
        let value = std::fs::read_to_string(&path)
            .with_context(|| format!("读取 {file_name} 失败：{path}"))?;
        return Ok(Some(value.trim_end_matches(['\r', '\n']).to_owned()));
    }
    Ok(direct)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_paths_never_escape_a_root() {
        for value in ["", "/absolute", "../escape", "a/../escape", "a\\b", "a//b"] {
            assert!(validate_relative(value).is_err(), "accepted {value}");
        }
        assert!(validate_relative("projects/one.git").is_ok());
    }

    #[test]
    fn database_digests_are_canonicalized() {
        assert_eq!(
            normalize_digest("A".repeat(64)).unwrap(),
            format!("sha256:{}", "a".repeat(64))
        );
        assert!(normalize_digest("not-a-digest".to_owned()).is_err());
    }

    #[test]
    fn root_classes_are_explicit() {
        assert_eq!(
            root_for_class("repository").unwrap(),
            StorageRoot::Repositories
        );
        assert!(root_for_class("unknown").is_err());
    }
}
