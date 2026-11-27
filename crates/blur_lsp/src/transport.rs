//! Content-Length framed JSON-RPC 2.0 transport for LSP.
//!
//! Generic over [`AsyncRead`] and [`AsyncWrite`] so both real child process
//! stdio pipes and in-memory duplex streams (for tests) use the exact same
//! framing implementation.

use anyhow::{anyhow, bail, Context, Result};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Maximum allowed single LSP message body size (64 MiB) to guard against
/// corrupt or runaway `Content-Length` headers.
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// Encode a JSON payload with an LSP `Content-Length` header.
pub fn encode_message(json: &str) -> Vec<u8> {
    let bytes = json.as_bytes();
    let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
    let mut out = Vec::with_capacity(header.len() + bytes.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(bytes);
    out
}

/// Write a single Content-Length framed JSON message and flush the writer.
pub async fn write_message<W: AsyncWrite + Unpin>(writer: &mut W, json: &str) -> Result<()> {
    let frame = encode_message(json);
    writer
        .write_all(&frame)
        .await
        .context("failed to write LSP frame")?;
    writer.flush().await.context("failed to flush LSP writer")?;
    Ok(())
}

/// Read a single Content-Length framed JSON message from a buffered reader.
///
/// Returns `Ok(None)` on clean EOF before any header bytes of the next frame
/// have been read. Handles CRLF and LF line endings, ignores unknown headers
/// (such as `Content-Type`), and handles arbitrary chunk boundaries.
pub async fn read_message<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<String>> {
    let mut content_length: Option<usize> = None;
    let mut saw_any_header = false;
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .context("failed to read LSP header line")?;
        if n == 0 {
            if !saw_any_header {
                return Ok(None);
            }
            bail!("unexpected EOF while reading LSP headers");
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            if saw_any_header {
                break;
            }
            // Tolerate leading blank lines between frames if any server emits them.
            continue;
        }

        saw_any_header = true;
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                let len: usize = value
                    .trim()
                    .parse()
                    .map_err(|e| anyhow!("invalid Content-Length header {:?}: {}", value, e))?;
                if len > MAX_MESSAGE_BYTES {
                    bail!(
                        "LSP Content-Length {} exceeds limit {}",
                        len,
                        MAX_MESSAGE_BYTES
                    );
                }
                content_length = Some(len);
            }
        }
    }

    let len = content_length.ok_or_else(|| anyhow!("missing Content-Length header in LSP frame"))?;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .await
        .context("unexpected EOF while reading LSP message body")?;

    let text = String::from_utf8(buf).context("LSP message body is not valid UTF-8")?;
    Ok(Some(text))
}
