//! Node and edge types stored in the [`crate::CodeGraph`].

/// A symbol's stable identity. Convention: `path::name::start_line`.
pub type SymbolId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Function,
    Method,
    Variable,
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeKind::File => "file",
            NodeKind::Class => "class",
            NodeKind::Struct => "struct",
            NodeKind::Enum => "enum",
            NodeKind::Trait => "trait",
            NodeKind::Interface => "interface",
            NodeKind::Function => "function",
            NodeKind::Method => "method",
            NodeKind::Variable => "variable",
        }
    }
}

/// A call reference captured at parse time. Resolution to a definition is
/// attempted later, best-effort, by name.
#[derive(Debug, Clone)]
pub struct CallSite {
    pub name: String,
    pub line: usize,
}

/// A graph node: either a file or a symbol within one.
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: SymbolId,
    pub kind: NodeKind,
    pub name: String,
    pub path: String,
    /// Language slug; set on file nodes, empty on symbol nodes.
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
    pub source: String,
    pub docstring: Option<String>,
    pub calls: Vec<CallSite>,
}

impl GraphNode {
    /// Build a file node. Its `id` is the file path itself.
    pub fn file(path: &str, language: &str) -> Self {
        Self {
            id: path.to_string(),
            kind: NodeKind::File,
            name: path.to_string(),
            path: path.to_string(),
            language: language.to_string(),
            start_line: 0,
            end_line: 0,
            signature: String::new(),
            source: String::new(),
            docstring: None,
            calls: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Contains,
    Calls,
    Imports,
    Inherits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeResolution {
    Resolved,
    Unresolved,
}

#[derive(Debug, Clone, Copy)]
pub struct GraphEdge {
    pub kind: EdgeKind,
    pub resolution: EdgeResolution,
}
