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
    // De-prioritize test code so a real implementation ranks above its tests.
    for (score, hit) in &mut ranked {
        if hit.is_test {
            *score *= 0.3;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(name: &str, path: &str, line: usize) -> SearchHit {
        SearchHit {
            name: name.to_string(),
            kind: "function".to_string(),
            path: path.to_string(),
            line,
            signature: String::new(),
            is_test: false,
            score: 0.0,
        }
    }

    #[test]
    fn fuse_merges_dedups_and_ranks_shared_hits_first() {
        let lexical = vec![hit("a", "f.rs", 1), hit("b", "f.rs", 2)];
        let semantic = vec![hit("b", "f.rs", 2), hit("c", "f.rs", 3)];
        let out = fuse(lexical, semantic, 10);
        assert_eq!(out.len(), 3, "deduped by (path, line)");
        assert_eq!(out[0].name, "b", "hit in both lists ranks first");
    }

    #[test]
    fn fuse_respects_the_limit() {
        let lexical = vec![
            hit("a", "f.rs", 1),
            hit("b", "f.rs", 2),
            hit("c", "f.rs", 3),
        ];
        assert_eq!(fuse(lexical, vec![], 2).len(), 2);
    }

    #[test]
    fn fuse_of_empty_inputs_is_empty() {
        assert!(fuse(vec![], vec![], 5).is_empty());
    }
}
