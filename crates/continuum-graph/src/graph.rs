//! The [`CodeGraph`]: nodes + edges + lookup indices, one source of truth.

use std::collections::HashMap;

use continuum_core::dto::{
    CallerRef, DependencyNode, FileOutline, GraphStats, OutlineItem, SearchHit, SymbolDefinition,
};
use petgraph::stable_graph::{NodeIndex, StableGraph};

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

    /// Outlines of every indexed file. Used to back-fill the semantic index
    /// once the embedding model finishes loading.
    pub fn all_outlines(&self) -> Vec<FileOutline> {
        self.by_file
            .keys()
            .filter_map(|path| self.file_outline(path))
            .collect()
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
            for call in &node.calls {
                match self.unique_symbol(&call.name) {
                    Some(callee) => children.push(self.build_dep(callee, depth - 1, stack)),
                    None => children.push(DependencyNode {
                        symbol: call.name.clone(),
                        kind: "unresolved".to_string(),
                        path: String::new(),
                        line: call.line,
                        resolved: false,
                        children: Vec::new(),
                    }),
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
                    score,
                }
            })
            .collect()
    }
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
}
