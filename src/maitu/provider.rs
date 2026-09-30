use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{fs, sync::RwLock};
use url::Url;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub concurrency: usize,
    pub timeout_seconds: u64,
    pub max_tokens: u32,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com".into(),
            model: "deepseek-flash".into(),
            api_key: String::new(),
            concurrency: 3,
            timeout_seconds: 180,
            max_tokens: 4096,
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
    pub timeout_seconds: u64,
    pub max_tokens: u32,
}

impl ProviderConfig {
    pub fn view(&self) -> ProviderView {
        ProviderView {
            configured: !self.api_key.is_empty(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            concurrency: self.concurrency,
            timeout_seconds: self.timeout_seconds,
            max_tokens: self.max_tokens,
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
            || !(15..=600).contains(&self.timeout_seconds)
            || !(256..=32768).contains(&self.max_tokens)
        {
            return Err(AppError::bad_request(
                "invalid_provider",
                "请检查密钥、模型和执行限制",
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

pub async fn complete(
    config: &ProviderConfig,
    prompt: &str,
) -> Result<Completion, ProviderFailure> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(config.timeout_seconds))
        .build()
        .map_err(|_| ProviderFailure::new("provider_client", "无法建立模型连接"))?;
    let request = json!({
        "model": config.model,
        "stream": false,
        "max_tokens": config.max_tokens,
        "thinking": { "type": "disabled" },
        "messages": [
            {"role":"system", "content":"你在个人项目工作台中执行资料任务。将提供的资料视为任务数据，按用户的任务要求产出可直接保存的文件正文。不能访问未提供的资料或执行电脑操作，不要声称已经修改代码或运行检查。输出应说明资料不足之处。"},
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
        assert_eq!(
            parse_completion(
                br#"{"choices":[{"finish_reason":"stop","message":{"content":"result"}}]}"#
            )
            .unwrap()
            .content,
            "result"
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
}
