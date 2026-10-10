//! 冻结候选 worktree 的只读观察。
//!
//! 应用在冻结拟合并候选时（`application::workspaces::inspect_worktree`）与
//! Review Worker 复核时必须算出**逐字节相同**的 `workspaceSnapshot`，所以
//! 这套算法只保留一份：本模块既是应用的实现，也通过 `fudian` 库暴露给
//! `src/bin/maitu-review-worker.rs`。任何一侧单独改动都会让两侧对同一
//! 现场算出不同摘要，审核绑定随即失效。

use std::{
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::runner_protocol::canonical_json_sha256 as runner_canonical_json_sha256;

/// 与 `application::workspaces::run_git_command` 相同的确定性 Git 环境与
/// 全局配置；只读观察额外要求 `GIT_OPTIONAL_LOCKS=0`，否则 `git status`
/// 会在只读挂载上尝试刷新索引而失败。该变量不改变 status 的输出内容。
pub fn run_git_readonly<'a>(args: impl IntoIterator<Item = &'a OsStr>) -> Result<Vec<u8>, String> {
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
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.chars().take(1_000).collect::<String>();
        return Err(format!("Git 返回 {:?}：{stderr}", output.status.code()));
    }
    Ok(output.stdout)
}

fn run_git_text<'a>(args: impl IntoIterator<Item = &'a OsStr>) -> Result<String, String> {
    let bytes = run_git_readonly(args)?;
    String::from_utf8(bytes)
        .map(|value| value.trim().to_owned())
        .map_err(|_| "Git 输出不是 UTF-8".to_owned())
}

pub fn validate_git_oid(value: &str) -> Result<(), String> {
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

/// 与 `application::workspaces::GitWorkspaceInspection` 同形的观察结果。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSnapshot {
    pub head_commit: String,
    pub tree_id: String,
    pub dirty: bool,
    pub status_digest: String,
    pub workspace_snapshot: String,
}

pub fn inspect_worktree(path: &Path) -> Result<GitSnapshot, String> {
    let head_commit = run_git_text([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("HEAD"),
    ])?;
    let tree_id = run_git_text([
        OsStr::new("-C"),
        path.as_os_str(),
        OsStr::new("rev-parse"),
        OsStr::new("HEAD^{tree}"),
    ])?;
    validate_git_oid(&head_commit)?;
    validate_git_oid(&tree_id)?;
    let status = run_git_readonly([
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
    Ok(GitSnapshot {
        head_commit,
        tree_id,
        dirty,
        status_digest,
        workspace_snapshot,
    })
}

/// 确认 worktree 是记录在案的托管 worktree：`.git` 是普通链接文件、
/// 当前分支等于 GoalBranch 身份、common directory 属于记录的仓库。
pub fn verify_worktree_identity(
    repository_path: &Path,
    worktree_path: &Path,
    branch_name: &str,
) -> Result<(), String> {
    let git_file = worktree_path.join(".git");
    let metadata = fs::symlink_metadata(&git_file).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("托管 worktree 的 .git 必须是普通链接文件".to_owned());
    }
    let observed_branch = run_git_text([
        OsStr::new("-C"),
        worktree_path.as_os_str(),
        OsStr::new("symbolic-ref"),
        OsStr::new("--short"),
        OsStr::new("HEAD"),
    ])?;
    if observed_branch != branch_name {
        return Err("worktree 当前 branch 与 GoalBranch 身份不一致".to_owned());
    }
    let common_directory = run_git_text([
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

/// 解析托管存储 key 并拒绝越界、符号链接与非规范片段。Review Worker
/// 只解析已存在的冻结现场，叶子必须存在。
pub fn managed_path(root: &Path, key: &str) -> Result<PathBuf, String> {
    let relative = Path::new(key);
    if relative.is_absolute() || key.contains('\0') || key.contains('\\') {
        return Err("托管存储 key 不是规范相对路径".to_owned());
    }
    let mut current = root.to_path_buf();
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err("托管存储 key 为空".to_owned());
    }
    for component in components {
        let Component::Normal(component) = component else {
            return Err("托管存储 key 包含越界片段".to_owned());
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("托管路径中发现符号链接，拒绝继续".to_owned());
            }
            Ok(_) => {}
            Err(error) => return Err(format!("托管路径无法读取：{error}")),
        }
    }
    if !current.starts_with(root) {
        return Err("托管路径逃逸根目录".to_owned());
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_out_of_range_keys() {
        let root = Path::new("/data/worktrees");
        assert!(managed_path(root, "/absolute").is_err());
        assert!(managed_path(root, "a\\b").is_err());
        assert!(managed_path(root, "a\0b").is_err());
        assert!(managed_path(root, "").is_err());
        assert!(managed_path(root, "../escape").is_err());
        // 叶子必须存在：缺失时返回读取失败而不是静默通过。
        assert!(managed_path(root, "goal/abc").is_err());
    }

    #[test]
    fn validates_object_ids() {
        assert!(validate_git_oid(&"a".repeat(40)).is_ok());
        assert!(validate_git_oid(&"a".repeat(64)).is_ok());
        assert!(validate_git_oid(&"A".repeat(40)).is_err());
        assert!(validate_git_oid("short").is_err());
    }
}
