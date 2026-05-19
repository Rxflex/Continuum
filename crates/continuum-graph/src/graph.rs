//! The [`CodeGraph`]: nodes + edges + lookup indices, one source of truth.

use std::collections::{HashMap, HashSet};

use continuum_core::dto::{
    CallerRef, DependencyNode, FileOutline, GraphStats, OutlineItem, SearchHit, SymbolDefinition,
};
use petgraph::stable_graph::{NodeIndex, StableGraph};
use serde::{Deserialize, Serialize};

use crate::model::{EdgeKind, EdgeResolution, GraphEdge, GraphNode, NodeKind};

/// Directed code graph. Not internally synchronized -- guard with an `RwLock`.
#[derive(Default)]
pub struct CodeGraph {
    graph: StableGraph<GraphNode, GraphEdge>,
    by_id: HashMap<String, NodeIndex>,
    by_name: HashMap<String, Vec<NodeIndex>>,
    by_file: HashMap<String, Vec<NodeIndex>>,
    /// Search tokens per node, computed once at insert time (see `search`).
    tokens: HashMap<NodeIndex, Vec<String>>,
}

impl CodeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace every node for `path` with a fresh file node plus its symbols.
    /// Called by the indexer on each debounced file change.
    pub fn replace_file(&mut self, path: &str, file_node: GraphNode, symbols: Vec<GraphNode>) {
        self.remove_file(path);

        let file_idx = self.insert_node(file_node);
        for sym in symbols {
            let sym_idx = self.insert_node(sym);
            self.graph.add_edge(
                file_idx,
                sym_idx,
                GraphEdge {
                    kind: EdgeKind::Contains,
                    resolution: EdgeResolution::Resolved,
                },
            );
        }
    }

    /// Drop all nodes (and their edges) belonging to `path`.
    pub fn remove_file(&mut self, path: &str) {
        let Some(indices) = self.by_file.remove(path) else {
            return;
        };
        for idx in indices {
            self.tokens.remove(&idx);
            if let Some(node) = self.graph.remove_node(idx) {
                self.by_id.remove(&node.id);
                if let Some(bucket) = self.by_name.get_mut(&node.name) {
                    bucket.retain(|i| *i != idx);
                }
            }
        }
    }

    fn insert_node(&mut self, node: GraphNode) -> NodeIndex {
        let id = node.id.clone();
        let name = node.name.clone();
        let path = node.path.clone();
        let toks = tokenize(&format!("{} {} {}", node.name, node.signature, node.source));
        let idx = self.graph.add_node(node);
        self.by_id.insert(id, idx);
        self.by_name.entry(name).or_default().push(idx);
        self.by_file.entry(path).or_default().push(idx);
        self.tokens.insert(idx, toks);
        idx
    }

    /// Structure of one file, definition bodies omitted.
    pub fn file_outline(&self, path: &str) -> Option<FileOutline> {
        let indices = self.by_file.get(path)?;
        let mut items = Vec::new();
        let mut language = String::new();
        for &idx in indices {
            let node = &self.graph[idx];
            if node.kind == NodeKind::File {
                language = node.language.clone();
                continue;
            }
            items.push(OutlineItem {
                kind: node.kind.as_str().to_string(),
                name: node.name.clone(),
                signature: node.signature.clone(),
                start_line: node.start_line,
                end_line: node.end_line,
            });
        }
        items.sort_by_key(|i| i.start_line);
        Some(FileOutline {
            path: path.to_string(),
            language,
            items,
        })
    }

    /// Find a symbol by name. `file_hint` (a substring of the path) breaks ties.
    pub fn find_symbol(&self, name: &str, file_hint: Option<&str>) -> Option<SymbolDefinition> {
        let candidates = self.by_name.get(name)?;
        let mut fallback: Option<NodeIndex> = None;
        for &idx in candidates {
            let node = &self.graph[idx];
            if node.kind == NodeKind::File {
                continue;
            }
            match file_hint {
                Some(hint) if node.path.contains(hint) => return Some(to_def(node)),
                _ => {
                    if fallback.is_none() {
                        fallback = Some(idx);
                    }
                }
            }
        }
        fallback.map(|i| to_def(&self.graph[i]))
    }

    /// Every call site that references `name`.
    pub fn callers(&self, name: &str) -> Vec<CallerRef> {
        let mut out = Vec::new();
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            for call in &node.calls {
                if call.name == name {
                    out.push(CallerRef {
                        path: node.path.clone(),
                        line: call.line,
                        caller_symbol: Some(node.name.clone()),
                    });
                }
            }
        }
        out
    }

    /// A tree of what `name` calls, recursively, down to `depth`.
    pub fn local_graph(&self, name: &str, depth: usize) -> Option<DependencyNode> {
        let idx = *self
            .by_name
            .get(name)?
            .iter()
            .find(|&&i| self.graph[i].kind != NodeKind::File)?;
        Some(self.build_dep(idx, depth, &mut Vec::new()))
    }

    fn build_dep(
        &self,
        idx: NodeIndex,
        depth: usize,
        stack: &mut Vec<NodeIndex>,
    ) -> DependencyNode {
        let node = &self.graph[idx];
        let mut children = Vec::new();
        if depth > 0 && !stack.contains(&idx) {
            stack.push(idx);
            let mut seen: HashSet<NodeIndex> = HashSet::new();
            for call in &node.calls {
                // Only follow calls that resolve to a known symbol. Unresolved
                // calls -- stdlib methods, macros, `.iter()`/`.clone()` and the
                // like -- are noise, not dependencies, and are omitted so the
                // tree stays small and meaningful.
                if let Some(callee) = self.unique_symbol(&call.name) {
                    if seen.insert(callee) {
                        children.push(self.build_dep(callee, depth - 1, stack));
                    }
                }
            }
            stack.pop();
        }
        DependencyNode {
            symbol: node.name.clone(),
            kind: node.kind.as_str().to_string(),
            path: node.path.clone(),
            line: node.start_line,
            resolved: true,
            children,
        }
    }

    /// The single non-file symbol with this name, if exactly one exists.
    fn unique_symbol(&self, name: &str) -> Option<NodeIndex> {
        let bucket = self.by_name.get(name)?;
        let mut symbols = bucket
            .iter()
            .copied()
            .filter(|&i| self.graph[i].kind != NodeKind::File);
        let first = symbols.next()?;
        match symbols.next() {
            Some(_) => None,
            None => Some(first),
        }
    }

    /// Best-effort: rebuild `Calls` edges between symbols resolvable by name.
    pub fn resolve_calls(&mut self) {
        let stale: Vec<_> = self
            .graph
            .edge_indices()
            .filter(|&e| self.graph[e].kind == EdgeKind::Calls)
            .collect();
        for e in stale {
            self.graph.remove_edge(e);
        }

        let nodes: Vec<NodeIndex> = self.graph.node_indices().collect();
        let mut to_add = Vec::new();
        for idx in nodes {
            if self.graph[idx].kind == NodeKind::File {
                continue;
            }
            let call_names: Vec<String> = self.graph[idx]
                .calls
                .iter()
                .map(|c| c.name.clone())
                .collect();
            for name in call_names {
                if let Some(callee) = self.unique_symbol(&name) {
                    to_add.push((idx, callee));
                }
            }
        }
        for (from, to) in to_add {
            self.graph.add_edge(
                from,
                to,
                GraphEdge {
                    kind: EdgeKind::Calls,
                    resolution: EdgeResolution::Resolved,
                },
            );
        }
    }

    pub fn stats(&self) -> GraphStats {
        let mut files = 0;
        let mut symbols = 0;
        let mut total_calls = 0;
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            if node.kind == NodeKind::File {
                files += 1;
            } else {
                symbols += 1;
                total_calls += node.calls.len();
            }
        }
        let call_edges = self
            .graph
            .edge_indices()
            .filter(|&e| self.graph[e].kind == EdgeKind::Calls)
            .count();
        GraphStats {
            files,
            symbols,
            call_edges,
            unresolved_calls: total_calls.saturating_sub(call_edges),
        }
    }

    /// Rank symbols against `query` with BM25 over name + signature + body.
    ///
    /// This is the token-efficient alternative to grep: results are ranked and
    /// each hit is a single structured row (kind, name, location, signature)
    /// rather than a dump of matching lines. `kind_filter` narrows to one
    /// symbol kind ("function", "struct", ...).
    pub fn search(&self, query: &str, limit: usize, kind_filter: Option<&str>) -> Vec<SearchHit> {
        let mut q_terms = tokenize(query);
        q_terms.sort();
        q_terms.dedup();
        if q_terms.is_empty() {
            return Vec::new();
        }

        let candidates: Vec<NodeIndex> = self
            .graph
            .node_indices()
            .filter(|&i| self.graph[i].kind != NodeKind::File)
            .filter(|&i| kind_filter.is_none_or(|k| self.graph[i].kind.as_str() == k))
            .collect();
        if candidates.is_empty() {
            return Vec::new();
        }

        let n_docs = candidates.len() as f32;
        let total_len: usize = candidates
            .iter()
            .map(|i| self.tokens.get(i).map_or(0, Vec::len))
            .sum();
        let avg_len = (total_len as f32 / n_docs).max(1.0);

        // Document frequency for each query term.
        let mut df: HashMap<&str, usize> = HashMap::new();
        for term in &q_terms {
            let count = candidates
                .iter()
                .filter(|i| {
                    self.tokens
                        .get(i)
                        .is_some_and(|toks| toks.iter().any(|t| t == term))
                })
                .count();
            df.insert(term.as_str(), count);
        }

        const K1: f32 = 1.2;
        const B: f32 = 0.75;
        let mut scored: Vec<(f32, NodeIndex)> = Vec::new();
        for &idx in &candidates {
            let Some(toks) = self.tokens.get(&idx) else {
                continue;
            };
            let dl = toks.len() as f32;
            let mut score = 0.0_f32;
            for term in &q_terms {
                let tf = toks.iter().filter(|t| *t == term).count() as f32;
                if tf == 0.0 {
                    continue;
                }
                let n_q = df.get(term.as_str()).copied().unwrap_or(0) as f32;
                let idf = (((n_docs - n_q + 0.5) / (n_q + 0.5)) + 1.0).ln();
                score += idf * (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * dl / avg_len));
            }
            if score > 0.0 {
                // De-prioritize test code so it never buries real code.
                if self.graph[idx].is_test {
                    score *= 0.25;
                }
                scored.push((score, idx));
            }
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
            .into_iter()
            .map(|(score, idx)| {
                let node = &self.graph[idx];
                SearchHit {
                    name: node.name.clone(),
                    kind: node.kind.as_str().to_string(),
                    path: node.path.clone(),
                    line: node.start_line,
                    signature: node.signature.clone(),
                    is_test: node.is_test,
                    score,
                }
            })
            .collect()
    }

    /// Capture the whole graph as a serializable snapshot for warm restart.
    pub fn snapshot(&self) -> GraphSnapshot {
        let mut files = Vec::new();
        for indices in self.by_file.values() {
            let mut file_node = None;
            let mut symbols = Vec::new();
            for &idx in indices {
                let node = self.graph[idx].clone();
                if node.kind == NodeKind::File {
                    file_node = Some(node);
                } else {
                    symbols.push(node);
                }
            }
            if let Some(file) = file_node {
                files.push(FileSnapshot { file, symbols });
            }
        }
        GraphSnapshot {
            version: SNAPSHOT_VERSION,
            files,
        }
    }

    /// Rebuild the graph from a snapshot. An incompatible version is ignored,
    /// leaving the graph untouched so the caller can fall back to a full index.
    pub fn restore(&mut self, snapshot: GraphSnapshot) {
        if snapshot.version != SNAPSHOT_VERSION {
            return;
        }
        for entry in snapshot.files {
            let path = entry.file.path.clone();
            self.replace_file(&path, entry.file, entry.symbols);
        }
        self.resolve_calls();
    }

    /// Drop every file not in `keep`, returning the removed paths. Used after a
    /// full re-index to evict files deleted while the daemon was offline.
    pub fn retain_files(&mut self, keep: &HashSet<String>) -> Vec<String> {
        let removed: Vec<String> = self
            .by_file
            .keys()
            .filter(|path| !keep.contains(*path))
            .cloned()
            .collect();
        for path in &removed {
            self.remove_file(path);
        }
        removed
    }
}

/// Bumped whenever the snapshot layout changes incompatibly.
const SNAPSHOT_VERSION: u32 = 1;

/// A serializable capture of the whole graph (see [`CodeGraph::snapshot`]).
#[derive(Serialize, Deserialize)]
pub struct GraphSnapshot {
    version: u32,
    files: Vec<FileSnapshot>,
}

impl GraphSnapshot {
    /// Number of files captured in the snapshot.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

#[derive(Serialize, Deserialize)]
struct FileSnapshot {
    file: GraphNode,
    symbols: Vec<GraphNode>,
}

fn to_def(node: &GraphNode) -> SymbolDefinition {
    SymbolDefinition {
        name: node.name.clone(),
        kind: node.kind.as_str().to_string(),
        path: node.path.clone(),
        start_line: node.start_line,
        end_line: node.end_line,
        signature: node.signature.clone(),
        source: node.source.clone(),
        docstring: node.docstring.clone(),
    }
}

/// Split text into lowercase search tokens, breaking identifiers on
/// non-alphanumeric characters and camelCase boundaries -- so a query for
/// "resolve" matches `resolve_calls` and "graph" matches `CodeGraph`.
fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut run = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            run.push(ch);
        } else if !run.is_empty() {
            split_identifier(&run, &mut out);
            run.clear();
        }
    }
    if !run.is_empty() {
        split_identifier(&run, &mut out);
    }
    out
}

fn split_identifier(run: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = run.chars().collect();
    let mut piece = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase() && !piece.is_empty() {
            out.push(piece.to_lowercase());
            piece.clear();
        }
        piece.push(c);
    }
    if !piece.is_empty() {
        out.push(piece.to_lowercase());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::CallSite;

    fn sym(name: &str, path: &str, kind: NodeKind, line: usize, sig: &str, src: &str) -> GraphNode {
        GraphNode {
            id: format!("{path}::{name}::{line}"),
            kind,
            name: name.to_string(),
            path: path.to_string(),
            language: String::new(),
            start_line: line,
            end_line: line + 5,
            signature: sig.to_string(),
            source: src.to_string(),
            docstring: None,
            calls: Vec::new(),
            is_test: false,
        }
    }

    #[test]
    fn outline_lists_symbols_sorted_by_line() {
        let mut g = CodeGraph::new();
        let later = sym(
            "beta",
            "a.rs",
            NodeKind::Function,
            20,
            "fn beta()",
            "fn beta() {}",
        );
        let earlier = sym(
            "alpha",
            "a.rs",
            NodeKind::Struct,
            5,
            "struct alpha",
            "struct alpha;",
        );
        g.replace_file(
            "a.rs",
            GraphNode::file("a.rs", "rust"),
            vec![later, earlier],
        );

        let outline = g.file_outline("a.rs").expect("outline");
        assert_eq!(outline.language, "rust");
        assert_eq!(outline.items.len(), 2);
        assert_eq!(outline.items[0].name, "alpha");
        assert_eq!(outline.items[1].name, "beta");
        assert!(g.file_outline("missing.rs").is_none());
    }

    #[test]
    fn find_symbol_honours_file_hint() {
        let mut g = CodeGraph::new();
        g.replace_file(
            "a.rs",
            GraphNode::file("a.rs", "rust"),
            vec![sym("foo", "a.rs", NodeKind::Function, 1, "fn foo", "x")],
        );
        g.replace_file(
            "b.rs",
            GraphNode::file("b.rs", "rust"),
            vec![sym("foo", "b.rs", NodeKind::Function, 1, "fn foo", "y")],
        );

        assert!(g.find_symbol("foo", None).is_some());
        assert_eq!(g.find_symbol("foo", Some("b.rs")).unwrap().path, "b.rs");
        assert!(g.find_symbol("nope", None).is_none());
    }

    #[test]
    fn callers_reports_call_sites() {
        let mut g = CodeGraph::new();
        let mut caller = sym("main", "a.rs", NodeKind::Function, 1, "fn main", "...");
        caller.calls.push(CallSite {
            name: "helper".to_string(),
            line: 3,
        });
        g.replace_file("a.rs", GraphNode::file("a.rs", "rust"), vec![caller]);

        let callers = g.callers("helper");
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].line, 3);
        assert_eq!(callers[0].caller_symbol.as_deref(), Some("main"));
        assert!(g.callers("unknown").is_empty());
    }

    #[test]
    fn remove_file_clears_all_state() {
        let mut g = CodeGraph::new();
        g.replace_file(
            "a.rs",
            GraphNode::file("a.rs", "rust"),
            vec![sym("foo", "a.rs", NodeKind::Function, 1, "fn foo", "x")],
        );
        g.remove_file("a.rs");

        assert!(g.file_outline("a.rs").is_none());
        assert!(g.find_symbol("foo", None).is_none());
        let stats = g.stats();
        assert_eq!(stats.files, 0);
        assert_eq!(stats.symbols, 0);
    }

    #[test]
    fn resolve_calls_builds_edges_and_local_graph() {
        let mut g = CodeGraph::new();
        let mut a = sym("a", "f.rs", NodeKind::Function, 1, "fn a", "");
        a.calls.push(CallSite {
            name: "b".to_string(),
            line: 2,
        });
        let b = sym("b", "f.rs", NodeKind::Function, 10, "fn b", "");
        g.replace_file("f.rs", GraphNode::file("f.rs", "rust"), vec![a, b]);
        g.resolve_calls();

        assert_eq!(g.stats().call_edges, 1);
        let dep = g.local_graph("a", 2).expect("local graph");
        assert_eq!(dep.symbol, "a");
        assert_eq!(dep.children.len(), 1);
        assert_eq!(dep.children[0].symbol, "b");
    }

    #[test]
    fn search_ranks_relevant_symbol_first() {
        let mut g = CodeGraph::new();
        g.replace_file(
            "f.rs",
            GraphNode::file("f.rs", "rust"),
            vec![
                sym(
                    "parse_config",
                    "f.rs",
                    NodeKind::Function,
                    1,
                    "fn parse_config",
                    "parse the config file",
                ),
                sym(
                    "unrelated",
                    "f.rs",
                    NodeKind::Function,
                    20,
                    "fn unrelated",
                    "does nothing",
                ),
            ],
        );
        let hits = g.search("parse config", 10, None);
        assert!(!hits.is_empty());
        assert_eq!(hits[0].name, "parse_config");
    }

    #[test]
    fn search_kind_filter_restricts_results() {
        let mut g = CodeGraph::new();
        g.replace_file(
            "f.rs",
            GraphNode::file("f.rs", "rust"),
            vec![
                sym(
                    "Thing",
                    "f.rs",
                    NodeKind::Struct,
                    1,
                    "struct Thing",
                    "thing data",
                ),
                sym(
                    "thing_fn",
                    "f.rs",
                    NodeKind::Function,
                    10,
                    "fn thing_fn",
                    "thing logic",
                ),
            ],
        );
        let hits = g.search("thing", 10, Some("struct"));
        assert!(!hits.is_empty());
        assert!(hits.iter().all(|h| h.kind == "struct"));
    }

    #[test]
    fn tokenize_splits_snake_and_camel_case() {
        let toks = tokenize("CodeGraph resolve_calls fileOutline");
        for expected in ["code", "graph", "resolve", "calls", "file", "outline"] {
            assert!(
                toks.contains(&expected.to_string()),
                "missing token {expected}"
            );
        }
    }

    #[test]
    fn snapshot_round_trips_through_json() {
        let mut g = CodeGraph::new();
        g.replace_file(
            "a.rs",
            GraphNode::file("a.rs", "rust"),
            vec![sym(
                "foo",
                "a.rs",
                NodeKind::Function,
                1,
                "fn foo",
                "fn foo() {}",
            )],
        );
        let json = serde_json::to_string(&g.snapshot()).unwrap();
        let restored: GraphSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.file_count(), 1);

        let mut g2 = CodeGraph::new();
        g2.restore(restored);
        let outline = g2.file_outline("a.rs").expect("restored outline");
        assert_eq!(outline.items.len(), 1);
        assert_eq!(outline.items[0].name, "foo");
    }

    #[test]
    fn retain_files_drops_absent_files() {
        let mut g = CodeGraph::new();
        g.replace_file("keep.rs", GraphNode::file("keep.rs", "rust"), vec![]);
        g.replace_file("drop.rs", GraphNode::file("drop.rs", "rust"), vec![]);

        let mut keep = HashSet::new();
        keep.insert("keep.rs".to_string());
        let removed = g.retain_files(&keep);

        assert_eq!(removed, vec!["drop.rs".to_string()]);
        assert!(g.file_outline("keep.rs").is_some());
        assert!(g.file_outline("drop.rs").is_none());
    }

    proptest::proptest! {
        /// Tokenizing and searching arbitrary text must never panic.
        #[test]
        fn tokenize_and_search_never_panic(text in ".{0,2000}") {
            let _ = tokenize(&text);
            let mut g = CodeGraph::new();
            g.replace_file(
                "f.rs",
                GraphNode::file("f.rs", "rust"),
                vec![sym("s", "f.rs", NodeKind::Function, 1, "sig", &text)],
            );
            let _ = g.search(&text, 10, None);
        }
    }
}
