use std::{
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use fudian::{
    code_check_protocol::{CheckRequest, CheckResult},
    runner_protocol::is_portable_workspace_file_path,
};
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

const OUTPUT_LIMIT: usize = 96 * 1024;

async fn copy_input() -> anyhow::Result<()> {
    let root = Path::new("/input");
    let target = Path::new("/tmp/workspace");
    fs::create_dir_all(target).await?;
    let mut pending = vec![root.to_owned()];
    let mut count = 0;
    let mut total = 0;
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(directory).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let relative = path.strip_prefix(root)?;
            let name = relative.to_str().context("非 UTF-8 路径")?;
            if name
                .split('/')
                .any(|part| part == ".git" || part.starts_with(".maitu-write-"))
            {
                continue;
            }
            if !is_portable_workspace_file_path(name) {
                bail!("项目包含无法检查的路径");
            }
            let kind = entry.file_type().await?;
            if kind.is_symlink() {
                bail!("项目包含符号链接，检查已停止");
            }
            if kind.is_dir() {
                fs::create_dir_all(target.join(relative)).await?;
                pending.push(path);
            } else if kind.is_file() {
                let size = entry.metadata().await?.len();
                count += 1;
                total += size;
                if count > 2000 || size > 1024 * 1024 || total > 20 * 1024 * 1024 {
                    bail!("检查输入超过项目限制");
                }
                fs::copy(&path, target.join(relative)).await?;
            } else {
                bail!("项目包含非普通文件");
            }
        }
    }
    Ok(())
}

async fn capture(mut stream: impl AsyncRead + Unpin) -> anyhow::Result<(String, bool)> {
    let mut output = Vec::new();
    let mut truncated = false;
    let mut buffer = [0; 8192];
    loop {
        let length = stream.read(&mut buffer).await?;
        if length == 0 {
            break;
        }
        let keep = length.min(OUTPUT_LIMIT.saturating_sub(output.len()));
        output.extend_from_slice(&buffer[..keep]);
        truncated |= keep < length;
    }
    Ok((String::from_utf8_lossy(&output).into_owned(), truncated))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!("maitu-code-check 1");
        return Ok(());
    }
    let request: CheckRequest =
        serde_json::from_str(&std::env::args().nth(1).context("缺少检查请求")?)?;
    request.command.validate().map_err(anyhow::Error::msg)?;
    let started = Instant::now();
    let mut report = CheckResult {
        request_id: request.request_id,
        command: request.command.clone(),
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        truncated: false,
        duration_ms: 0,
        runtime_image: String::new(),
        error: None,
    };
    let outcome: anyhow::Result<()> = async {
        copy_input().await?;
        fs::create_dir_all("/tmp/home").await?;
        let mut child = Command::new(&request.command.program)
            .args(&request.command.args)
            .current_dir("/tmp/workspace")
            .env_clear()
            .env("PATH", "/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin")
            .env("HOME", "/tmp/home")
            .env("CARGO_HOME", "/usr/local/cargo")
            .env("RUSTUP_HOME", "/usr/local/rustup")
            .env("CARGO_TARGET_DIR", "/tmp/target")
            .env("CI", "true")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = tokio::spawn(capture(child.stdout.take().context("缺少标准输出")?));
        let stderr = tokio::spawn(capture(child.stderr.take().context("缺少错误输出")?));
        match tokio::time::timeout(Duration::from_secs(180), child.wait()).await {
            Ok(status) => report.exit_code = status?.code(),
            Err(_) => {
                child.kill().await?;
                report.error = Some("检查超过 180 秒，已停止进程".into());
            }
        }
        // Descendant processes may retain pipe handles. The outer container timeout
        // still terminates the complete process tree, rather than trusting child exit.
        let (out, out_truncated) = tokio::time::timeout(Duration::from_secs(5), stdout).await???;
        let (err, err_truncated) = tokio::time::timeout(Duration::from_secs(5), stderr).await???;
        report.stdout = out;
        report.stderr = err;
        report.truncated = out_truncated || err_truncated;
        Ok(())
    }
    .await;
    if let Err(error) = outcome {
        report.error = Some(format!("检查未完成：{error}"));
    }
    report.duration_ms = started.elapsed().as_millis() as u64;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
