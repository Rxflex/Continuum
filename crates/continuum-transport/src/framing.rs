//! Newline-delimited JSON framing, shared by the stdio and TCP transports.

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Hard cap on a single framed message. A peer that streams bytes without a
/// newline cannot make the daemon allocate without bound — the read fails
/// instead. Generous: the largest real message is bounded by the index's
/// per-file size limit.
const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;

/// Read one newline-delimited message. Returns `Ok(None)` on a clean EOF, and
/// an error if the message exceeds [`MAX_LINE_BYTES`].
pub async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> std::io::Result<Option<String>> {
    read_line_capped(reader, MAX_LINE_BYTES).await
}

/// [`read_line`] with an explicit cap, so the bound is testable.
async fn read_line_capped<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Option<String>> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let (done, consumed) = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                if buf.is_empty() {
                    return Ok(None);
                }
                (true, 0)
            } else if let Some(pos) = available.iter().position(|&b| b == b'\n') {
                buf.extend_from_slice(&available[..=pos]);
                (true, pos + 1)
            } else {
                buf.extend_from_slice(available);
                (false, available.len())
            }
        };
        reader.consume(consumed);
        if done {
            break;
        }
        if buf.len() > max {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "framed message exceeds the maximum line length",
            ));
        }
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
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

    #[tokio::test]
    async fn read_line_rejects_input_past_the_cap() {
        let data = vec![b'x'; 4096]; // 4 KiB, no newline
        let mut reader = tokio::io::BufReader::new(&data[..]);
        assert!(read_line_capped(&mut reader, 256).await.is_err());
    }

    #[tokio::test]
    async fn read_line_capped_accepts_lines_within_the_cap() {
        let data: &[u8] = b"within\n";
        let mut reader = tokio::io::BufReader::new(data);
        assert_eq!(
            read_line_capped(&mut reader, 1024)
                .await
                .unwrap()
                .as_deref(),
            Some("within\n")
        );
    }
}
