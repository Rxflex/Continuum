//! Heuristic cross-file reference resolver.
//!
//! Best-effort by design: a call name is resolved only when exactly one symbol
//! of that name exists in the graph. Ambiguous names (overloads, same name in
//! many files) and names with no definition are left unresolved. Re-exports,
//! aliases, and dynamic dispatch are out of scope -- see DESIGN.md.

use crate::graph::CodeGraph;

/// Re-run resolution over the whole graph. Cheap enough to call after every
/// indexing batch; the daemon does so under the graph write lock.
pub fn resolve(graph: &mut CodeGraph) {
    graph.resolve_calls();
}
