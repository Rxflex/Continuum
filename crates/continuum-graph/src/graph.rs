//! The [`CodeGraph`]: nodes + edges + lookup indices, one source of truth.

use std::collections::HashMap;

use continuum_core::dto::{
    CallerRef, DependencyNode, FileOutline, GraphStats, OutlineItem, SymbolDefinition,
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
                GraphEdge { kind: EdgeKind::Contains, resolution: EdgeResolution::Resolved },
            );
        }
    }

    /// Drop all nodes (and their edges) belonging to `path`.
    pub fn remove_file(&mut self, path: &str) {
        let Some(indices) = self.by_file.remove(path) else {
            return;
        };
        for idx in indices {
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
        let idx = self.graph.add_node(node);
        self.by_id.insert(id, idx);
        self.by_name.entry(name).or_default().push(idx);
        self.by_file.entry(path).or_default().push(idx);
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
        Some(FileOutline { path: path.to_string(), language, items })
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
            let call_names: Vec<String> =
                self.graph[idx].calls.iter().map(|c| c.name.clone()).collect();
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
                GraphEdge { kind: EdgeKind::Calls, resolution: EdgeResolution::Resolved },
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
