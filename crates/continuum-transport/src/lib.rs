//! Continuum transport layer.
//!
//! Owns the JSON-RPC + MCP wire types, newline framing, and the adapter's
//! stdio<->TCP proxy. It knows nothing of the graph or memory domains: only
//! JSON and the DTOs from `continuum-core` cross this boundary.

pub mod framing;
pub mod jsonrpc;
pub mod mcp;
pub mod proxy;
