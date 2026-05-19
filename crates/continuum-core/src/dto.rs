//! Data Transfer Objects: the only domain shapes that cross the transport
//! boundary. Tree-sitter and petgraph types never appear here.

use serde::{Deserialize, Serialize};

// ----- Code navigation -----------------------------------------------------

/// One entry in a file outline -- a definition with its body folded away.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineItem {
    pub kind: String,
    pub name: String,
    pub signature: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Structure of a single file: every top-level definition, bodies omitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOutline {
    pub path: String,
    pub language: String,
    pub items: Vec<OutlineItem>,
}

/// Full source of a resolved symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolDefinition {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docstring: Option<String>,
}

/// A single call site referencing a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallerRef {
    pub path: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_symbol: Option<String>,
}

/// A node in a local dependency tree (`get_local_graph`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyNode {
    pub symbol: String,
    pub kind: String,
    pub path: String,
    pub line: usize,
    pub resolved: bool,
    pub children: Vec<DependencyNode>,
}

/// One ranked hit from `search_code`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: usize,
    pub signature: String,
    /// True if the hit is test code — de-prioritized in ranking.
    #[serde(default)]
    pub is_test: bool,
    pub score: f32,
}

/// One line that matched a `find_text` query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
}

/// Coarse graph statistics, useful for diagnostics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphStats {
    pub files: usize,
    pub symbols: usize,
    pub call_edges: usize,
    pub unresolved_calls: usize,
}

// ----- Agent memory --------------------------------------------------------

/// A persisted architectural decision / ADR entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturalDecision {
    pub id: i64,
    pub topic: String,
    pub description: String,
    pub created_at: i64,
}

/// One logged agent intent from the action history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRecord {
    pub id: i64,
    pub agent_id: String,
    pub intent: String,
    pub files_touched: Vec<String>,
    pub seq: i64,
    pub created_at: i64,
}

/// One appended scratchpad message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchpadEntry {
    pub id: i64,
    pub agent_id: String,
    pub message: String,
    pub seq: i64,
    pub created_at: i64,
}
