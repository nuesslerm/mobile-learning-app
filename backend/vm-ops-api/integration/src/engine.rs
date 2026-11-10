//! The fake Docker Engine: an HTTP/1.1 server on the world's `DOCKER_SOCKET` that records every request with its body
//! headers, body and time of arrival, and answers from the case's script (plan section 3, "The doubles"): the container
//! list (only running containers without `all=true`), exec create, start (Docker's multiplexed frames) and inspect by
//! rules keyed on the exec `Cmd`, logs as a snapshot or a follow stream (chunks cut anywhere, a failure after the
//! headers), stats, the archive PUT (its tar read into entries), and a status injected per path. Exec create and start
//! refuse a body that is not `application/json`, as the daemon does. Anything unscripted is answered 404 with a message
//! that says so, so a case never mistakes it for a real answer.
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

/// How long a follow stream waits between its chunks, so each reaches the service as a read of its own.
const CHUNK_GAP: Duration = Duration::from_millis(30);
const STREAM_TYPE: &str = "application/vnd.docker.multiplexed-stream";
/// The headers that frame a request's body, which `requests()` records when present: what the daemon reads before the body.
const FRAMING: [&str; 3] = ["content-type", "content-length", "transfer-encoding"];

/// One frame of a non-TTY stream: the stream (1 stdout, 2 stderr), three zero bytes, the length (u32 BE), the bytes.
pub fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = vec![stream, 0, 0, 0];
    f.extend_from_slice(&u32::try_from(payload.len()).expect("a frame under 4 GiB").to_be_bytes());
    f.extend_from_slice(payload);
    f
}

/// A rule for the execs whose `Cmd` it matches: what exec create answers, then the output and exit code of each run in
/// turn, the last one repeating, so a polled status can go from running to done.
pub struct Exec {
    prefix: Vec<String>,
    rest: Option<Regex>,
    create: Option<(u16, String)>,
    answers: Vec<(Vec<u8>, i64)>,
    runs: usize,
}

impl Exec {
    /// Matches a `Cmd` equal to `argv`.
    pub fn exact(argv: &[&str]) -> Exec {
        Exec { prefix: argv.iter().map(|a| a.to_string()).collect(), rest: None, create: None, answers: Vec::new(), runs: 0 }
    }

    /// Matches a `Cmd` that starts with `prefix` and whose further arguments, joined with one space, match `rest`.
    pub fn prefix(prefix: &[&str], rest: &str) -> Exec {
        let rest = Regex::new(rest).unwrap_or_else(|e| panic!("the exec rule's regex {rest:?}: {e}"));
        Exec { rest: Some(rest), ..Exec::exact(prefix) }
    }

    /// Exec create answers `status` with `body` instead of 201, as Docker does for a container that cannot exec.
    pub fn create(mut self, status: u16, body: &str) -> Exec {
        self.create = Some((status, body.to_string()));
        self
    }

    /// The next run prints `stdout` in one frame (none when empty) and exits `exit_code`.
    pub fn answer(self, stdout: &str, exit_code: i64) -> Exec {
        let output = if stdout.is_empty() { Vec::new() } else { frame(1, stdout.as_bytes()) };
        self.frames(output, exit_code)
    }

    /// The next run's output as raw frame bytes, so a case can cut a character across two frames.
    pub fn frames(mut self, output: Vec<u8>, exit_code: i64) -> Exec {
        self.answers.push((output, exit_code));
        self
    }

    fn matches(&self, cmd: &[String]) -> bool {
        match &self.rest {
            None => cmd == self.prefix.as_slice(),
            Some(rest) => cmd.starts_with(&self.prefix) && rest.is_match(&cmd[self.prefix.len()..].join(" ")),
        }
    }

    fn next_run(&mut self) -> (Vec<u8>, i64) {
        let run = self.answers.get(self.runs.min(self.answers.len().saturating_sub(1))).cloned().unwrap_or((Vec::new(), 0));
        self.runs += 1;
        run
    }
}

/// A container's logs: the chunks the Engine writes, each already framed or cut through a frame anywhere. A snapshot
/// answers them as one body; a follow stream writes them one by one and then ends, or fails after the headers.
#[derive(Clone)]
pub struct Logs {
    chunks: Vec<Vec<u8>>,
    abort: bool,
}

impl Logs {
    pub fn chunks(chunks: Vec<Vec<u8>>) -> Logs {
        Logs { chunks, abort: false }
    }

    /// `text` on stdout, one frame in one chunk.
    pub fn text(text: &str) -> Logs {
        Logs::chunks(vec![frame(1, text.as_bytes())])
    }

    /// A follow stream whose connection the Engine drops after the last chunk, instead of ending the body.
    pub fn then_abort(mut self) -> Logs {
        self.abort = true;
        self
    }
}

/// One file of an archive PUT's tar: its name (with any ustar prefix), mode, owner and bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TarEntry {
    pub name: String,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub bytes: Vec<u8>,
}

/// An archive PUT the Engine took: the container as the path named it, the `path` query decoded, and the tar's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    pub container: String,
    pub path: String,
    pub entries: Vec<TarEntry>,
}

#[derive(Default)]
struct Script {
    containers: Vec<Value>,
    execs: Vec<Exec>,
    /// Each created exec: its run's output and exit code.
    created: HashMap<String, (Vec<u8>, i64)>,
    logs: HashMap<String, Logs>,
    stats: HashMap<String, String>,
    failures: HashMap<String, (u16, String)>,
    archives: Vec<Archive>,
    requests: Vec<Value>,
    /// Each request's body as received, beside the lossy text in `requests`, so a binary body compares byte for byte.
    bodies: Vec<Bytes>,
    /// When each request arrived, on this process's clock, so a case can order it against its own steps.
    times: Vec<Instant>,
}

type Shared = Arc<Mutex<Script>>;

pub struct Engine {
    script: Shared,
    _runtime: tokio::runtime::Runtime,
}

impl Engine {
    /// Serves on `socket`, which the caller has checked is inside the case's dir; it lists no container until told.
    pub(crate) fn serve(socket: &Path) -> Engine {
        let script = Arc::new(Mutex::new(Script::default()));
        let runtime =
            tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().expect("the fake Engine's runtime");
        let listener = runtime
            .block_on(async { tokio::net::UnixListener::bind(socket) })
            .unwrap_or_else(|e| panic!("the fake Engine cannot bind {}: {e}", socket.display()));
        let router = axum::Router::new().fallback(answer).with_state(script.clone());
        runtime.spawn(async move { axum::serve(listener, router).await });
        Engine { script, _runtime: runtime }
    }

    fn with(&self, f: impl FnOnce(&mut Script)) -> &Engine {
        f(&mut self.script.lock().unwrap_or_else(|e| e.into_inner()));
        self
    }

    /// The container list: 200 with a JSON array sets the containers; any other answer is what every list gets.
    pub fn list(&self, status: u16, body: &str) -> &Engine {
        match serde_json::from_str::<Vec<Value>>(body) {
            Ok(containers) if status == 200 => self.with(|s| s.containers = containers),
            _ => self.fail("/containers/json", status, body),
        }
    }

    pub fn exec(&self, rule: Exec) -> &Engine {
        self.with(|s| s.execs.push(rule))
    }

    /// The logs of the container with this `Id`.
    pub fn logs(&self, id: &str, logs: Logs) -> &Engine {
        self.with(|s| _ = s.logs.insert(id.to_string(), logs))
    }

    /// The one-shot stats (`stream=false`) of the container with this `Id`.
    pub fn stats(&self, id: &str, body: &str) -> &Engine {
        self.with(|s| _ = s.stats.insert(id.to_string(), body.to_string()))
    }

    /// Every request to `path` (without its query), whatever its method, answers `status` with `body`.
    pub fn fail(&self, path: &str, status: u16, body: &str) -> &Engine {
        self.with(|s| _ = s.failures.insert(path.to_string(), (status, body.to_string())))
    }

    /// Every request so far, in order: `{method, target, headers, body}`, `headers` holding those of the body headers
    /// (content-type, content-length, transfer-encoding) it carried, and the body decoded as lossy UTF-8.
    pub fn requests(&self) -> Vec<Value> {
        self.script.lock().unwrap_or_else(|e| e.into_inner()).requests.clone()
    }

    /// When each request so far arrived, in the order of `requests()`.
    pub fn request_times(&self) -> Vec<Instant> {
        self.script.lock().unwrap_or_else(|e| e.into_inner()).times.clone()
    }

    /// Every archive PUT the Engine answered 200, in order.
    pub fn archives(&self) -> Vec<Archive> {
        self.script.lock().unwrap_or_else(|e| e.into_inner()).archives.clone()
    }

    /// The raw body of the `i`th request, for bytes that are no UTF-8 (an archive's tar).
    pub fn request_body(&self, i: usize) -> Vec<u8> {
        let s = self.script.lock().unwrap_or_else(|e| e.into_inner());
        s.bodies.get(i).unwrap_or_else(|| panic!("the fake Engine received {} requests, not {}", s.bodies.len(), i + 1)).to_vec()
    }
}

fn reply(status: u16, content_type: &str, body: impl Into<Body>) -> Response {
    let status = StatusCode::from_u16(status).expect("a scripted status");
    (status, [(header::CONTENT_TYPE, content_type.to_string())], body.into()).into_response()
}

fn json_reply(status: u16, body: impl Into<Body>) -> Response {
    reply(status, "application/json", body)
}

fn message(status: u16, text: &str) -> Response {
    json_reply(status, json!({ "message": text }).to_string())
}

fn no_rule(what: &str) -> Response {
    message(404, &format!("the fake Engine has no rule for {what}"))
}

fn flag(query: &str, name: &str) -> bool {
    query.split('&').filter_map(|p| p.split_once('=')).any(|(k, v)| k == name && (v == "true" || v == "1"))
}

/// The listed container that `id` names, by `Id` or by name, as Docker resolves it.
fn container<'a>(s: &'a Script, id: &str) -> Option<&'a Value> {
    s.containers.iter().find(|c| {
        c["Id"] == id || c["Names"].as_array().is_some_and(|n| n.iter().any(|n| n.as_str() == Some(&format!("/{id}"))))
    })
}

fn is_running(c: &Value) -> bool {
    matches!(c["State"].as_str(), Some("running" | "paused" | "restarting"))
}

/// moby's `httputils.CheckForJSON`, which exec create and start run first: a request with a body (a length over 0, or
/// chunked) must name `application/json` as its media type, or the daemon answers 400 before it reads the body.
fn check_json(headers: &HeaderMap) -> Option<Response> {
    let has_body = headers.contains_key(header::TRANSFER_ENCODING)
        || headers.get(header::CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.trim().parse::<u64>().ok()).is_some_and(|n| n > 0);
    let content_type = headers.get(header::CONTENT_TYPE).map(|v| String::from_utf8_lossy(v.as_bytes()).to_string()).unwrap_or_default();
    if content_type.is_empty() && !has_body {
        return None;
    }
    match content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "application/json" => None,
        "" => Some(message(400, &format!("malformed Content-Type header ({content_type}): mime: no media type"))),
        _ => Some(message(400, &format!("unsupported Content-Type header ({content_type}): must be 'application/json'"))),
    }
}

async fn answer(State(script): State<Shared>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let target = uri.path_and_query().map_or_else(|| uri.path().to_string(), |p| p.as_str().to_string());
    let query = uri.query().unwrap_or("");
    let framing: serde_json::Map<String, Value> =
        FRAMING.iter().filter_map(|&k| Some((k.to_string(), json!(String::from_utf8_lossy(headers.get(k)?.as_bytes()))))).collect();
    let mut s = script.lock().unwrap_or_else(|e| e.into_inner());
    let lossy = String::from_utf8_lossy(&body);
    s.requests.push(json!({ "method": method.as_str(), "target": target, "headers": framing, "body": lossy }));
    s.bodies.push(body.clone());
    s.times.push(Instant::now());
    if let Some((status, body)) = s.failures.get(uri.path()) {
        return json_reply(*status, body.clone());
    }
    let parts: Vec<&str> = uri.path().trim_start_matches('/').split('/').collect();
    let reads_json = method == Method::POST && matches!(parts.as_slice(), ["containers", _, "exec"] | ["exec", _, "start"]);
    if let Some(refused) = reads_json.then(|| check_json(&headers)).flatten() {
        return refused;
    }
    match (method.as_str(), parts.as_slice()) {
        ("GET", ["containers", "json"]) => {
            let all = flag(query, "all");
            let listed: Vec<&Value> = s.containers.iter().filter(|c| all || is_running(c)).collect();
            json_reply(200, json!(listed).to_string())
        }
        ("POST", ["containers", id, "exec"]) => exec_create(&mut s, id, &body),
        ("POST", ["exec", id, "start"]) => match s.created.get(*id) {
            None => message(404, &format!("No such exec instance: {id}")),
            Some(_) if serde_json::from_slice::<Value>(&body).is_ok_and(|b| b["Detach"] == true) => reply(200, STREAM_TYPE, ""),
            Some((output, _)) => reply(200, STREAM_TYPE, output.clone()),
        },
        ("GET", ["exec", id, "json"]) => match s.created.get(*id) {
            None => message(404, &format!("No such exec instance: {id}")),
            Some((_, code)) => json_reply(200, json!({ "ID": id, "Running": false, "ExitCode": code }).to_string()),
        },
        ("GET", ["containers", id, "logs"]) => match (container(&s, id), s.logs.get(*id)) {
            (None, _) => message(404, &format!("No such container: {id}")),
            (Some(_), None) => no_rule(&format!("the logs of {id}")),
            (Some(_), Some(logs)) if flag(query, "follow") => follow(logs.clone()),
            (Some(_), Some(logs)) => reply(200, STREAM_TYPE, logs.chunks.concat()),
        },
        ("GET", ["containers", id, "stats"]) => match (container(&s, id), s.stats.get(*id)) {
            (None, _) => message(404, &format!("No such container: {id}")),
            (Some(_), None) => no_rule(&format!("the stats of {id}")),
            (Some(_), Some(stats)) => json_reply(200, stats.clone()),
        },
        ("PUT", ["containers", id, "archive"]) => put_archive(&mut s, id, query, &body),
        _ => no_rule(&format!("{method} {target}")),
    }
}

fn exec_create(s: &mut Script, id: &str, body: &[u8]) -> Response {
    let Some(c) = container(s, id) else {
        return message(404, &format!("No such container: {id}"));
    };
    if !is_running(c) {
        return message(409, &format!("container {id} is not running"));
    }
    let cmd: Vec<String> = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|b| b["Cmd"].as_array().map(|a| a.iter().map(|v| v.as_str().unwrap_or_default().to_string()).collect()))
        .unwrap_or_default();
    let Some(rule) = s.execs.iter_mut().find(|r| r.matches(&cmd)) else {
        return no_rule(&format!("exec {}", json!(cmd)));
    };
    if let Some((status, body)) = &rule.create {
        return json_reply(*status, body.clone());
    }
    let run = rule.next_run();
    let exec_id = format!("exec-{}", s.created.len() + 1);
    s.created.insert(exec_id.clone(), run);
    json_reply(201, json!({ "Id": exec_id }).to_string())
}

/// Docker's archive PUT: the tar is extracted into `path`, so the fake reads it as Docker would and refuses one it cannot.
fn put_archive(s: &mut Script, id: &str, query: &str, body: &[u8]) -> Response {
    if container(s, id).is_none() {
        return message(404, &format!("No such container: {id}"));
    }
    let Some(path) = query.split('&').filter_map(|p| p.split_once('=')).find(|(k, _)| *k == "path").map(|(_, v)| decode(v)) else {
        return message(400, "the archive PUT names no path");
    };
    match untar(body) {
        Ok(entries) => {
            s.archives.push(Archive { container: id.to_string(), path, entries });
            reply(200, "text/plain; charset=utf-8", "")
        }
        Err(e) => message(400, &format!("the fake Engine cannot read the archive: {e}")),
    }
}

/// A query value as Go's form parser reads it: `%XX` escapes and `+` as a space.
fn decode(v: &str) -> String {
    let (b, mut out, mut i) = (v.as_bytes(), Vec::new(), 0);
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (b[i], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (c, _) => {
                out.push(if c == b'+' { b' ' } else { c });
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// The regular files of a ustar (or GNU) archive, each header's checksum checked, ending at the end-of-archive blocks or
/// at the end of the input, as Go's archive/tar reads it.
fn untar(tar: &[u8]) -> Result<Vec<TarEntry>, String> {
    if tar.len() % 512 != 0 {
        return Err(format!("{} bytes is no whole number of 512-byte blocks", tar.len()));
    }
    let mut entries = Vec::new();
    let mut at = 0;
    while at < tar.len() {
        let header = &tar[at..at + 512];
        if header.iter().all(|&b| b == 0) {
            return match tar[at..].iter().all(|&b| b == 0) {
                true => Ok(entries),
                false => Err(format!("bytes after the end-of-archive block at {at}")),
            };
        }
        let text = |from: usize, to: usize| {
            let field = &header[from..to];
            String::from_utf8_lossy(&field[..field.iter().position(|&b| b == 0).unwrap_or(field.len())]).to_string()
        };
        let octal = |name: &str, from: usize, to: usize| {
            let digits = text(from, to);
            u64::from_str_radix(digits.trim(), 8).map_err(|_| format!("the header at {at} has {name} {digits:?}, no octal number"))
        };
        let sum: u64 = header.iter().enumerate().map(|(i, &b)| u64::from(if (148..156).contains(&i) { b' ' } else { b })).sum();
        let stored = octal("chksum", 148, 156)?;
        if stored != sum {
            return Err(format!("the header at {at} says checksum {stored:o}, but it sums to {sum:o}"));
        }
        if !matches!(&header[257..265], b"ustar\x0000" | b"ustar  \x00") {
            return Err(format!("the header at {at} has no ustar magic: {:?}", String::from_utf8_lossy(&header[257..265])));
        }
        if !matches!(header[156], b'0' | 0) {
            return Err(format!("the header at {at} is type {:?}, not a regular file", header[156] as char));
        }
        let size = usize::try_from(octal("size", 124, 136)?).map_err(|e| e.to_string())?;
        let data = at + 512;
        let end = data + size.div_ceil(512) * 512;
        if end > tar.len() {
            return Err(format!("the file at {at} needs {size} bytes, but the archive ends first"));
        }
        let (prefix, name) = (text(345, 500), text(0, 100));
        let id = |name: &str, from: usize, to: usize| octal(name, from, to).and_then(|n| u32::try_from(n).map_err(|e| e.to_string()));
        entries.push(TarEntry {
            name: if prefix.is_empty() { name } else { format!("{prefix}/{name}") },
            mode: id("mode", 100, 108)?,
            uid: id("uid", 108, 116)?,
            gid: id("gid", 116, 124)?,
            bytes: tar[data..data + size].to_vec(),
        });
        at = end;
    }
    Ok(entries)
}

/// The header bytes that hold the mtime (136..148) and the checksum (148..156), which plan section 3 ("Comparing
/// recorded calls") leaves out of a byte-for-byte comparison.
const TAR_STAMPS: std::ops::Range<usize> = 136..156;

/// The archive the service writes for one file (the bytes Node's tar writer produced), its mtime and checksum zeroed: a 512-byte ustar header
/// (octal fields NUL-terminated, typeflag `0`, magic `ustar\0` and version `00`, no owner names or prefix), the bytes
/// padded to 512, then two zero blocks. Compare it with what the Engine received through [`unstamped`].
pub fn node_tar(file: &TarEntry) -> Vec<u8> {
    let mut header = vec![0u8; 512];
    let mut put = |at: usize, bytes: &[u8]| header[at..at + bytes.len()].copy_from_slice(bytes);
    let name = file.name.as_bytes();
    put(0, &name[..name.len().min(100)]);
    put(100, format!("{:07o}\0", file.mode).as_bytes());
    put(108, format!("{:07o}\0", file.uid).as_bytes());
    put(116, format!("{:07o}\0", file.gid).as_bytes());
    put(124, format!("{:011o}\0", file.bytes.len()).as_bytes());
    put(156, b"0");
    put(257, b"ustar\x0000");
    let mut tar = header;
    tar.extend_from_slice(&file.bytes);
    tar.resize(512 + file.bytes.len().div_ceil(512) * 512 + 1024, 0);
    tar
}

/// `tar` with its first header's mtime and checksum zeroed, to compare with [`node_tar`].
pub fn unstamped(tar: &[u8]) -> Vec<u8> {
    let mut tar = tar.to_vec();
    if let Some(stamps) = tar.get_mut(TAR_STAMPS) {
        stamps.fill(0);
    }
    tar
}

/// The chunks of a follow stream as hyper takes a body: one item per chunk, then the end or an error, which drops the
/// connection with the chunked body unfinished.
struct Chunks(tokio::sync::mpsc::Receiver<Result<Bytes, std::io::Error>>);

impl futures_core::Stream for Chunks {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

fn follow(logs: Logs) -> Response {
    let (tx, rx) = tokio::sync::mpsc::channel(logs.chunks.len() + 1);
    tokio::spawn(async move {
        for chunk in logs.chunks {
            if tx.send(Ok(Bytes::from(chunk))).await.is_err() {
                return;
            }
            tokio::time::sleep(CHUNK_GAP).await;
        }
        if logs.abort {
            let _ = tx.send(Err(std::io::Error::other("the fake Engine drops the follow stream"))).await;
        }
    });
    reply(200, STREAM_TYPE, Body::from_stream(Chunks(rx)))
}
