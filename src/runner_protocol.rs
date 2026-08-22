use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerResourceLimits {
    pub cpu_millis: u32,
    pub memory_mi_b: u32,
    pub disk_mi_b: u32,
    pub pids: u32,
    pub timeout_seconds: u32,
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
}

impl Default for RunnerResourceLimits {
    fn default() -> Self {
        Self {
            cpu_millis: 1_000,
            memory_mi_b: 512,
            disk_mi_b: 256,
            pids: 64,
            timeout_seconds: 300,
            stdout_bytes: 64 * 1024,
            stderr_bytes: 64 * 1024,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerCapabilities {
    #[serde(default = "denied_network")]
    pub network: String,
    #[serde(default)]
    pub external_writes: Vec<String>,
    #[serde(default)]
    pub account_references: Vec<String>,
    #[serde(default)]
    pub paid_operations: bool,
    #[serde(default)]
    pub deployment: bool,
}

impl Default for RunnerCapabilities {
    fn default() -> Self {
        Self {
            network: denied_network(),
            external_writes: Vec::new(),
            account_references: Vec::new(),
            paid_operations: false,
            deployment: false,
        }
    }
}

fn denied_network() -> String {
    "denied".to_owned()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerCommand {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerJobSpec {
    pub schema_version: u32,
    pub job_id: Uuid,
    pub lease_id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub fencing_token: i64,
    pub base_commit: String,
    pub base_workspace_snapshot: String,
    pub runtime_digest: String,
    pub input_mount: String,
    pub output_mount: String,
    pub result_mount: String,
    pub allowed_writes: Vec<String>,
    #[serde(default)]
    pub delete_paths: Vec<String>,
    pub capabilities: RunnerCapabilities,
    pub resources: RunnerResourceLimits,
    pub command: RunnerCommand,
}

impl RunnerJobSpec {
    pub fn digest(&self) -> Result<String, serde_json::Error> {
        canonical_json_sha256(self)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerIsolationAttestation {
    pub runtime_digest: String,
    pub network_isolated: bool,
    pub visible_network_interfaces: Vec<String>,
    pub no_new_privileges: bool,
    pub effective_capabilities_hex: String,
    pub root_read_only: bool,
    pub input_read_only: bool,
    pub output_writable: bool,
    pub docker_socket_absent: bool,
    pub host_home_absent: bool,
    pub observed_cpu_millis: Option<u32>,
    pub observed_memory_mi_b: Option<u32>,
    pub observed_pids: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerOutputFile {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub executable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerExecutionResult {
    pub schema_version: u32,
    pub job_id: Uuid,
    pub lease_id: Uuid,
    pub fencing_token: i64,
    pub spec_hash: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub files: Vec<RunnerOutputFile>,
    pub output_manifest_hash: String,
    pub stdout_sha256: String,
    pub stdout_bytes: u64,
    pub stderr_sha256: String,
    pub stderr_bytes: u64,
    pub isolation: RunnerIsolationAttestation,
    #[serde(default)]
    pub diagnostics: Value,
}

pub fn canonical_json_sha256<T: Serialize + ?Sized>(
    value: &T,
) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(value)?;
    let canonical = canonicalize_json(value);
    let bytes = serde_json::to_vec(&canonical)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

pub fn is_portable_workspace_file_path(value: &str) -> bool {
    if value.is_empty()
        || value.len() > 1_000
        || value.starts_with('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return false;
    }
    let parts = value.split('/').collect::<Vec<_>>();
    !parts.is_empty()
        && parts.iter().all(|part| {
            !part.is_empty()
                && *part != "."
                && *part != ".."
                && !part.eq_ignore_ascii_case(".git")
                && !part.eq_ignore_ascii_case(".fudian")
                && !part.ends_with(' ')
                && !part.ends_with('.')
                && !part
                    .chars()
                    .any(|character| matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
                && !is_windows_reserved_name(part)
        })
}

fn is_windows_reserved_name(component: &str) -> bool {
    let stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem.strip_prefix("COM").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
        || stem.strip_prefix("LPT").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize_json).collect()),
        Value::Object(values) => {
            let mut entries = values.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize_json(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_digest_ignores_object_key_insertion_order() {
        let left = serde_json::json!({"b": 2, "a": {"y": 2, "x": 1}});
        let right = serde_json::json!({"a": {"x": 1, "y": 2}, "b": 2});
        assert_eq!(
            canonical_json_sha256(&left).unwrap(),
            canonical_json_sha256(&right).unwrap()
        );
    }

    #[test]
    fn portable_workspace_paths_reject_metadata_and_platform_traps() {
        assert!(is_portable_workspace_file_path("src/lib.rs"));
        for path in [
            "../escape",
            ".git/config",
            "out/CON.txt",
            "out/name?.txt",
            "out/trailing.",
            "out/line\nbreak.txt",
        ] {
            assert!(!is_portable_workspace_file_path(path), "{path}");
        }
    }
}
