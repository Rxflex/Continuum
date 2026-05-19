//! The in-memory vector index and the engine that drives it.
//!
//! Symbol counts are modest (thousands), so the index is a flat list scanned
//! by brute-force cosine similarity per query — sub-millisecond at this scale,
//! and no approximate-nearest-neighbour structure to maintain.

use std::collections::HashMap;

use continuum_core::dto::SearchHit;
use tokio::sync::RwLock;

use crate::embedder::Embedder;

/// A symbol handed to the semantic index for embedding.
pub struct SymbolDoc {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: usize,
    pub signature: String,
    /// The text actually embedded (typically `name + signature`).
    pub embed_text: String,
}

struct Entry {
    name: String,
    kind: String,
    path: String,
    line: usize,
    signature: String,
    vector: Vec<f32>,
}

#[derive(Default)]
struct SemanticIndex {
    by_file: HashMap<String, Vec<Entry>>,
}

impl SemanticIndex {
    fn search(&self, query: &[f32], limit: usize, kind: Option<&str>) -> Vec<SearchHit> {
        let mut scored: Vec<(f32, &Entry)> = Vec::new();
        for entries in self.by_file.values() {
            for entry in entries {
                if kind.is_some_and(|k| entry.kind != k) {
                    continue;
                }
                scored.push((dot(query, &entry.vector), entry));
            }
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
            .into_iter()
            .map(|(score, e)| SearchHit {
                name: e.name.clone(),
                kind: e.kind.clone(),
                path: e.path.clone(),
                line: e.line,
                signature: e.signature.clone(),
                score,
            })
            .collect()
    }
}

/// Embedding-backed semantic search over the workspace's symbols.
pub struct SemanticEngine {
    embedder: Embedder,
    index: RwLock<SemanticIndex>,
}

impl SemanticEngine {
    pub fn new(embedder: Embedder) -> Self {
        Self {
            embedder,
            index: RwLock::new(SemanticIndex::default()),
        }
    }

    /// Embed and store all symbols of one file, replacing any prior entries.
    pub async fn index_file(&self, path: &str, docs: Vec<SymbolDoc>) {
        if docs.is_empty() {
            self.index.write().await.by_file.remove(path);
            return;
        }
        let texts: Vec<String> = docs.iter().map(|d| d.embed_text.clone()).collect();
        let vectors = self.embedder.embed(&texts);
        let entries: Vec<Entry> = docs
            .into_iter()
            .zip(vectors)
            .map(|(doc, vector)| Entry {
                name: doc.name,
                kind: doc.kind,
                path: doc.path,
                line: doc.line,
                signature: doc.signature,
                vector: normalize(vector),
            })
            .collect();
        self.index.write().await.by_file.insert(path.to_string(), entries);
    }

    pub async fn remove_file(&self, path: &str) {
        self.index.write().await.by_file.remove(path);
    }

    /// Embed `query` and return the nearest symbols by cosine similarity.
    pub async fn search(&self, query: &str, limit: usize, kind: Option<&str>) -> Vec<SearchHit> {
        let q = normalize(self.embedder.embed_one(query));
        self.index.read().await.search(&q, limit, kind)
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut v {
            *x /= norm;
        }
    }
    v
}
