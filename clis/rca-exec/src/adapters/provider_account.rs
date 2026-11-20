//! One-shot requests to a provider's account at this executor, under the login its CLI keeps in the executor's home: the CLI's self-update, one
//! request to a `codex app-server` started for it, and the Claude account's usage. Nothing is kept between calls and no token is returned.
use exec_proc::{ClaudeAnswer, CodexAnswer, CodexRequest, UpdateProgram, CLAUDE_CREDENTIALS, UPDATE_OUTPUT_MAX};
use exec_proto::{Code, ExecError, ExecResult};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

fn io(e: std::io::Error) -> ExecError {
    ExecError::new(Code::Io, e.to_string())
}

fn kill_group(child: &mut Child) {
    // SAFETY: killing the process group `process_group(0)` made at the child's start.
    unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) };
    let _ = child.wait();
}

fn tail(text: &str, max: usize) -> String {
    let skip = text.len().saturating_sub(max);
    let mut from = skip;
    while !text.is_char_boundary(from) {
        from += 1;
    }
    text[from..].trim().to_string()
}

/// The CLI's own update, run to its end with no input; its exit code and the tail of what it printed.
pub fn update(args: &UpdateProgram, path: &Path, search: &str, home: &Path, limit: Duration) -> ExecResult<(i32, String)> {
    let mut child = Command::new(path).args(args.args()).env("PATH", search).env("HOME", home).env("NO_COLOR", "1").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0).spawn().map_err(io)?;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let (mut from, mut buf) = (stream, [0u8; 4096]);
            while let Ok(n) = from.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let deadline = Instant::now() + limit;
    let mut out = Vec::new();
    let status = loop {
        while let Ok(chunk) = rx.try_recv() {
            out.extend_from_slice(&chunk);
            if out.len() > UPDATE_OUTPUT_MAX * 4 {
                out.drain(..out.len() - UPDATE_OUTPUT_MAX * 2);
            }
        }
        if let Some(status) = child.try_wait().map_err(io)? {
            break status;
        }
        if Instant::now() >= deadline {
            kill_group(&mut child);
            return Err(ExecError::new(Code::Io, format!("the update did not end in {} seconds", limit.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // The pump threads may still hold the last lines of a finished process
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
        out.extend_from_slice(&chunk);
    }
    use std::os::unix::process::ExitStatusExt;
    let code = status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
    Ok((code, tail(&String::from_utf8_lossy(&out), UPDATE_OUTPUT_MAX)))
}

/// What the app-server printed, a line at a time, until it closes its output.
fn lines(child: &mut Child) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    if let Some(out) = child.stdout.take() {
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    rx
}

/// The answer to request `id`: its `result`, or the app-server's own `error` message as an `IO_ERROR`.
fn answer(rx: &mpsc::Receiver<String>, id: u64, deadline: Instant) -> ExecResult<Value> {
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let line = match rx.recv_timeout(left) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => return Err(ExecError::new(Code::Io, "the Codex app-server did not answer in time")),
            Err(RecvTimeoutError::Disconnected) => return Err(ExecError::new(Code::Io, "the Codex app-server ended before it answered")),
        };
        // Output that is not a JSON-RPC line is not part of the protocol stream
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if message["id"] != json!(id) {
            continue;
        }
        if let Some(error) = message.get("error").filter(|e| !e.is_null()) {
            let text = error["message"].as_str().unwrap_or("the app-server refused the request");
            return Err(ExecError::new(Code::Io, text.to_string()));
        }
        return Ok(message["result"].clone());
    }
}

/// Starts `codex app-server`, does its handshake, sends the one request and stops the server.
pub fn codex(args: &CodexRequest, path: &Path, search: &str, home: &Path) -> ExecResult<CodexAnswer> {
    let mut child = Command::new(path).arg("app-server").env("PATH", search).env("HOME", home).env("NO_COLOR", "1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0).spawn().map_err(io)?;
    let rx = lines(&mut child);
    let outcome = (|| {
        let mut input = child.stdin.take().ok_or_else(|| ExecError::new(Code::Io, "the Codex app-server takes no input"))?;
        let deadline = Instant::now() + Duration::from_millis(args.timeout_ms);
        let mut send = |value: Value| writeln!(input, "{value}").and_then(|()| input.flush()).map_err(io);
        send(json!({"id": 1, "method": "initialize", "params": {"clientInfo": {"name": "rca-exec", "version": "1.0.0"}}}))?;
        answer(&rx, 1, deadline)?;
        send(json!({"method": "initialized", "params": {}}))?;
        send(json!({"id": 2, "method": args.method, "params": args.params}))?;
        answer(&rx, 2, deadline).map(|result| CodexAnswer { result })
    })();
    kill_group(&mut child);
    outcome
}

/// The token of the Claude login in the executor's home, or why there is none.
fn claude_token(home: &Path) -> ExecResult<String> {
    let raw = std::fs::read_to_string(home.join(CLAUDE_CREDENTIALS)).map_err(|_| ExecError::new(Code::NotFound, "claude holds no login here"))?;
    let parsed: Value = serde_json::from_str(&raw).map_err(|_| ExecError::new(Code::Io, "the Claude login file is not JSON"))?;
    parsed["claudeAiOauth"]["accessToken"].as_str().filter(|t| !t.is_empty()).map(str::to_string).ok_or_else(|| ExecError::new(Code::NotFound, "claude holds no login here"))
}

/// The usage of the Claude account at `url`: the token goes to `curl` on its input, never on its command line.
pub fn claude_usage(url: &str, search: &str, home: &Path) -> ExecResult<ClaudeAnswer> {
    let token = claude_token(home)?;
    let curl = std::env::split_paths(search).map(|dir| dir.join("curl")).find(|p| p.is_file()).ok_or_else(|| ExecError::new(Code::NotFound, "curl is not installed here"))?;
    let mut child = Command::new(&curl)
        .args(["-sS", "--max-time", "10", "-w", "\n%{http_code}", "-H", "@-", url])
        .env("PATH", search)
        .env("HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(io)?;
    let headers = format!("Authorization: Bearer {token}\nanthropic-beta: oauth-2025-04-20\nUser-Agent: claude-code/2.1.0\n");
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(headers.as_bytes());
    }
    let done = child.wait_with_output().map_err(io)?;
    if !done.status.success() {
        let why = String::from_utf8_lossy(&done.stderr).trim().replace(&token, "<token>");
        return Err(ExecError::new(Code::Io, if why.is_empty() { "the usage request failed".to_string() } else { tail(&why, 400) }));
    }
    let text = String::from_utf8_lossy(&done.stdout).into_owned();
    let (body, status) = text.rsplit_once('\n').ok_or_else(|| ExecError::new(Code::Io, "the usage answer has no status"))?;
    let status: u16 = status.trim().parse().map_err(|_| ExecError::new(Code::Io, "the usage answer has no status"))?;
    let body = serde_json::from_str::<Value>(body).unwrap_or_else(|_| Value::String(tail(body, 400)));
    Ok(ClaudeAnswer { status, body })
}
