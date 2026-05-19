//! Continuum daemon: one long-lived, stateful process per workspace. Holds the
//! code graph in memory, owns the SQLite-backed agent memory, and serves MCP
//! over a TCP loopback socket to any number of thin adapters.

mod lifecycle;
mod mcp;
mod tools;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use continuum_core::{Handshake, HandshakeReply, LockFile, PROTOCOL_VERSION};
use continuum_graph::CodeGraph;
use continuum_memory::Memory;
use continuum_transport::framing::{read_line, write_line};
use tokio::io::{AsyncBufRead, AsyncWrite, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};
use tracing_subscriber::EnvFilter;

use crate::lifecycle::Workspace;

#[derive(Parser)]
#[command(
    name = "continuum-daemon",
    version,
    about = "Continuum workspace daemon"
)]
struct Args {
    /// Workspace root directory.
    #[arg(long)]
    workspace: PathBuf,
    /// Idle minutes before the daemon shuts itself down (0 = never).
    #[arg(long, env = "CONTINUUM_IDLE_MINUTES", default_value_t = 30)]
    idle_minutes: u64,
}

/// Shared daemon state, handed to every connection task.
pub(crate) struct Daemon {
    pub(crate) token: String,
    pub(crate) graph: Arc<RwLock<CodeGraph>>,
    pub(crate) memory: Memory,
    /// Semantic search engine. Dormant until its model loads in the background.
    pub(crate) semantic: Arc<continuum_search::SemanticEngine>,
    pub(crate) conns: AtomicUsize,
    pub(crate) last_activity: Mutex<Instant>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();
    let ws = Workspace::resolve(&args.workspace)?;

    // One daemon per workspace: hold the singleton lock for our whole lifetime.
    let _singleton = match ws.acquire_singleton()? {
        Some(lock) => lock,
        None => {
            tracing::info!("another daemon already owns this workspace; exiting");
            return Ok(());
        }
    };

    let memory = Memory::open(&ws.db_path()).map_err(|e| anyhow::anyhow!("open memory: {e}"))?;
    let graph = Arc::new(RwLock::new(CodeGraph::new()));

    // The semantic engine exists immediately but stays dormant until the
    // embedding model finishes loading in the background, so the daemon never
    // blocks startup on a model download.
    let semantic = Arc::new(continuum_search::SemanticEngine::new());

    // Index in the background so the daemon serves immediately; navigation
    // tools return progressively richer results as the scan completes.
    {
        let graph = graph.clone();
        let semantic = semantic.clone();
        let root = ws.root_path();
        tokio::spawn(async move {
            let n = continuum_indexer::index_workspace(&root, graph, semantic).await;
            tracing::info!("initial index complete: {n} files");
        });
    }
    let _watcher =
        continuum_indexer::start_watcher(ws.root_path(), graph.clone(), semantic.clone())
            .map_err(|e| anyhow::anyhow!("start file watcher: {e}"))?;

    // Load the embedding model off the startup path; back-fill the semantic
    // index once it is ready.
    spawn_model_load(semantic.clone(), graph.clone());

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("bind IPC socket")?;
    let addr = listener.local_addr()?;
    let token = generate_token();

    ws.write_lockfile(&LockFile {
        pid: std::process::id(),
        endpoint: addr.to_string(),
        token: token.clone(),
        protocol_version: PROTOCOL_VERSION,
    })?;
    tracing::info!(
        "continuum daemon listening on {addr} (workspace {})",
        ws.display()
    );

    let daemon = Arc::new(Daemon {
        token,
        graph,
        memory,
        semantic,
        conns: AtomicUsize::new(0),
        last_activity: Mutex::new(Instant::now()),
    });

    if args.idle_minutes > 0 {
        spawn_idle_watch(
            daemon.clone(),
            ws.clone(),
            Duration::from_secs(args.idle_minutes * 60),
        );
    }

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted.context("accept")?;
                // Reserve a connection slot up front; reject past the cap so a
                // flood of connections cannot spawn unbounded tasks.
                if daemon.conns.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    daemon.conns.fetch_sub(1, Ordering::SeqCst);
                    tracing::warn!("connection cap ({MAX_CONNECTIONS}) reached; rejecting {peer}");
                    continue;
                }
                let d = daemon.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, &d).await {
                        tracing::debug!("connection {peer} ended: {e}");
                    }
                    d.conns.fetch_sub(1, Ordering::SeqCst);
                    *d.last_activity.lock().await = Instant::now();
                });
            }
            _ = shutdown_signal() => {
                tracing::info!("shutdown signal received; stopping");
                break;
            }
        }
    }

    // Clean exit: drop the lockfile so the next adapter spawns a fresh daemon
    // instead of dialing a dead endpoint. The singleton lock and file watcher
    // are released as their guards drop.
    ws.remove_lockfile();
    tracing::info!("continuum daemon stopped");
    Ok(())
}

/// Resolves when the process is asked to shut down: Ctrl-C on any platform,
/// or additionally SIGTERM on Unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

/// Hard cap on concurrent adapter connections. Past this the daemon rejects new
/// connections rather than spawning unbounded tasks.
const MAX_CONNECTIONS: usize = 128;

fn generate_token() -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

/// Background task: exit once no adapters have been connected for `idle`.
fn spawn_idle_watch(daemon: Arc<Daemon>, ws: Workspace, idle: Duration) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if daemon.conns.load(Ordering::SeqCst) != 0 {
                continue;
            }
            let idle_for = daemon.last_activity.lock().await.elapsed();
            if idle_for >= idle {
                tracing::info!("idle for {idle_for:?}; shutting down");
                ws.remove_lockfile();
                std::process::exit(0);
            }
        }
    });
}

/// Load the embedding model off the startup path. On success, install it into
/// the semantic engine and back-fill the index from whatever the graph already
/// holds; on failure the daemon stays on lexical-only search.
fn spawn_model_load(
    semantic: Arc<continuum_search::SemanticEngine>,
    graph: Arc<RwLock<CodeGraph>>,
) {
    tokio::spawn(async move {
        match tokio::task::spawn_blocking(continuum_search::Embedder::load).await {
            Ok(Ok(embedder)) => {
                semantic.activate(embedder);
                let outlines = graph.read().await.all_outlines();
                for outline in outlines {
                    let docs: Vec<continuum_search::SymbolDoc> = outline
                        .items
                        .iter()
                        .map(|item| continuum_search::SymbolDoc {
                            name: item.name.clone(),
                            kind: item.kind.clone(),
                            path: outline.path.clone(),
                            line: item.start_line,
                            signature: item.signature.clone(),
                            embed_text: format!("{} {}", item.name, item.signature),
                        })
                        .collect();
                    semantic.index_file(&outline.path, docs).await;
                }
                tracing::info!("embedding model ready; semantic search enabled");
            }
            Ok(Err(e)) => tracing::warn!("semantic search disabled (model load failed): {e}"),
            Err(e) => tracing::warn!("semantic search disabled (load task panicked): {e}"),
        }
    });
}

/// Validate the Continuum handshake, then serve MCP for the connection's life.
/// Connection accounting (the `conns` counter, `last_activity`) is handled by
/// the accept loop's spawn wrapper.
async fn handle_connection(stream: TcpStream, daemon: &Arc<Daemon>) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let line = match read_line(&mut reader).await? {
        Some(l) => l,
        None => return Ok(()),
    };
    let hs: Handshake = serde_json::from_str(line.trim()).context("parse handshake")?;
    let ok = hs.protocol_version == PROTOCOL_VERSION && hs.token == daemon.token;
    let reply = HandshakeReply {
        ok,
        error: (!ok).then(|| "protocol version or token mismatch".to_string()),
        protocol_version: PROTOCOL_VERSION,
    };
    write_line(&mut write_half, &serde_json::to_string(&reply)?).await?;
    if !ok {
        tracing::warn!("rejected adapter handshake");
        return Ok(());
    }

    serve_mcp(&mut reader, &mut write_half, daemon).await
}

/// The per-connection MCP request/response loop.
async fn serve_mcp<R, W>(reader: &mut R, writer: &mut W, daemon: &Arc<Daemon>) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    while let Some(line) = read_line(reader).await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        *daemon.last_activity.lock().await = Instant::now();
        if let Some(response) = mcp::handle_message(trimmed, daemon).await {
            write_line(writer, &serde_json::to_string(&response)?).await?;
        }
    }
    Ok(())
}
