//! Filesystem watcher with a ~300 ms debouncer. Coalesced change batches are
//! re-indexed into the graph under a single write lock.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use continuum_graph::CodeGraph;
use continuum_search::SemanticEngine;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{mpsc, RwLock};

/// Debounce window for coalescing filesystem events — override with
/// `CONTINUUM_DEBOUNCE_MS`.
static DEBOUNCE: std::sync::LazyLock<Duration> = std::sync::LazyLock::new(|| {
    let ms = std::env::var("CONTINUUM_DEBOUNCE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(300);
    Duration::from_millis(ms)
});

/// Begin watching `root`. The returned `RecommendedWatcher` must be kept alive
/// for watching to continue -- dropping it stops the watch.
pub fn start_watcher(
    root: PathBuf,
    graph: Arc<RwLock<CodeGraph>>,
    semantic: Arc<SemanticEngine>,
) -> notify::Result<RecommendedWatcher> {
    let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            for path in event.paths {
                let _ = tx.send(path);
            }
        }
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            let mut batch: HashSet<PathBuf> = HashSet::new();
            batch.insert(first);

            let timer = tokio::time::sleep(*DEBOUNCE);
            tokio::pin!(timer);
            loop {
                tokio::select! {
                    _ = &mut timer => break,
                    maybe = rx.recv() => match maybe {
                        Some(path) => { batch.insert(path); }
                        None => break,
                    },
                }
            }

            for path in &batch {
                crate::reindex_one(&root, path, &graph, &semantic).await;
            }
            let mut guard = graph.write().await;
            continuum_graph::resolver::resolve(&mut guard);
        }
    });

    Ok(watcher)
}
