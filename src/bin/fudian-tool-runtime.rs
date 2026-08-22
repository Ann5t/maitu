use std::{
    env, fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Component, Path, PathBuf},
    process::{Command, Output, Stdio},
};

use anyhow::{Context, bail};
use fudian::runner_protocol::is_portable_workspace_file_path;
use serde_json::{Value, json};

const MAX_CAPTURE_BYTES: usize = 64 * 1024;

fn main() -> anyhow::Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.first().map(String::as_str) == Some("serve-static") {
        if args.len() != 3 || args[2] != "4173" {
            bail!("usage: fudian-tool-runtime serve-static <source-path> 4173");
        }
        return serve_static(&args[1]);
    }
    if args.len() != 5 || args[0] != "execute" {
        bail!(
            "usage: fudian-tool-runtime execute <plugin-id> <plugin-version> <tool-name> <input-json>"
        );
    }
    let input: Value = serde_json::from_str(&args[4]).context("parse structured tool input")?;
    let input = input
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("tool input must be a JSON object"))?;
    let input_root = fixed_root("FUDIAN_INPUT", "/workspace/input", false)?;
    let output_root = fixed_root("FUDIAN_OUTPUT", "/workspace/output", true)?;
    let report_path = required_string(input, "reportPath")?;

    let (toolchain, execution, detail) = match (args[1].as_str(), args[3].as_str()) {
        ("fudian.tools.rust", "check") => {
            ensure_version(&args[2], &["1.0.0"])?;
            execute_rust_check(&input_root, input)?
        }
        ("fudian.tools.python", "run") => {
            ensure_version(&args[2], &["1.0.0", "2.0.0"])?;
            execute_python(&input_root, input)?
        }
        ("fudian.tools.cxx", "check") => {
            ensure_version(&args[2], &["1.0.0"])?;
            execute_cxx_check(&input_root, input)?
        }
        ("fudian.tools.playwright", "inspect") => {
            ensure_version(&args[2], &["1.0.0"])?;
            execute_playwright(&input_root, &output_root, input)?
        }
        _ => bail!("plugin identity or tool name is not provided by this runtime image"),
    };
    let report = json!({
        "schemaVersion": 1,
        "pluginId": args[1],
        "pluginVersion": args[2],
        "toolName": args[3],
        "toolchain": toolchain,
        "succeeded": execution.status.success(),
        "exitCode": execution.status.code(),
        "stdout": limited_text(&execution.stdout),
        "stderr": limited_text(&execution.stderr),
        "detail": detail,
    });
    write_output_file(
        &output_root,
        report_path,
        &serde_json::to_vec_pretty(&report)?,
    )?;
    if !execution.status.success() {
        bail!("tool process exited unsuccessfully");
    }
    Ok(())
}

fn serve_static(relative_source: &str) -> anyhow::Result<()> {
    let input_root = fixed_root("FUDIAN_INPUT", "/workspace/input", false)?;
    let source = safe_input_file(&input_root, relative_source)?;
    let metadata = fs::metadata(&source).context("inspect static preview input")?;
    if metadata.len() > 8 * 1024 * 1024 {
        bail!("static preview input exceeds 8 MiB");
    }
    let body = fs::read(&source).context("read static preview input")?;
    let listener = TcpListener::bind("0.0.0.0:4173").context("bind static preview endpoint")?;
    println!("fudian static preview ready on 0.0.0.0:4173");
    std::io::stdout().flush().context("flush readiness log")?;
    for stream in listener.incoming() {
        let mut stream = stream.context("accept static preview connection")?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .context("set preview read timeout")?;
        let mut request = [0_u8; 8 * 1024];
        let read = stream.read(&mut request).context("read preview request")?;
        let first_line = String::from_utf8_lossy(&request[..read])
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
        let (status, content_type, response_body): (&str, &str, &[u8]) =
            if first_line.starts_with("GET /health ") {
                ("200 OK", "text/plain; charset=utf-8", b"ok")
            } else if first_line.starts_with("GET / ") {
                ("200 OK", "text/html; charset=utf-8", &body)
            } else {
                ("404 Not Found", "text/plain; charset=utf-8", b"not found")
            };
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
            response_body.len()
        )
        .context("write preview headers")?;
        stream
            .write_all(response_body)
            .context("write preview body")?;
    }
    Ok(())
}

fn execute_rust_check(
    input_root: &Path,
    input: &serde_json::Map<String, Value>,
) -> anyhow::Result<(String, Output, Value)> {
    let source = safe_input_file(input_root, required_string(input, "sourcePath")?)?;
    let rust_environment = [
        ("RUSTUP_HOME", "/usr/local/rustup"),
        ("CARGO_HOME", "/usr/local/cargo"),
    ];
    let toolchain = first_line(&run_process(
        "/usr/local/cargo/bin/rustc",
        &["--version"],
        &rust_environment,
    )?)?;
    let output = run_process(
        "/usr/local/cargo/bin/rustc",
        &[
            "--edition=2024",
            "--crate-name=fudian_plugin_check",
            "--crate-type=lib",
            "--emit=metadata",
            "-o",
            "/tmp/fudian-plugin-check.rmeta",
            path_text(&source)?,
        ],
        &rust_environment,
    )?;
    Ok((
        toolchain,
        output,
        json!({ "sourcePath": path_text(&source)? }),
    ))
}

fn execute_python(
    input_root: &Path,
    input: &serde_json::Map<String, Value>,
) -> anyhow::Result<(String, Output, Value)> {
    let source = safe_input_file(input_root, required_string(input, "sourcePath")?)?;
    let arguments = optional_string_array(input, "arguments")?;
    let version_output = run_process("/usr/local/bin/python3", &["--version"], &[])?;
    let toolchain = first_line(&version_output)?;
    let mut args = vec![
        "-I".to_owned(),
        "-B".to_owned(),
        path_text(&source)?.to_owned(),
    ];
    args.extend(arguments.iter().cloned());
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = run_process(
        "/usr/local/bin/python3",
        &refs,
        &[("PYTHONDONTWRITEBYTECODE", "1")],
    )?;
    Ok((
        toolchain,
        output,
        json!({ "sourcePath": path_text(&source)?, "argumentCount": arguments.len() }),
    ))
}

fn execute_cxx_check(
    input_root: &Path,
    input: &serde_json::Map<String, Value>,
) -> anyhow::Result<(String, Output, Value)> {
    let source = safe_input_file(input_root, required_string(input, "sourcePath")?)?;
    let language = required_string(input, "language")?;
    let (program, language_flag) = match language {
        "c" => ("/usr/local/bin/gcc", "c"),
        "cpp" => ("/usr/local/bin/g++", "c++"),
        _ => bail!("language must be c or cpp"),
    };
    let toolchain = first_line(&run_process(program, &["--version"], &[])?)?;
    let output = run_process(
        program,
        &["-x", language_flag, "-fsyntax-only", path_text(&source)?],
        &[],
    )?;
    Ok((
        toolchain,
        output,
        json!({ "sourcePath": path_text(&source)?, "language": language }),
    ))
}

fn execute_playwright(
    input_root: &Path,
    output_root: &Path,
    input: &serde_json::Map<String, Value>,
) -> anyhow::Result<(String, Output, Value)> {
    let source = safe_input_file(input_root, required_string(input, "sourcePath")?)?;
    let screenshot_path = required_string(input, "screenshotPath")?;
    let screenshot = safe_output_path(output_root, screenshot_path)?;
    let version_output = run_process(
        "/usr/bin/node",
        &["/opt/fudian-playwright/version.mjs"],
        &[("PLAYWRIGHT_BROWSERS_PATH", "/ms-playwright")],
    )?;
    let toolchain = first_line(&version_output)?;
    let output = run_process(
        "/usr/bin/node",
        &[
            "/opt/fudian-playwright/inspect.mjs",
            path_text(&source)?,
            path_text(&screenshot)?,
        ],
        &[("PLAYWRIGHT_BROWSERS_PATH", "/ms-playwright")],
    )?;
    Ok((
        toolchain,
        output,
        json!({
            "sourcePath": path_text(&source)?,
            "screenshotPath": screenshot_path,
        }),
    ))
}

fn run_process(
    program: &str,
    args: &[&str],
    environment: &[(&str, &str)],
) -> anyhow::Result<Output> {
    fs::create_dir_all("/tmp/fudian-tool-home").context("create isolated tool HOME")?;
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", "/tmp/fudian-tool-home")
        .env("TMPDIR", "/tmp")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, value) in environment {
        command.env(name, value);
    }
    command
        .output()
        .with_context(|| format!("execute fixed toolchain program {program}"))
}

fn fixed_root(name: &str, expected: &str, writable: bool) -> anyhow::Result<PathBuf> {
    let value = env::var(name).with_context(|| format!("read {name}"))?;
    if value != expected {
        bail!("{name} does not match the fixed Worker mount contract");
    }
    let root = PathBuf::from(value);
    let metadata = fs::symlink_metadata(&root).with_context(|| format!("inspect {name}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("{name} is not a safe directory");
    }
    if writable {
        let probe = root.join(format!(".fudian-tool-probe-{}", std::process::id()));
        fs::write(&probe, b"").context("probe output mount")?;
        fs::remove_file(probe).context("remove output probe")?;
    }
    Ok(root)
}

fn safe_input_file(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    if !is_portable_workspace_file_path(relative) {
        bail!("sourcePath is not a portable workspace file path");
    }
    let path = walk_without_symlinks(root, relative, false)?;
    if !fs::symlink_metadata(&path)?.is_file() {
        bail!("sourcePath is not a regular file");
    }
    Ok(path)
}

fn safe_output_path(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    if !is_portable_workspace_file_path(relative) {
        bail!("output path is not a portable workspace file path");
    }
    let path = walk_without_symlinks(root, relative, true)?;
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (metadata.file_type().is_symlink() || !metadata.is_file())
    {
        bail!("output path already exists as a non-regular file");
    }
    Ok(path)
}

fn walk_without_symlinks(
    root: &Path,
    relative: &str,
    create_parents: bool,
) -> anyhow::Result<PathBuf> {
    let mut current = root.to_path_buf();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(component) = component else {
            bail!("path contains a non-normal component");
        };
        current.push(component);
        let final_component = index + 1 == components.len();
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => bail!("path contains a symlink"),
            Ok(metadata) if !final_component && !metadata.is_dir() => {
                bail!("path parent is not a directory")
            }
            Ok(_) => {}
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && create_parents
                    && !final_component =>
            {
                fs::create_dir(&current).context("create safe output parent")?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && final_component => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(current)
}

fn write_output_file(root: &Path, relative: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let path = safe_output_path(root, relative)?;
    fs::write(path, bytes).context("write structured tool report")
}

fn required_string<'a>(
    input: &'a serde_json::Map<String, Value>,
    key: &str,
) -> anyhow::Result<&'a str> {
    let value = input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("{key} must be a string"))?;
    if value.is_empty() || value.len() > 4_000 || value.contains('\0') {
        bail!("{key} is empty, too long, or contains NUL");
    }
    Ok(value)
}

fn optional_string_array(
    input: &serde_json::Map<String, Value>,
    key: &str,
) -> anyhow::Result<Vec<String>> {
    let Some(value) = input.get(key) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("{key} must be an array"))?;
    if values.len() > 50 {
        bail!("{key} contains too many values");
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| value.len() <= 1_000 && !value.contains('\0'))
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("{key} must contain bounded strings"))
        })
        .collect()
}

fn ensure_version(observed: &str, allowed: &[&str]) -> anyhow::Result<()> {
    if !allowed.contains(&observed) {
        bail!("plugin version is not provided by this runtime binary");
    }
    Ok(())
}

fn first_line(output: &Output) -> anyhow::Result<String> {
    if !output.status.success() {
        bail!("toolchain version probe failed");
    }
    let combined = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    String::from_utf8_lossy(combined)
        .lines()
        .next()
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("toolchain version probe returned no text"))
}

fn limited_text(bytes: &[u8]) -> String {
    let end = bytes.len().min(MAX_CAPTURE_BYTES);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn path_text(path: &Path) -> anyhow::Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("runtime path is not UTF-8"))
}
