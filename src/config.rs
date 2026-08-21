use std::{env, path::PathBuf};

use anyhow::{Context, bail};

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: String,
    pub database_url: String,
    pub database_max_connections: u32,
    pub artifact_root: PathBuf,
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

        Ok(Self {
            bind,
            database_url,
            database_max_connections,
            artifact_root,
        })
    }
}
