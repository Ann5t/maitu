mod application;
mod artifacts;
mod config;
pub mod context_memory;
mod domain;
mod error;
pub mod goal_domain;
pub mod goal_models;
pub mod idea_domain;
pub mod idea_models;
pub mod input_artifacts;
mod migrations;
mod models;
pub mod scheduler;
mod security;
pub mod tooling;
mod web;
pub mod workspace;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use sqlx::postgres::PgPoolOptions;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::info;

use crate::{config::Config, web::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        let address = std::env::args()
            .nth(2)
            .unwrap_or_else(|| "127.0.0.1:3000".to_owned());
        return healthcheck(&address).await;
    }
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
    tokio::fs::create_dir_all(&config.repository_root)
        .await
        .context("创建托管 Git 仓库目录失败")?;
    tokio::fs::create_dir_all(&config.worktree_root)
        .await
        .context("创建目标 worktree 目录失败")?;
    tokio::fs::create_dir_all(&config.runner_output_root)
        .await
        .context("创建 Runner 输出目录失败")?;

    let tool_proxy_client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
        .build()
        .context("初始化 ToolLease 安全代理失败")?;
    let bind = config.bind.clone();
    let state = Arc::new(AppState {
        pool,
        config,
        tool_proxy_client,
    });
    let app = web::router(state);
    let listener = TcpListener::bind(&bind)
        .await
        .with_context(|| format!("监听 {bind} 失败"))?;

    info!(%bind, "浮点 Rust 服务已启动");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("HTTP 服务异常退出")?;
    Ok(())
}

async fn healthcheck(address: &str) -> anyhow::Result<()> {
    let check = async {
        let mut stream = TcpStream::connect(address).await?;
        stream
            .write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await?;
        let mut response = Vec::with_capacity(512);
        stream.read_to_end(&mut response).await?;
        anyhow::ensure!(
            response.starts_with(b"HTTP/1.1 200") || response.starts_with(b"HTTP/1.0 200"),
            "健康端点没有返回 200"
        );
        Ok::<_, anyhow::Error>(())
    };
    tokio::time::timeout(Duration::from_secs(4), check)
        .await
        .context("健康检查超时")??;
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
