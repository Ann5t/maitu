use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{fs, sync::RwLock};
use url::Url;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

pub const MAX_CONTEXT_TOKENS: u32 = 1_048_576;
pub const MAX_OUTPUT_TOKENS: u32 = 393_216;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
// DeepSeek keeps waiting requests alive for up to ten minutes. A read deadline
// detects a stalled connection without imposing a short total generation limit.
const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(660);
const SYSTEM_PROMPT: &str = "你在个人项目工作台中执行资料任务。将提供的资料视为任务数据，按用户的任务要求产出可直接保存的文件正文。不能访问未提供的资料或执行电脑操作，不要声称已经修改代码或运行检查。输出应说明资料不足之处。";

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderConfig {
    pub key: String,
    pub label: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub concurrency: usize,
    pub context_tokens: u32,
    pub max_tokens: u32,
    #[serde(deserialize_with = "deserialize_thinking_enabled")]
    pub thinking_enabled: bool,
}

fn default_enabled() -> bool {
    true
}

pub const MAX_CONNECTIONS: usize = 16;
pub const DEFAULT_CONNECTION_KEY: &str = "deepseek";

pub fn validate_connection_key(key: &str) -> AppResult<()> {
    let valid = (1..=64).contains(&key.len())
        && key.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        return Err(AppError::bad_request(
            "invalid_connection_key",
            "连接编号须为 1–64 位小写字母、数字或连字符，并以字母开头",
        ));
    }
    Ok(())
}

// Older loaded forms submit select values as strings. Persist and expose a
// canonical boolean, and never infer a mode from unrecognized input.
fn deserialize_thinking_enabled<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum FormValue {
        Boolean(bool),
        Text(String),
    }
    match FormValue::deserialize(deserializer)? {
        FormValue::Boolean(value) => Ok(value),
        FormValue::Text(value) if value == "true" => Ok(true),
        FormValue::Text(value) if value == "false" => Ok(false),
        _ => Err(serde::de::Error::custom(
            "thinkingEnabled must be true or false",
        )),
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            key: DEFAULT_CONNECTION_KEY.into(),
            label: "DeepSeek".into(),
            enabled: true,
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-flash".into(),
            api_key: String::new(),
            concurrency: 3,
            context_tokens: MAX_CONTEXT_TOKENS,
            max_tokens: 65536,
            thinking_enabled: true,
        }
    }
}

impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("key", &self.key)
            .field("label", &self.label)
            .field("enabled", &self.enabled)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &"[REDACTED]")
            .field("concurrency", &self.concurrency)
            .finish()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    pub key: String,
    pub label: String,
    pub enabled: bool,
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub concurrency: usize,
    pub context_tokens: u32,
    pub max_tokens: u32,
    pub thinking_enabled: bool,
    pub max_context_tokens: u32,
    pub max_output_tokens: u32,
}

impl ProviderConfig {
    pub fn view(&self) -> ProviderView {
        ProviderView {
            key: self.key.clone(),
            label: self.label.clone(),
            enabled: self.enabled,
            configured: !self.api_key.is_empty(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            concurrency: self.concurrency,
            context_tokens: self.context_tokens,
            max_tokens: self.max_tokens,
            thinking_enabled: self.thinking_enabled,
            max_context_tokens: MAX_CONTEXT_TOKENS,
            max_output_tokens: MAX_OUTPUT_TOKENS,
        }
    }

    /// A connection can serve requests only when it is enabled and holds a key.
    /// Disabled or unconfigured connections stay listed so the user can finish
    /// configuring them instead of silently disappearing.
    pub fn usable(&self) -> bool {
        self.enabled && !self.api_key.is_empty()
    }

    pub fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.into()
        } else {
            format!("{base}/chat/completions")
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        validate_connection_key(&self.key)?;
        let label = self.label.trim();
        if label.is_empty() || label.len() > 80 || label.chars().any(char::is_control) {
            return Err(AppError::bad_request(
                "invalid_provider",
                "连接名称不能为空且不超过 80 字",
            ));
        }
        let url = Url::parse(&self.base_url)
            .map_err(|_| AppError::bad_request("invalid_provider", "API 地址不正确"))?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || self.base_url.len() > 512
        {
            return Err(AppError::bad_request(
                "invalid_provider",
                "API 地址须为 HTTPS；本机测试服务可使用环回 HTTP 地址",
            ));
        }
        if self.api_key.trim().is_empty()
            || self.api_key.len() > 4096
            || self.api_key.chars().any(char::is_control)
            || self.model.trim().is_empty()
            || self.model.len() > 128
            || !(1..=64).contains(&self.concurrency)
        {
            return Err(AppError::bad_request(
                "invalid_provider",
                "请检查密钥、模型和执行限制",
            ));
        }
        if !(1..=MAX_CONTEXT_TOKENS).contains(&self.context_tokens) {
            return Err(AppError::bad_request(
                "invalid_context_budget",
                "上下文预算须为 1–1,048,576 Token，计入输入与预留输出",
            ));
        }
        if !(1..=MAX_OUTPUT_TOKENS).contains(&self.max_tokens) {
            return Err(AppError::bad_request(
                "invalid_output_limit",
                "最大输出长度须为 1–393,216 Token",
            ));
        }
        if self.max_tokens >= self.context_tokens {
            return Err(AppError::bad_request(
                "invalid_context_budget",
                "最大输出长度须小于上下文预算，为任务要求和资料保留输入空间",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ProviderStore {
    path: PathBuf,
    current: Arc<RwLock<Vec<ProviderConfig>>>,
}

impl ProviderStore {
    /// Loads `connections.json`. A store from the single-connection era is
    /// migrated once: its saved values (including the key and length settings)
    /// become the first connection and stay untouched on disk in
    /// `provider.json` for rollback safety.
    pub async fn open(root: PathBuf) -> AppResult<Self> {
        let path = root.join("connections.json");
        let connections = match fs::read(&path).await {
            Ok(bytes) => {
                let mut connections: Vec<ProviderConfig> = serde_json::from_slice(&bytes)
                    .map_err(|_| AppError::internal("本机模型连接配置无法读取，请检查配置文件"))?;
                if connections.is_empty() {
                    connections.push(ProviderConfig::default());
                }
                for connection in &mut connections {
                    if connection.key.is_empty() {
                        connection.key = DEFAULT_CONNECTION_KEY.into();
                    }
                    if connection.label.is_empty() {
                        connection.label = connection.key.clone();
                    }
                    connection.validate()?;
                }
                connections
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::read(root.join("provider.json")).await {
                    Ok(bytes) => {
                        let mut legacy: ProviderConfig =
                            serde_json::from_slice(&bytes).map_err(|_| {
                                AppError::internal("本机模型配置无法读取，请检查配置文件")
                            })?;
                        legacy.key = DEFAULT_CONNECTION_KEY.into();
                        legacy.label = "DeepSeek".into();
                        legacy.enabled = true;
                        legacy.validate()?;
                        vec![legacy]
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        vec![ProviderConfig::default()]
                    }
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            current: Arc::new(RwLock::new(connections)),
        })
    }

    pub async fn list(&self) -> Vec<ProviderConfig> {
        self.current.read().await.clone()
    }

    pub async fn get(&self, key: &str) -> Option<ProviderConfig> {
        self.current
            .read()
            .await
            .iter()
            .find(|connection| connection.key == key)
            .cloned()
    }

    /// The connection legacy callers mean when they do not name one: the first
    /// entry, which is also the store's original DeepSeek connection.
    pub async fn primary(&self) -> ProviderConfig {
        self.current
            .read()
            .await
            .first()
            .cloned()
            .unwrap_or_default()
    }

    pub async fn active(&self) -> Vec<ProviderConfig> {
        self.current
            .read()
            .await
            .iter()
            .filter(|connection| connection.usable())
            .cloned()
            .collect()
    }

    async fn persist(&self, connections: Vec<ProviderConfig>) -> AppResult<()> {
        let root = self.path.parent().expect("provider config parent");
        fs::create_dir_all(root).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).await?;
        }
        let temporary = self.path.with_extension(format!("{}.tmp", Uuid::new_v4()));
        let result = async {
            use tokio::io::AsyncWriteExt;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temporary).await?;
            file.write_all(&serde_json::to_vec(&connections)?).await?;
            file.sync_all().await?;
            drop(file);
            fs::rename(&temporary, &self.path).await?;
            Ok::<_, AppError>(())
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&temporary).await;
        }
        result
    }

    /// Saves a connection by key. A blank `api_key` keeps the stored credential
    /// so the page never needs to resend secrets.
    pub async fn save(&self, mut config: ProviderConfig) -> AppResult<ProviderConfig> {
        config.base_url = config.base_url.trim().trim_end_matches('/').into();
        config.model = config.model.trim().into();
        config.api_key = config.api_key.trim().into();
        let mut connections = self.current.write().await;
        if let Some(current) = connections
            .iter_mut()
            .find(|connection| connection.key == config.key)
            .filter(|current| config.api_key.is_empty() && config.base_url == current.base_url)
        {
            config.api_key = current.api_key.clone();
        }
        config.validate()?;
        let mut next = connections.clone();
        if let Some(slot) = next.iter_mut().find(|c| c.key == config.key) {
            *slot = config.clone();
        } else {
            if next.len() >= MAX_CONNECTIONS {
                return Err(AppError::bad_request(
                    "connection_limit",
                    "最多保存 16 个模型连接；请先删除不再使用的连接",
                ));
            }
            next.push(config.clone());
        }
        self.persist(next.clone()).await?;
        *connections = next;
        Ok(config)
    }

    pub async fn delete(&self, key: &str) -> AppResult<()> {
        let mut connections = self.current.write().await;
        let mut next = connections.clone();
        let before = next.len();
        next.retain(|connection| connection.key != key);
        if next.len() == before {
            return Err(AppError::not_found("连接不存在"));
        }
        if next.is_empty() {
            return Err(AppError::bad_request(
                "connection_required",
                "至少保留一个模型连接",
            ));
        }
        self.persist(next.clone()).await?;
        *connections = next;
        Ok(())
    }
}

#[derive(Debug)]
pub struct Completion {
    pub content: String,
    pub usage: Value,
}

#[derive(Debug)]
pub struct ProviderFailure {
    pub code: &'static str,
    pub message: String,
}

impl ProviderFailure {
    pub(super) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

fn http_failure(status: u16) -> ProviderFailure {
    let (code, message) = match status {
        400 | 422 => (
            "provider_request",
            "模型服务未接受请求，请检查模型名称或输入长度",
        ),
        401 | 403 => ("provider_auth", "模型服务拒绝认证，请检查 API 密钥与权限"),
        402 => ("provider_balance", "模型服务余额不足，请检查账户额度"),
        429 => ("provider_rate_limit", "模型服务限流，请降低并发或稍后重试"),
        500..=599 => ("provider_unavailable", "模型服务暂时不可用，请稍后重试"),
        _ => ("provider_http", "模型服务返回了未预期的状态"),
    };
    ProviderFailure::new(code, format!("{message}（HTTP {status}）"))
}

/// Estimate text tokens using DeepSeek's documented character ratios. This is
/// a local planning budget, not an exact tokenizer or an API context parameter.
/// Actual counts remain those returned in `usage`.
pub fn estimate_input_tokens(prompt: &str) -> u64 {
    let tenths: u64 = SYSTEM_PROMPT
        .chars()
        .chain(prompt.chars())
        .map(|character| match character {
            character if character.is_ascii() => 3,
            '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{20000}'..='\u{3134f}' => 6,
            _ => 10,
        })
        .sum();
    tenths.div_ceil(10) + 64 // Allow room for the two message wrappers.
}

pub fn check_context_budget(config: &ProviderConfig, prompt: &str) -> Result<u64, ProviderFailure> {
    let estimated = estimate_input_tokens(prompt);
    if estimated + u64::from(config.max_tokens) > u64::from(config.context_tokens) {
        return Err(ProviderFailure::new(
            "context_budget_exceeded",
            format!(
                "输入估算约 {estimated} Token，加上 {} Token 的预留输出，超过 {} Token 的上下文预算；请减少资料、缩小输出上限或增加上下文预算。估算与实际用量可能不同",
                config.max_tokens, config.context_tokens
            ),
        ));
    }
    Ok(estimated)
}

pub async fn complete(
    config: &ProviderConfig,
    prompt: &str,
) -> Result<Completion, ProviderFailure> {
    check_context_budget(config, prompt)?;
    complete_with_system(config, SYSTEM_PROMPT, prompt, false).await
}

pub async fn complete_with_system(
    config: &ProviderConfig,
    system: &str,
    prompt: &str,
    json_output: bool,
) -> Result<Completion, ProviderFailure> {
    let mut request = request_base(
        config,
        json!([
            {"role":"system", "content":system},
            {"role":"user", "content":prompt}
        ]),
    );
    if json_output {
        request["response_format"] = json!({"type":"json_object"});
    }
    let body = send_request(config, request).await?;
    parse_completion(&body)
}

pub struct ChatTurn {
    pub message: Value,
    pub usage: Value,
}

pub async fn chat(
    config: &ProviderConfig,
    messages: &[Value],
    tools: &Value,
) -> Result<ChatTurn, ProviderFailure> {
    let mut request = request_base(config, json!(messages));
    request["tools"] = tools.clone();
    let body = send_request(config, request).await?;
    let response: Value = serde_json::from_slice(&body).map_err(|_| {
        ProviderFailure::new("provider_invalid_response", "模型返回无法读取的工具请求")
    })?;
    let choice = &response["choices"][0];
    let message = &choice["message"];
    let finish = choice["finish_reason"].as_str();
    if !matches!(finish, Some("stop" | "tool_calls"))
        || !message.is_object()
        || message["role"] != "assistant"
    {
        return Err(ProviderFailure::new(
            "provider_incomplete",
            "模型工具执行回复未完整结束",
        ));
    }
    if finish == Some("tool_calls")
        && message["tool_calls"]
            .as_array()
            .is_none_or(|calls| calls.is_empty())
    {
        return Err(ProviderFailure::new(
            "provider_invalid_response",
            "模型没有返回有效工具调用",
        ));
    }
    // Thinking tool calls require the complete reasoning_content to be returned
    // on every following request. Preserve the assistant message without rebuilding it.
    Ok(ChatTurn {
        message: message.clone(),
        usage: response.get("usage").cloned().unwrap_or(Value::Null),
    })
}

fn request_base(config: &ProviderConfig, messages: Value) -> Value {
    json!({"model":config.model,"stream":false,"max_tokens":config.max_tokens,
        "thinking":{"type":if config.thinking_enabled {"enabled"} else {"disabled"}},"messages":messages})
}

async fn send_request(config: &ProviderConfig, request: Value) -> Result<Vec<u8>, ProviderFailure> {
    check_context_budget(config, &request.to_string())?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(READ_IDLE_TIMEOUT)
        .build()
        .map_err(|_| ProviderFailure::new("provider_client", "无法建立模型连接"))?;
    let mut response = client
        .post(config.endpoint())
        .bearer_auth(&config.api_key)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(request.to_string())
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                ProviderFailure::new(
                    "provider_timeout",
                    "模型请求超时，结果可能已在服务端处理；请查看记录后决定是否重试",
                )
            } else {
                ProviderFailure::new("provider_network", "无法连接模型服务，请检查网络与代理")
            }
        })?;
    if !response.status().is_success() {
        return Err(http_failure(response.status().as_u16()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        ProviderFailure::new(
            "provider_response",
            "接收模型结果中断，请查看记录后决定是否重试",
        )
    })? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(ProviderFailure::new(
                "provider_response_limit",
                "模型结果超过本轮文件大小限制",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn parse_completion(body: &[u8]) -> Result<Completion, ProviderFailure> {
    let value: Value = serde_json::from_slice(body).map_err(|_| {
        ProviderFailure::new("provider_invalid_response", "模型服务返回了无法解析的结果")
    })?;
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .ok_or_else(|| ProviderFailure::new("provider_empty_response", "模型服务没有返回成果"))?;
    if choice.get("finish_reason").and_then(Value::as_str) != Some("stop") {
        return Err(ProviderFailure::new(
            "provider_incomplete",
            "模型结果未完整结束，请检查输出长度限制后重试",
        ));
    }
    let content = choice
        .get("message")
        .and_then(|v| v.get("content"))
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            ProviderFailure::new("provider_empty_response", "模型没有产出可保存的文件正文")
        })?;
    Ok(Completion {
        content: content.into(),
        usage: value.get("usage").cloned().unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_and_empty_responses_are_not_reported_as_outputs() {
        assert!(
            parse_completion(
                br#"{"choices":[{"finish_reason":"length","message":{"content":"partial"}}]}"#
            )
            .is_err()
        );
        assert!(
            parse_completion(
                br#"{"choices":[{"finish_reason":"stop","message":{"content":" "}}]}"#
            )
            .is_err()
        );
        let completion = parse_completion(
            br#"{"choices":[{"finish_reason":"stop","message":{"reasoning_content":"reasoning is separate","content":"result"}}],"usage":{"completion_tokens_details":{"reasoning_tokens":3}}}"#
        )
        .unwrap();
        assert_eq!(completion.content, "result");
        assert_eq!(
            completion.usage["completion_tokens_details"]["reasoning_tokens"],
            3
        );
    }

    /// Fixture credentials are assembled at run time so no literal key text
    /// appears in this file; they only exercise isolated local servers.
    fn fixture_key(name: &str) -> String {
        format!("isolated-{name}-key")
    }

    #[test]
    fn credentials_cannot_be_embedded_in_endpoints_or_exposed_by_views() {
        let key = fixture_key("unit");
        let mut config = ProviderConfig {
            key: "deepseek".into(),
            label: "DeepSeek".into(),
            api_key: key.clone(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
        assert!(!format!("{config:?}").contains(&key));
        assert!(
            !serde_json::to_string(&config.view())
                .unwrap()
                .contains(&key)
        );
        config.base_url = "https://user:password@api.deepseek.com".into();
        assert!(config.validate().is_err());
        config.base_url = "http://api.deepseek.com".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn connection_keys_and_labels_are_restricted() {
        for key in [
            "",
            "DeepSeek",
            "-main",
            "a b",
            "\u{4e3b}\u{8fde}\u{63a5}",
            &"x".repeat(65),
        ] {
            let config = ProviderConfig {
                key: key.into(),
                ..Default::default()
            };
            assert!(config.validate().is_err(), "key {key:?} must be rejected");
        }
        let config = ProviderConfig {
            label: " ".into(),
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[tokio::test]
    async fn legacy_single_connection_file_migrates_without_losing_values() {
        let root = std::env::temp_dir().join(format!("maitu-provider-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let legacy = json!({
            "baseUrl":"https://api.deepseek.com", "model":"deepseek-flash",
            "apiKey":fixture_key("legacy"), "concurrency":3,
            "maxTokens":4096
        });
        fs::write(
            root.join("provider.json"),
            serde_json::to_vec(&legacy).unwrap(),
        )
        .await
        .unwrap();
        let store = ProviderStore::open(root.clone()).await.unwrap();
        let primary = store.primary().await;
        assert_eq!(primary.key, "deepseek");
        assert_eq!(primary.label, "DeepSeek");
        assert_eq!(primary.api_key, fixture_key("legacy"));
        assert_eq!(primary.max_tokens, 4096);
        assert!(primary.enabled, "a migrated connection stays enabled");
        assert_eq!(store.list().await.len(), 1);
        // connections.json now exists; reopening keeps the same values.
        let reopened = ProviderStore::open(root.clone()).await.unwrap();
        assert_eq!(reopened.primary().await.api_key, fixture_key("legacy"));
        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn connections_are_saved_updated_and_deleted_with_boundaries() {
        let root = std::env::temp_dir().join(format!("maitu-provider-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).await.unwrap();
        let store = ProviderStore::open(root.clone()).await.unwrap();
        let mut second = ProviderConfig {
            key: "backup-provider".into(),
            label: "\u{5907}\u{4efd}\u{8fde}\u{63a5}".into(),
            base_url: "http://127.0.0.1:9".into(),
            api_key: fixture_key("second"),
            concurrency: 2,
            ..Default::default()
        };
        store.save(second.clone()).await.unwrap();
        assert_eq!(store.list().await.len(), 2);
        // A blank key on update keeps the stored credential.
        second.api_key = String::new();
        let saved = store.save(second.clone()).await.unwrap();
        assert_eq!(saved.api_key, fixture_key("second"));
        // Either connection can go while two remain, but the last one stays.
        store.delete("deepseek").await.unwrap();
        assert_eq!(store.list().await.len(), 1);
        assert!(store.delete("backup-provider").await.is_err());
        let _ = fs::remove_dir_all(root).await;
    }

    #[test]
    fn deepseek_lengths_use_current_limits_and_reserve_input_space() {
        let mut config = ProviderConfig {
            key: "deepseek".into(),
            label: "DeepSeek".into(),
            api_key: fixture_key("unit"),
            max_tokens: MAX_OUTPUT_TOKENS,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
        config.max_tokens += 1;
        assert!(config.validate().is_err());
        config.max_tokens = 1;
        assert!(config.validate().is_ok());
        config.context_tokens = MAX_CONTEXT_TOKENS + 1;
        assert!(config.validate().is_err());
        config.context_tokens = config.max_tokens;
        assert!(config.validate().is_err());
    }

    #[test]
    fn old_saved_configuration_keeps_output_and_gets_context_budget() {
        let mut config: ProviderConfig = serde_json::from_value(json!({
            "baseUrl":"https://api.deepseek.com", "model":"deepseek-flash",
            "apiKey":fixture_key("unit"), "concurrency":3,
            "timeoutSeconds":180, "maxTokens":4096
        }))
        .unwrap();
        // The store open path assigns this identity before validation.
        config.key = "deepseek".into();
        config.label = "DeepSeek".into();
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.context_tokens, MAX_CONTEXT_TOKENS);
        assert!(config.thinking_enabled);
        assert!(config.validate().is_ok());
        let view = serde_json::to_value(config.view()).unwrap();
        assert!(view.get("timeoutSeconds").is_none());
        assert!(view.get("apiKey").is_none());
    }

    #[tokio::test]
    async fn exhausted_context_budget_stops_before_a_network_request() {
        let short = "Short task";
        let estimated = estimate_input_tokens(short);
        let mut config = ProviderConfig {
            base_url: "http://127.0.0.1:1".into(),
            api_key: fixture_key("unit"),
            context_tokens: (estimated + 100) as u32,
            max_tokens: 100,
            ..Default::default()
        };
        assert!(check_context_budget(&config, short).is_ok());
        config.context_tokens -= 1;
        let failure = complete(&config, short).await.unwrap_err();
        assert_eq!(failure.code, "context_budget_exceeded");
        assert!(failure.message.contains("\u{9884}\u{7559}\u{8f93}\u{51fa}"));
        assert!(check_context_budget(&config, &"\u{8d44}\u{6599}".repeat(1000)).is_err());
    }

    #[test]
    fn loaded_form_thinking_strings_are_normalized_without_guessing() {
        for (text, expected) in [("true", true), ("false", false)] {
            let config: ProviderConfig =
                serde_json::from_value(json!({"thinkingEnabled":text})).unwrap();
            assert_eq!(config.thinking_enabled, expected);
            assert_eq!(
                serde_json::to_value(config.view()).unwrap()["thinkingEnabled"],
                expected
            );
        }
        for value in [json!("enabled"), json!(""), json!(1), Value::Null] {
            assert!(
                serde_json::from_value::<ProviderConfig>(json!({"thinkingEnabled":value})).is_err()
            );
        }
    }
}
