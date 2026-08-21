use std::{collections::BTreeMap, future::Future};

use chrono::{DateTime, Utc};
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    goal_domain::{canonical_json_sha256, validate_sha256_id},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissions {
    pub network: String,
    #[serde(default)]
    pub workspace_read: Vec<String>,
    #[serde(default)]
    pub workspace_write: Vec<String>,
    #[serde(default)]
    pub external_writes: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginToolDescriptor {
    pub name: String,
    pub description: String,
    #[serde(default = "empty_object")]
    pub input_schema: Value,
    #[serde(default = "empty_object")]
    pub output_schema: Value,
    pub idempotency: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSkill {
    pub entry: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntime {
    pub kind: String,
    pub content_digest: String,
    pub entrypoint: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginAsset {
    pub path: String,
    pub content_digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePolicy {
    pub cpu_millis: u32,
    pub memory_mi_b: u32,
    pub disk_mi_b: u32,
    pub timeout_seconds: u32,
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self {
            cpu_millis: 1_000,
            memory_mi_b: 256,
            disk_mi_b: 256,
            timeout_seconds: 30,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifestDraft {
    pub schema_version: u32,
    pub plugin_id: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub permissions: PluginPermissions,
    #[serde(default)]
    pub tools: Vec<PluginToolDescriptor>,
    pub skill: Option<PluginSkill>,
    pub runtime: PluginRuntime,
    #[serde(default)]
    pub assets: Vec<PluginAsset>,
    #[serde(default)]
    pub resource_hints: ResourcePolicy,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub schema_version: u32,
    pub plugin_id: String,
    pub version: String,
    pub content_digest: String,
    pub display_name: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub permissions: PluginPermissions,
    pub tools: Vec<PluginToolDescriptor>,
    pub skill: Option<PluginSkill>,
    pub runtime: PluginRuntime,
    pub assets: Vec<PluginAsset>,
    pub resource_hints: ResourcePolicy,
}

impl PluginManifestDraft {
    pub fn seal(mut self) -> AppResult<PluginManifest> {
        if self.schema_version != 1 {
            return Err(AppError::bad_request(
                "unsupported_plugin_schema",
                "只支持 schemaVersion 1 的插件 Manifest",
            ));
        }
        self.plugin_id = normalize_plugin_id(self.plugin_id)?;
        self.version = normalize_exact_version(self.version)?;
        self.display_name = required_text("插件显示名称", self.display_name, 120)?;
        self.description = required_text("插件简介", self.description, 1_000)?;
        normalize_string_list("插件能力", &mut self.capabilities, 100, 120)?;
        normalize_permission(&mut self.permissions)?;
        normalize_tools(&mut self.tools)?;
        if let Some(skill) = &mut self.skill {
            skill.entry = normalize_relative_path("Skill 入口", std::mem::take(&mut skill.entry))?;
        }
        self.runtime.kind = required_text("Runtime 类型", self.runtime.kind, 40)?;
        if !matches!(self.runtime.kind.as_str(), "mock" | "oci" | "wasm") {
            return Err(AppError::bad_request(
                "invalid_plugin_runtime",
                "Runtime 类型必须是 mock、oci 或 wasm",
            ));
        }
        self.runtime.content_digest =
            validate_sha256_id("Runtime 摘要", self.runtime.content_digest)?;
        self.runtime.entrypoint = required_text("Runtime 入口", self.runtime.entrypoint, 200)?;
        for asset in &mut self.assets {
            asset.path = normalize_relative_path("插件 Asset", std::mem::take(&mut asset.path))?;
            asset.content_digest =
                validate_sha256_id("Asset 摘要", std::mem::take(&mut asset.content_digest))?;
        }
        self.assets
            .sort_by(|left, right| left.path.cmp(&right.path));
        reject_duplicate_keys(
            self.assets.iter().map(|asset| asset.path.as_str()),
            "插件 Asset",
        )?;
        validate_resource_policy(&self.resource_hints)?;

        let content_digest = canonical_json_sha256(&self)?;
        Ok(PluginManifest {
            schema_version: self.schema_version,
            plugin_id: self.plugin_id,
            version: self.version,
            content_digest,
            display_name: self.display_name,
            description: self.description,
            capabilities: self.capabilities,
            permissions: self.permissions,
            tools: self.tools,
            skill: self.skill,
            runtime: self.runtime,
            assets: self.assets,
            resource_hints: self.resource_hints,
        })
    }
}

impl PluginManifest {
    pub fn resolved_ref(&self) -> ResolvedPluginRef {
        ResolvedPluginRef {
            plugin_id: self.plugin_id.clone(),
            version: self.version.clone(),
            content_digest: self.content_digest.clone(),
        }
    }

    pub fn summary(&self) -> PluginCatalogEntry {
        PluginCatalogEntry {
            plugin: self.resolved_ref(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            capabilities: self.capabilities.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSelector {
    pub plugin_id: String,
    pub version: String,
}

impl PluginSelector {
    pub fn normalize(mut self) -> AppResult<Self> {
        self.plugin_id = normalize_plugin_id(self.plugin_id)?;
        self.version = self.version.trim().to_owned();
        if self.version.eq_ignore_ascii_case("latest") {
            self.version = "latest".into();
        } else {
            self.version = normalize_exact_version(self.version)?;
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPluginRef {
    pub plugin_id: String,
    pub version: String,
    pub content_digest: String,
}

impl ResolvedPluginRef {
    pub fn validate(mut self) -> AppResult<Self> {
        self.plugin_id = normalize_plugin_id(self.plugin_id)?;
        self.version = normalize_exact_version(self.version)?;
        self.content_digest = validate_sha256_id("插件摘要", self.content_digest)?;
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCatalogEntry {
    pub plugin: ResolvedPluginRef,
    pub display_name: String,
    pub description: String,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BaseRuntime {
    pub kind: String,
    pub digest: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyLock {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPolicy {
    #[serde(default)]
    pub allowed_names: Vec<String>,
    #[serde(default)]
    pub secret_references: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentManifest {
    pub schema_version: u32,
    pub base_runtime: BaseRuntime,
    #[serde(default)]
    pub plugins: Vec<ResolvedPluginRef>,
    #[serde(default)]
    pub toolchains: BTreeMap<String, String>,
    #[serde(default)]
    pub dependency_locks: Vec<DependencyLock>,
    pub target_platform: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub build_parameters: BTreeMap<String, String>,
    pub network_policy: String,
    #[serde(default)]
    pub resource_policy: ResourcePolicy,
    pub environment_policy: EnvironmentPolicy,
}

impl EnvironmentManifest {
    pub fn normalize(mut self) -> AppResult<Self> {
        if self.schema_version != 1 {
            return Err(AppError::bad_request(
                "unsupported_environment_schema",
                "只支持 schemaVersion 1 的 EnvironmentManifest",
            ));
        }
        self.base_runtime.kind = required_text("基础 Runtime 类型", self.base_runtime.kind, 40)?;
        self.base_runtime.digest =
            validate_sha256_id("基础 Runtime 摘要", self.base_runtime.digest)?;
        let mut normalized_plugins = Vec::with_capacity(self.plugins.len());
        for plugin in self.plugins {
            normalized_plugins.push(plugin.validate()?);
        }
        normalized_plugins.sort();
        reject_duplicate_keys(
            normalized_plugins
                .iter()
                .map(|plugin| plugin.plugin_id.as_str()),
            "环境插件 ID",
        )?;
        self.plugins = normalized_plugins;
        normalize_map("工具链", &mut self.toolchains)?;
        for dependency_lock in &mut self.dependency_locks {
            dependency_lock.path =
                normalize_relative_path("依赖锁路径", std::mem::take(&mut dependency_lock.path))?;
            dependency_lock.sha256 =
                normalize_bare_sha256("依赖锁摘要", std::mem::take(&mut dependency_lock.sha256))?;
        }
        self.dependency_locks
            .sort_by(|left, right| left.path.cmp(&right.path));
        reject_duplicate_keys(
            self.dependency_locks.iter().map(|lock| lock.path.as_str()),
            "依赖锁路径",
        )?;
        self.target_platform = required_text("目标平台", self.target_platform, 120)?;
        normalize_string_list("Feature", &mut self.features, 200, 120)?;
        normalize_map("构建参数", &mut self.build_parameters)?;
        self.network_policy = required_text("网络策略", self.network_policy, 40)?;
        if !matches!(
            self.network_policy.as_str(),
            "denied" | "public_read_only" | "granted_capabilities_only"
        ) {
            return Err(AppError::bad_request(
                "invalid_network_policy",
                "未知的环境网络策略",
            ));
        }
        validate_resource_policy(&self.resource_policy)?;
        normalize_string_list(
            "允许的环境变量名",
            &mut self.environment_policy.allowed_names,
            200,
            120,
        )?;
        normalize_string_list(
            "秘密引用",
            &mut self.environment_policy.secret_references,
            100,
            200,
        )?;
        Ok(self)
    }

    pub fn fingerprint(&self) -> AppResult<String> {
        canonical_json_sha256(self)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub call_id: Uuid,
    pub client_request_id: Uuid,
    pub project_id: Uuid,
    pub goal_branch_id: Uuid,
    pub session_id: Uuid,
    pub plugin: ResolvedPluginRef,
    pub tool_name: String,
    pub input: Value,
    pub environment_fingerprint: String,
    pub base_workspace_snapshot: String,
    #[serde(default)]
    pub allowed_writes: Vec<String>,
    pub timeout_seconds: u32,
}

impl ToolCall {
    pub fn validate(mut self, manifest: &PluginManifest) -> AppResult<Self> {
        self.plugin = self.plugin.validate()?;
        if self.plugin != manifest.resolved_ref() {
            return Err(AppError::conflict(
                "plugin_digest_conflict",
                "ToolCall 插件引用与已加载 Manifest 不一致",
            ));
        }
        self.tool_name = required_text("工具名", self.tool_name, 120)?;
        if !manifest
            .tools
            .iter()
            .any(|tool| tool.name == self.tool_name)
        {
            return Err(AppError::bad_request(
                "tool_not_found",
                "插件没有声明这个工具",
            ));
        }
        self.environment_fingerprint =
            validate_sha256_id("环境指纹", self.environment_fingerprint)?;
        self.base_workspace_snapshot =
            validate_sha256_id("工作区快照", self.base_workspace_snapshot)?;
        for allowed_write in &mut self.allowed_writes {
            *allowed_write =
                normalize_relative_pattern("允许写入路径", std::mem::take(allowed_write))?;
            if !manifest
                .permissions
                .workspace_write
                .iter()
                .any(|declared| pattern_contains(declared, allowed_write))
            {
                return Err(AppError::conflict(
                    "tool_not_allowed",
                    "调用请求的写入路径超出插件声明",
                ));
            }
        }
        self.allowed_writes.sort();
        self.allowed_writes.dedup();
        if self.timeout_seconds == 0
            || self.timeout_seconds > manifest.resource_hints.timeout_seconds
        {
            return Err(AppError::bad_request(
                "invalid_tool_timeout",
                "ToolCall 超时必须在插件资源上限内",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolArtifactRef {
    pub artifact_id: Uuid,
    pub sha256: String,
    pub media_type: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub call_id: Uuid,
    pub status: String,
    pub output: Value,
    pub base_workspace_snapshot: String,
    pub result_workspace_snapshot: String,
    pub environment_fingerprint: String,
    #[serde(default)]
    pub change_set: Vec<Value>,
    #[serde(default)]
    pub artifacts: Vec<ToolArtifactRef>,
    #[serde(default)]
    pub evidence: Vec<Value>,
    pub log_reference: Option<String>,
    pub retry_safety: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
}

pub trait ToolBroker {
    fn execute(
        &self,
        manifest: &PluginManifest,
        call: ToolCall,
    ) -> impl Future<Output = AppResult<ToolResult>> + Send;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MockToolBroker;

impl ToolBroker for MockToolBroker {
    async fn execute(&self, manifest: &PluginManifest, call: ToolCall) -> AppResult<ToolResult> {
        let call = call.validate(manifest)?;
        if manifest.runtime.kind != "mock" {
            return Err(AppError::conflict(
                "runner_unavailable",
                "v0.1 MockToolBroker 只能运行 mock Runtime",
            ));
        }
        let started_at = Utc::now();
        let output = match call.tool_name.as_str() {
            "echo" => json!({ "echo": call.input }),
            "inspect" => json!({
                "inputSha256": canonical_json_sha256(&call.input)?,
                "kind": value_kind(&call.input),
            }),
            _ => {
                return Err(AppError::bad_request(
                    "tool_not_found",
                    "参考 Mock Runtime 没有该入口",
                ));
            }
        };
        let completed_at = Utc::now();
        Ok(ToolResult {
            call_id: call.call_id,
            status: "succeeded".into(),
            output,
            base_workspace_snapshot: call.base_workspace_snapshot.clone(),
            result_workspace_snapshot: call.base_workspace_snapshot,
            environment_fingerprint: call.environment_fingerprint,
            change_set: Vec::new(),
            artifacts: Vec::new(),
            evidence: vec![json!({
                "plugin": call.plugin,
                "toolName": call.tool_name,
            })],
            log_reference: None,
            retry_safety: "safe".into(),
            started_at,
            completed_at,
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolLeaseStatus {
    Requested,
    Active,
    Expired,
    Released,
    Failed,
    Cancelled,
}

impl ToolLeaseStatus {
    pub fn activate(self) -> AppResult<Self> {
        match self {
            Self::Requested => Ok(Self::Active),
            _ => Err(lease_transition_error(self, "activate")),
        }
    }

    pub fn heartbeat(self, before_hard_expiry: bool) -> AppResult<Self> {
        match (self, before_hard_expiry) {
            (Self::Active, true) => Ok(Self::Active),
            (Self::Active, false) => Err(AppError::conflict(
                "lease_expired",
                "ToolLease 已经过硬到期时间",
            )),
            _ => Err(lease_transition_error(self, "heartbeat")),
        }
    }

    pub fn release(self) -> AppResult<Self> {
        match self {
            Self::Active => Ok(Self::Released),
            _ => Err(lease_transition_error(self, "release")),
        }
    }

    pub fn expire(self) -> AppResult<Self> {
        match self {
            Self::Active => Ok(Self::Expired),
            _ => Err(lease_transition_error(self, "expire")),
        }
    }

    pub fn cancel(self) -> AppResult<Self> {
        match self {
            Self::Requested | Self::Active => Ok(Self::Cancelled),
            _ => Err(lease_transition_error(self, "cancel")),
        }
    }
}

pub fn reference_mock_plugin(version: &str) -> AppResult<PluginManifest> {
    let runtime_digest = canonical_json_sha256(&format!("fudian.mock.runtime:{version}"))?;
    PluginManifestDraft {
        schema_version: 1,
        plugin_id: "fudian.tools.reference".into(),
        version: version.into(),
        display_name: "Fudian reference tools".into(),
        description: "Deterministic in-process tools for protocol and audit verification".into(),
        capabilities: vec!["text.echo".into(), "value.inspect".into()],
        permissions: PluginPermissions {
            network: "denied".into(),
            workspace_read: vec!["**/*".into()],
            workspace_write: Vec::new(),
            external_writes: false,
        },
        tools: vec![
            PluginToolDescriptor {
                name: "echo".into(),
                description: "Return structured input without side effects".into(),
                input_schema: json!({ "type": "object" }),
                output_schema: json!({ "type": "object" }),
                idempotency: "pure".into(),
            },
            PluginToolDescriptor {
                name: "inspect".into(),
                description: "Return the deterministic hash and JSON kind of an input".into(),
                input_schema: json!({}),
                output_schema: json!({ "type": "object" }),
                idempotency: "pure".into(),
            },
        ],
        skill: None,
        runtime: PluginRuntime {
            kind: "mock".into(),
            content_digest: runtime_digest,
            entrypoint: "built-in".into(),
        },
        assets: Vec::new(),
        resource_hints: ResourcePolicy::default(),
    }
    .seal()
}

fn normalize_plugin_id(value: String) -> AppResult<String> {
    let value = value.trim().to_ascii_lowercase();
    let segments = value.split('.').collect::<Vec<_>>();
    let valid = segments.len() >= 2
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_lowercase())
                && segment.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'-'
                        || byte == b'_'
                })
        });
    if !valid || value.len() > 160 {
        return Err(AppError::bad_request(
            "invalid_plugin_id",
            "插件 ID 必须是小写、分段的稳定命名空间",
        ));
    }
    Ok(value)
}

fn normalize_exact_version(value: String) -> AppResult<String> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("latest")
        || value.contains('*')
        || value.contains('^')
        || value.contains('~')
        || value.contains(['<', '>', '='])
    {
        return Err(AppError::bad_request(
            "unresolved_plugin_version",
            "环境中的插件版本必须是准确 SemVer，不能使用 latest 或范围",
        ));
    }
    Version::parse(value)
        .map(|version| version.to_string())
        .map_err(|_| AppError::bad_request("invalid_plugin_version", "插件版本不是有效 SemVer"))
}

fn normalize_permission(permission: &mut PluginPermissions) -> AppResult<()> {
    permission.network =
        required_text("插件网络权限", std::mem::take(&mut permission.network), 40)?;
    if !matches!(
        permission.network.as_str(),
        "denied" | "public_read_only" | "granted_capabilities_only"
    ) {
        return Err(AppError::bad_request(
            "invalid_plugin_permission",
            "未知的插件网络权限",
        ));
    }
    for pattern in &mut permission.workspace_read {
        *pattern = normalize_relative_pattern("插件读取路径", std::mem::take(pattern))?;
    }
    for pattern in &mut permission.workspace_write {
        *pattern = normalize_relative_pattern("插件写入路径", std::mem::take(pattern))?;
    }
    permission.workspace_read.sort();
    permission.workspace_read.dedup();
    permission.workspace_write.sort();
    permission.workspace_write.dedup();
    Ok(())
}

fn normalize_tools(tools: &mut [PluginToolDescriptor]) -> AppResult<()> {
    if tools.is_empty() || tools.len() > 100 {
        return Err(AppError::bad_request(
            "invalid_plugin_tools",
            "插件必须声明 1 到 100 个工具",
        ));
    }
    for tool in tools.iter_mut() {
        tool.name = required_text("工具名", std::mem::take(&mut tool.name), 120)?;
        if !tool.name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        }) {
            return Err(AppError::bad_request(
                "invalid_tool_name",
                "工具名只能使用小写字母、数字、点、横线和下划线",
            ));
        }
        tool.description = required_text("工具简介", std::mem::take(&mut tool.description), 1_000)?;
        if !tool.input_schema.is_object() || !tool.output_schema.is_object() {
            return Err(AppError::bad_request(
                "invalid_tool_schema",
                "工具输入和输出 schema 必须是 JSON 对象",
            ));
        }
        tool.idempotency =
            required_text("工具幂等类型", std::mem::take(&mut tool.idempotency), 40)?;
        if !matches!(tool.idempotency.as_str(), "pure" | "safe" | "unsafe") {
            return Err(AppError::bad_request(
                "invalid_tool_idempotency",
                "工具幂等类型必须是 pure、safe 或 unsafe",
            ));
        }
    }
    tools.sort_by(|left, right| left.name.cmp(&right.name));
    reject_duplicate_keys(tools.iter().map(|tool| tool.name.as_str()), "工具名")
}

fn validate_resource_policy(policy: &ResourcePolicy) -> AppResult<()> {
    if policy.cpu_millis == 0
        || policy.memory_mi_b == 0
        || policy.disk_mi_b == 0
        || policy.timeout_seconds == 0
        || policy.timeout_seconds > 86_400
    {
        return Err(AppError::bad_request(
            "invalid_resource_policy",
            "资源策略必须为正数，且超时不能超过 24 小时",
        ));
    }
    Ok(())
}

fn normalize_map(label: &str, values: &mut BTreeMap<String, String>) -> AppResult<()> {
    if values.len() > 200 {
        return Err(AppError::bad_request(
            "too_many_items",
            format!("{label}条目过多"),
        ));
    }
    let original = std::mem::take(values);
    for (key, value) in original {
        let key = required_text(label, key, 120)?;
        let value = required_text(label, value, 1_000)?;
        values.insert(key, value);
    }
    Ok(())
}

fn normalize_string_list(
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
        *value = required_text(label, std::mem::take(value), max_chars)?;
    }
    values.sort();
    values.dedup();
    Ok(())
}

fn normalize_relative_path(label: &str, value: String) -> AppResult<String> {
    let value = required_text(label, value.replace('\\', "/"), 1_000)?;
    let invalid = value.starts_with('/')
        || value.contains('\0')
        || value.contains(':')
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."));
    if invalid {
        return Err(AppError::bad_request(
            "unsafe_artifact_path",
            format!("{label}必须是规范相对路径"),
        ));
    }
    Ok(value)
}

fn normalize_relative_pattern(label: &str, value: String) -> AppResult<String> {
    let value = required_text(label, value.replace('\\', "/"), 1_000)?;
    let invalid = value.starts_with('/')
        || value.contains('\0')
        || value.contains(':')
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."));
    if invalid {
        return Err(AppError::bad_request(
            "unsafe_artifact_path",
            format!("{label}必须是安全的相对路径模式"),
        ));
    }
    Ok(value)
}

fn normalize_bare_sha256(label: &str, value: String) -> AppResult<String> {
    let value = value.trim().to_ascii_lowercase();
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::bad_request(
            "invalid_digest",
            format!("{label}不是有效的 SHA-256 摘要"),
        ));
    }
    Ok(value)
}

fn pattern_contains(declared: &str, requested: &str) -> bool {
    declared == "**/*"
        || declared == requested
        || declared
            .strip_suffix("/**")
            .is_some_and(|prefix| requested.starts_with(prefix))
}

fn reject_duplicate_keys<'a>(keys: impl Iterator<Item = &'a str>, label: &str) -> AppResult<()> {
    let mut previous: Option<&str> = None;
    for key in keys {
        if previous == Some(key) {
            return Err(AppError::bad_request(
                "duplicate_item",
                format!("{label}不能重复"),
            ));
        }
        previous = Some(key);
    }
    Ok(())
}

fn required_text(label: &str, value: String, max: usize) -> AppResult<String> {
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

fn empty_object() -> Value {
    json!({})
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn lease_transition_error(status: ToolLeaseStatus, action: &str) -> AppError {
    AppError::conflict(
        "invalid_state_transition",
        format!("ToolLease 当前状态 {status:?} 不允许 {action}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(plugin: ResolvedPluginRef, lock_hash: &str) -> EnvironmentManifest {
        EnvironmentManifest {
            schema_version: 1,
            base_runtime: BaseRuntime {
                kind: "mock".into(),
                digest: format!("sha256:{}", "b".repeat(64)),
            },
            plugins: vec![plugin],
            toolchains: BTreeMap::from([("rust".into(), "1.97.0".into())]),
            dependency_locks: vec![DependencyLock {
                path: "Cargo.lock".into(),
                sha256: lock_hash.into(),
            }],
            target_platform: "x86_64-unknown-linux-gnu".into(),
            features: vec!["default".into()],
            build_parameters: BTreeMap::new(),
            network_policy: "denied".into(),
            resource_policy: ResourcePolicy::default(),
            environment_policy: EnvironmentPolicy {
                allowed_names: vec!["CI".into()],
                secret_references: Vec::new(),
            },
        }
    }

    #[test]
    fn reference_plugin_is_sealed_with_exact_version_and_digest() {
        let plugin = reference_mock_plugin("1.0.0").unwrap();
        assert_eq!(plugin.plugin_id, "fudian.tools.reference");
        assert_eq!(plugin.version, "1.0.0");
        assert!(plugin.content_digest.starts_with("sha256:"));
        assert_eq!(plugin.tools[0].name, "echo");
    }

    #[test]
    fn manifest_rejects_latest_as_a_persisted_version() {
        let mut draft = reference_mock_plugin("1.0.0").unwrap();
        draft.version = "latest".into();
        let reference = PluginManifestDraft {
            schema_version: draft.schema_version,
            plugin_id: draft.plugin_id,
            version: draft.version,
            display_name: draft.display_name,
            description: draft.description,
            capabilities: draft.capabilities,
            permissions: draft.permissions,
            tools: draft.tools,
            skill: draft.skill,
            runtime: draft.runtime,
            assets: draft.assets,
            resource_hints: draft.resource_hints,
        };
        assert_eq!(
            reference.seal().unwrap_err().code(),
            "unresolved_plugin_version"
        );
    }

    #[test]
    fn environment_fingerprint_is_stable_but_lock_sensitive() {
        let plugin = reference_mock_plugin("1.0.0").unwrap().resolved_ref();
        let first = environment(plugin.clone(), &"a".repeat(64))
            .normalize()
            .unwrap();
        let reordered = environment(plugin, &"a".repeat(64)).normalize().unwrap();
        assert_eq!(
            first.fingerprint().unwrap(),
            reordered.fingerprint().unwrap()
        );

        let changed = environment(
            reference_mock_plugin("1.0.0").unwrap().resolved_ref(),
            &"c".repeat(64),
        )
        .normalize()
        .unwrap();
        assert_ne!(first.fingerprint().unwrap(), changed.fingerprint().unwrap());
    }

    #[tokio::test]
    async fn mock_broker_binds_result_to_call_environment_and_snapshot() {
        let manifest = reference_mock_plugin("1.0.0").unwrap();
        let environment_fingerprint = format!("sha256:{}", "d".repeat(64));
        let workspace_snapshot = format!("sha256:{}", "e".repeat(64));
        let call = ToolCall {
            call_id: Uuid::new_v4(),
            client_request_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            goal_branch_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            plugin: manifest.resolved_ref(),
            tool_name: "inspect".into(),
            input: json!({ "answer": 42 }),
            environment_fingerprint: environment_fingerprint.clone(),
            base_workspace_snapshot: workspace_snapshot.clone(),
            allowed_writes: Vec::new(),
            timeout_seconds: 10,
        };
        let result = MockToolBroker.execute(&manifest, call).await.unwrap();
        assert_eq!(result.status, "succeeded");
        assert_eq!(result.environment_fingerprint, environment_fingerprint);
        assert_eq!(result.base_workspace_snapshot, workspace_snapshot);
        assert_eq!(result.result_workspace_snapshot, workspace_snapshot);
        assert!(result.change_set.is_empty());
    }

    #[test]
    fn lease_transitions_are_explicit() {
        assert_eq!(
            ToolLeaseStatus::Requested.activate().unwrap(),
            ToolLeaseStatus::Active
        );
        assert_eq!(
            ToolLeaseStatus::Active.release().unwrap(),
            ToolLeaseStatus::Released
        );
        assert_eq!(
            ToolLeaseStatus::Active.heartbeat(false).unwrap_err().code(),
            "lease_expired"
        );
    }
}
