//! Runs and formats submissions by spawning a language runtime on this host, never leetcode.com. One short-lived child per
//! request: a JSON payload on stdin, one JSON document back on stdout. The harness scripts own the run, format and compare
//! logic; this module owns the process.

use async_trait::async_trait;
use leetcode_backend_core::application::ports::{CodeRunner, PortResult};
use leetcode_backend_core::application::requests::{FormatRequest, RunRequest};
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::time::timeout;

const PYTHON_HARNESS: &str = include_str!("../../harness/python.py");
const JS_HARNESS: &str = include_str!("../../harness/node.js");
const PER_CASE_TIMEOUT_MS: u64 = 5000;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(30);
const STDOUT_CAP: usize = 16 * 1024 * 1024;
const STDERR_CAP: usize = 1024 * 1024;

pub struct ProcessRunner;

struct Harness {
    cmd: &'static str,
    flag: &'static str,
    script: &'static str,
}

fn harness_for(lang: &str) -> Option<Harness> {
    match lang {
        "python3" | "python" => Some(Harness { cmd: "python3", flag: "-c", script: PYTHON_HARNESS }),
        "javascript" | "typescript" => Some(Harness { cmd: "node", flag: "-e", script: JS_HARNESS }),
        _ => None,
    }
}

#[derive(Default)]
struct Output {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    overflowed: bool,
}

/// At most `cap` bytes of the pipe and whether there was more.
async fn read_capped(pipe: impl AsyncRead + Unpin, cap: usize) -> (Vec<u8>, bool) {
    let mut bytes = Vec::new();
    let _ = pipe.take(cap as u64 + 1).read_to_end(&mut bytes).await;
    let overflowed = bytes.len() > cap;
    bytes.truncate(cap);
    (bytes, overflowed)
}

/// The first `cap` bytes of the pipe; the rest is read and thrown away so the child never blocks on a full pipe.
async fn read_head(mut pipe: impl AsyncRead + Unpin, cap: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = (&mut pipe).take(cap as u64).read_to_end(&mut bytes).await;
    let _ = tokio::io::copy(&mut pipe, &mut tokio::io::sink()).await;
    bytes
}

async fn run_process(harness: &Harness, input: String) -> Result<Output, String> {
    let mut command = Command::new(harness.cmd);
    command.args([harness.flag, harness.script]).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    for name in ["PATH", "HOME", "LANG", "LC_ALL"] {
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
    let mut child = command.spawn().map_err(|error| format!("spawn {} failed: {error}", harness.cmd))?;
    let started = Instant::now();

    let mut stdin = child.stdin.take();
    let writer = tokio::spawn(async move {
        if let Some(pipe) = stdin.as_mut() {
            let _ = pipe.write_all(input.as_bytes()).await;
        }
    });
    let stderr = child.stderr.take().map(|pipe| tokio::spawn(read_head(pipe, STDERR_CAP)));
    let stdout = child.stdout.take().ok_or("the child has no stdout")?;

    let mut out = Output::default();
    match timeout(PROCESS_TIMEOUT, read_capped(stdout, STDOUT_CAP)).await {
        Err(_) => out.timed_out = true,
        Ok((bytes, overflowed)) => {
            out.stdout = bytes;
            out.overflowed = overflowed;
            // The pipe closed; a child that closed it but keeps running still answers to the process limit.
            if !overflowed && timeout(PROCESS_TIMEOUT.saturating_sub(started.elapsed()), child.wait()).await.is_err() {
                out.timed_out = true;
            }
        }
    }
    if out.timed_out || out.overflowed {
        let _ = child.kill().await;
    }
    writer.abort();
    if let Some(task) = stderr {
        out.stderr = timeout(Duration::from_secs(1), task).await.ok().and_then(Result::ok).unwrap_or_default();
    }
    Ok(out)
}

fn parse_output(out: &Output, none_message: &str) -> Result<Value, String> {
    let text = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str::<Value>(text.trim()).map_err(|_| {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if stderr.is_empty() {
            none_message.to_string()
        } else {
            stderr
        }
    })
}

#[async_trait]
impl CodeRunner for ProcessRunner {
    async fn run(&self, request: &RunRequest) -> PortResult<Value> {
        let lang = request.lang.as_str();
        let Some(harness) = harness_for(lang) else {
            return Ok(json!({ "ok": false, "results": [], "error": format!("Unsupported language: {lang}") }));
        };
        let payload = json!({
            "mode": "run",
            "code": request.code,
            "methodName": request.method_name.as_str(),
            "testCases": request.test_cases,
            "timeoutMs": PER_CASE_TIMEOUT_MS,
        });
        let out = run_process(&harness, payload.to_string()).await?;
        if out.timed_out {
            return Ok(json!({ "ok": false, "results": [], "error": "Timeout: execution exceeded process limit" }));
        }
        if out.overflowed {
            return Ok(json!({ "ok": false, "results": [], "error": "Runner output exceeded the size limit" }));
        }
        Ok(parse_output(&out, "Runner produced no parseable output").unwrap_or_else(|error| json!({ "ok": false, "results": [], "error": error })))
    }

    async fn format(&self, request: &FormatRequest) -> PortResult<Value> {
        let lang = request.lang.as_str();
        let Some(harness) = harness_for(lang) else {
            return Ok(json!({ "ok": false, "formatted": "", "error": format!("Unsupported language: {lang}") }));
        };
        let payload = json!({ "mode": "format", "code": request.code });
        let out = run_process(&harness, payload.to_string()).await?;
        if out.timed_out {
            return Ok(json!({ "ok": false, "formatted": "", "error": "Timeout: formatting exceeded process limit" }));
        }
        if out.overflowed {
            return Ok(json!({ "ok": false, "formatted": "", "error": "Formatter output exceeded the size limit" }));
        }
        Ok(parse_output(&out, "Formatter produced no parseable output").unwrap_or_else(|error| json!({ "ok": false, "formatted": "", "error": error })))
    }
}
