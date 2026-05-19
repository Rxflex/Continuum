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
        self.index
            .write()
            .await
            .by_file
            .insert(path.to_string(), entries);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_yields_unit_length() {
        let n = normalize(vec![3.0, 4.0]);
        let len = (n[0] * n[0] + n[1] * n[1]).sqrt();
        assert!((len - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_zero_vector_is_safe() {
        assert_eq!(normalize(vec![0.0, 0.0]), vec![0.0, 0.0]);
    }

    #[test]
    fn cosine_of_identical_normalized_vectors_is_one() {
        let a = normalize(vec![1.0, 2.0, 3.0, 4.0]);
        assert!((dot(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_of_orthogonal_vectors_is_zero() {
        let a = normalize(vec![1.0, 0.0]);
        let b = normalize(vec![0.0, 1.0]);
        assert!(dot(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn index_search_ranks_nearest_entry_first() {
        let mut index = SemanticIndex::default();
        index.by_file.insert(
            "f.rs".to_string(),
            vec![
                Entry {
                    name: "near".to_string(),
                    kind: "function".to_string(),
                    path: "f.rs".to_string(),
                    line: 1,
                    signature: String::new(),
                    vector: normalize(vec![1.0, 0.1, 0.0]),
                },
                Entry {
                    name: "far".to_string(),
                    kind: "function".to_string(),
                    path: "f.rs".to_string(),
                    line: 2,
                    signature: String::new(),
                    vector: normalize(vec![0.0, 0.0, 1.0]),
                },
            ],
        );
        let query = normalize(vec![1.0, 0.0, 0.0]);
        let hits = index.search(&query, 10, None);
        assert_eq!(hits[0].name, "near");
    }
}
