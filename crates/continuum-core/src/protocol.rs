//! Wire-protocol constants and the daemon<->adapter handshake types.

use serde::{Deserialize, Serialize};

/// Continuum daemon<->adapter IPC protocol version. Bump on any breaking change
/// to the handshake or framing so a stale adapter is forced to respawn the daemon.
pub const PROTOCOL_VERSION: u32 = 1;

/// MCP protocol version advertised to connected agents.
pub const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

/// Contents of `<workspace>/.continuum/daemon.lock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    pub pid: u32,
    pub endpoint: String,
    pub token: String,
    pub protocol_version: u32,
}

/// First line an adapter sends on a fresh IPC connection, before any MCP traffic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Handshake {
    pub protocol_version: u32,
    pub token: String,
}

/// The daemon's reply to a [`Handshake`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeReply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub protocol_version: u32,
}
