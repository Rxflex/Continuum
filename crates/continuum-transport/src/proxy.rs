//! Adapter-side proxy: perform the Continuum handshake, then pipe the agent's
//! MCP stdio traffic straight through to the daemon's TCP socket.

use continuum_core::{Handshake, HandshakeReply, PROTOCOL_VERSION};
use tokio::io::BufReader;
use tokio::net::TcpStream;

use crate::framing::{read_line, write_line};

/// Outcome of attempting to attach to a daemon endpoint.
pub enum AttachResult {
    Connected(TcpStream),
    Refused(String),
}

/// Connect to a daemon and run the Continuum handshake. On success the returned
/// `TcpStream` is positioned ready for MCP traffic.
pub async fn attach(endpoint: &str, token: &str) -> std::io::Result<AttachResult> {
    let stream = TcpStream::connect(endpoint).await?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let hs = Handshake {
        protocol_version: PROTOCOL_VERSION,
        token: token.to_string(),
    };
    write_line(&mut write_half, &serde_json::to_string(&hs)?).await?;

    let reply_line = match read_line(&mut reader).await? {
        Some(line) => line,
        None => return Ok(AttachResult::Refused("daemon closed connection".into())),
    };
    let reply: HandshakeReply = serde_json::from_str(reply_line.trim())
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    let stream = reader
        .into_inner()
        .reunite(write_half)
        .map_err(|e| std::io::Error::other(e.to_string()))?;

    if reply.ok {
        Ok(AttachResult::Connected(stream))
    } else {
        Ok(AttachResult::Refused(
            reply.error.unwrap_or_else(|| "handshake refused".into()),
        ))
    }
}

/// Pump bytes both ways between the process's stdio and the daemon socket
/// until either side closes.
pub async fn run_proxy(stream: TcpStream) {
    let (mut sock_read, mut sock_write) = stream.into_split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    let up = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut stdin, &mut sock_write).await;
    });
    let down = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut sock_read, &mut stdout).await;
    });

    tokio::select! {
        _ = up => {}
        _ = down => {}
    }
}
