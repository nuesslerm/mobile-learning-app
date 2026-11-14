//! The bucket serve verb: a directory as a blob store over HTTP, for any always-on host (plan `docs/plans/multi-cloud-infra/plan.md`
//! section 3). Keys follow `Key::at`, writes go through `DirStore`, so the server and the `file://` store agree on both.
use crate::ci_metrics;
use blob_cache::adapters::DirStore;
use blob_cache::domain::Key;
use blob_cache::ports::{BlobStore, PutOutcome, StoreError};
use serde_json::json;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec {
    values: &["dir", "listen", "read-token-file", "write-token-file", "max-object-mb"],
    required: &["dir", "listen", "read-token-file", "write-token-file"],
    positional: Positional::None,
    ..Spec::NONE
};

const DEFAULT_MAX_MB: u64 = 1024;

struct State {
    root: PathBuf,
    store: DirStore,
    read: String,
    write: String,
    max_bytes: u64,
}

enum Who {
    Nobody,
    Reader,
    Writer,
}

/// the bucket verbs.
pub fn run(args: &[String]) -> Option<i32> {
    match args.split_first()? {
        (verb, rest) if verb == "serve" => serve(rest),
        _ => None,
    }
}

fn fail(error: &str) -> Option<i32> {
    wf_kit::wf_err!("wf bucket serve: {error}");
    println!("{}", json!({ "ok": false, "error": error }));
    Some(1)
}

fn token(path: &str, role: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("cannot read the {role} token file: {error}"))?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(format!("the {role} token file is empty"));
    }
    Ok(text)
}

fn serve(args: &[String]) -> Option<i32> {
    let parsed = parse_spec(args, &SPEC)?;
    let max_mb = match parsed.val("max-object-mb").map(str::parse::<u64>) {
        None => DEFAULT_MAX_MB,
        Some(Ok(mb)) if mb > 0 => mb,
        Some(_) => {
            wf_kit::wf_err!("wf bucket serve: --max-object-mb must be a whole number above 0");
            return Some(2);
        }
    };
    let (dir, listen) = (parsed.val("dir")?, parsed.val("listen")?);
    let (read, write) = match (token(parsed.val("read-token-file")?, "read"), token(parsed.val("write-token-file")?, "write")) {
        (Ok(read), Ok(write)) => (read, write),
        (Err(error), _) | (_, Err(error)) => return fail(&error),
    };
    if let Err(error) = std::fs::create_dir_all(dir) {
        return fail(&format!("cannot create {dir}: {error}"));
    }
    let server = match Server::http(listen) {
        Ok(server) => Arc::new(server),
        Err(error) => return fail(&format!("cannot listen on {listen}: {error}")),
    };
    let bound = server.server_addr().to_ip().map_or_else(|| listen.to_string(), |addr| addr.to_string());
    let state = Arc::new(State { root: PathBuf::from(dir), store: DirStore::new(dir), read, write, max_bytes: max_mb.saturating_mul(1024 * 1024) });
    println!("{}", json!({ "ok": true, "listen": bound, "dir": dir }));
    for request in server.incoming_requests() {
        let state = state.clone();
        std::thread::spawn(move || handle(&state, request));
    }
    Some(0)
}

fn matches(given: &str, wanted: &str) -> bool {
    given.len() == wanted.len() && given.bytes().zip(wanted.bytes()).fold(0, |diff, (a, b)| diff | (a ^ b)) == 0
}

fn who(state: &State, request: &Request) -> Who {
    let bearer = request.headers().iter().find(|h| h.field.equiv("Authorization")).and_then(|h| h.value.as_str().strip_prefix("Bearer ")).map(str::trim);
    match bearer {
        Some(given) if matches(given, &state.write) => Who::Writer,
        Some(given) if matches(given, &state.read) => Who::Reader,
        _ => Who::Nobody,
    }
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("a static header")
}

fn say(request: Request, status: u16, reason: &str) {
    let mut response = Response::from_string(format!("{reason}\n")).with_status_code(StatusCode(status));
    if status == 401 {
        response = response.with_header(header("WWW-Authenticate", "Bearer"));
    }
    reply(request, response);
}

fn reply<R: Read>(request: Request, response: Response<R>) {
    if let Err(error) = request.respond(response) {
        ci_metrics::event("warn", "bucket.serve.respond.failed", &[("error", error.to_string())]);
    }
}

fn handle(state: &State, request: Request) {
    let who = who(state, &request);
    if matches!(who, Who::Nobody) {
        return say(request, 401, "a bearer token is required");
    }
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or("");
    let Some(raw) = path.strip_prefix("/o/") else {
        return say(request, 404, "objects live under /o/<key>");
    };
    let Ok(key) = Key::at(raw) else {
        return say(request, 400, "not a valid key");
    };
    match request.method() {
        Method::Get | Method::Head => read(state, request, &key),
        Method::Put if matches!(who, Who::Writer) => write(state, request, &key),
        Method::Put => say(request, 401, "the write token is required to put"),
        _ => say(request, 405, "only GET, HEAD and PUT are served"),
    }
}

fn file_of(root: &Path, key: &Key) -> Option<(PathBuf, u64)> {
    let path = root.join(key.as_str());
    let meta = std::fs::metadata(&path).ok()?;
    meta.is_file().then(|| (path, meta.len()))
}

fn read(state: &State, request: Request, key: &Key) {
    let Some((path, length)) = file_of(&state.root, key) else {
        return say(request, 404, "no such object");
    };
    let length = Some(length as usize);
    let headers = vec![header("Content-Type", "application/octet-stream")];
    if *request.method() == Method::Head {
        return reply(request, Response::new(StatusCode(200), headers, std::io::empty(), length, None));
    }
    match File::open(path) {
        Ok(file) => reply(request, Response::new(StatusCode(200), headers, file, length, None)),
        Err(_) => say(request, 404, "no such object"),
    }
}

fn write(state: &State, mut request: Request, key: &Key) {
    let first_writer = request.headers().iter().any(|h| h.field.equiv("If-None-Match") && h.value.as_str().trim() == "*");
    if request.body_length().is_some_and(|n| n as u64 > state.max_bytes) {
        return say(request, 413, "the object is over the size limit");
    }
    if first_writer && matches!(state.store.exists(key), Ok(true)) {
        return say(request, 412, "the object already exists");
    }
    let mut body = Vec::new();
    // one byte past the limit tells a body with no declared length that it is too big
    let read = request.as_reader().take(state.max_bytes + 1).read_to_end(&mut body);
    if let Err(error) = read {
        return say(request, 400, &format!("the body ended early: {error}"));
    }
    if body.len() as u64 > state.max_bytes {
        return say(request, 413, "the object is over the size limit");
    }
    match state.store.put(key, &body, first_writer) {
        Ok(PutOutcome::Stored) => say(request, 200, "stored"),
        Ok(PutOutcome::Exists) => say(request, 412, "the object already exists"),
        Err(StoreError::Rejected(why)) => say(request, 400, &why),
        Err(error) => {
            ci_metrics::event("warn", "bucket.serve.put.failed", &[("error", error.to_string())]);
            say(request, 500, "the object could not be stored")
        }
    }
}
