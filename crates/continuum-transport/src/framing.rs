//! Newline-delimited JSON framing, shared by the stdio and TCP transports.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Read one newline-delimited message. Returns `Ok(None)` on a clean EOF.
pub async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> std::io::Result<Option<String>> {
    let mut buf = String::new();
    let n = reader.read_line(&mut buf).await?;
    if n == 0 {
        Ok(None)
    } else {
        Ok(Some(buf))
    }
}

/// Write one message with exactly one trailing newline, then flush.
pub async fn write_line<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &str,
) -> std::io::Result<()> {
    let trimmed = msg.trim_end_matches('\n');
    writer.write_all(trimmed.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}
