//! Reciprocal Rank Fusion of the lexical and semantic result lists.

use std::collections::HashMap;

use continuum_core::dto::SearchHit;

/// Fuse two ranked result lists into one.
///
/// The RRF score of an item is the sum over lists of `1 / (K + rank)`. It needs
/// no score calibration between the lexical (BM25) and semantic (cosine)
/// rankers — only their ranks — which makes it robust to their very different
/// score scales. A hit is identified by `(path, line)`.
pub fn fuse(lexical: Vec<SearchHit>, semantic: Vec<SearchHit>, limit: usize) -> Vec<SearchHit> {
    const K: f32 = 60.0;
    let mut merged: HashMap<(String, usize), (f32, SearchHit)> = HashMap::new();

    for (rank, hit) in lexical.into_iter().enumerate() {
        let key = (hit.path.clone(), hit.line);
        let slot = merged.entry(key).or_insert((0.0, hit));
        slot.0 += 1.0 / (K + rank as f32 + 1.0);
    }
    for (rank, hit) in semantic.into_iter().enumerate() {
        let key = (hit.path.clone(), hit.line);
        let slot = merged.entry(key).or_insert((0.0, hit));
        slot.0 += 1.0 / (K + rank as f32 + 1.0);
    }

    let mut ranked: Vec<(f32, SearchHit)> = merged.into_values().collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(limit);
    ranked
        .into_iter()
        .map(|(score, mut hit)| {
            hit.score = score;
            hit
        })
        .collect()
}
