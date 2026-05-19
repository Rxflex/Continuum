//! Continuum adapter: the thin MCP client an AI IDE/CLI spawns. It exposes a
//! standard MCP stdio interface to the agent and proxies every byte to the
//! per-workspace daemon over TCP loopback, spawning that daemon on demand.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use clap::Parser;
use continuum_core::{LockFile, PROTOCOL_VERSION};
use continuum_transport::proxy::{attach, run_proxy, AttachResult};
use tokio::net::TcpStream;

#[derive(Parser)]
#[command(
    name = "continuum-adapter",
    version,
    about = "Continuum thin MCP adapter"
)]
struct Args {
    /// Workspace root. Defaults to the current working directory.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Idle minutes passed through to a freshly spawned daemon.
    #[arg(long, default_value_t = 30)]
    idle_minutes: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = Args::parse();
    let workspace = args
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("current dir"));
    let workspace = workspace.canonicalize().unwrap_or(workspace);

    match connect_or_spawn(&workspace, args.idle_minutes).await {
        Ok(stream) => run_proxy(stream).await,
        Err(e) => {
            eprintln!("continuum-adapter: could not reach daemon: {e}");
            std::process::exit(1);
        }
    }
}

/// Attach to a running daemon, or spawn one and wait for it to come up.
async fn connect_or_spawn(workspace: &Path, idle_minutes: u64) -> Result<TcpStream, String> {
    if let Some(stream) = try_attach(workspace).await {
        return Ok(stream);
    }
    spawn_daemon(workspace, idle_minutes)?;

    // Poll for readiness (~15s). Multiple adapters may race here; the daemon's
    // singleton lock guarantees exactly one survives, and all of us attach to it.
    for _ in 0..150 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(stream) = try_attach(workspace).await {
            return Ok(stream);
        }
    }
    Err("daemon did not become ready within 15s".into())
}

/// One attach attempt: read the lockfile, connect, run the handshake.
async fn try_attach(workspace: &Path) -> Option<TcpStream> {
    let lock = read_lockfile(workspace)?;
    if lock.protocol_version != PROTOCOL_VERSION {
        return None; // stale daemon -- force a respawn.
    }
    match attach(&lock.endpoint, &lock.token).await {
        Ok(AttachResult::Connected(stream)) => Some(stream),
        _ => None,
    }
}

fn read_lockfile(workspace: &Path) -> Option<LockFile> {
    let path = workspace.join(".continuum").join("daemon.lock");
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

fn spawn_daemon(workspace: &Path, idle_minutes: u64) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let daemon_exe = exe.with_file_name(daemon_filename());
    if !daemon_exe.exists() {
        return Err(format!(
            "daemon binary not found at {}",
            daemon_exe.display()
        ));
    }

    let continuum_dir = workspace.join(".continuum");
    let _ = std::fs::create_dir_all(&continuum_dir);
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(continuum_dir.join("daemon.log"))
        .map_err(|e| format!("open daemon log: {e}"))?;

    let mut cmd = std::process::Command::new(&daemon_exe);
    cmd.arg("--workspace")
        .arg(workspace)
        .arg("--idle-minutes")
        .arg(idle_minutes.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log));
    configure_detached(&mut cmd);
    cmd.spawn().map_err(|e| format!("spawn daemon: {e}"))?;
    Ok(())
}

#[cfg(windows)]
fn daemon_filename() -> &'static str {
    "continuum-daemon.exe"
}

#[cfg(not(windows))]
fn daemon_filename() -> &'static str {
    "continuum-daemon"
}

#[cfg(windows)]
fn configure_detached(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn configure_detached(_cmd: &mut std::process::Command) {}
