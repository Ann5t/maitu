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
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub concurrency: usize,
    pub context_tokens: u32,
    pub max_tokens: u32,
    #[serde(deserialize_with = "deserialize_thinking_enabled")]
    pub thinking_enabled: bool,
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

    pub fn endpoint(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.into()
        } else {
            format!("{base}/chat/completions")
        }
    }

    pub fn validate(&self) -> AppResult<()> {
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
    current: Arc<RwLock<ProviderConfig>>,
}

impl ProviderStore {
    pub async fn open(root: PathBuf) -> AppResult<Self> {
        let path = root.join("provider.json");
        let config = match fs::read(&path).await {
            Ok(bytes) => {
                let config: ProviderConfig = serde_json::from_slice(&bytes)
                    .map_err(|_| AppError::internal("本机模型配置无法读取，请检查配置文件"))?;
                config.validate()?;
                config
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ProviderConfig::default(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            path,
            current: Arc::new(RwLock::new(config)),
        })
    }

    pub async fn get(&self) -> ProviderConfig {
        self.current.read().await.clone()
    }

    pub async fn save(&self, mut config: ProviderConfig) -> AppResult<ProviderView> {
        let mut current = self.current.write().await;
        config.base_url = config.base_url.trim().trim_end_matches('/').into();
        config.model = config.model.trim().into();
        config.api_key = config.api_key.trim().into();
        if config.api_key.is_empty() && config.base_url == current.base_url {
            config.api_key = current.api_key.clone();
        }
        config.validate()?;
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
            file.write_all(&serde_json::to_vec(&config)?).await?;
            file.sync_all().await?;
            drop(file);
            fs::rename(&temporary, &self.path).await?;
            Ok::<_, AppError>(())
        }
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&temporary).await;
        }
        result?;
        *current = config;
        Ok(current.view())
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
    fn new(code: &'static str, message: impl Into<String>) -> Self {
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
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(READ_IDLE_TIMEOUT)
        .build()
        .map_err(|_| ProviderFailure::new("provider_client", "无法建立模型连接"))?;
    let request = json!({
        "model": config.model,
        "stream": false,
        "max_tokens": config.max_tokens,
        "thinking": { "type": if config.thinking_enabled { "enabled" } else { "disabled" } },
        "messages": [
            {"role":"system", "content":SYSTEM_PROMPT},
            {"role":"user", "content":prompt}
        ]
    });
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
    parse_completion(&body)
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

    #[test]
    fn credentials_cannot_be_embedded_in_endpoints_or_exposed_by_views() {
        let mut config = ProviderConfig {
            api_key: "private-test-secret".into(),
            ..Default::default()
        };
        assert!(config.validate().is_ok());
        assert!(!format!("{config:?}").contains("private-test-secret"));
        assert!(
            !serde_json::to_string(&config.view())
                .unwrap()
                .contains("private-test-secret")
        );
        config.base_url = "https://user:password@api.deepseek.com".into();
        assert!(config.validate().is_err());
        config.base_url = "http://api.deepseek.com".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn deepseek_lengths_use_current_limits_and_reserve_input_space() {
        let mut config = ProviderConfig {
            api_key: "isolated-unit-key".into(),
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
        let config: ProviderConfig = serde_json::from_value(json!({
            "baseUrl":"https://api.deepseek.com", "model":"deepseek-flash",
            "apiKey":"isolated-unit-key", "concurrency":3,
            "timeoutSeconds":180, "maxTokens":4096
        }))
        .unwrap();
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
            api_key: "isolated-unit-key".into(),
            context_tokens: (estimated + 100) as u32,
            max_tokens: 100,
            ..Default::default()
        };
        assert!(check_context_budget(&config, short).is_ok());
        config.context_tokens -= 1;
        let failure = complete(&config, short).await.unwrap_err();
        assert_eq!(failure.code, "context_budget_exceeded");
        assert!(failure.message.contains("预留输出"));
        assert!(check_context_budget(&config, &"资料".repeat(1000)).is_err());
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
