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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfile_round_trips_through_json() {
        let lock = LockFile {
            pid: 4242,
            endpoint: "127.0.0.1:9000".to_string(),
            token: "secret".to_string(),
            protocol_version: PROTOCOL_VERSION,
        };
        let json = serde_json::to_string(&lock).unwrap();
        let back: LockFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.pid, 4242);
        assert_eq!(back.endpoint, "127.0.0.1:9000");
        assert_eq!(back.token, "secret");
        assert_eq!(back.protocol_version, PROTOCOL_VERSION);
    }

    #[test]
    fn handshake_round_trips_through_json() {
        let hs = Handshake {
            protocol_version: PROTOCOL_VERSION,
            token: "abc".to_string(),
        };
        let back: Handshake = serde_json::from_str(&serde_json::to_string(&hs).unwrap()).unwrap();
        assert_eq!(back.protocol_version, PROTOCOL_VERSION);
        assert_eq!(back.token, "abc");
    }

    #[test]
    fn handshake_reply_omits_absent_error() {
        let reply = HandshakeReply {
            ok: true,
            error: None,
            protocol_version: PROTOCOL_VERSION,
        };
        let json = serde_json::to_string(&reply).unwrap();
        assert!(!json.contains("error"));
    }
}
