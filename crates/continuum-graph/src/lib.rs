//! In-memory code knowledge graph.
//!
//! A single `StableGraph` plus name/file indices form one source of truth.
//! The daemon guards the whole `CodeGraph` behind an `RwLock`: MCP reads take
//! a read lock, the indexer takes a write lock once per debounced batch.

pub mod graph;
pub mod model;
pub mod resolver;

pub use graph::CodeGraph;
pub use model::{
    CallSite, EdgeKind, EdgeResolution, GraphEdge, GraphNode, NodeKind, SymbolId,
};
