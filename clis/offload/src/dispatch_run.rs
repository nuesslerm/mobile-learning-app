//! `wf offload ask <prompt>`: one prompt on the cheap-model rotation through `opencode run`, the delegation path.
//! The ladder, plugin chain and run-log row come from `rca_ladder::run`; this file is the process, the stores and the
//! printing around them.
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_kit::js::js_num;
use wf_kit::logfmt::mcp;
use secrets::load_all_into_process_env;
use wf_kit::time::now_ms;
use rca_core::file_store::FileRunLog;
use rca_ladder::file_store::FileLedgerStore;
use rca_ladder::ladder::{classify_failure, model_ladders, Verdict};
use rca_core::opencode::{
    build_opencode_args, format_dispatch_summary, has_completed_write, last_text_part, looks_like_cap_error, looks_like_transient_crash,
    opencode_env_overrides, parse_opencode_json_stream, tool_lines_from_events, OpencodeTokens, SummaryInput, ToolEvent,
};
use rca_ladder::pool_ledger::{cold_pools, Entry, Ledger};
use rca_ladder::run::{run_once, AttemptMetrics, AttemptOutput, RunHost, RunInput, RunOutcome};
use rca_core::time::iso;
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MODEL_ROTATION: [&str; 4] = [
    "minimax/MiniMax-M3",
    "opencode-go/deepseek-v4.1-flash",
    "kimi-code-plan-global/kimi-for-coding",
    "kimi-code-plan-global/k3",
];
const PREFLIGHT_POOL: &str = "opencode-go";
pub(crate) const DEFAULT_IDLE_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_PREFLIGHT_URL: &str = "https://opencode.ai/zen/go/v1";
const DEFAULT_CAP_RESET_MS: i64 = 5 * 60 * 60 * 1000;
const SOURCE: &str = "opencode.dispatch";

fn rotation_starting_from(model: Option<&str>) -> Vec<String> {
    let all: Vec<String> = MODEL_ROTATION.iter().map(|m| m.to_string()).collect();
    match model.and_then(|m| MODEL_ROTATION.iter().position(|x| *x == m)) {
        Some(i) => all[i..].iter().chain(all[..i].iter()).cloned().collect(),
        None => all,
    }
}

struct Options {
    prompt: Option<String>,
    prompt_file: Option<String>,
    positional: Vec<String>,
    summary: Option<String>, // "" = bare flag, otherwise the mode text
    agent: Option<String>,
    output_file: Option<String>,
    caller: Option<String>,
    cwd: Option<String>,
    auto: bool,
    heartbeat: bool,
    idle_ms: Option<u64>,
    timeout_ms: Option<u64>,
    start_model: Option<String>,
    /// The ladder shape (`runbook`) and the models to try, when the caller resolved them.
    shape: Option<String>,
    pins: Option<Vec<String>>,
    live: bool,
    /// Files the prompt reads, sent along with the run (relative to the repository root), which has no checkout to read.
    files: Vec<String>,
    /// The `-f` files: sent like `--files`, and carried by the message as file parts, as `opencode run -f` attaches them.
    attach: Vec<String>,
    /// `--command`: a project command (`.opencode/command/<name>.md`) run with the message as its arguments.
    command: Option<String>,
    /// An opencode session to continue, or the last one this checkout made.
    session: Option<String>,
    cont: bool,
    fork: bool,
    no_auto: bool,
    /// `--format json`: the result as one JSON object instead of the answer text.
    json: bool,
    variant: Option<String>,
    /// The title of the session this run creates ("" for a bare `--title`: the service uses the prompt).
    title: Option<String>,
}

/// What a prompt run of `dispatch` takes.
pub(crate) const SPEC: Spec = Spec {
    values: &["prompt", "prompt-file", "agent", "output-file", "caller", "cwd", "start-model", "idle-timeout-ms", "timeout-ms", "shape", "model", "file", "files", "session", "variant", "dir", "format", "command"],
    // `--no-auto` is not opencode's: refused with the way to a read-only run. `--fork` is opencode's, refused until the service forks.
    flags: &["auto", "no-auto", "continue", "fork", "no-heartbeat", "live", "json"],
    // `--summary [mode]` and `--title [text]` take the next word unless it is an option.
    optional: &["summary", "title"],
    // opencode's own short forms: -m, -f, -s, -c.
    short: &[('m', "model"), ('f', "file"), ('s', "session"), ('c', "continue")],
    positional: Positional::Any,
    ..Spec::NONE
};

/// The options `dispatch` takes; `None` for a shape it does not serve.
fn parse_options(args: &[String]) -> Option<Options> {
    let p = parse_spec(args, &SPEC)?;
    let millis = |name: &str| -> Option<Option<u64>> {
        match p.val(name) {
            None => Some(None),
            Some(v) => v.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0).map(|n| Some(n as u64)),
        }
    };
    let text = |name: &str| p.val(name).map(str::to_string);
    Some(Options {
        prompt: text("prompt"),
        prompt_file: text("prompt-file"),
        positional: p.positional.clone(),
        summary: text("summary"),
        agent: text("agent"),
        output_file: text("output-file"),
        caller: text("caller"),
        cwd: text("cwd").or_else(|| text("dir")),
        auto: p.flag("auto"),
        heartbeat: !p.flag("no-heartbeat"),
        idle_ms: millis("idle-timeout-ms")?,
        timeout_ms: millis("timeout-ms")?,
        start_model: text("start-model"),
        shape: text("shape"),
        pins: Some(p.values("model").into_iter().map(str::to_string).collect::<Vec<_>>()).filter(|m| !m.is_empty()),
        live: p.flag("live"),
        files: text("files").iter().flat_map(|f| f.split(',')).filter(|s| !s.is_empty()).map(str::to_string).chain(p.values("file").into_iter().map(str::to_string)).collect(),
        command: text("command"),
        attach: p.values("file").into_iter().map(str::to_string).collect(),
        fork: p.flag("fork"),
        no_auto: p.flag("no-auto"),
        json: p.val("format") == Some("json") || p.flag("json"),
        session: text("session"),
        cont: p.flag("continue"),
        variant: text("variant"),
        title: text("title"),
    })
}

/// `readDispatchPrompt`: `None` where the TypeScript would raise its own error.
fn resolve_prompt(o: &Options) -> Option<String> {
    let read = |file: &str| -> Option<String> {
        if file == "-" {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).ok()?;
            Some(s)
        } else {
            std::fs::read_to_string(file).ok()
        }
    };
    if let Some(file) = &o.prompt_file {
        return if o.prompt.is_some() { None } else { read(file) };
    }
    // as `opencode run [message..]`: the words are the message, `-` or no message at all reads it from a pipe
    match o.positional.as_slice() {
        [dash] if dash == "-" => {
            if o.prompt.is_some() {
                None
            } else {
                read("-")
            }
        }
        [] => {
            if let Some(prompt) = &o.prompt {
                return Some(prompt.clone());
            }
            // `opencode run --command name` takes no message: the command's arguments are empty
            if o.command.is_some() {
                return Some(" ".to_string());
            }
            use std::io::IsTerminal;
            if std::io::stdin().is_terminal() {
                return None;
            }
            read("-").map(|s| s.trim_end().to_string()).filter(|s| !s.is_empty())
        }
        // as `opencode run`: a word with a space in it is quoted, the words are joined, and what is piped in follows on a new line
        words => {
            let message = words.iter().map(|w| if w.contains(' ') { format!("\"{}\"", w.replace('"', "\\\"")) } else { w.clone() }).collect::<Vec<_>>().join(" ");
            use std::io::IsTerminal;
            let piped = if o.prompt.is_none() && !std::io::stdin().is_terminal() { read("-").map(|s| s.trim_end().to_string()).filter(|s| !s.is_empty()) } else { None };
            Some(match piped {
                Some(piped) => format!("{message}\n{piped}"),
                None => message,
            })
        }
    }
}

fn last_session_file(root: &Path) -> PathBuf {
    root.join(".agents/tmp/rca-last-session")
}

fn last_session(root: &Path) -> Option<String> {
    std::fs::read_to_string(last_session_file(root)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn save_last_session(root: &Path, id: &str) {
    let file = last_session_file(root);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, id);
}

pub(crate) fn repo_root_from(from: &Path) -> PathBuf {
    let mut dir = from.to_path_buf();
    for _ in 0..20 {
        if dir.join(".git").exists() {
            return dir;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    from.to_path_buf()
}

fn uuid_v4() -> String {
    let mut b = [0u8; 16];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        let t = now_ms() as u128 * 6_364_136_223_846_793_005 + std::process::id() as u128;
        b.copy_from_slice(&t.to_le_bytes());
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn model_in_catalog(model: &str) -> Option<bool> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    let catalog: Value = serde_json::from_str(&std::fs::read_to_string(base.join("opencode/models.json")).ok()?).ok()?;
    let (provider, name) = model.split_once('/')?;
    Some(catalog.get(provider).and_then(|p| p.get("models")).and_then(Value::as_object).is_some_and(|m| m.contains_key(name)))
}

enum Preflight {
    Ok,
    Cap(i64),
    Other,
}

/// One-token probe of the opencode-go pool: `opencode run` swallows its provider's 429s until the idle timer fires.
fn preflight(env_key: Option<&str>) -> Preflight {
    let Some(key) = env_key.filter(|k| !k.is_empty()) else { return Preflight::Other };
    let base = std::env::var("OPENCODE_PREFLIGHT_BASE_URL").unwrap_or_else(|_| DEFAULT_PREFLIGHT_URL.to_string());
    let body = serde_json::json!({ "model": "deepseek-v4.1-flash", "max_tokens": 1, "messages": [{ "role": "user", "content": "ok" }] });
    let Ok(response) = cli_http::Request::post(&format!("{base}/chat/completions"))
        .header("authorization", &format!("Bearer {key}"))
        .json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send()
    else {
        return Preflight::Other;
    };
    match response.status {
        429 => {
            let retry = response
                .header("retry-after")
                .and_then(|v| v.trim().parse::<f64>().ok())
                .filter(|s| s.is_finite() && *s > 0.0)
                .map_or(DEFAULT_CAP_RESET_MS, |s| (s * 1000.0) as i64);
            Preflight::Cap(retry)
        }
        200..=299 => Preflight::Ok,
        _ => Preflight::Other,
    }
}

fn apply_preflight_cap(ledger: &Ledger, pool: &str, retry_after_ms: i64, at: i64) -> Ledger {
    let mut next = ledger.clone();
    next.insert(
        pool.to_string(),
        Entry {
            cold_until: iso(at + retry_after_ms),
            learned_reset_ms: retry_after_ms,
            last_cap_at: iso(at),
            last_timeout_at: None,
            cold_source: None,
            consecutive_timeouts: None,
        },
    );
    next
}

struct Attempted {
    exit_code: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

/// The tee to the live opencode-log dashboard on the VM; never fails or slows the dispatch beyond a 3 s call.
#[derive(Clone)]
struct Live {
    run_id: String,
}

impl Live {
    fn post(&self, what: &str, body: Value) {
        let path = format!("/opencode-log/{}/{what}", wf_vmops::vmops::encode_uri_component(&self.run_id));
        let opts = wf_vmops::vmops::Opts { post: true, body: Some(body.to_string()), timeout_sec: 3, ok_statuses: &[200] };
        if let Err(e) = wf_vmops::vmops::request_raw(&path, &opts) {
            wf_kit::logfmt::info("opencode.dispatch.live", "log tee failed (non-fatal)", &[("err", format!("WfError: {e}"))]);
        }
    }
}

struct Limits {
    idle: Duration,
    absolute: Option<Duration>,
    heartbeat: Option<Duration>,
}

fn spawn_error_text(e: &std::io::Error) -> String {
    let code = match e.kind() {
        std::io::ErrorKind::NotFound => "ENOENT".to_string(),
        std::io::ErrorKind::PermissionDenied => "EACCES".to_string(),
        _ => e.to_string(),
    };
    format!("Error: spawn opencode {code}")
}

/// Streams the child's output: the idle timer resets on every byte and fires only on silence, the absolute one is a
/// ceiling; either kills the child and marks the result timed out (exit code 124).
fn jsn(n: f64) -> Value {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 9e15 {
        serde_json::json!(n as i64)
    } else {
        serde_json::json!(n)
    }
}

fn run_opencode(args: &[String], cwd: &str, env: &[(String, String)], model: &str, limits: &Limits, live: Option<&Live>) -> Attempted {
    let spawned = Command::new("opencode")
        .args(args)
        .current_dir(cwd)
        .envs(env.iter().map(|(k, v)| (k, v)))
        // opencode never holds an Anthropic credential of any kind (infra/oracle-cloud/AGENTS.md).
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return Attempted { exit_code: 1, stdout: String::new(), stderr: format!("\n{}", spawn_error_text(&e)), timed_out: false },
    };
    let (tx, rx) = mpsc::channel::<(bool, Vec<u8>)>();
    for (is_err, stream) in [(false, child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>)), (true, child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>))] {
        let Some(mut stream) = stream else { continue };
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 || tx.send((is_err, buf[..n].to_vec())).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let started = Instant::now();
    let mut last_activity = started;
    let mut next_heartbeat = limits.heartbeat.map(|h| started + h);
    let (mut line_count, mut last_line) = (0usize, String::new());
    let mut timed_out = false;
    let mut sent_lines = 0usize;
    let mut next_flush = Instant::now() + Duration::from_secs(1);
    let flush = |out: &[u8], sent: &mut usize| {
        let Some(live) = live else { return };
        let parsed = parse_opencode_json_stream(&String::from_utf8_lossy(out));
        let lines = tool_lines_from_events(&parsed.tool_events);
        if lines.len() > *sent {
            live.post("line", serde_json::json!({ "lines": lines[*sent..].to_vec() }));
            *sent = lines.len();
        }
    };
    loop {
        let mut deadline = last_activity + limits.idle;
        if live.is_some() {
            deadline = deadline.min(next_flush);
        }
        if let Some(abs) = limits.absolute {
            deadline = deadline.min(started + abs);
        }
        if let Some(h) = next_heartbeat {
            deadline = deadline.min(h);
        }
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((is_err, data)) => {
                last_activity = Instant::now();
                let text = String::from_utf8_lossy(&data);
                let lines: Vec<&str> = text.split('\n').filter(|l| !l.is_empty()).collect();
                line_count += lines.len();
                if let Some(l) = lines.last() {
                    last_line = (*l).to_string();
                }
                if is_err { err.extend(data) } else { out.extend(data) }
                if live.is_some() && Instant::now() >= next_flush {
                    flush(&out, &mut sent_lines);
                    next_flush = Instant::now() + Duration::from_secs(1);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                if live.is_some() && now >= next_flush {
                    flush(&out, &mut sent_lines);
                    next_flush = now + Duration::from_secs(1);
                }
                if now >= last_activity + limits.idle || limits.absolute.is_some_and(|a| now >= started + a) {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                if let (Some(h), Some(due)) = (limits.heartbeat, next_heartbeat) {
                    if now >= due {
                        let last = if utf16_len(&last_line) > 120 {
                            let cut: String = last_line.chars().take(120).collect();
                            format!("{cut}...")
                        } else {
                            last_line.clone()
                        };
                        eprintln!(
                            "[heartbeat: elapsed {}s, {line_count} line(s) of output so far, model {model}, last: \"{last}\"]",
                            ((now - started).as_millis() as f64 / 1000.0 + 0.5).floor() as i64
                        );
                        next_heartbeat = Some(due + h);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().ok();
    flush(&out, &mut sent_lines);
    let exit_code = if timed_out { 124 } else { status.and_then(|s| s.code()).unwrap_or(1) };
    Attempted { exit_code, stdout: String::from_utf8_lossy(&out).into_owned(), stderr: String::from_utf8_lossy(&err).into_owned(), timed_out }
}

fn git_diff_stat(cwd: &str, events: &[ToolEvent]) -> Option<String> {
    if !repo_root_from(Path::new(cwd)).join(".git").exists() {
        return None;
    }
    let mut seen = HashSet::new();
    let touched: Vec<String> = events
        .iter()
        .filter(|e| matches!(e.tool.as_str(), "edit" | "write" | "patch" | "multiedit") && e.status == "completed")
        .filter_map(|e| e.input.get("filePath").and_then(Value::as_str))
        .filter(|p| seen.insert(p.to_string()))
        .map(String::from)
        .collect();
    let mut cmd = Command::new("git");
    cmd.args(["diff", "--stat"]).current_dir(cwd);
    if !touched.is_empty() {
        cmd.arg("--").args(&touched);
    }
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

struct Detail {
    summary: String,
    tool_lines: Vec<String>,
    session_id: Option<String>,
    tokens: Option<OpencodeTokens>,
    cost: Option<f64>,
}

struct Host {
    run_id: String,
    cwd: String,
    auto: bool,
    env: Vec<(String, String)>,
    limits: Limits,
    agent: Option<String>,
    prompt: String,
    store: Ledger,
    local: FileLedgerStore,
    local_path: String,
    run_log: FileRunLog,
    run_log_path: String,
    details: HashMap<usize, Detail>,
    last_diff: Option<String>,
    live: Option<Live>,
}

impl Host {
    fn persist_local(&self, ledger: &Ledger) {
        if let Err(e) = self.local.write(ledger) {
            mcp("warn", SOURCE, "could not persist pool cooldowns", &[("path", self.local_path.clone()), ("err", e.to_string())]);
        }
    }
}

impl RunHost for Host {
    fn now(&mut self) -> i64 {
        now_ms()
    }
    fn read_ledger(&mut self) -> Ledger {
        self.store.clone()
    }
    fn write_ledger(&mut self, ledger: &Ledger) {
        self.store = ledger.clone();
        self.persist_local(ledger);
    }
    fn append_run_log(&mut self, entry: &Map<String, Value>) {
        let mut text = Value::Object(entry.clone()).to_string();
        text.push('\n');
        let r = (|| {
            if let Some(dir) = Path::new(&self.run_log_path).parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::OpenOptions::new().create(true).append(true).open(&self.run_log_path)?.write_all(text.as_bytes())
        })();
        let _ = (&self.run_log, r);
    }
    fn diff_stat(&mut self, _cwd: &str, _touched: &[String]) -> Option<String> {
        self.last_diff.clone()
    }
    fn bad_output(&mut self, _text: &str) -> bool {
        false
    }
    fn attempt(&mut self, index: usize, model: &str) -> AttemptOutput {
        self.last_diff = None;
        mcp("info", SOURCE, "trying model", &[("model", model.to_string()), ("runId", self.run_id.clone())]);
        let failed = |class: &str, text: String| AttemptOutput { text: text.clone(), tool_events: vec![], structured: false, error: Some((class.to_string(), text)) };
        if model_in_catalog(model) == Some(false) {
            mcp(
                "error",
                SOURCE,
                "model missing from the opencode catalog, skipping rung",
                &[
                    ("model", model.to_string()),
                    ("runId", self.run_id.clone()),
                    ("fix", "rename it in libraries/rca-core/plugins/ladder/data/model-ladders.json to the id `opencode models` lists".into()),
                ],
            );
            return failed("model-missing", String::new());
        }
        let args = build_opencode_args(self.auto, true, None, self.agent.as_deref(), &self.cwd, model, &self.prompt);
        let result = run_opencode(&args, &self.cwd, &self.env, model, &self.limits, self.live.as_ref());
        let combined = format!("{}\n{}", result.stdout, result.stderr);
        if result.exit_code == 0 {
            let parsed = parse_opencode_json_stream(&result.stdout);
            let saw_json = parsed.saw_json;
            let prose = if saw_json { parsed.text_parts.join("") } else { result.stdout.trim_end().to_string() };
            let tool_events = if saw_json { parsed.tool_events.clone() } else { vec![] };
            self.details.insert(
                index,
                Detail {
                    summary: if saw_json { last_text_part(&parsed.text_parts) } else { prose.trim().to_string() },
                    tool_lines: tool_lines_from_events(&tool_events),
                    session_id: if saw_json { parsed.session_id.clone() } else { None },
                    tokens: saw_json.then(|| parsed.tokens.clone()),
                    cost: saw_json.then_some(parsed.cost),
                },
            );
            self.last_diff = if self.auto { git_diff_stat(&self.cwd, &tool_events) } else { None };
            let _ = has_completed_write(&tool_events);
            return AttemptOutput { text: prose, tool_events, structured: saw_json, error: None };
        }
        let capped = looks_like_cap_error(&combined);
        let crashed = !capped && looks_like_transient_crash(&combined);
        let timed_out = !capped && !crashed && result.timed_out;
        let reason = if capped { "capped" } else if crashed { "crashed" } else if timed_out { "timedOut" } else { "other" };
        failed(classify_failure(reason, &combined, false), combined)
    }
    fn metrics(&mut self, index: usize) -> Option<AttemptMetrics> {
        self.details.get(&index).map(|d| AttemptMetrics {
            tokens: d.tokens.clone(),
            cost: d.cost,
            opencode_session_id: d.session_id.clone(),
            summary_chars: Some(utf16_len(&d.summary) as f64),
        })
    }
    fn judged(&mut self, model: &str, verdict: &Verdict) {
        if verdict.ok {
            return;
        }
        let class = verdict.failure_class.as_deref().unwrap_or_default();
        let fields = [("model", model.to_string()), ("runId", self.run_id.clone())];
        if class == "fatal" {
            mcp("info", SOURCE, "fatal failure, not rotating", &fields);
            return;
        }
        let msg = match class {
            "cap" => "usage cap detected, rotating to next model".to_string(),
            "crash" => "transient crash detected, rotating to next model".to_string(),
            "timeout" => "per-attempt timeout, rotating to next model".to_string(),
            "wrote-nothing" => "wrote-nothing, rotating to next model".to_string(),
            "bad-output" => "bad-output, rotating to next model".to_string(),
            other => format!("{other}, rotating to next model"),
        };
        mcp("info", SOURCE, &msg, &fields);
    }
}

fn write_transcript(path: &str, header: &[(&str, String)], tool_lines: &[String], prose: &str) -> std::io::Result<()> {
    if let Some(dir) = Path::new(path).parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let meta: Vec<String> = header.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    let tools = if tool_lines.is_empty() { "(none)".to_string() } else { tool_lines.join("\n") };
    let body = ["---", &meta.join("\n"), "---", "", "## Tool calls", "", &tools, "", "## Transcript", "", prose, ""].join("\n");
    std::fs::write(path, body)
}

struct Dispatched {
    ok: bool,
    model: Option<String>,
    output: String,
    attempts: Vec<(String, bool, String)>,
    summary: SummaryInput,
    /// The turn as `opencode run --format json` prints it (what the service returned).
    events: Vec<Value>,
}

fn reason_of(a: &rca_ladder::run::AttemptRecord) -> String {
    match &a.verdict {
        Some(v) if !v.ok => v.failure_class.clone().unwrap_or_default(),
        _ => "ok".into(),
    }
}

#[derive(Clone, Copy)]
struct Finish<'a> {
    ladder_len: usize,
    output_file: Option<&'a str>,
    caller: &'a str,
    session: Option<&'a str>,
    duration_ms: u64,
}

fn build_result(outcome: &RunOutcome, host: &Host, o: &Options, finish: Finish<'_>) -> Dispatched {
    let Finish { ladder_len, output_file, caller, session, duration_ms } = finish;
    let attempts: Vec<(String, bool, String)> = outcome
        .attempts
        .iter()
        .filter(|a| a.verdict.is_some())
        .map(|a| {
            let r = reason_of(a);
            (a.model.clone(), r == "cap", r)
        })
        .collect();
    let mut summary = SummaryInput { duration_ms: duration_ms as f64, run_id: host.run_id.clone(), ..Default::default() };
    if !outcome.ok {
        let fatal = outcome.attempts.iter().rfind(|a| a.verdict.is_some()).is_some_and(|a| reason_of(a) == "fatal");
        let output = if fatal {
            outcome.last_output.as_ref().map(|l| l.text.clone()).unwrap_or_default()
        } else {
            format!(
                "All {ladder_len} models were exhausted: {}",
                attempts.iter().map(|(m, _, r)| format!("{m} ({r})")).collect::<Vec<_>>().join(", ")
            )
        };
        return Dispatched { ok: false, model: None, output, attempts, summary, events: Vec::new() };
    }
    let index = outcome.ok_attempt.unwrap_or(0);
    let model = outcome.attempts[index].model.clone();
    let detail = host.details.get(&index);
    let text = outcome.last_output.as_ref().map(|l| l.text.clone()).unwrap_or_default();
    let mut transcript = None;
    if let (Some(path), Some(d)) = (output_file, detail) {
        let mut header = vec![("runId", host.run_id.clone()), ("model", model.clone()), ("caller", caller.to_string()), ("cwd", host.cwd.clone())];
        if let Some(s) = session {
            header.push(("claudeSessionId", s.to_string()));
        }
        match write_transcript(path, &header, &d.tool_lines, &text) {
            Ok(()) => transcript = Some(path.to_string()),
            Err(e) => mcp("warn", SOURCE, "could not write transcript", &[("path", path.to_string()), ("err", e.to_string())]),
        }
    }
    summary.tool_lines = detail.map(|d| d.tool_lines.clone()).unwrap_or_default();
    summary.summary = detail.map_or_else(|| text.trim().to_string(), |d| d.summary.clone());
    summary.diff_stat = host.last_diff.clone();
    summary.model = Some(model.clone());
    summary.tokens = detail.and_then(|d| d.tokens.clone());
    summary.cost = detail.and_then(|d| d.cost);
    summary.transcript_path = transcript;
    let _ = o;
    Dispatched { ok: true, model: Some(model), output: text, attempts, summary, events: Vec::new() }
}

fn out(s: &str) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(s.as_bytes());
    let _ = o.flush();
}

/// One prompt served by the rca service: the ladder, the keys and the shared cooldowns are its, so nothing here falls back to a local
/// opencode. The files the prompt reads go along (`--files`); what a writing run changed comes back and is written into the
/// repository at `cwd`.
fn dispatch_one(o: &Options, prompt: String) -> (Dispatched, String) {
    let started_at = now_ms();
    let run_id = uuid_v4();
    let cwd = o.cwd.clone().unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".into()));
    let root = repo_root_from(Path::new(&cwd));
    let output_file = o.output_file.clone().or_else(|| o.summary.as_ref().map(|_| format!("{}/.agents/tmp/dispatch-{run_id}.md", root.display())));
    let caller = o.caller.clone().unwrap_or_else(|| "dispatch".into());
    let result = dispatch_remote(o, &prompt, &root, &caller, &run_id, started_at, output_file.as_deref(), &cwd);
    if result.ok {
        let mut fields = vec![("model", result.model.clone().unwrap_or_default()), ("runId", run_id.clone())];
        if let Some(t) = &result.summary.tokens {
            fields.push(("tokens", js_num(t.total)));
        }
        mcp("info", SOURCE, "served", &fields);
    }
    (result, run_id)
}

#[allow(clippy::too_many_arguments)]
fn dispatch_remote(o: &Options, prompt: &str, root: &Path, caller: &str, run_id: &str, started_at: i64, output_file: Option<&str>, cwd: &str) -> Dispatched {
    let failed = |output: String| Dispatched { ok: false, model: None, output, attempts: Vec::new(), summary: SummaryInput { run_id: run_id.to_string(), ..Default::default() }, events: Vec::new() };
    let shape = o.shape.clone().unwrap_or_else(|| "document".into());
    let mut models = o.pins.clone().unwrap_or_default();
    if models.is_empty() {
        models.extend(o.start_model.clone());
    }
    let session = match (&o.session, o.cont) {
        (Some(id), _) => Some(id.clone()),
        // as `opencode run --continue` with nothing to continue: a new session
        (None, true) => last_session(root),
        (None, false) => None,
    };
    let mut files = o.files.clone();
    // An opencode agent defined in the repository travels with the run: the session's directory is where opencode looks for it.
    if let Some(agent) = o.agent.as_deref() {
        let definition = format!(".opencode/agents/{agent}.md");
        let source = root.join(&definition);
        if source.is_file() {
            files.push(format!("{}={definition}", source.display()));
        }
    }
    // As with `opencode run`, relative files, and the run's own directory, are the working directory's (`--dir`).
    let base = Path::new(cwd);
    let extras = crate::rca_client::Extras { session: session.as_deref(), agent: o.agent.as_deref(), variant: o.variant.as_deref(), title: o.title.as_deref(), auto: o.auto, attach: &o.attach, command: o.command.as_deref() };
    let request = match crate::rca_client::request(prompt, &shape, &models, &files, caller, &extras, base) {
        Ok(r) => r,
        Err(e) => return failed(e),
    };
    let answered = match crate::rca_client::call(&request, crate::rca_client::timeout_of(o.timeout_ms)) {
        Ok(a) => a,
        Err(e) => return failed(e),
    };
    let body = &answered.body;
    if answered.status != 200 {
        let why = body["error"].as_str().unwrap_or("no reason given");
        return failed(format!("the rca service answered {}: {why}", answered.status));
    }
    let attempts: Vec<(String, bool, String)> = body["attempts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a.get("skipped").is_none_or(Value::is_null))
        .map(|a| {
            let reason = if a["verdict"]["ok"].as_bool().unwrap_or(false) { "ok".to_string() } else { a["verdict"]["failureClass"].as_str().unwrap_or("failed").to_string() };
            (a["choice"]["model"].as_str().unwrap_or_default().to_string(), reason == "cap", reason)
        })
        .collect();
    let duration_ms = (now_ms() - started_at).max(0) as f64;
    if body["ok"].as_bool() != Some(true) {
        let last = body["lastOutput"]["text"].as_str().unwrap_or_default().to_string();
        let output = if body["reason"].as_str() == Some("ladder-exhausted") {
            let skipped: Vec<String> = body["attempts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| !a.get("skipped").is_none_or(Value::is_null))
                .map(|a| format!("{} ({})", a["choice"]["model"].as_str().unwrap_or_default(), a["skipped"]["reason"].as_str().unwrap_or("skipped")))
                .collect();
            let mut tried = attempts.iter().map(|(m, _, r)| format!("{m} ({r})")).collect::<Vec<_>>().join(", ");
            if !skipped.is_empty() {
                tried = format!("{tried}{}skipped: {}", if tried.is_empty() { "" } else { "; " }, skipped.join(", "));
            }
            format!("All {} models were exhausted: {tried}", attempts.len() + skipped.len())
        } else {
            last
        };
        return Dispatched { ok: false, model: None, output, attempts, summary: SummaryInput { duration_ms, run_id: run_id.to_string(), ..Default::default() }, events: Vec::new() };
    }
    let text = body["output"]["text"].as_str().unwrap_or_default().to_string();
    let model = body["attempt"]["choice"]["model"].as_str().map(str::to_string);
    let answered_session = body["session"].as_str().map(str::to_string);
    if let Some(id) = &answered_session {
        save_last_session(root, id);
    }
    let tokens = body["usage"].as_object().map(|u| {
        let n = |k: &str| u.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        rca_core::opencode::OpencodeTokens { total: n("input") + n("output") + n("reasoning"), input: n("input"), output: n("output"), reasoning: n("reasoning"), cache_read: n("cacheRead"), cache_write: n("cacheWrite") }
    });
    let changes: Vec<Value> = body["changes"].as_array().cloned().unwrap_or_default();
    let tool_lines = match crate::rca_client::apply_changes(base, &changes) {
        Ok(lines) => lines,
        Err(e) => return failed(e),
    };
    let mut transcript = None;
    if let Some(path) = output_file {
        let mut header = vec![("runId", run_id.to_string()), ("model", model.clone().unwrap_or_default()), ("caller", caller.to_string()), ("cwd", cwd.to_string())];
        if let Some(s) = std::env::var("CLAUDE_CODE_SESSION_ID").ok().filter(|s| !s.is_empty()) {
            header.push(("claudeSessionId", s));
        }
        match write_transcript(path, &header, &tool_lines, &text) {
            Ok(()) => transcript = Some(path.to_string()),
            Err(e) => mcp("warn", SOURCE, "could not write transcript", &[("path", path.to_string()), ("err", e.to_string())]),
        }
    }
    let summary = SummaryInput {
        summary: text.trim().to_string(),
        tool_lines,
        diff_stat: (!changes.is_empty()).then(|| format!("{} file(s) changed", changes.len())),
        model: model.clone(),
        duration_ms,
        run_id: run_id.to_string(),
        transcript_path: transcript,
        session: answered_session,
        tokens,
        ..Default::default()
    };
    let events = body["events"].as_array().cloned().unwrap_or_default();
    Dispatched { ok: true, model, output: text, attempts, summary, events }
}

/// One prompt through the rotation on a local opencode. Only a runbook still takes this path: its agent drives a browser and a
/// device from a real checkout, which the rca service does not have. `None` when a rung runs on the Claude pool, which this binary
/// does not drive.
fn dispatch_local(o: &Options, prompt: String) -> Option<(Dispatched, String)> {
    let started_at = now_ms();
    let run_id = uuid_v4();
    let cwd = o.cwd.clone().unwrap_or_else(|| std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|_| ".".into()));
    let summary_mode = o.summary.as_deref();
    let output_file = o.output_file.clone().or_else(|| {
        summary_mode.filter(|s| !s.is_empty() || o.summary.is_some()).map(|_| {
            format!("{}/.agents/tmp/dispatch-{run_id}.md", repo_root_from(Path::new(&cwd)).display())
        })
    });
    let caller = o.caller.clone().unwrap_or_else(|| "dispatch".into());
    let session = std::env::var("CLAUDE_CODE_SESSION_ID").ok().filter(|s| !s.is_empty()).or_else(|| std::env::var("CLAUDE_SESSION_ID").ok().filter(|s| !s.is_empty()));
    let live = o.live.then(|| Live { run_id: run_id.clone() });
    if let Some(l) = &live {
        let vm_host = secrets::load_values(&["VM_HOST"]).get("VM_HOST").cloned().flatten();
        match vm_host {
            Some(h) => eprintln!("[live: https://{h}.sslip.io/opencode-log/{run_id}]"),
            None => eprintln!("[live: runId {run_id} -- could not resolve VM_HOST to print the dashboard URL]"),
        }
        let mut start = serde_json::Map::new();
        start.insert("runId".into(), serde_json::json!(run_id));
        start.insert("prompt".into(), serde_json::json!(prompt));
        start.insert("caller".into(), serde_json::json!(caller));
        if let Some(s) = &session {
            start.insert("claudeSessionId".into(), serde_json::json!(s));
        }
        if let Some(m) = &o.start_model {
            start.insert("model".into(), serde_json::json!(m));
        }
        l.post("start", Value::Object(start));
    }
    let under_test = std::env::var("VITEST").as_deref() == Ok("true");
    let ladders = model_ladders();

    let overrides = opencode_env_overrides(&|k| std::env::var(k).ok());
    let env_of = |k: &str| overrides.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).or_else(|| std::env::var(k).ok());

    let root = repo_root_from(Path::new(&cwd));
    let run_log_path = std::env::var("OPENCODE_DISPATCH_RUN_LOG").unwrap_or_else(|_| format!("{}/.agents/tmp/opencode-dispatch-runs.jsonl", root.display()));
    let local_path = std::env::var("OPENCODE_POOL_LEDGER").unwrap_or_else(|_| format!("{}/.agents/tmp/pool-cooldowns.json", root.display()));
    let local = FileLedgerStore::new(&local_path);
    let store = local.read();

    let ladder = o.pins.clone().unwrap_or_else(|| rotation_starting_from(o.start_model.as_deref()));
    let heartbeat_ms = std::env::var("WF_HEARTBEAT_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(60_000);
    let mut host = Host {
        run_id: run_id.clone(),
        cwd: cwd.clone(),
        auto: o.auto,
        env: overrides.clone(),
        limits: Limits {
            idle: Duration::from_millis(o.idle_ms.unwrap_or(DEFAULT_IDLE_TIMEOUT_MS)),
            absolute: o.timeout_ms.map(Duration::from_millis),
            heartbeat: o.heartbeat.then(|| Duration::from_millis(heartbeat_ms)),
        },
        agent: o.agent.clone(),
        prompt: prompt.clone(),
        store,
        local,
        local_path: local_path.clone(),
        run_log: FileRunLog::new(&run_log_path),
        run_log_path: run_log_path.clone(),
        details: HashMap::new(),
        last_diff: None,
        live: live.clone(),
    };

    let cold = cold_pools(&host.store, now_ms());
    let pool_of = |m: &str| rca_ladder::ladder::pool_of(&ladders, m).to_string();
    let mut skipped: HashSet<String> = HashSet::new();
    if !under_test && ladder.iter().any(|m| pool_of(m) == PREFLIGHT_POOL) && !cold.contains(PREFLIGHT_POOL) {
        if let Preflight::Cap(retry) = preflight(env_of("OPENCODE_API_KEY").as_deref()) {
            let next = apply_preflight_cap(&host.store, PREFLIGHT_POOL, retry, now_ms());
            host.write_ledger(&next);
            mcp("info", SOURCE, "pool capped (preflight)", &[("pool", PREFLIGHT_POOL.into()), ("retryAfterMs", retry.to_string())]);
            skipped.insert(PREFLIGHT_POOL.to_string());
        }
    }
    let runnable: Vec<String> = ladder.iter().filter(|m| !skipped.contains(&pool_of(m))).cloned().collect();
    // A rung on the Claude pool runs through the Agent SDK, which only the TypeScript can do; the rotation never has one.
    if runnable.iter().any(|m| pool_of(m) == "claude-max") {
        return None;
    }

    let input = RunInput {
        run_id: run_id.clone(),
        cwd: cwd.clone(),
        writes_expected: o.auto,
        prompt: prompt.clone(),
        shape: o.shape.clone(),
        pins: runnable,
        honor_cold: false,
        caller: caller.clone(),
        tier: None,
        claude_session_id: session.clone(),
        run_started_at: started_at,
    };
    let outcome = run_once(&mut host, &input, &ladders);
    let duration = (now_ms() - started_at).max(0) as u64;
    let result = build_result(&outcome, &host, o, Finish { ladder_len: ladder.len(), output_file: output_file.as_deref(), caller: &caller, session: session.as_deref(), duration_ms: duration });
    if let Some(l) = &live {
        let mut fin = serde_json::Map::new();
        fin.insert("ok".into(), serde_json::json!(result.ok));
        if let Some(m) = &result.model {
            fin.insert("model".into(), serde_json::json!(m));
        }
        if let Some(t) = &result.summary.tokens {
            fin.insert("tokens".into(), serde_json::json!({ "total": jsn(t.total), "input": jsn(t.input), "output": jsn(t.output) }));
        }
        if let Some(c) = result.summary.cost {
            fin.insert("cost".into(), serde_json::json!(c));
        }
        fin.insert("durationMs".into(), serde_json::json!(duration));
        l.post("finish", Value::Object(fin));
    }
    if result.ok {
        let mut fields = vec![("model", result.model.clone().unwrap_or_default()), ("runId", run_id.clone())];
        if let Some(t) = &result.summary.tokens {
            fields.push(("tokens", js_num(t.total)));
            fields.push(("cost", js_num(result.summary.cost.unwrap_or(0.0))));
        }
        mcp("info", SOURCE, "served", &fields);
    }
    Some((result, run_id))
}

pub(crate) fn run(args: &[String]) -> Option<i32> {
    let o = parse_options(args)?;
    if o.no_auto {
        wf_kit::wf_err!("wf: --no-auto is gone: `opencode run` has no such option. Without --auto what opencode would ask is rejected and edits and commands are allowed; for a run that must not change files, use a read-only agent (--agent read-only)");
        return Some(2);
    }
    if o.fork {
        wf_kit::wf_err!("wf: --fork is not supported by the rca service yet (continue the session with -c or -s, or start a new one)");
        return Some(2);
    }
    let prompt = resolve_prompt(&o)?;
    load_all_into_process_env();
    let summary_mode = o.summary.as_deref();
    let (result, _) = dispatch_one(&o, prompt);
    if !result.ok && o.json {
        // as `opencode run --format json` on a failure: one error event, on stdout
        let event = serde_json::json!({ "type": "error", "timestamp": now_ms(), "sessionID": result.summary.session, "error": { "name": "UnknownError", "data": { "message": result.output } } });
        out(&format!("{event}\n"));
    } else if !result.ok {
        eprintln!("\x1b[91m\x1b[1mError: \x1b[0m{}", result.output);
    } else if o.json {
        // As `opencode run --format json`: one event per line on stdout.
        for event in &result.events {
            out(&format!("{event}\n"));
        }
    } else if let Some(mode) = summary_mode {
        out(&format!("{}\n", format_dispatch_summary(&result.summary, mode == "brief")));
    } else {
        // As `opencode run`: the header and the tool trace on stderr, each finished text part and a newline on stdout.
        if !result.events.is_empty() {
            eprint!("{}", crate::run_format::trace(&result.events, o.agent.as_deref().unwrap_or("build"), result.model.as_deref().unwrap_or_default()));
        }
        let parts = crate::run_format::text_parts(&result.events);
        if parts.is_empty() {
            out(&format!("{}\n", result.output));
        }
        for part in parts {
            out(&format!("{part}\n"));
        }
    }
    // What the service served the run with, for the callers that read it (stderr: stdout is `opencode run`'s).
    if let (true, Some(model)) = (result.ok, &result.model) {
        let mut meta = vec![format!("served by {model}")];
        if let Some(t) = &result.summary.tokens {
            meta.push(format!("tokens={}", js_num(t.total)));
        }
        if let Some(c) = result.summary.cost {
            meta.push(format!("cost=${}", rca_core::stats::to_fixed(c, 4)));
        }
        if let Some(s) = &result.summary.session {
            meta.push(format!("session {s}"));
        }
        if let Some(p) = &result.summary.transcript_path {
            meta.push(format!("transcript {p}"));
        }
        eprintln!("[{}]", meta.join(" | "));
    }
    if result.model.is_none() {
        let list: Vec<String> = result.attempts.iter().map(|(m, capped, _)| format!("{m}{}", if *capped { " (capped)" } else { "" })).collect();
        eprintln!("[failed after: {}]", list.join(", "));
    }
    Some(if result.ok { 0 } else { 1 })
}

/// The runbook driver on the opencode rungs of the `runbook` ladder: Ok is the agent's final text, Err names the rungs that
/// failed. `None` for a shape it does not serve.
pub(crate) fn dispatch_runbook(prompt: String) -> Option<Result<String, String>> {
    let ladders = model_ladders();
    let rungs: Vec<String> = ladders.ladders.get("runbook")?.iter().filter(|m| rca_ladder::ladder::pool_of(&ladders, m) != "claude-max").cloned().collect();
    let o = Options {
        prompt: None,
        prompt_file: None,
        positional: Vec::new(),
        summary: None,
        agent: None,
        output_file: None,
        caller: Some("runbook".into()),
        cwd: None,
        auto: true,
        heartbeat: false,
        idle_ms: Some(900_000),
        timeout_ms: Some(1_500_000),
        start_model: None,
        shape: Some("runbook".into()),
        pins: Some(rungs),
        live: false,
        files: Vec::new(),
        session: None,
        cont: false,
        attach: Vec::new(),
        command: None,
        fork: false,
        no_auto: false,
        json: false,
        variant: None,
        title: None,
    };
    load_all_into_process_env();
    let (d, _) = dispatch_local(&o, prompt)?;
    Some(if d.ok {
        Ok(d.output)
    } else {
        let trail: Vec<String> = d.attempts.iter().map(|(m, _, reason)| format!("{m} ({reason})")).collect();
        let tail: String = d.output.chars().rev().take(500).collect::<Vec<_>>().into_iter().rev().collect();
        Err(format!("opencode dispatch failed after {}: {tail}", trail.join(", ")))
    })
}

/// What a read-only caller needs of one dispatch.
pub struct ReadOnlyDispatch {
    pub ok: bool,
    pub model: Option<String>,
    pub output: String,
    pub attempts: Vec<(String, bool, String)>,
    pub cost: Option<f64>,
    pub tokens: Option<f64>,
}

/// One read-only prompt on the rotation (no file edits), answered by the cheapest rung that works; its stats carry `caller`.
pub fn dispatch_read_only(prompt: String, idle_ms: u64, caller: &str) -> Option<ReadOnlyDispatch> {
    load_all_into_process_env();
    let o = Options {
        prompt: None,
        prompt_file: None,
        positional: Vec::new(),
        summary: None,
        agent: None,
        output_file: None,
        caller: Some(caller.into()),
        cwd: None,
        auto: false,
        heartbeat: false,
        idle_ms: Some(idle_ms),
        timeout_ms: None,
        start_model: None,
        shape: None,
        pins: None,
        live: false,
        files: Vec::new(),
        session: None,
        cont: false,
        attach: Vec::new(),
        command: None,
        fork: false,
        no_auto: false,
        json: false,
        variant: None,
        title: None,
    };
    let (d, _) = dispatch_one(&o, prompt);
    let (cost, tokens) = (d.summary.cost, d.summary.tokens.as_ref().map(|t| t.total));
    Some(ReadOnlyDispatch { ok: d.ok, model: d.model, output: d.output, attempts: d.attempts, cost, tokens })
}

/// What `--batch` needs of one finished file.
pub(crate) struct BatchItem {
    pub ok: bool,
    pub duration_ms: f64,
    pub cost: f64,
    pub tokens: f64,
    pub diff_stat: Option<String>,
}

/// One file's prompt on the rotation, as the batch caller: auto mode, no heartbeat, transcript at `output_file`.
pub(crate) fn dispatch_for_batch(prompt: String, cwd: &str, file: &str, output_file: String, idle_ms: Option<u64>, start_model: Option<String>) -> BatchItem {
    let o = Options {
        prompt: None,
        prompt_file: None,
        positional: Vec::new(),
        summary: None,
        agent: None,
        output_file: Some(output_file),
        caller: Some("batch".into()),
        cwd: Some(cwd.to_string()),
        auto: true,
        heartbeat: false,
        idle_ms,
        timeout_ms: None,
        start_model,
        shape: Some("implementer:unit".into()),
        pins: None,
        live: false,
        files: vec![file.to_string()],
        session: None,
        cont: false,
        attach: Vec::new(),
        command: None,
        fork: false,
        no_auto: false,
        json: false,
        variant: None,
        title: None,
    };
    let (d, _) = dispatch_one(&o, prompt);
    BatchItem {
        ok: d.ok,
        duration_ms: d.summary.duration_ms,
        cost: d.summary.cost.unwrap_or(0.0),
        tokens: d.summary.tokens.as_ref().map_or(0.0, |t| t.total),
        diff_stat: d.summary.diff_stat.clone(),
    }
}
