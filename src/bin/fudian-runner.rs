use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    io::{self, Write},
    net::{SocketAddr, TcpStream},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command as StdCommand, Stdio},
    time::Duration,
};

use anyhow::{Context, bail};
use fudian::runner_protocol::{
    RunnerExecutionResult, RunnerIsolationAttestation, RunnerJobSpec, RunnerOutputFile,
    canonical_json_sha256, is_portable_workspace_file_path,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    time,
};

const RUNNER_PROGRAM: &str = "/usr/local/bin/fudian-runner";

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("execute") if args.len() == 2 => execute(Path::new(&args[1])).await,
        Some("digest") if args.len() == 1 => {
            println!("{}", executable_digest()?);
            Ok(())
        }
        Some("fixture-write") if args.len() == 3 => fixture_write(&args[1], args[2].as_bytes()),
        Some("fixture-symlink") if args.len() == 3 => fixture_symlink(&args[1], &args[2]),
        Some("fixture-network-probe") if args.len() == 2 => fixture_network_probe(&args[1]),
        Some("fixture-sleep") if args.len() == 2 => fixture_sleep(&args[1]),
        Some("fixture-disk") if args.len() == 3 => fixture_disk(&args[1], &args[2]),
        Some("fixture-memory") if args.len() == 2 => fixture_memory(&args[1]),
        Some("fixture-pids") if args.len() == 2 => fixture_pids(&args[1]),
        Some("fixture-probe") if args.len() == 2 => fixture_probe(&args[1]),
        _ => bail!(
            "usage: fudian-runner execute <spec.json> (fixture commands are test-only runtime entries)"
        ),
    }
}

async fn execute(spec_path: &Path) -> anyhow::Result<()> {
    let spec_bytes = fs::read(spec_path).context("read RunnerJobSpec")?;
    let spec: RunnerJobSpec = serde_json::from_slice(&spec_bytes).context("parse RunnerJobSpec")?;
    validate_spec_shape(&spec)?;
    let spec_hash = spec.digest().context("hash RunnerJobSpec")?;
    let isolation = inspect_isolation(&spec)?;
    let mut diagnostics = Vec::<String>::new();
    if !isolation_satisfies_spec(&spec, &isolation, &mut diagnostics) {
        let result = empty_result(
            &spec,
            spec_hash,
            "policy_denied",
            None,
            isolation,
            json!({ "reasons": diagnostics }),
        )?;
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    let mut command = Command::new(&spec.command.program);
    command
        .args(&spec.command.args)
        .current_dir(&spec.input_mount)
        .env_clear()
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("HOME", "/tmp/fudian-home")
        .env("TMPDIR", "/tmp")
        .env("FUDIAN_INPUT", &spec.input_mount)
        .env("FUDIAN_OUTPUT", &spec.output_mount)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (name, value) in &spec.command.environment {
        command.env(name, value);
    }
    fs::create_dir_all("/tmp/fudian-home").context("create isolated HOME")?;

    let mut child = command.spawn().context("start isolated runtime entry")?;
    let stdout = child.stdout.take().context("capture stdout")?;
    let stderr = child.stderr.take().context("capture stderr")?;
    let stdout_task = tokio::spawn(hash_stream(stdout, spec.resources.stdout_bytes));
    let stderr_task = tokio::spawn(hash_stream(stderr, spec.resources.stderr_bytes));
    let wait = time::timeout(
        Duration::from_secs(u64::from(spec.resources.timeout_seconds)),
        child.wait(),
    )
    .await;
    let (status_name, exit_code) = match wait {
        Ok(status) => {
            let status = status.context("wait for runtime entry")?;
            if status.success() {
                ("succeeded", status.code())
            } else {
                ("failed", status.code())
            }
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            ("timed_out", None)
        }
    };
    let stdout = stdout_task.await.context("join stdout reader")??;
    let stderr = stderr_task.await.context("join stderr reader")??;

    let scan = scan_output(Path::new(&spec.output_mount), spec.resources.disk_mi_b);
    let (files, output_manifest_hash, scan_error) = match scan {
        Ok((files, hash)) => (files, hash, None),
        Err(error) => (
            Vec::new(),
            canonical_json_sha256(&Vec::<RunnerOutputFile>::new())?,
            Some(error.to_string()),
        ),
    };
    if stdout.exceeded {
        diagnostics.push("stdout exceeded the declared byte limit".to_owned());
    }
    if stderr.exceeded {
        diagnostics.push("stderr exceeded the declared byte limit".to_owned());
    }
    if let Some(error) = scan_error {
        diagnostics.push(error);
    }
    let final_status = if diagnostics.is_empty() {
        status_name
    } else {
        "policy_denied"
    };
    let result = RunnerExecutionResult {
        schema_version: 1,
        job_id: spec.job_id,
        lease_id: spec.lease_id,
        fencing_token: spec.fencing_token,
        spec_hash,
        status: final_status.to_owned(),
        exit_code,
        files,
        output_manifest_hash,
        stdout_sha256: stdout.sha256,
        stdout_bytes: stdout.bytes,
        stderr_sha256: stderr.sha256,
        stderr_bytes: stderr.bytes,
        isolation,
        diagnostics: json!({ "reasons": diagnostics }),
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

fn validate_spec_shape(spec: &RunnerJobSpec) -> anyhow::Result<()> {
    if spec.schema_version != 1 {
        bail!("unsupported RunnerJobSpec schema");
    }
    if spec.input_mount != "/workspace/input"
        || spec.output_mount != "/workspace/output"
        || spec.result_mount != "/workspace/result"
    {
        bail!("Runner mounts do not match the fixed container contract");
    }
    if spec.command.program != RUNNER_PROGRAM && !spec.command.program.starts_with("/runtime/") {
        bail!("runtime program is outside the immutable runtime mount");
    }
    if spec.runtime_entry_digest.is_some() && !spec.command.program.starts_with("/runtime/") {
        bail!("runtime entry digest requires an immutable /runtime program");
    }
    if spec.capabilities.network != "denied"
        || !spec.capabilities.external_writes.is_empty()
        || !spec.capabilities.account_references.is_empty()
        || spec.capabilities.paid_operations
        || spec.capabilities.deployment
    {
        bail!("this Runner version only executes the fully isolated capability profile");
    }
    if spec.resources.cpu_millis == 0
        || spec.resources.memory_mi_b == 0
        || spec.resources.disk_mi_b == 0
        || spec.resources.pids == 0
        || spec.resources.timeout_seconds == 0
        || spec.resources.stdout_bytes == 0
        || spec.resources.stderr_bytes == 0
    {
        bail!("resource limits must be positive");
    }
    if spec.delete_paths.len() > 200
        || spec
            .delete_paths
            .iter()
            .any(|path| !is_portable_workspace_file_path(path))
    {
        bail!("deletion manifest contains an unsafe workspace path");
    }
    let mut normalized_deletes = spec.delete_paths.clone();
    normalized_deletes.sort();
    normalized_deletes.dedup();
    let mut case_folded_deletes = spec
        .delete_paths
        .iter()
        .map(|path| path.to_lowercase())
        .collect::<Vec<_>>();
    case_folded_deletes.sort();
    case_folded_deletes.dedup();
    if normalized_deletes != spec.delete_paths
        || case_folded_deletes.len() != spec.delete_paths.len()
        || normalized_deletes.iter().any(|path| {
            !spec
                .allowed_writes
                .iter()
                .any(|pattern| runner_path_matches_pattern(pattern, path))
        })
    {
        bail!("deletion manifest is not canonical or exceeds allowed writes");
    }
    Ok(())
}

fn runner_path_matches_pattern(pattern: &str, path: &str) -> bool {
    pattern == "**"
        || pattern == path
        || pattern
            .strip_suffix("/**")
            .is_some_and(|prefix| path.starts_with(&format!("{prefix}/")))
}

#[derive(Debug)]
struct StreamDigest {
    sha256: String,
    bytes: u64,
    exceeded: bool,
}

async fn hash_stream<R: AsyncRead + Unpin>(mut reader: R, limit: u64) -> io::Result<StreamDigest> {
    let mut digest = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        bytes = bytes.saturating_add(read as u64);
    }
    Ok(StreamDigest {
        sha256: format!("sha256:{}", hex::encode(digest.finalize())),
        bytes,
        exceeded: bytes > limit,
    })
}

fn inspect_isolation(spec: &RunnerJobSpec) -> anyhow::Result<RunnerIsolationAttestation> {
    let interfaces = fs::read_dir("/sys/class/net")
        .context("inspect network interfaces")?
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    let status = fs::read_to_string("/proc/self/status").context("inspect process status")?;
    let no_new_privileges = status
        .lines()
        .find_map(|line| line.strip_prefix("NoNewPrivs:"))
        .is_some_and(|value| value.trim() == "1");
    let effective_capabilities_hex = status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:"))
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    let mount_options = parse_mount_options()?;
    let root_read_only = mount_is(&mount_options, "/", "ro");
    let input_read_only = mount_is(&mount_options, &spec.input_mount, "ro");
    let output_writable = mount_is(&mount_options, &spec.output_mount, "rw");
    let mut sorted_interfaces = interfaces;
    sorted_interfaces.sort();
    Ok(RunnerIsolationAttestation {
        runtime_digest: executable_digest()?,
        runtime_entry_digest: runtime_entry_digest(spec)?,
        network_isolated: sorted_interfaces.iter().all(|name| name == "lo"),
        visible_network_interfaces: sorted_interfaces,
        no_new_privileges,
        effective_capabilities_hex,
        root_read_only,
        input_read_only,
        output_writable,
        docker_socket_absent: !Path::new("/var/run/docker.sock").exists(),
        host_home_absent: !Path::new("/host-home").exists(),
        observed_cpu_millis: observed_cpu_millis(),
        observed_memory_mi_b: observed_limit_mib("/sys/fs/cgroup/memory.max"),
        observed_pids: observed_u32("/sys/fs/cgroup/pids.max"),
    })
}

fn executable_digest() -> anyhow::Result<String> {
    let executable = env::current_exe().context("locate Runner executable")?;
    let bytes = fs::read(executable).context("hash Runner executable")?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn runtime_entry_digest(spec: &RunnerJobSpec) -> anyhow::Result<Option<String>> {
    let Some(expected) = &spec.runtime_entry_digest else {
        return Ok(None);
    };
    let path = Path::new(&spec.command.program);
    let metadata = fs::symlink_metadata(path).context("inspect runtime entry")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("runtime entry is not an immutable regular file");
    }
    let bytes = fs::read(path).context("hash runtime entry")?;
    let observed = format!("sha256:{}", hex::encode(Sha256::digest(bytes)));
    if &observed != expected {
        bail!("runtime entry digest does not match the signed installation");
    }
    Ok(Some(observed))
}

fn isolation_satisfies_spec(
    spec: &RunnerJobSpec,
    isolation: &RunnerIsolationAttestation,
    diagnostics: &mut Vec<String>,
) -> bool {
    if !isolation.network_isolated {
        diagnostics.push("worker network namespace is not isolated".to_owned());
    }
    if !isolation.no_new_privileges {
        diagnostics.push("no-new-privileges is missing".to_owned());
    }
    if !isolation
        .effective_capabilities_hex
        .chars()
        .all(|character| character == '0')
    {
        diagnostics.push("worker still has effective Linux capabilities".to_owned());
    }
    if !isolation.root_read_only {
        diagnostics.push("worker root filesystem is writable".to_owned());
    }
    if !isolation.input_read_only {
        diagnostics.push("workspace input is not read-only".to_owned());
    }
    if !isolation.output_writable {
        diagnostics.push("worker output layer is not writable".to_owned());
    }
    if !isolation.docker_socket_absent {
        diagnostics.push("Docker Socket is visible".to_owned());
    }
    if !isolation.host_home_absent {
        diagnostics.push("host home mount is visible".to_owned());
    }
    if isolation
        .observed_cpu_millis
        .is_none_or(|value| value > spec.resources.cpu_millis)
    {
        diagnostics.push("CPU cgroup limit is missing or wider than the spec".to_owned());
    }
    if isolation
        .observed_memory_mi_b
        .is_none_or(|value| value > spec.resources.memory_mi_b)
    {
        diagnostics.push("memory cgroup limit is missing or wider than the spec".to_owned());
    }
    if isolation
        .observed_pids
        .is_none_or(|value| value > spec.resources.pids)
    {
        diagnostics.push("pids cgroup limit is missing or wider than the spec".to_owned());
    }
    diagnostics.is_empty()
}

fn parse_mount_options() -> anyhow::Result<BTreeMap<String, Vec<String>>> {
    let mountinfo = fs::read_to_string("/proc/self/mountinfo").context("read mountinfo")?;
    let mut result = BTreeMap::new();
    for line in mountinfo.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 6 {
            continue;
        }
        result.insert(
            unescape_mount_field(fields[4]),
            fields[5].split(',').map(str::to_owned).collect(),
        );
    }
    Ok(result)
}

fn unescape_mount_field(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn mount_is(mounts: &BTreeMap<String, Vec<String>>, path: &str, option: &str) -> bool {
    mounts
        .get(path)
        .is_some_and(|options| options.iter().any(|candidate| candidate == option))
}

fn observed_cpu_millis() -> Option<u32> {
    let value = fs::read_to_string("/sys/fs/cgroup/cpu.max").ok()?;
    let mut fields = value.split_whitespace();
    let quota = fields.next()?;
    let period = fields.next()?.parse::<u64>().ok()?;
    if quota == "max" || period == 0 {
        return None;
    }
    let quota = quota.parse::<u64>().ok()?;
    u32::try_from(quota.saturating_mul(1_000).div_ceil(period)).ok()
}

fn observed_limit_mib(path: &str) -> Option<u32> {
    let bytes = fs::read_to_string(path).ok()?.trim().parse::<u64>().ok()?;
    u32::try_from(bytes.div_ceil(1024 * 1024)).ok()
}

fn observed_u32(path: &str) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn scan_output(root: &Path, disk_mi_b: u32) -> anyhow::Result<(Vec<RunnerOutputFile>, String)> {
    let mut files = Vec::new();
    let mut total = 0_u64;
    scan_output_directory(root, root, &mut files, &mut total)?;
    let maximum = u64::from(disk_mi_b) * 1024 * 1024;
    if total > maximum {
        bail!("output layer exceeds the declared disk limit");
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    let hash = canonical_json_sha256(&files)?;
    Ok((files, hash))
}

fn scan_output_directory(
    root: &Path,
    directory: &Path,
    files: &mut Vec<RunnerOutputFile>,
    total: &mut u64,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(directory).with_context(|| format!("scan {}", directory.display()))? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            bail!("worker output contains a symbolic link");
        }
        if metadata.is_dir() {
            scan_output_directory(root, &path, files, total)?;
            continue;
        }
        if !metadata.is_file() {
            bail!("worker output contains a non-regular file");
        }
        let relative = path
            .strip_prefix(root)
            .context("output path escaped root")?;
        let relative = portable_relative_path(relative)?;
        let bytes = fs::read(&path)?;
        *total = total.saturating_add(bytes.len() as u64);
        files.push(RunnerOutputFile {
            path: relative,
            sha256: format!("sha256:{}", hex::encode(Sha256::digest(&bytes))),
            size_bytes: bytes.len() as u64,
            executable: metadata.permissions().mode() & 0o111 != 0,
        });
    }
    Ok(())
}

fn portable_relative_path(path: &Path) -> anyhow::Result<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => {
                let value = value.to_str().context("output path is not UTF-8")?;
                if value.is_empty() || value == "." || value == ".." || value.contains('\\') {
                    bail!("output path is not portable");
                }
                parts.push(value);
            }
            _ => bail!("output path is not a normalized relative path"),
        }
    }
    if parts.is_empty() {
        bail!("output path is empty");
    }
    Ok(parts.join("/"))
}

fn empty_result(
    spec: &RunnerJobSpec,
    spec_hash: String,
    status: &str,
    exit_code: Option<i32>,
    isolation: RunnerIsolationAttestation,
    diagnostics: Value,
) -> anyhow::Result<RunnerExecutionResult> {
    let files = Vec::<RunnerOutputFile>::new();
    Ok(RunnerExecutionResult {
        schema_version: 1,
        job_id: spec.job_id,
        lease_id: spec.lease_id,
        fencing_token: spec.fencing_token,
        spec_hash,
        status: status.to_owned(),
        exit_code,
        output_manifest_hash: canonical_json_sha256(&files)?,
        files,
        stdout_sha256: format!("sha256:{}", hex::encode(Sha256::digest([]))),
        stdout_bytes: 0,
        stderr_sha256: format!("sha256:{}", hex::encode(Sha256::digest([]))),
        stderr_bytes: 0,
        isolation,
        diagnostics,
    })
}

fn output_path(relative: &str) -> anyhow::Result<PathBuf> {
    let root = env::var("FUDIAN_OUTPUT").context("FUDIAN_OUTPUT is not set")?;
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\0')
        || relative.contains('\\')
        || relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("invalid fixture output path");
    }
    Ok(Path::new(&root).join(relative))
}

fn fixture_write(relative: &str, bytes: &[u8]) -> anyhow::Result<()> {
    let path = output_path(relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;
    Ok(())
}

fn fixture_symlink(relative: &str, target: &str) -> anyhow::Result<()> {
    let path = output_path(relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    symlink(target, path)?;
    Ok(())
}

fn fixture_network_probe(relative: &str) -> anyhow::Result<()> {
    let address: SocketAddr = "1.1.1.1:53".parse()?;
    let connected = TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok();
    fixture_write(relative, if connected { b"connected" } else { b"blocked" })
}

fn fixture_sleep(seconds: &str) -> anyhow::Result<()> {
    let seconds = seconds.parse::<u64>().context("parse sleep seconds")?;
    std::thread::sleep(Duration::from_secs(seconds));
    Ok(())
}

fn fixture_disk(relative: &str, byte_count: &str) -> anyhow::Result<()> {
    let byte_count = byte_count.parse::<u64>().context("parse byte count")?;
    let path = output_path(relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    let buffer = [0_u8; 64 * 1024];
    let mut remaining = byte_count;
    while remaining > 0 {
        let count = usize::try_from(remaining.min(buffer.len() as u64))?;
        file.write_all(&buffer[..count])?;
        remaining -= count as u64;
    }
    file.sync_all()?;
    Ok(())
}

fn fixture_memory(mebibytes: &str) -> anyhow::Result<()> {
    let mebibytes = mebibytes.parse::<usize>().context("parse memory MiB")?;
    let mut bytes = vec![0_u8; mebibytes.saturating_mul(1024 * 1024)];
    for index in (0..bytes.len()).step_by(4096) {
        bytes[index] = 1;
    }
    std::hint::black_box(&bytes);
    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

fn fixture_pids(count: &str) -> anyhow::Result<()> {
    let count = count.parse::<usize>().context("parse process count")?;
    let mut children = Vec::new();
    for _ in 0..count {
        match StdCommand::new(RUNNER_PROGRAM)
            .args(["fixture-sleep", "10"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(child) => children.push(child),
            Err(error) => {
                for child in &mut children {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                bail!(
                    "process limit stopped spawn after {} children: {error}",
                    children.len()
                );
            }
        }
    }
    for child in &mut children {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

fn fixture_probe(relative: &str) -> anyhow::Result<()> {
    let input = env::var("FUDIAN_INPUT").context("FUDIAN_INPUT is not set")?;
    let input_write = Path::new(&input).join(".fudian-write-probe");
    let input_writable = fs::write(&input_write, b"probe").is_ok();
    if input_writable {
        let _ = fs::remove_file(input_write);
    }
    fixture_write(
        relative,
        serde_json::to_string(&json!({
            "dockerSocketVisible": Path::new("/var/run/docker.sock").exists(),
            "hostHomeVisible": Path::new("/host-home").exists(),
            "inputWritable": input_writable,
        }))?
        .as_bytes(),
    )
}
