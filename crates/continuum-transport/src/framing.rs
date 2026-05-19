//! Newline-delimited JSON framing, shared by the stdio and TCP transports.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Read one newline-delimited message. Returns `Ok(None)` on a clean EOF.
pub async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut buf = String::new();
    let n = reader.read_line(&mut buf).await?;
    if n == 0 {
        Ok(None)
    } else {
        Ok(Some(buf))
    }
}

/// Write one message with exactly one trailing newline, then flush.
pub async fn write_line<W: AsyncWrite + Unpin>(writer: &mut W, msg: &str) -> std::io::Result<()> {
    let trimmed = msg.trim_end_matches('\n');
    writer.write_all(trimmed.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn write_line_appends_single_newline() {
        let mut buf: Vec<u8> = Vec::new();
        write_line(&mut buf, "hello").await.unwrap();
        assert_eq!(buf, b"hello\n");

        let mut buf2: Vec<u8> = Vec::new();
        write_line(&mut buf2, "trailing\n").await.unwrap();
        assert_eq!(buf2, b"trailing\n");
    }

    #[tokio::test]
    async fn read_line_yields_lines_then_none_at_eof() {
        let data: &[u8] = b"one\ntwo\n";
        let mut reader = tokio::io::BufReader::new(data);
        assert_eq!(
            read_line(&mut reader).await.unwrap().as_deref(),
            Some("one\n")
        );
        assert_eq!(
            read_line(&mut reader).await.unwrap().as_deref(),
            Some("two\n")
        );
        assert_eq!(read_line(&mut reader).await.unwrap(), None);
    }
}
