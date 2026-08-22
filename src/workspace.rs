use std::collections::{BTreeMap, BTreeSet};

use fudian::runner_protocol::{
    RunnerCapabilities, RunnerCommand, RunnerExecutionResult, RunnerJobSpec, RunnerResourceLimits,
    is_portable_workspace_file_path,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub const RUNNER_PROGRAM: &str = "/usr/local/bin/fudian-runner";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCapabilityPolicy {
    #[serde(default = "denied_network")]
    pub network: String,
    #[serde(default)]
    pub network_destinations: Vec<String>,
    #[serde(default)]
    pub external_writes: Vec<String>,
    #[serde(default)]
    pub account_references: Vec<String>,
    #[serde(default)]
    pub paid_operations: bool,
    #[serde(default)]
    pub deployment: bool,
    #[serde(default = "default_read_scopes")]
    pub read_scopes: Vec<String>,
    #[serde(default = "default_write_paths")]
    pub write_paths: Vec<String>,
    #[serde(default)]
    pub maximum_resources: RunnerResourceLimits,
}

impl Default for WorkspaceCapabilityPolicy {
    fn default() -> Self {
        Self {
            network: denied_network(),
            network_destinations: Vec::new(),
            external_writes: Vec::new(),
            account_references: Vec::new(),
            paid_operations: false,
            deployment: false,
            read_scopes: default_read_scopes(),
            write_paths: default_write_paths(),
            maximum_resources: RunnerResourceLimits::default(),
        }
    }
}

impl WorkspaceCapabilityPolicy {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.network = self.network.trim().to_ascii_lowercase();
        if !matches!(
            self.network.as_str(),
            "denied" | "public_read_only" | "granted_destinations"
        ) {
            return Err(AppError::bad_request(
                "invalid_workspace_policy",
                "网络能力必须是 denied、public_read_only 或 granted_destinations",
            ));
        }
        normalize_names("网络目的地", &mut self.network_destinations, 100, 253)?;
        if self.network == "denied" && !self.network_destinations.is_empty() {
            return Err(AppError::bad_request(
                "invalid_workspace_policy",
                "断网策略不能同时声明网络目的地",
            ));
        }
        if self.network == "granted_destinations" && self.network_destinations.is_empty() {
            return Err(AppError::bad_request(
                "invalid_workspace_policy",
                "按目的地联网至少需要一个明确目的地",
            ));
        }
        normalize_names("外部写能力", &mut self.external_writes, 100, 200)?;
        normalize_names("账号引用", &mut self.account_references, 100, 200)?;
        normalize_names("读取范围", &mut self.read_scopes, 10, 80)?;
        if self
            .read_scopes
            .iter()
            .any(|scope| !matches!(scope.as_str(), "current_worktree" | "parent_snapshot"))
        {
            return Err(AppError::bad_request(
                "invalid_workspace_policy",
                "读取范围只支持 current_worktree 与 parent_snapshot",
            ));
        }
        normalize_write_patterns(&mut self.write_paths)?;
        validate_resources(&self.maximum_resources, true)?;
        Ok(self)
    }

    pub fn authorize(
        &self,
        requested: &RunnerCapabilities,
        allowed_writes: &[String],
        resources: &RunnerResourceLimits,
    ) -> AppResult<()> {
        if requested.network != self.network
            && !(requested.network == "denied" && self.network != "denied")
        {
            return Err(AppError::forbidden(
                "capability_denied",
                "Runner 请求的网络能力超出 BranchProposal 授权",
            ));
        }
        if requested
            .external_writes
            .iter()
            .any(|value| !self.external_writes.contains(value))
            || requested
                .account_references
                .iter()
                .any(|value| !self.account_references.contains(value))
            || (requested.paid_operations && !self.paid_operations)
            || (requested.deployment && !self.deployment)
        {
            return Err(AppError::forbidden(
                "capability_denied",
                "Runner 请求了 BranchProposal 未授权的高风险能力",
            ));
        }
        if allowed_writes.iter().any(|candidate| {
            !self
                .write_paths
                .iter()
                .any(|policy| pattern_contains(policy, candidate))
        }) {
            return Err(AppError::forbidden(
                "workspace_write_denied",
                "Runner 请求的写路径超出目标枝干权限",
            ));
        }
        validate_resources(resources, false)?;
        let maximum = &self.maximum_resources;
        if resources.cpu_millis > maximum.cpu_millis
            || resources.memory_mi_b > maximum.memory_mi_b
            || resources.disk_mi_b > maximum.disk_mi_b
            || resources.pids > maximum.pids
            || resources.timeout_seconds > maximum.timeout_seconds
            || resources.stdout_bytes > maximum.stdout_bytes
            || resources.stderr_bytes > maximum.stderr_bytes
        {
            return Err(AppError::forbidden(
                "resource_limit_denied",
                "Runner 请求的资源上限超过 BranchProposal 固定边界",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRunnerJobRequest {
    pub client_request_id: Uuid,
    pub base_workspace_snapshot: String,
    #[serde(default)]
    pub allowed_writes: Vec<String>,
    #[serde(default)]
    pub delete_paths: Vec<String>,
    #[serde(default)]
    pub capabilities: RunnerCapabilities,
    #[serde(default)]
    pub resources: RunnerResourceLimits,
    pub command: RunnerCommand,
}

impl PrepareRunnerJobRequest {
    pub fn normalize(mut self) -> AppResult<Self> {
        validate_sha256("workspace snapshot", &self.base_workspace_snapshot)?;
        normalize_write_patterns(&mut self.allowed_writes)?;
        if self.allowed_writes.is_empty() {
            return Err(AppError::bad_request(
                "invalid_allowed_writes",
                "RunnerJob 至少要声明一个允许写路径",
            ));
        }
        if self.delete_paths.len() > 200 {
            return Err(AppError::bad_request(
                "too_many_delete_paths",
                "单个 RunnerJob 删除路径过多",
            ));
        }
        for path in &mut self.delete_paths {
            *path = normalize_relative_file_path(path)?;
            if !self
                .allowed_writes
                .iter()
                .any(|pattern| path_matches_pattern(pattern, path))
            {
                return Err(AppError::bad_request(
                    "delete_path_not_allowed",
                    format!("删除路径 {path} 不在 RunnerJob 声明的允许写范围内"),
                ));
            }
        }
        self.delete_paths.sort();
        self.delete_paths.dedup();
        let mut case_folded = BTreeSet::new();
        if self
            .delete_paths
            .iter()
            .any(|path| !case_folded.insert(path.to_lowercase()))
        {
            return Err(AppError::bad_request(
                "workspace_case_collision",
                "删除清单包含在大小写不敏感文件系统中冲突的路径",
            ));
        }
        self.capabilities.network = self.capabilities.network.trim().to_ascii_lowercase();
        if !matches!(
            self.capabilities.network.as_str(),
            "denied" | "public_read_only" | "granted_destinations"
        ) {
            return Err(AppError::bad_request(
                "invalid_runner_capability",
                "Runner 网络能力不合法",
            ));
        }
        normalize_names(
            "Runner 外部写能力",
            &mut self.capabilities.external_writes,
            100,
            200,
        )?;
        normalize_names(
            "Runner 账号引用",
            &mut self.capabilities.account_references,
            100,
            200,
        )?;
        validate_resources(&self.resources, false)?;
        normalize_command(&mut self.command)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareRunnerJobResponse {
    pub replayed: bool,
    pub job_id: Uuid,
    pub lease_id: Uuid,
    pub lease_token: Option<String>,
    pub fencing_token: i64,
    pub output_key: String,
    pub spec: RunnerJobSpec,
    pub spec_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinalizeRunnerJobRequest {
    pub lease_token: String,
    pub result: RunnerExecutionResult,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailRunnerJobRequest {
    pub lease_token: String,
    pub failure_kind: String,
    pub summary: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerJobOutcome {
    pub replayed: bool,
    pub job_id: Uuid,
    pub status: String,
    pub workspace_snapshot: String,
    pub head_commit: String,
    pub session_status: String,
}

fn denied_network() -> String {
    "denied".to_owned()
}

fn default_read_scopes() -> Vec<String> {
    vec!["current_worktree".to_owned(), "parent_snapshot".to_owned()]
}

fn default_write_paths() -> Vec<String> {
    vec!["**".to_owned()]
}

pub fn normalize_write_patterns(patterns: &mut Vec<String>) -> AppResult<()> {
    if patterns.len() > 200 {
        return Err(AppError::bad_request(
            "too_many_allowed_writes",
            "允许写路径过多",
        ));
    }
    for pattern in patterns.iter_mut() {
        *pattern = normalize_write_pattern(pattern)?;
    }
    patterns.sort();
    patterns.dedup();
    Ok(())
}

pub fn normalize_relative_file_path(value: &str) -> AppResult<String> {
    if !is_portable_workspace_file_path(value) {
        return Err(AppError::bad_request(
            "unsafe_workspace_path",
            "路径必须是可移植的规范相对文件路径，且不能触及 Git/Fudian 元数据",
        ));
    }
    Ok(value.to_owned())
}

fn normalize_write_pattern(value: &str) -> AppResult<String> {
    let trimmed = value.trim();
    if trimmed == "**" {
        return Ok(trimmed.to_owned());
    }
    if let Some(prefix) = trimmed.strip_suffix("/**") {
        return Ok(format!("{}/**", normalize_relative_file_path(prefix)?));
    }
    if trimmed.contains('*') || trimmed.contains('?') || trimmed.contains('[') {
        return Err(AppError::bad_request(
            "invalid_allowed_writes",
            "写路径只支持精确文件、目录/** 或 **",
        ));
    }
    normalize_relative_file_path(trimmed)
}

pub fn path_matches_pattern(pattern: &str, path: &str) -> bool {
    pattern == "**"
        || pattern == path
        || pattern
            .strip_suffix("/**")
            .is_some_and(|prefix| path.starts_with(&format!("{prefix}/")))
}

fn pattern_contains(policy: &str, requested: &str) -> bool {
    if policy == "**" || policy == requested {
        return true;
    }
    match (policy.strip_suffix("/**"), requested.strip_suffix("/**")) {
        (Some(policy_prefix), Some(requested_prefix)) => {
            requested_prefix == policy_prefix
                || requested_prefix.starts_with(&format!("{policy_prefix}/"))
        }
        (Some(policy_prefix), None) => requested.starts_with(&format!("{policy_prefix}/")),
        _ => false,
    }
}

fn normalize_command(command: &mut RunnerCommand) -> AppResult<()> {
    command.program = command.program.trim().to_owned();
    if command.program != RUNNER_PROGRAM && !command.program.starts_with("/runtime/") {
        return Err(AppError::bad_request(
            "invalid_runner_program",
            "Runner 程序只能来自固定 Runner 或不可变 /runtime 挂载",
        ));
    }
    if command.program.contains("..")
        || command.program.contains('\\')
        || command.program.contains('\0')
    {
        return Err(AppError::bad_request(
            "invalid_runner_program",
            "Runner 程序路径不安全",
        ));
    }
    if command.args.len() > 200
        || command
            .args
            .iter()
            .any(|arg| arg.len() > 8_000 || arg.contains('\0'))
    {
        return Err(AppError::bad_request(
            "invalid_runner_arguments",
            "Runner 参数数量、长度或字符不合法",
        ));
    }
    if command.environment.len() > 100 {
        return Err(AppError::bad_request(
            "invalid_runner_environment",
            "Runner 环境变量过多",
        ));
    }
    let original = std::mem::take(&mut command.environment);
    let mut normalized = BTreeMap::new();
    for (name, value) in original {
        if name.is_empty()
            || name.len() > 120
            || !name.chars().all(|character| {
                character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
            })
            || !name
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_uppercase() || character == '_')
            || matches!(name.as_str(), "PATH" | "HOME" | "TMPDIR")
            || name.starts_with("FUDIAN_")
            || ["TOKEN", "PASSWORD", "SECRET", "PRIVATE_KEY", "COOKIE"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
            || value.len() > 4_000
            || value.contains('\0')
        {
            return Err(AppError::bad_request(
                "invalid_runner_environment",
                "Runner 环境只接受非秘密、受限名称和值",
            ));
        }
        normalized.insert(name, value);
    }
    command.environment = normalized;
    Ok(())
}

fn validate_resources(resources: &RunnerResourceLimits, policy_maximum: bool) -> AppResult<()> {
    let absolute = RunnerResourceLimits {
        cpu_millis: 4_000,
        memory_mi_b: 4_096,
        disk_mi_b: 4_096,
        pids: 256,
        timeout_seconds: 3_600,
        stdout_bytes: 1024 * 1024,
        stderr_bytes: 1024 * 1024,
    };
    if resources.cpu_millis == 0
        || resources.memory_mi_b == 0
        || resources.disk_mi_b == 0
        || resources.pids == 0
        || resources.timeout_seconds == 0
        || resources.stdout_bytes == 0
        || resources.stderr_bytes == 0
        || resources.cpu_millis > absolute.cpu_millis
        || resources.memory_mi_b > absolute.memory_mi_b
        || resources.disk_mi_b > absolute.disk_mi_b
        || resources.pids > absolute.pids
        || resources.timeout_seconds > absolute.timeout_seconds
        || resources.stdout_bytes > absolute.stdout_bytes
        || resources.stderr_bytes > absolute.stderr_bytes
    {
        return Err(AppError::bad_request(
            "invalid_resource_policy",
            if policy_maximum {
                "BranchProposal 资源上限必须为正数且不能超过服务器绝对安全边界"
            } else {
                "RunnerJob 资源上限必须为正数且不能超过服务器绝对安全边界"
            },
        ));
    }
    Ok(())
}

fn normalize_names(
    label: &str,
    values: &mut Vec<String>,
    maximum_items: usize,
    maximum_chars: usize,
) -> AppResult<()> {
    if values.len() > maximum_items {
        return Err(AppError::bad_request(
            "too_many_items",
            format!("{label}条目过多"),
        ));
    }
    for value in values.iter_mut() {
        *value = value.trim().to_owned();
        if value.is_empty() || value.chars().count() > maximum_chars || value.contains('\0') {
            return Err(AppError::bad_request(
                "invalid_workspace_policy",
                format!("{label}包含空值、超长值或非法字符"),
            ));
        }
    }
    values.sort();
    values.dedup();
    Ok(())
}

fn validate_sha256(label: &str, value: &str) -> AppResult<()> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    {
        return Err(AppError::bad_request(
            "invalid_digest",
            format!("{label}不是规范 sha256 摘要"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escape_and_platform_paths() {
        for path in [
            "/etc/passwd",
            "../escape",
            "a/../escape",
            "C:/Windows",
            "\\\\server\\share",
            "a//b",
            "a/./b",
            ".git/config",
            "src/.GIT/index",
            ".fudian/state.json",
            "out/name?.txt",
            "out/trailing. ",
            "out/CON.txt",
            "out/com1.log",
            "out/line\nbreak.txt",
        ] {
            assert!(normalize_relative_file_path(path).is_err(), "{path}");
        }
        assert_eq!(
            normalize_relative_file_path("src/lib.rs").unwrap(),
            "src/lib.rs"
        );
    }

    #[test]
    fn pattern_scope_is_monotonic() {
        assert!(pattern_contains("**", "private/**"));
        assert!(pattern_contains("src/**", "src/web/**"));
        assert!(pattern_contains("src/**", "src/lib.rs"));
        assert!(!pattern_contains("src/**", "tests/**"));
        assert!(path_matches_pattern("src/**", "src/web/mod.rs"));
        assert!(!path_matches_pattern("src/**", "README.md"));
    }

    #[test]
    fn default_policy_denies_high_risk_capabilities() {
        let policy = WorkspaceCapabilityPolicy::default().normalize().unwrap();
        let requested = RunnerCapabilities {
            deployment: true,
            ..RunnerCapabilities::default()
        };
        assert!(
            policy
                .authorize(
                    &requested,
                    &["out/**".to_owned()],
                    &RunnerResourceLimits::default()
                )
                .is_err()
        );
    }
}
