use crate::api::Receipt;
use crate::config::ProjectTool;
use crate::hash::sha256_hex;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Instant;
use tokio::io::AsyncReadExt;

#[derive(Debug, Clone)]
pub struct RunnerLimits {
    pub timeout_ms: u64,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

#[derive(Debug, Default)]
struct TailBuffer {
    max: usize,
    buf: Vec<u8>,
}

impl TailBuffer {
    fn new(max: usize) -> Self {
        Self { max, buf: vec![] }
    }

    fn push(&mut self, chunk: &[u8]) {
        if self.max == 0 {
            return;
        }
        if chunk.len() >= self.max {
            self.buf.clear();
            self.buf
                .extend_from_slice(&chunk[chunk.len().saturating_sub(self.max)..]);
            return;
        }
        self.buf.extend_from_slice(chunk);
        if self.buf.len() > self.max {
            let excess = self.buf.len() - self.max;
            self.buf.drain(0..excess);
        }
    }

    fn into_string(self) -> String {
        String::from_utf8_lossy(&self.buf).into_owned()
    }
}

#[derive(Debug)]
struct StreamCapture {
    tail: String,
    total_bytes: usize,
    sha256: String,
}

async fn read_stream<R: tokio::io::AsyncRead + Unpin>(
    mut r: R,
    max_tail: usize,
) -> std::io::Result<StreamCapture> {
    let mut tail = TailBuffer::new(max_tail);
    let mut hasher = Sha256::new();
    let mut total_bytes = 0usize;
    let mut buf = vec![0u8; 8 * 1024];
    loop {
        let n = r.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let chunk = &buf[..n];
        tail.push(chunk);
        hasher.update(chunk);
        total_bytes += n;
    }

    Ok(StreamCapture {
        tail: tail.into_string(),
        total_bytes,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

async fn finalize_capture_task(
    task: &mut tokio::task::JoinHandle<std::io::Result<StreamCapture>>,
    stream_name: &str,
    timed_out: bool,
) -> std::io::Result<StreamCapture> {
    let joined = if timed_out {
        match tokio::time::timeout(std::time::Duration::from_millis(250), &mut *task).await {
            Ok(joined) => Some(joined),
            Err(_) => {
                task.abort();
                None
            }
        }
    } else {
        Some(task.await)
    };

    match joined {
        Some(Ok(Ok(capture))) => Ok(capture),
        Some(Ok(Err(err))) => Err(err),
        Some(Err(_join_err)) => Ok(StreamCapture {
            tail: format!("<{stream_name} join error>"),
            total_bytes: 0,
            sha256: sha256_hex(&[]),
        }),
        None => Ok(StreamCapture {
            tail: format!("<{stream_name} capture aborted after timeout>"),
            total_bytes: 0,
            sha256: sha256_hex(&[]),
        }),
    }
}

pub async fn run_project_tool(
    repo_root: &Path,
    tool: &ProjectTool,
    extra_args: &[String],
    dry_run: bool,
) -> Result<Receipt, std::io::Error> {
    run_project_tool_with_timeout_override(repo_root, tool, extra_args, dry_run, None).await
}

pub async fn run_project_tool_with_timeout_override(
    repo_root: &Path,
    tool: &ProjectTool,
    extra_args: &[String],
    dry_run: bool,
    timeout_override_ms: Option<u64>,
) -> Result<Receipt, std::io::Error> {
    let base_timeout_ms = tool.timeout_ms.unwrap_or(600_000);
    let timeout_ms = timeout_override_ms
        .map(|v| v.max(1))
        .map(|v| v.min(base_timeout_ms))
        .unwrap_or(base_timeout_ms);
    let limits = RunnerLimits {
        timeout_ms,
        max_stdout_bytes: tool.max_stdout_bytes.unwrap_or(20_000),
        max_stderr_bytes: tool.max_stderr_bytes.unwrap_or(20_000),
    };

    let mut argv: Vec<String> = vec![];
    argv.extend(tool.args.clone());
    argv.extend(extra_args.iter().cloned());

    if dry_run {
        let stdout = b"[dry_run]";
        let stderr = b"";
        return Ok(Receipt {
            tool_id: tool.id.clone(),
            success: true,
            exit_code: Some(0),
            timed_out: false,
            duration_ms: 0,
            command: tool.command.clone(),
            args: argv,
            stdout_tail: "[dry_run]".to_string(),
            stderr_tail: "".to_string(),
            stdout_bytes: stdout.len(),
            stderr_bytes: stderr.len(),
            stdout_sha256: sha256_hex(stdout),
            stderr_sha256: sha256_hex(stderr),
        });
    }

    let start = Instant::now();
    let mut cmd = tokio::process::Command::new(&tool.command);
    cmd.args(&argv);
    cmd.current_dir(match &tool.cwd {
        Some(cwd) => repo_root.join(cwd),
        None => repo_root.to_path_buf(),
    });
    if !tool.env.is_empty() {
        cmd.envs(tool.env.clone());
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("stdout is not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("stderr is not captured"))?;

    let mut stdout_task = tokio::spawn(read_stream(stdout, limits.max_stdout_bytes));
    let mut stderr_task = tokio::spawn(read_stream(stderr, limits.max_stderr_bytes));

    let timeout = std::time::Duration::from_millis(limits.timeout_ms);
    let mut timed_out = false;
    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(res) => res?,
        Err(_) => {
            timed_out = true;
            let _ = child.kill().await;
            child.wait().await?
        }
    };

    let stdout = finalize_capture_task(&mut stdout_task, "stdout", timed_out).await?;
    let stderr = finalize_capture_task(&mut stderr_task, "stderr", timed_out).await?;

    Ok(Receipt {
        tool_id: tool.id.clone(),
        success: status.success() && !timed_out,
        exit_code: status.code(),
        timed_out,
        duration_ms: start.elapsed().as_millis() as u64,
        command: tool.command.clone(),
        args: argv,
        stdout_tail: stdout.tail,
        stderr_tail: stderr.tail,
        stdout_bytes: stdout.total_bytes,
        stderr_bytes: stderr.total_bytes,
        stdout_sha256: stdout.sha256,
        stderr_sha256: stderr.sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn dry_run_receipt_contains_hash_and_sizes() {
        let tool = ProjectTool {
            id: "dry".to_string(),
            description: "Dry run fixture tool".to_string(),
            command: "echo".to_string(),
            args: vec![],
            cwd: None,
            timeout_ms: None,
            max_stdout_bytes: None,
            max_stderr_bytes: None,
            receipt_contract: None,
            env: BTreeMap::new(),
        };

        let receipt = run_project_tool(Path::new("."), &tool, &[], true)
            .await
            .expect("dry-run receipt");
        assert_eq!(receipt.stdout_tail, "[dry_run]");
        assert_eq!(receipt.stderr_tail, "");
        assert_eq!(receipt.stdout_bytes, b"[dry_run]".len());
        assert_eq!(receipt.stderr_bytes, 0);
        assert_eq!(receipt.stdout_sha256, sha256_hex(b"[dry_run]"));
        assert_eq!(receipt.stderr_sha256, sha256_hex(b""));
    }

    #[tokio::test]
    async fn read_stream_reports_total_bytes_tail_and_hash() {
        let payload = b"abcdef".to_vec();
        let (mut tx, rx) = tokio::io::duplex(64);
        tokio::spawn(async move {
            tx.write_all(&payload).await.expect("write payload");
        });

        let capture = read_stream(rx, 3).await.expect("capture stream");
        assert_eq!(capture.total_bytes, 6);
        assert_eq!(capture.tail, "def");
        assert_eq!(capture.sha256, sha256_hex(b"abcdef"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_returns_even_if_descendants_keep_pipes_open() {
        let tool = ProjectTool {
            id: "timeout-descendants".to_string(),
            description: "Timeout harness with background child".to_string(),
            command: "bash".to_string(),
            args: vec![
                "-lc".to_string(),
                "(sleep 5) & while true; do sleep 1; done".to_string(),
            ],
            cwd: None,
            timeout_ms: Some(100),
            max_stdout_bytes: None,
            max_stderr_bytes: None,
            receipt_contract: None,
            env: BTreeMap::new(),
        };

        let receipt = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            run_project_tool(Path::new("."), &tool, &[], false),
        )
        .await
        .expect("runner must return promptly on timeout")
        .expect("receipt");

        assert!(receipt.timed_out);
    }
}
