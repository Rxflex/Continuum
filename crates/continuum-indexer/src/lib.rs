//! The indexer: filesystem watching plus tree-sitter parsing that keeps the
//! [`continuum_graph::CodeGraph`] in sync with files on disk.

mod languages;
mod parser;
mod watcher;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use continuum_graph::CodeGraph;
use tokio::sync::RwLock;

use languages::Lang;

pub use watcher::start_watcher;

/// Directory names never descended into during indexing.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".continuum",
    "dist",
    "build",
    ".venv",
    "__pycache__",
];

/// Full one-shot index of a workspace. Returns the number of files indexed.
pub async fn index_workspace(root: &Path, graph: Arc<RwLock<CodeGraph>>) -> usize {
    let mut count = 0;
    for abs in collect_source_files(root) {
        // Parsing is CPU-bound and lock-free; only the graph update takes the lock.
        if let Some((rel, parsed)) = parse_path(root, &abs) {
            let mut guard = graph.write().await;
            guard.replace_file(&rel, parsed.file_node, parsed.symbols);
            count += 1;
        }
    }
    let mut guard = graph.write().await;
    continuum_graph::resolver::resolve(&mut guard);
    count
}

/// Re-index a single path after a filesystem change (or drop it if deleted).
pub(crate) async fn reindex_one(root: &Path, abs: &Path, graph: &Arc<RwLock<CodeGraph>>) {
    let supported = abs
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| Lang::from_extension(e).is_some())
        .unwrap_or(false);
    if !supported {
        return;
    }
    let rel = rel_path(root, abs);
    if abs.is_file() {
        if let Some((_, parsed)) = parse_path(root, abs) {
            let mut guard = graph.write().await;
            guard.replace_file(&rel, parsed.file_node, parsed.symbols);
            tracing::debug!("re-indexed {rel}");
        }
    } else {
        let mut guard = graph.write().await;
        guard.remove_file(&rel);
        tracing::debug!("dropped {rel}");
    }
}

fn parse_path(root: &Path, abs: &Path) -> Option<(String, parser::ParsedFile)> {
    let ext = abs.extension()?.to_str()?;
    let lang = Lang::from_extension(ext)?;
    let source = std::fs::read_to_string(abs).ok()?;
    let rel = rel_path(root, abs);
    let parsed = parser::parse(&rel, &source, lang)?;
    Some((rel, parsed))
}

/// Workspace-relative path with forward slashes, used as the graph's file key.
fn rel_path(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| {
            let is_skipped_dir = entry.file_type().is_dir()
                && entry
                    .file_name()
                    .to_str()
                    .map(|n| SKIP_DIRS.contains(&n))
                    .unwrap_or(false);
            !is_skipped_dir
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().to_path_buf())
        .collect()
}
