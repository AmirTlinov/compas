use rmcp::{
    RoleServer,
    service::{RxJsonRpcMessage, TxJsonRpcMessage},
    transport::Transport,
};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
    time::{Duration, sleep},
};

const MODE_UNKNOWN: u8 = 0;
const MODE_NDJSON: u8 = 1;
const MODE_CONTENT_LENGTH: u8 = 2;

pub struct HybridStdioTransport {
    reader: tokio::io::Stdin,
    writer: Arc<Mutex<tokio::io::Stdout>>,
    mode: Arc<AtomicU8>,
    buf: Vec<u8>,
}

impl HybridStdioTransport {
    pub fn new() -> Self {
        Self {
            reader: tokio::io::stdin(),
            writer: Arc::new(Mutex::new(tokio::io::stdout())),
            mode: Arc::new(AtomicU8::new(MODE_UNKNOWN)),
            buf: Vec::with_capacity(16 * 1024),
        }
    }

    fn detect_mode(buf: &[u8]) -> Option<u8> {
        let trimmed = trim_leading_ascii_ws(buf);
        if trimmed.is_empty() {
            return None;
        }
        if starts_with_ascii_case(trimmed, b"content-length:") {
            return Some(MODE_CONTENT_LENGTH);
        }
        if trimmed.first().copied() == Some(b'{') || trimmed.first().copied() == Some(b'[') {
            return Some(MODE_NDJSON);
        }
        None
    }

    fn parse_ndjson_message(&mut self) -> io::Result<Option<RxJsonRpcMessage<RoleServer>>> {
        let Some(newline_idx) = self.buf.iter().position(|b| *b == b'\n') else {
            return Ok(None);
        };
        let mut line: Vec<u8> = self.buf.drain(..=newline_idx).collect();
        if line.last().copied() == Some(b'\n') {
            line.pop();
        }
        if line.last().copied() == Some(b'\r') {
            line.pop();
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            return Ok(None);
        }
        let msg = serde_json::from_slice::<RxJsonRpcMessage<RoleServer>>(&line).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid NDJSON MCP message: {e}"),
            )
        })?;
        Ok(Some(msg))
    }

    fn parse_content_length_message(&mut self) -> io::Result<Option<RxJsonRpcMessage<RoleServer>>> {
        let (header_end, sep_len) = match find_header_end(&self.buf) {
            Some(v) => v,
            None => return Ok(None),
        };
        let headers = std::str::from_utf8(&self.buf[..header_end]).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid content-length header utf8: {e}"),
            )
        })?;
        let mut content_length: Option<usize> = None;
        for raw in headers.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((k, v)) = line.split_once(':')
                && k.trim().eq_ignore_ascii_case("content-length")
            {
                let n: usize = v.trim().parse().map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid Content-Length value: {e}"),
                    )
                })?;
                content_length = Some(n);
            }
        }
        let content_length = content_length.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "missing Content-Length header in MCP frame",
            )
        })?;

        let body_start = header_end + sep_len;
        let body_end = body_start + content_length;
        if self.buf.len() < body_end {
            return Ok(None);
        }
        let body = self.buf[body_start..body_end].to_vec();
        self.buf.drain(..body_end);
        let msg = serde_json::from_slice::<RxJsonRpcMessage<RoleServer>>(&body).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid Content-Length MCP body JSON: {e}"),
            )
        })?;
        Ok(Some(msg))
    }
}

impl Default for HybridStdioTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport<RoleServer> for HybridStdioTransport {
    type Error = io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let writer = self.writer.clone();
        let mode = self.mode.clone();
        async move {
            let mut out = writer.lock().await;
            let payload = serde_json::to_vec(&item).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("failed to encode MCP response JSON: {e}"),
                )
            })?;
            if mode.load(Ordering::Relaxed) == MODE_CONTENT_LENGTH {
                let header = format!("Content-Length: {}\r\n\r\n", payload.len());
                out.write_all(header.as_bytes()).await?;
                out.write_all(&payload).await?;
            } else {
                out.write_all(&payload).await?;
                out.write_all(b"\n").await?;
            }
            out.flush().await
        }
    }

    async fn receive(&mut self) -> Option<RxJsonRpcMessage<RoleServer>> {
        loop {
            let mode = self.mode.load(Ordering::Relaxed);
            let parse_result = match mode {
                MODE_CONTENT_LENGTH => self.parse_content_length_message(),
                MODE_NDJSON => self.parse_ndjson_message(),
                _ => {
                    if let Some(detected) = Self::detect_mode(&self.buf) {
                        self.mode.store(detected, Ordering::Relaxed);
                        continue;
                    }
                    Ok(None)
                }
            };

            match parse_result {
                Ok(Some(msg)) => return Some(msg),
                Ok(None) => {}
                Err(err) => {
                    eprintln!("compas mcp transport decode error: {err}");
                    self.buf.clear();
                }
            }

            let mut tmp = [0u8; 8192];
            let n = match self.reader.read(&mut tmp).await {
                Ok(n) => n,
                Err(err) => {
                    if is_retryable_read_error(&err) {
                        sleep(Duration::from_millis(2)).await;
                        continue;
                    }
                    eprintln!("compas mcp transport read error: {err}");
                    return None;
                }
            };
            if n == 0 {
                // EOF: try one last parse pass for trailing frame.
                let tail = match self.mode.load(Ordering::Relaxed) {
                    MODE_CONTENT_LENGTH => self.parse_content_length_message(),
                    MODE_NDJSON | MODE_UNKNOWN => self.parse_ndjson_message(),
                    _ => Ok(None),
                };
                return tail.unwrap_or(None);
            }
            self.buf.extend_from_slice(&tmp[..n]);
        }
    }

    async fn close(&mut self) -> Result<(), Self::Error> {
        let mut out = self.writer.lock().await;
        out.flush().await
    }
}

fn starts_with_ascii_case(haystack: &[u8], needle_lower: &[u8]) -> bool {
    if haystack.len() < needle_lower.len() {
        return false;
    }
    haystack
        .iter()
        .zip(needle_lower.iter())
        .take(needle_lower.len())
        .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

fn trim_leading_ascii_ws(mut s: &[u8]) -> &[u8] {
    while let Some((head, rest)) = s.split_first() {
        if head.is_ascii_whitespace() {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn find_header_end(buf: &[u8]) -> Option<(usize, usize)> {
    if let Some(i) = find_subslice(buf, b"\r\n\r\n") {
        return Some((i, 4));
    }
    if let Some(i) = find_subslice(buf, b"\n\n") {
        return Some((i, 2));
    }
    None
}

fn find_subslice(buf: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || buf.len() < needle.len() {
        return None;
    }
    buf.windows(needle.len()).position(|w| w == needle)
}

fn is_retryable_read_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    ) || matches!(err.raw_os_error(), Some(11) | Some(4))
}

#[cfg(test)]
mod tests {
    use super::{find_header_end, is_retryable_read_error, trim_leading_ascii_ws};
    use std::io;

    #[test]
    fn header_end_detects_both_styles() {
        assert_eq!(
            find_header_end(b"Content-Length: 5\r\n\r\n{\"a\":1}"),
            Some((17, 4))
        );
        assert_eq!(
            find_header_end(b"Content-Length: 5\n\n{\"a\":1}"),
            Some((17, 2))
        );
    }

    #[test]
    fn trim_leading_ws_works() {
        assert_eq!(trim_leading_ascii_ws(b"  \n\tabc"), b"abc");
        assert_eq!(trim_leading_ascii_ws(b"abc"), b"abc");
    }

    #[test]
    fn retryable_read_errors_cover_wouldblock_and_interrupted() {
        assert!(is_retryable_read_error(&io::Error::from(
            io::ErrorKind::WouldBlock
        )));
        assert!(is_retryable_read_error(&io::Error::from(
            io::ErrorKind::Interrupted
        )));
        assert!(!is_retryable_read_error(&io::Error::from(
            io::ErrorKind::BrokenPipe
        )));
    }
}
