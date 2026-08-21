use std::{env, path::PathBuf};

use anyhow::{Context, bail};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub database_url: String,
    pub database_max_connections: u32,
    pub artifact_root: PathBuf,
    pub input_max_bytes: u64,
    pub input_chunk_max_bytes: usize,
    pub input_inbox_copy_max_bytes: u64,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let database_url = env::var("DATABASE_URL").context("必须设置 DATABASE_URL")?;
        let bind = env::var("FUDIAN_BIND").unwrap_or_else(|_| "0.0.0.0:3000".to_owned());
        let artifact_root = env::var("ARTIFACT_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./data/artifacts"));
        let database_max_connections = env::var("DATABASE_MAX_CONNECTIONS")
            .unwrap_or_else(|_| "10".to_owned())
            .parse::<u32>()
            .context("DATABASE_MAX_CONNECTIONS 必须是正整数")?;
        if database_max_connections == 0 {
            bail!("DATABASE_MAX_CONNECTIONS 必须大于 0");
        }
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
            input_max_bytes,
            input_chunk_max_bytes,
            input_inbox_copy_max_bytes,
        })
    }
}

fn positive_u64_env(name: &str, default: u64) -> anyhow::Result<u64> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .with_context(|| format!("{name} 必须是正整数"))?,
        Err(env::VarError::NotPresent) => default,
        Err(error) => return Err(error).with_context(|| format!("读取 {name} 失败")),
    };
    if value == 0 {
        bail!("{name} 必须大于 0");
    }
    Ok(value)
}
