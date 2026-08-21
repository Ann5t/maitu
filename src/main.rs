mod application;
mod artifacts;
mod config;
mod domain;
mod error;
pub mod goal_domain;
pub mod goal_models;
mod migrations;
mod models;
pub mod tooling;
mod web;

use std::sync::Arc;

use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tracing::info;

use crate::{config::Config, web::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "fudian=debug,tower_http=info".into()),
        )
        .init();

    let config = Config::from_env()?;
    let pool = PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .connect(&config.database_url)
        .await
        .context("连接 PostgreSQL 失败")?;

    migrations::run(&pool).await.context("数据库迁移失败")?;
    application::plugins::ensure_reference_plugins(&pool)
        .await
        .context("初始化参考插件失败")?;
    tokio::fs::create_dir_all(&config.artifact_root)
        .await
        .context("创建产物目录失败")?;

    let bind = config.bind.clone();
    let state = Arc::new(AppState { pool, config });
    let app = web::router(state);
    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("监听 {bind} 失败"))?;

    info!(%bind, "浮点 Rust 服务已启动");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP 服务异常退出")?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("安装 Ctrl+C 处理器失败");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("安装 SIGTERM 处理器失败")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
