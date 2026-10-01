use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, bail};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use fudian::code_check_protocol::{CheckRequest, CheckResult};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    fs,
    sync::{Mutex, Semaphore},
};

struct Worker {
    docker: reqwest::Client,
    token_hash: [u8; 32],
    image: String,
    volume: String,
    root: PathBuf,
    admission: Mutex<()>,
    slots: Semaphore,
    active: Mutex<HashSet<uuid::Uuid>>,
}

type ApiError = (StatusCode, Json<Value>);
fn failure(message: &str) -> ApiError {
    (StatusCode::CONFLICT, Json(json!({"error":message})))
}

impl Worker {
    async fn docker(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> anyhow::Result<Vec<u8>> {
        let mut request = self
            .docker
            .request(method, format!("http://localhost/v1.51{path}"));
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(body.to_string());
        }
        let mut response = request.send().await?;
        if !response.status().is_success() {
            bail!("检查容器服务返回 {}", response.status().as_u16());
        }
        let mut output = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if output.len() + chunk.len() > 512 * 1024 {
                bail!("检查容器结果超过限制");
            }
            output.extend_from_slice(&chunk);
        }
        Ok(output)
    }

    async fn collect(&self, name: &str, request: &CheckRequest) -> anyhow::Result<CheckResult> {
        let wait_path = format!("/containers/{name}/wait?condition=not-running");
        let wait = self.docker(reqwest::Method::POST, &wait_path, None);
        match tokio::time::timeout(Duration::from_secs(200), wait).await {
            Ok(result) => {
                result?;
            }
            Err(_) => {
                let _ = self
                    .docker(
                        reqwest::Method::POST,
                        &format!("/containers/{name}/kill"),
                        None,
                    )
                    .await;
                bail!("检查容器超过执行时限，已请求停止");
            }
        }
        let bytes = self
            .docker(
                reqwest::Method::GET,
                &format!("/containers/{name}/logs?stdout=1&stderr=1"),
                None,
            )
            .await?;
        let mut output = Vec::new();
        let mut offset = 0;
        while offset + 8 <= bytes.len() {
            let length = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into()?) as usize;
            let end = offset + 8 + length;
            if end > bytes.len() {
                bail!("检查日志不完整");
            }
            output.extend_from_slice(&bytes[offset + 8..end]);
            offset = end;
        }
        if offset != bytes.len() {
            bail!("检查日志无法读取");
        }
        let mut report: CheckResult = serde_json::from_slice(&output)?;
        if report.request_id != request.request_id || report.command != request.command {
            bail!("检查结果身份不一致");
        }
        report.runtime_image.clone_from(&self.image);
        Ok(report)
    }

    async fn run(&self, request: CheckRequest, resume: bool) -> anyhow::Result<CheckResult> {
        let _slot = self.slots.acquire().await?;
        let name = format!("maitu-check-{}", request.request_id);
        if !resume {
            self.docker(reqwest::Method::POST, &format!("/containers/create?name={name}"), Some(json!({
                "Image":self.image, "User":"1000:1000", "Cmd":[serde_json::to_string(&request)?],
                "Labels":{"maitu.check.request":request.request_id.to_string(),"maitu.check.volume":self.volume},
                "HostConfig":{
                    "NetworkMode":"none", "ReadonlyRootfs":true, "CapDrop":["ALL"],
                    "SecurityOpt":["no-new-privileges:true"], "PidsLimit":128,
                    "Memory":1073741824_u64, "NanoCpus":2000000000_u64,
                    "Tmpfs":{"/tmp":"rw,nosuid,nodev,size=512m,mode=1777"},
                    "Mounts":[{"Type":"volume","Source":self.volume,"Target":"/input","ReadOnly":true,
                        "VolumeOptions":{"NoCopy":true,"Subpath":format!("maitu-code/{}",request.workspace_key)}}],
                    "LogConfig":{"Type":"json-file","Config":{"max-size":"1m","max-file":"1"}}
                }
            }))).await?;
            self.docker(
                reqwest::Method::POST,
                &format!("/containers/{name}/start"),
                None,
            )
            .await?;
        } else {
            // A known container can be observed after a connection loss. A missing or
            // never-started container is uncertain and is never recreated automatically.
            let details: Value = serde_json::from_slice(
                &self
                    .docker(
                        reqwest::Method::GET,
                        &format!("/containers/{name}/json"),
                        None,
                    )
                    .await?,
            )?;
            if !matches!(
                details["State"]["Status"].as_str(),
                Some("running" | "exited")
            ) {
                bail!("上次检查未确定启动，请核对后创建新的检查");
            }
        }
        let report = self.collect(&name, &request).await?;
        let directory = self.root.join(request.request_id.to_string());
        atomic_json(&directory.join("result.json"), &report).await?;
        let _ = self
            .docker(
                reqwest::Method::DELETE,
                &format!("/containers/{name}"),
                None,
            )
            .await;
        Ok(report)
    }
}

async fn atomic_json(path: &Path, value: &impl serde::Serialize) -> anyhow::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec(value)?).await?;
    fs::rename(temporary, path).await?;
    Ok(())
}

async fn check(
    State(worker): State<Arc<Worker>>,
    headers: HeaderMap,
    Json(request): Json<CheckRequest>,
) -> Result<Json<CheckResult>, ApiError> {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or("");
    let provided: [u8; 32] = Sha256::digest(authorization.as_bytes()).into();
    if provided
        .iter()
        .zip(worker.token_hash)
        .fold(0, |acc, (left, right)| acc | (left ^ right))
        != 0
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":"检查服务授权失败"})),
        ));
    }
    request.command.validate().map_err(failure)?;
    let guard = worker.admission.lock().await;
    if worker.active.lock().await.contains(&request.request_id) {
        return Err(failure("此检查仍在执行，请稍后查看结果"));
    }
    let directory = worker.root.join(request.request_id.to_string());
    let resume = fs::try_exists(&directory)
        .await
        .map_err(|_| failure("检查记录无法读取"))?;
    if resume {
        let previous: CheckRequest = serde_json::from_slice(
            &fs::read(directory.join("request.json"))
                .await
                .map_err(|_| failure("上次检查记录不完整，不能自动重新执行"))?,
        )
        .map_err(|_| failure("检查记录格式无效"))?;
        if previous.workspace_key != request.workspace_key || previous.command != request.command {
            return Err(failure("此检查编号已用于另一条命令"));
        }
        if let Ok(bytes) = fs::read(directory.join("result.json")).await {
            return Ok(Json(
                serde_json::from_slice(&bytes).map_err(|_| failure("检查结果记录损坏"))?,
            ));
        }
    } else {
        fs::create_dir(&directory)
            .await
            .map_err(|_| failure("检查编号已被使用"))?;
        atomic_json(&directory.join("request.json"), &request)
            .await
            .map_err(|_| failure("检查请求无法保存"))?;
    }
    worker.active.lock().await.insert(request.request_id);
    drop(guard);
    let result = tokio::spawn(async move {
        let id = request.request_id;
        let result = worker.run(request, resume).await;
        worker.active.lock().await.remove(&id);
        result
    })
    .await
    .map_err(|_| failure("检查服务执行中断，不能自动重新执行"))?
    .map_err(|_| failure("检查结果尚不确定，请保留记录后核对；本次命令不会自动重复"))?;
    Ok(Json(result))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let token = fs::read_to_string(
        std::env::var("MAITU_CHECK_WORKER_TOKEN_FILE")
            .unwrap_or_else(|_| "/run/maitu-executor/token".into()),
    )
    .await?;
    if token.trim().len() < 32 {
        bail!("检查服务授权文件不完整");
    }
    let docker = reqwest::Client::builder()
        .unix_socket("/var/run/docker.sock")
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .read_timeout(Duration::from_secs(210))
        .build()?;
    let tag =
        std::env::var("MAITU_CODE_IMAGE").unwrap_or_else(|_| "maitu-code-runtime:local".into());
    let response = docker
        .get(format!("http://localhost/v1.51/images/{tag}/json"))
        .send()
        .await?;
    if !response.status().is_success() {
        bail!("代码检查镜像尚未构建");
    }
    let details: Value = serde_json::from_slice(&response.bytes().await?)?;
    let image = details["Id"]
        .as_str()
        .context("缺少检查镜像标识")?
        .to_owned();
    let root = PathBuf::from(
        std::env::var("MAITU_CHECK_RECORD_ROOT")
            .unwrap_or_else(|_| "/data/runner/code-checks".into()),
    );
    fs::create_dir_all(&root).await?;
    let worker = Arc::new(Worker {
        docker,
        token_hash: Sha256::digest(token.trim().as_bytes()).into(),
        image,
        root,
        volume: std::env::var("MAITU_WORKTREE_VOLUME").context("缺少工作区卷名称")?,
        admission: Mutex::new(()),
        slots: Semaphore::new(2),
        active: Mutex::new(HashSet::new()),
    });
    let router = Router::new()
        .route(
            "/api/health",
            get(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/checks", post(check))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .with_state(worker);
    axum::serve(tokio::net::TcpListener::bind("0.0.0.0:3001").await?, router).await?;
    Ok(())
}
