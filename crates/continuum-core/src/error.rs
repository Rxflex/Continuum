//! The single error type carried across Continuum's modules.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContinuumError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("symbol not found: {0}")]
    SymbolNotFound(String),

    #[error("file not indexed: {0}")]
    FileNotFound(String),

    #[error("protocol version mismatch: daemon={daemon}, client={client}")]
    ProtocolMismatch { daemon: u32, client: u32 },

    #[error("authentication failed")]
    AuthFailed,

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, ContinuumError>;
