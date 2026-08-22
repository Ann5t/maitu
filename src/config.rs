use std::{env, fmt, fs, net::IpAddr, path::PathBuf, str::FromStr};

use anyhow::{Context, bail};
use ipnet::IpNet;
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecurityMode {
    Disabled,
    Required,
}

#[derive(Clone)]
pub struct SecurityConfig {
    pub mode: SecurityMode,
    pub public_origin: Option<String>,
    pub auth_pepper: Vec<u8>,
    pub setup_token_digest: Option<String>,
    pub trusted_proxy_cidrs: Vec<IpNet>,
    pub tool_proxy_allowed_cidrs: Vec<IpNet>,
    pub session_ttl_seconds: i64,
    pub session_idle_seconds: i64,
    pub session_rotation_seconds: i64,
    pub login_window_seconds: i64,
    pub login_attempts_per_window: i32,
    pub mutation_window_seconds: i64,
    pub mutation_attempts_per_window: i32,
    pub high_cost_attempts_per_window: i32,
    pub tool_proxy_body_max_bytes: usize,
    pub tool_proxy_response_max_bytes: usize,
}

impl fmt::Debug for SecurityConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SecurityConfig")
            .field("mode", &self.mode)
            .field("public_origin", &self.public_origin)
            .field("auth_pepper", &"[REDACTED]")
            .field(
                "setup_token_digest",
                &self.setup_token_digest.as_ref().map(|_| "[REDACTED]"),
            )
            .field("trusted_proxy_cidrs", &self.trusted_proxy_cidrs)
            .field("tool_proxy_allowed_cidrs", &self.tool_proxy_allowed_cidrs)
            .field("session_ttl_seconds", &self.session_ttl_seconds)
            .field("session_idle_seconds", &self.session_idle_seconds)
            .field("session_rotation_seconds", &self.session_rotation_seconds)
            .field("login_window_seconds", &self.login_window_seconds)
            .field("login_attempts_per_window", &self.login_attempts_per_window)
            .field("mutation_window_seconds", &self.mutation_window_seconds)
            .field(
                "mutation_attempts_per_window",
                &self.mutation_attempts_per_window,
            )
            .field(
                "high_cost_attempts_per_window",
                &self.high_cost_attempts_per_window,
            )
            .field("tool_proxy_body_max_bytes", &self.tool_proxy_body_max_bytes)
            .field(
                "tool_proxy_response_max_bytes",
                &self.tool_proxy_response_max_bytes,
            )
            .finish()
    }
}

impl SecurityConfig {
    pub fn required(&self) -> bool {
        self.mode == SecurityMode::Required
    }

    pub fn peer_is_trusted_proxy(&self, peer: IpAddr) -> bool {
        self.trusted_proxy_cidrs
            .iter()
            .any(|network| network.contains(&peer))
    }

    pub fn tool_endpoint_ip_allowed(&self, address: IpAddr) -> bool {
        self.tool_proxy_allowed_cidrs
            .iter()
            .any(|network| network.contains(&address))
    }
}

#[derive(Clone)]
pub struct Config {
    pub bind: String,
    pub database_url: String,
    pub database_max_connections: u32,
    pub artifact_root: PathBuf,
    pub repository_root: PathBuf,
    pub worktree_root: PathBuf,
    pub runner_output_root: PathBuf,
    pub runner_runtime_digest: String,
    pub worker_bootstrap_token_digest: Option<String>,
    pub input_max_bytes: u64,
    pub input_chunk_max_bytes: usize,
    pub input_inbox_copy_max_bytes: u64,
    pub security: SecurityConfig,
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("bind", &self.bind)
            .field("database_url", &"[REDACTED]")
            .field("database_max_connections", &self.database_max_connections)
            .field("artifact_root", &self.artifact_root)
            .field("repository_root", &self.repository_root)
            .field("worktree_root", &self.worktree_root)
            .field("runner_output_root", &self.runner_output_root)
            .field("runner_runtime_digest", &self.runner_runtime_digest)
            .field(
                "worker_bootstrap_token_digest",
                &self
                    .worker_bootstrap_token_digest
                    .as_ref()
                    .map(|_| "[REDACTED]"),
            )
            .field("input_max_bytes", &self.input_max_bytes)
            .field("input_chunk_max_bytes", &self.input_chunk_max_bytes)
            .field(
                "input_inbox_copy_max_bytes",
                &self.input_inbox_copy_max_bytes,
            )
            .field("security", &self.security)
            .finish()
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = secret_or_env("DATABASE_URL", "DATABASE_URL_FILE")?
            .context("必须设置 DATABASE_URL 或 DATABASE_URL_FILE")?;
        let bind = env::var("FUDIAN_BIND").unwrap_or_else(|_| "127.0.0.1:3000".to_owned());
        let artifact_root = env::var("ARTIFACT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/artifacts"));
        let data_root = artifact_root
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./data"));
        let repository_root = env::var("REPOSITORY_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_root.join("repositories"));
        let worktree_root = env::var("WORKTREE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_root.join("worktrees"));
        let runner_output_root = env::var("RUNNER_OUTPUT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_root.join("runner"));
        let runner_runtime_digest = env::var("RUNNER_RUNTIME_DIGEST").unwrap_or_else(|_| {
            "sha256:0000000000000000000000000000000000000000000000000000000000000000".to_owned()
        });
        if runner_runtime_digest.len() != 71
            || !runner_runtime_digest.starts_with("sha256:")
            || !runner_runtime_digest[7..]
                .chars()
                .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
        {
            bail!("RUNNER_RUNTIME_DIGEST 必须是规范 sha256 摘要");
        }
        let worker_bootstrap_token_digest = secret_or_env(
            "FUDIAN_WORKER_BOOTSTRAP_TOKEN",
            "FUDIAN_WORKER_BOOTSTRAP_TOKEN_FILE",
        )?
        .map(|token| validate_token("工作器引导 token", token))
        .transpose()?
        .map(|token| format!("sha256:{:x}", Sha256::digest(token.as_bytes())));
        let database_max_connections = positive_u32_env("DATABASE_MAX_CONNECTIONS", 10)?;
        let input_max_bytes = positive_u64_env("INPUT_MAX_BYTES", 64 * 1024 * 1024)?;
        let input_chunk_max_bytes = positive_u64_env("INPUT_CHUNK_MAX_BYTES", 4 * 1024 * 1024)?;
        let input_inbox_copy_max_bytes =
            positive_u64_env("INPUT_INBOX_COPY_MAX_BYTES", 1024 * 1024)?;
        let input_chunk_max_bytes = usize::try_from(input_chunk_max_bytes)
            .context("INPUT_CHUNK_MAX_BYTES 超出当前平台范围")?;
        if input_chunk_max_bytes as u64 > input_max_bytes {
            bail!("INPUT_CHUNK_MAX_BYTES 不能大于 INPUT_MAX_BYTES");
        }

        Ok(Self {
            bind,
            database_url,
            database_max_connections,
            artifact_root,
            repository_root,
            worktree_root,
            runner_output_root,
            runner_runtime_digest,
            worker_bootstrap_token_digest,
            input_max_bytes,
            input_chunk_max_bytes,
            input_inbox_copy_max_bytes,
            security: security_config_from_env()?,
        })
    }
}

fn security_config_from_env() -> anyhow::Result<SecurityConfig> {
    let mode = match env::var("FUDIAN_SECURITY_MODE")
        .unwrap_or_else(|_| "required".to_owned())
        .as_str()
    {
        "required" => SecurityMode::Required,
        "disabled" => SecurityMode::Disabled,
        _ => bail!("FUDIAN_SECURITY_MODE 只能是 required 或 disabled"),
    };

    if mode == SecurityMode::Disabled {
        return Ok(SecurityConfig {
            mode,
            public_origin: None,
            auth_pepper: Vec::new(),
            setup_token_digest: None,
            trusted_proxy_cidrs: Vec::new(),
            tool_proxy_allowed_cidrs: Vec::new(),
            session_ttl_seconds: 12 * 60 * 60,
            session_idle_seconds: 2 * 60 * 60,
            session_rotation_seconds: 30 * 60,
            login_window_seconds: 15 * 60,
            login_attempts_per_window: 5,
            mutation_window_seconds: 60,
            mutation_attempts_per_window: 120,
            high_cost_attempts_per_window: 20,
            tool_proxy_body_max_bytes: 4 * 1024 * 1024,
            tool_proxy_response_max_bytes: 16 * 1024 * 1024,
        });
    }

    let origin_raw =
        env::var("FUDIAN_PUBLIC_ORIGIN").context("安全模式必须设置 FUDIAN_PUBLIC_ORIGIN")?;
    let origin = canonical_https_origin(&origin_raw)?;
    let auth_pepper = secret_or_env("FUDIAN_AUTH_PEPPER", "FUDIAN_AUTH_PEPPER_FILE")?
        .context("安全模式必须设置 FUDIAN_AUTH_PEPPER_FILE")?
        .into_bytes();
    if !(32..=128).contains(&auth_pepper.len()) {
        bail!("FUDIAN_AUTH_PEPPER 必须是 32–128 字节");
    }
    let setup_token = secret_or_env("FUDIAN_SETUP_TOKEN", "FUDIAN_SETUP_TOKEN_FILE")?
        .context("安全模式必须设置 FUDIAN_SETUP_TOKEN_FILE")?;
    let setup_token = validate_token("初始化 token", setup_token)?;
    let setup_token_digest = Some(keyed_digest(
        &auth_pepper,
        "setup-token",
        setup_token.as_bytes(),
    ));
    let trusted_proxy_cidrs = parse_cidrs(
        "FUDIAN_TRUSTED_PROXY_CIDRS",
        &env::var("FUDIAN_TRUSTED_PROXY_CIDRS")
            .context("安全模式必须明确设置 FUDIAN_TRUSTED_PROXY_CIDRS")?,
    )?;
    if trusted_proxy_cidrs.is_empty() {
        bail!("FUDIAN_TRUSTED_PROXY_CIDRS 不能为空");
    }
    let tool_proxy_allowed_cidrs = parse_cidrs(
        "FUDIAN_TOOL_PROXY_ALLOWED_CIDRS",
        &env::var("FUDIAN_TOOL_PROXY_ALLOWED_CIDRS")
            .unwrap_or_else(|_| "127.0.0.0/8,::1/128".to_owned()),
    )?;

    Ok(SecurityConfig {
        mode,
        public_origin: Some(origin),
        auth_pepper,
        setup_token_digest,
        trusted_proxy_cidrs,
        tool_proxy_allowed_cidrs,
        session_ttl_seconds: positive_i64_env("FUDIAN_SESSION_TTL_SECONDS", 12 * 60 * 60)?,
        session_idle_seconds: positive_i64_env("FUDIAN_SESSION_IDLE_SECONDS", 2 * 60 * 60)?,
        session_rotation_seconds: positive_i64_env("FUDIAN_SESSION_ROTATION_SECONDS", 30 * 60)?,
        login_window_seconds: positive_i64_env("FUDIAN_LOGIN_WINDOW_SECONDS", 15 * 60)?,
        login_attempts_per_window: positive_i32_env("FUDIAN_LOGIN_ATTEMPTS", 5)?,
        mutation_window_seconds: positive_i64_env("FUDIAN_MUTATION_WINDOW_SECONDS", 60)?,
        mutation_attempts_per_window: positive_i32_env("FUDIAN_MUTATION_ATTEMPTS", 120)?,
        high_cost_attempts_per_window: positive_i32_env("FUDIAN_HIGH_COST_ATTEMPTS", 20)?,
        tool_proxy_body_max_bytes: usize_env("FUDIAN_TOOL_PROXY_BODY_MAX_BYTES", 4 * 1024 * 1024)?,
        tool_proxy_response_max_bytes: usize_env(
            "FUDIAN_TOOL_PROXY_RESPONSE_MAX_BYTES",
            16 * 1024 * 1024,
        )?,
    })
}

pub fn keyed_digest(key: &[u8], purpose: &str, value: &[u8]) -> String {
    use hmac::{Hmac, Mac};

    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC-SHA256 accepts keys of every size");
    mac.update(b"fudian-v1\0");
    mac.update(purpose.as_bytes());
    mac.update(b"\0");
    mac.update(value);
    format!("sha256:{}", hex::encode(mac.finalize().into_bytes()))
}

fn canonical_https_origin(value: &str) -> anyhow::Result<String> {
    let parsed = Url::parse(value).context("FUDIAN_PUBLIC_ORIGIN 必须是完整 URL")?;
    if parsed.scheme() != "https"
        || parsed.cannot_be_a_base()
        || parsed.host_str().is_none()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        bail!("FUDIAN_PUBLIC_ORIGIN 必须是无路径、无凭据的 HTTPS origin");
    }
    Ok(parsed.origin().ascii_serialization())
}

fn parse_cidrs(name: &str, value: &str) -> anyhow::Result<Vec<IpNet>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| IpNet::from_str(item).with_context(|| format!("{name} 包含无效 CIDR: {item}")))
        .collect()
}

fn secret_or_env(value_name: &str, file_name: &str) -> anyhow::Result<Option<String>> {
    let direct = env::var(value_name).ok();
    let file = env::var(file_name).ok();
    if direct.is_some() && file.is_some() {
        bail!("{value_name} 与 {file_name} 不能同时设置");
    }
    let value = match (direct, file) {
        (Some(value), None) => Some(value),
        (None, Some(path)) => Some(
            fs::read_to_string(&path)
                .with_context(|| format!("读取 {file_name} 指向的文件失败"))?,
        ),
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!(),
    };
    value
        .map(|value| {
            let value = value.trim_end_matches(['\r', '\n']).to_owned();
            if value.is_empty() || value.contains('\0') {
                bail!("{value_name} 不能为空或包含 NUL");
            }
            Ok(value)
        })
        .transpose()
}

fn validate_token(label: &str, token: String) -> anyhow::Result<String> {
    if !(32..=256).contains(&token.len())
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        bail!("{label} 必须是 32–256 位安全 ASCII token");
    }
    Ok(token)
}

fn positive_u64_env(name: &str, default: u64) -> anyhow::Result<u64> {
    let value = env_parse(name, default)?;
    if value == 0 {
        bail!("{name} 必须大于 0");
    }
    Ok(value)
}

fn positive_u32_env(name: &str, default: u32) -> anyhow::Result<u32> {
    let value = env_parse(name, default)?;
    if value == 0 {
        bail!("{name} 必须大于 0");
    }
    Ok(value)
}

fn positive_i64_env(name: &str, default: i64) -> anyhow::Result<i64> {
    let value = env_parse(name, default)?;
    if value <= 0 {
        bail!("{name} 必须大于 0");
    }
    Ok(value)
}

fn positive_i32_env(name: &str, default: i32) -> anyhow::Result<i32> {
    let value = env_parse(name, default)?;
    if value <= 0 {
        bail!("{name} 必须大于 0");
    }
    Ok(value)
}

fn usize_env(name: &str, default: usize) -> anyhow::Result<usize> {
    let value = env_parse(name, default)?;
    if value == 0 {
        bail!("{name} 必须大于 0");
    }
    Ok(value)
}

fn env_parse<T>(name: &str, default: T) -> anyhow::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    match env::var(name) {
        Ok(value) => value
            .parse::<T>()
            .with_context(|| format!("{name} 格式错误")),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error).with_context(|| format!("读取 {name} 失败")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_origin_is_an_exact_https_origin() {
        assert_eq!(
            canonical_https_origin("https://fudian.example:8443").unwrap(),
            "https://fudian.example:8443"
        );
        assert!(canonical_https_origin("http://fudian.example").is_err());
        assert!(canonical_https_origin("https://fudian.example/path").is_err());
        assert!(canonical_https_origin("https://user@fudian.example").is_err());
    }

    #[test]
    fn keyed_digests_are_domain_separated() {
        let key = b"a sufficiently long authentication pepper";
        assert_ne!(
            keyed_digest(key, "session", b"same"),
            keyed_digest(key, "csrf", b"same")
        );
    }
}
