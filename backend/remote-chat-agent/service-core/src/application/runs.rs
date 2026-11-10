//! One-shot runs (`use-cases/runOnce.ts`): a prompt put to the models of a ladder, one after the other, until one gives an answer that
//! passes the output checks. What decides which model is next, which pools are cold, how a failure is classed and what a run leaves in
//! the run log is `rca-core`, which `wf offload ask` shares; this module is the attempt (a turn against opencode, its tool calls folded
//! into events), the two files, the diff of what a run wrote, and the result as JSON.

use super::opencode_client::Client;
use crate::application::one_shot;
use remote_chat_agent_core::application::ports::Commands;
use super::log;
use super::settings::Settings;
use rca_core::run_log::{parse_run_log, RunLogEntry};
use rca_ladder::store::CooldownStore;
use remote_chat_agent_core::application::ports::{Ledgers, WorkspaceFiles};
use std::sync::Arc;
use rca_ladder::ladder::{model_ladders, resolve_ladder, LadderFile, Verdict};
use rca_ladder::pool_ledger::{cold_pools};
use rca_ladder::run::{run_once, AttemptOutput, AttemptRecord, RunHost, RunInput, RunOutcome};
use rca_core::stats::shape_stats;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::runtime::Handle;

pub const DEFAULT_SHAPE: &str = "implementer:unit";

/// A file a caller sends along with its prompt, for a run that has no checkout to read: a path relative to the staging directory.
pub struct AttachedFile {
    pub path: String,
    pub content: Vec<u8>,
}

/// Why a run did not start: the request cannot be staged, or the shared cooldown ledger cannot be used.
pub enum Refused {
    Invalid(String),
    Ledger(String),
}

const MAX_FILES: usize = 400;
const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;

const SESSION_MARKER: &str = ".rca-session";
/// Who asked for the session (the run's `caller`), so a caller can drop everything it made: `purge`.
const CALLER_MARKER: &str = ".rca-caller";
/// What the caller has of a session's directory after its last turn (path and digest of each file): the baseline of the next turn, so
/// a file a background task makes between turns comes back with that turn.
const BASELINE: &str = ".rca-baseline";

/// FNV-1a, stable between builds, since the baseline is read by a later one.
fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3))
}

/// How long the directory of a finished session is kept for a later turn: `RCA_SESSION_TTL_HOURS`, a day by default.
fn session_ttl(settings: &Settings) -> Duration {
    Duration::from_secs(settings.session_ttl_hours * 3600)
}

/// The directory of a run that named none: the attached files, in a place opencode shares with this service. It is removed when the
/// run ends, unless the run produced a session, whose later turns need the same directory: that one is kept, marked with the session
/// id, for `session_ttl`.
struct Staged {
    fs: Arc<dyn WorkspaceFiles>,
    dir: PathBuf,
    /// What each file held before the run, to tell what it changed.
    originals: HashMap<String, u64>,
    /// What the last `changes` saw, which is what the caller was given and so the next turn's baseline.
    delivered: HashMap<String, u64>,
    keep: bool,
}

fn within_limits(files: &[AttachedFile]) -> Result<(), String> {
    if files.len() > MAX_FILES || files.iter().map(|f| f.content.len()).sum::<usize>() > MAX_FILE_BYTES {
        return Err(format!("at most {MAX_FILES} files and {MAX_FILE_BYTES} bytes of content"));
    }
    Ok(())
}

impl Staged {
    fn create(fs: Arc<dyn WorkspaceFiles>, files: &[AttachedFile], settings: &Settings) -> Result<Self, String> {
        within_limits(files)?;
        sweep(fs.as_ref(), settings);
        let dir = settings.ask_root.join(crate::domain::util::uuid_v4());
        fs.create_dir(&dir).map_err(|e| format!("cannot stage the files: {e}"))?;
        let mut staged = Self { fs, dir, originals: HashMap::new(), delivered: HashMap::new(), keep: false };
        staged.lay(files)?;
        staged.originals = staged.snapshot().iter().map(|(path, bytes)| (path.clone(), digest(bytes))).collect();
        Ok(staged)
    }

    /// The directory `session` ran in, with `files` laid over it.
    fn resume(fs: Arc<dyn WorkspaceFiles>, session: &str, files: &[AttachedFile], settings: &Settings) -> Result<Self, String> {
        within_limits(files)?;
        let found = fs.list(&settings.ask_root).into_iter().find(|dir| read_text(fs.as_ref(), &dir.join(SESSION_MARKER)).is_some_and(|id| id.trim() == session));
        let dir = found.ok_or_else(|| format!("session {session} is unknown here or its files expired (kept for {} h)", session_ttl(settings).as_secs() / 3600))?;
        let mut staged = Self { fs, dir, originals: HashMap::new(), delivered: HashMap::new(), keep: true };
        staged.lay(files)?;
        // what the caller has: the last turn's delivered state (a directory from before the baseline is taken as it stands) with the
        // files it sent now laid over it
        staged.originals = match read_text(staged.fs.as_ref(), &staged.dir.join(BASELINE)).and_then(|t| serde_json::from_str::<HashMap<String, u64>>(&t).ok()) {
            Some(mut delivered) => {
                for file in files {
                    delivered.insert(file.path.clone(), digest(&file.content));
                }
                delivered
            }
            None => staged.snapshot().iter().map(|(path, bytes)| (path.clone(), digest(bytes))).collect(),
        };
        Ok(staged)
    }

    fn lay(&self, files: &[AttachedFile]) -> Result<(), String> {
        for file in files {
            let relative = std::path::Path::new(&file.path);
            if file.path.is_empty() || !relative.components().all(|c| matches!(c, std::path::Component::Normal(_))) || file.path == SESSION_MARKER || file.path == CALLER_MARKER || file.path == BASELINE {
                return Err(format!("file path {:?} must be relative and stay inside the directory", file.path));
            }
            let target = self.dir.join(relative);
            if let Some(parent) = target.parent() {
                self.fs.create_dir(parent).map_err(|e| format!("cannot stage {}: {e}", file.path))?;
            }
            self.fs.write(&target, &file.content).map_err(|e| format!("cannot stage {}: {e}", file.path))?;
        }
        Ok(())
    }

    /// Removes what opencode installed for the plugins and tools of the project (see `generated`): tens of megabytes a directory, kept
    /// for a day with the session, and installed again from the npm cache when a later turn boots the directory.
    fn prune_generated(&self) {
        for relative in [".opencode/node_modules", ".opencode/package.json", ".opencode/package-lock.json", ".opencode/bun.lock", ".opencode/bun.lockb", ".opencode/.gitignore"] {
            // a file the caller sent is theirs, and its removal would come back as a deletion
            if self.originals.contains_key(relative) {
                continue;
            }
            self.fs.remove(&self.dir.join(relative));
        }
    }

    /// Marks the directory as the session's and keeps it.
    fn remember(&mut self, session: &str, caller: &str) {
        if self.fs.write(&self.dir.join(SESSION_MARKER), session.as_bytes()).is_ok() {
            self.keep = true;
            let _ = self.fs.write(&self.dir.join(CALLER_MARKER), caller.as_bytes());
            let _ = self.fs.write(&self.dir.join(BASELINE), serde_json::to_string(&self.delivered).unwrap_or_default().as_bytes());
        }
    }

    fn snapshot(&self) -> HashMap<String, Vec<u8>> {
        self.fs
            .files_under(&self.dir)
            .into_iter()
            .filter_map(|(path, bytes)| Some((path.strip_prefix(&self.dir).ok()?.to_string_lossy().into_owned(), bytes)))
            .filter(|(name, _)| name != SESSION_MARKER && name != CALLER_MARKER && name != BASELINE)
            .collect()
    }

    /// The files the run created, rewrote or removed, for a caller that has the real tree: text as `content`, anything else as
    /// `contentBase64`, a removal as `deleted`. Skips what would not fit a reply.
    fn changes(&mut self) -> Vec<Value> {
        use base64::Engine;
        let found = self.snapshot();
        self.delivered = found.iter().map(|(path, bytes)| (path.clone(), digest(bytes))).collect();
        let mut changes = Vec::new();
        let mut budget = MAX_FILE_BYTES / 2;
        let mut paths: Vec<&String> = found.keys().chain(self.originals.keys()).collect();
        paths.sort();
        paths.dedup();
        for path in paths {
            match (found.get(path), self.originals.get(path)) {
                (Some(now), Some(before)) if digest(now) == *before => {}
                (Some(_), None) if generated(path) => {}
                (Some(now), _) if now.len() <= budget => {
                    budget -= now.len();
                    changes.push(match std::str::from_utf8(now) {
                        Ok(text) => json!({ "path": path, "content": text }),
                        Err(_) => json!({ "path": path, "contentBase64": base64::engine::general_purpose::STANDARD.encode(now) }),
                    });
                }
                (None, Some(_)) => changes.push(json!({ "path": path, "deleted": true })),
                // too big for the reply: not delivered, so not part of what the caller has
                (Some(_), _) => {
                    self.delivered.remove(path);
                }
                _ => {}
            }
        }
        changes
    }
}

/// What opencode makes in a project directory by itself: the dependencies of its plugins and tools and the files that install keeps.
/// They are not the run's work, and a `node_modules` would use up the reply's budget before a file that is.
fn generated(path: &str) -> bool {
    path.split('/').any(|part| part == "node_modules" || part == ".git") || matches!(path, ".opencode/.gitignore" | ".opencode/package.json" | ".opencode/bun.lock" | ".opencode/bun.lockb" | ".opencode/package-lock.json")
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.keep {
            self.fs.remove(&self.dir);
        }
    }
}

/// The opencode sessions whose kept directory is older than `ttl`. opencode keeps every session it served in memory until told to
/// drop it (4.8 GB on a 12 GB host after a day of CI), so a session that expired with its directory is deleted from the server too.
fn expired_sessions(fs: &dyn WorkspaceFiles, settings: &Settings) -> Vec<String> {
    let ttl = session_ttl(settings);
    fs.list(&settings.ask_root)
        .into_iter()
        .filter_map(|dir| {
            let marker = dir.join(SESSION_MARKER);
            let old = fs.age(&marker).is_some_and(|age| age > ttl);
            old.then(|| read_text(fs, &marker)).flatten().map(|id| id.trim().to_string()).filter(|id| session_id(id))
        })
        .collect()
}

fn read_text(fs: &dyn WorkspaceFiles, path: &Path) -> Option<String> {
    fs.read(path).and_then(|bytes| String::from_utf8(bytes).ok())
}

/// The rows of the run log, none when it cannot be read.
fn read_run_log(fs: &dyn WorkspaceFiles, path: &Path) -> Vec<RunLogEntry> {
    read_text(fs, path).map_or_else(Vec::new, |text| parse_run_log(&text))
}

/// The run log of runs that name no checkout: `OPENCODE_DISPATCH_RUN_LOG`, else one in the ask directory.
fn default_run_log(settings: &Settings) -> PathBuf {
    settings.dispatch_run_log.clone().unwrap_or_else(|| settings.ask_root.join("runs.jsonl"))
}

fn session_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Drops the sessions whose kept directory outlived the TTL from opencode, and says how many. A timer calls it as well as every run, so
/// an idle service lets go of the memory too.
pub fn reap(runtime: &Handle, client: &Client, fs: &dyn WorkspaceFiles) -> usize {
    expired_sessions(fs, &client.env().settings()).into_iter().filter(|session| runtime.block_on(client.request("DELETE", &format!("/session/{session}"), None)).is_ok()).count()
}

/// Drops every session `caller` made, from opencode and from the ask directory, and says how many: a test that is done with its
/// sessions says so instead of waiting for the expiry. An empty or unknown caller drops nothing.
pub fn purge(runtime: &Handle, client: &Client, fs: &dyn WorkspaceFiles, caller: &str) -> usize {
    if caller.is_empty() {
        return 0;
    }
    let mut dropped = 0;
    for dir in fs.list(&client.env().settings().ask_root) {
        let theirs = read_text(fs, &dir.join(CALLER_MARKER)).is_some_and(|c| c.trim() == caller);
        let session = read_text(fs, &dir.join(SESSION_MARKER)).map(|id| id.trim().to_string()).filter(|id| session_id(id));
        if let (true, Some(session)) = (theirs, session) {
            let _ = runtime.block_on(client.request("DELETE", &format!("/session/{session}"), None));
            fs.remove(&dir);
            dropped += 1;
        }
    }
    dropped
}

/// Removes the kept directories of sessions older than the TTL.
fn sweep(fs: &dyn WorkspaceFiles, settings: &Settings) {
    let ttl = session_ttl(settings);
    for dir in fs.list(&settings.ask_root) {
        if fs.age(&dir.join(SESSION_MARKER)).is_some_and(|age| age > ttl) {
            fs.remove(&dir);
        }
    }
}

pub struct Request {
    pub prompt: String,
    /// Absent: the run gets a staging directory holding `files`.
    pub cwd: Option<String>,
    pub files: Vec<AttachedFile>,
    pub shape: Option<String>,
    pub models: Vec<String>,
    pub cli: Option<String>,
    /// Who asked, for the run log's `caller` column (default `dispatch`).
    pub caller: Option<String>,
    /// An opencode session to continue: its staging directory (kept for a day) and its conversation, whichever model answers.
    pub session: Option<String>,
    /// An opencode agent defined in the server's config, which sets the tools the model may use.
    pub agent: Option<String>,
    /// The model variant (reasoning effort).
    pub variant: Option<String>,
    /// The title of the session the run creates.
    pub title: Option<String>,
    /// `opencode run --auto`: approve what opencode would ask (env files, other directories, repeated calls). False rejects those; edits and
    /// commands are allowed either way, and what a run changed comes back.
    pub auto: bool,
    /// Staged paths the prompt carries as file parts (`opencode run -f`): the model sees them without a tool call.
    pub attach: Vec<String>,
    /// `opencode run --command`: a project command run with the prompt as its arguments.
    pub command: Option<String>,
}

/// The nearest ancestor holding `.git`, or the directory itself outside a checkout.
fn repo_root_from(fs: &dyn WorkspaceFiles, from: &str) -> PathBuf {
    let mut dir = PathBuf::from(from);
    for _ in 0..20 {
        if fs.exists(&dir.join(".git")) {
            return dir;
        }
        match dir.parent() {
            Some(parent) if parent != dir => dir = parent.to_path_buf(),
            _ => break,
        }
    }
    PathBuf::from(from)
}

fn run_log_in(fs: &dyn WorkspaceFiles, settings: &Settings, cwd: &str, default: &str) -> PathBuf {
    settings.dispatch_run_log.clone().unwrap_or_else(|| repo_root_from(fs, cwd).join(default))
}

/// What a run hands the host that answers its attempts: everything one run reads, and nothing it opens itself.
pub struct HostSetup<'a> {
    /// The runtime the blocking run calls back into for its network and process work.
    pub runtime: &'a Handle,
    pub client: &'a Client,
    pub commands: &'a dyn Commands,
    pub fs: &'a dyn WorkspaceFiles,
    pub prompt: String,
    pub cwd: String,
    pub pinned_cli: Option<String>,
    /// `AGENT_CLI`: the CLI a model of no known provider runs on.
    pub agent_cli: Option<String>,
    pub ledger: Box<dyn CooldownStore>,
    pub run_log_path: PathBuf,
    pub session: Option<String>,
    pub agent: Option<String>,
    pub variant: Option<String>,
    pub title: Option<String>,
    pub auto: bool,
    pub attach: Vec<String>,
    pub command: Option<String>,
}

/// The ladder's `RunHost` plus what the result reports of each attempt that ran.
pub trait AttemptHost: RunHost {
    /// The `git diff --stat` after the attempt.
    fn diff(&self, attempt: usize) -> Option<String>;
    /// The opencode session of an attempt that answered.
    fn session(&self, attempt: usize) -> Option<String>;
    /// The turn as `opencode run --format json` prints it.
    fn events(&self, attempt: usize) -> Option<Vec<Value>>;
    fn usage(&self, attempt: usize) -> Option<one_shot::Usage>;
}

/// Opens the host of one run; `remote-chat-agent-host` implements it.
pub trait RunHosts: Send + Sync {
    fn open<'a>(&self, setup: HostSetup<'a>) -> Box<dyn AttemptHost + 'a>;
}

fn now() -> i64 {
    log::now_ms() as i64
}

fn verdict_json(verdict: &Verdict) -> Value {
    if verdict.ok {
        return json!({ "ok": true });
    }
    json!({ "ok": false, "failureClass": verdict.failure_class, "reason": verdict.reason })
}

fn attempt_json(attempt: &AttemptRecord) -> Value {
    let mut choice = json!({ "model": attempt.model });
    if let Some(pool) = &attempt.pool {
        choice["pool"] = json!(pool);
    }
    let mut out = json!({ "index": attempt.index, "choice": choice });
    if let Some((by, reason)) = &attempt.skipped {
        out["skipped"] = json!({ "by": by, "reason": reason });
    }
    if let Some(verdict) = &attempt.verdict {
        out["verdict"] = verdict_json(verdict);
    }
    out
}

fn output_json(output: &AttemptOutput, diff: Option<String>) -> Value {
    let mut out = json!({ "text": output.text, "toolEvents": output.tool_events, "structured": output.structured });
    if let Some(diff) = diff {
        out["diffStat"] = json!(diff);
    }
    if let Some((class, message)) = &output.error {
        out["error"] = json!({ "failureClass": class, "message": message });
    }
    out
}

fn result_json(outcome: &RunOutcome, host: &dyn AttemptHost) -> Value {
    let attempts: Vec<Value> = outcome.attempts.iter().map(attempt_json).collect();
    let last_ran = outcome.attempts.iter().rev().find(|a| a.skipped.is_none()).map(|a| a.index);
    let output = outcome.last_output.as_ref().map(|o| output_json(o, last_ran.and_then(|i| host.diff(i))));
    match (outcome.ok, outcome.ok_attempt, output) {
        (true, Some(ok), Some(output)) => json!({ "ok": true, "output": output, "attempt": attempt_json(&outcome.attempts[ok]), "attempts": attempts }),
        (_, _, output) => {
            let mut out = json!({ "ok": false, "attempts": attempts });
            if let Some(output) = output {
                out["lastOutput"] = output;
            }
            if outcome.ladder_exhausted {
                out["reason"] = json!("ladder-exhausted");
            }
            out
        }
    }
}

fn ladders() -> &'static LadderFile {
    static LADDERS: std::sync::OnceLock<LadderFile> = std::sync::OnceLock::new();
    LADDERS.get_or_init(model_ladders)
}

/// Runs the request to its end and answers with the result as `RunResult` is serialised.
pub fn run(runtime: &Handle, client: &Client, commands: &dyn Commands, ledgers: &dyn Ledgers, fs: &Arc<dyn WorkspaceFiles>, hosts: &dyn RunHosts, request: &Request) -> Result<Value, Refused> {
    reap(runtime, client, fs.as_ref());
    let settings = client.env().settings();
    let (cwd, mut staged) = match (&request.cwd, &request.session) {
        (Some(cwd), _) if request.files.is_empty() => (cwd.clone(), None),
        (Some(_), _) => return Err(Refused::Invalid("files are staged for a run that names no cwd".into())),
        (None, Some(session)) => {
            let staged = Staged::resume(Arc::clone(fs), session, &request.files, &settings).map_err(Refused::Invalid)?;
            (staged.dir.to_string_lossy().into_owned(), Some(staged))
        }
        (None, None) => {
            let staged = Staged::create(Arc::clone(fs), &request.files, &settings).map_err(Refused::Invalid)?;
            (staged.dir.to_string_lossy().into_owned(), Some(staged))
        }
    };
    let ladders = ladders();
    let shape = request.shape.clone().unwrap_or_else(|| DEFAULT_SHAPE.to_string());
    let run_log_path = if request.cwd.is_some() { run_log_in(fs.as_ref(), &settings, &cwd, ".agents/tmp/opencode-dispatch-runs.jsonl") } else { default_run_log(&settings) };
    let ledger_store = ledgers.open(runtime, &cwd).map_err(Refused::Ledger)?;
    let cooldowns = ledger_store.read();
    if let Some(why) = ledger_store.last_error() {
        return Err(Refused::Ledger(format!("the pool cooldowns could not be read: {why}")));
    }

    let pins = if request.models.is_empty() {
        // The ladder of the shape, ranked by how its models have fared, minus the pools that are cold.
        let stats = (!settings.ladder_ignore_stats).then(|| shape_stats(&read_run_log(fs.as_ref(), &run_log_path), &shape));
        let cold = cold_pools(&cooldowns, now()).into_iter().collect();
        resolve_ladder(&shape, ladders, &cold, stats.as_ref(), None)
    } else {
        request.models.clone()
    };
    let claude_session_id = settings.claude_session_id.clone();
    let input = RunInput {
        run_id: crate::domain::util::uuid_v4(),
        cwd: cwd.clone(),
        writes_expected: shape.starts_with("implementer:") && request.auto,
        prompt: request.prompt.clone(),
        shape: Some(shape),
        pins,
        // one model the caller pinned is the one asked for (`opencode run -m`): it is neither skipped for a cold pool nor allowed to cool one
        // for everyone else; a list of candidates, or none, is a rotation that leaves the cold pools out
        honor_cold: request.models.len() != 1,
        caller: request.caller.clone().unwrap_or_else(|| "dispatch".into()),
        tier: None,
        claude_session_id,
        run_started_at: now(),
    };
    let setup = HostSetup { runtime, client, commands, fs: fs.as_ref(), prompt: request.prompt.clone(), cwd: cwd.clone(), pinned_cli: request.cli.clone(), agent_cli: settings.agent_cli.clone(), ledger: ledger_store, run_log_path, session: request.session.clone(), agent: request.agent.clone(), variant: request.variant.clone(), title: request.title.clone(), auto: request.auto, attach: request.attach.clone(), command: request.command.clone() };
    let mut host = hosts.open(setup);
    let outcome = run_once(&mut *host, &input, ladders);
    let mut result = result_json(&outcome, &*host);
    let answered = outcome.ok_attempt.and_then(|i| host.session(i).map(|s| (i, s)));
    if let (Some((index, session)), Some(object)) = (&answered, result.as_object_mut()) {
        object.insert("session".into(), json!(session));
        if let Some(events) = host.events(*index) {
            object.insert("events".into(), Value::Array(events));
        }
        if let Some(usage) = host.usage(*index) {
            object.insert("usage".into(), json!({ "input": usage.input, "output": usage.output, "reasoning": usage.reasoning, "cacheRead": usage.cache_read, "cacheWrite": usage.cache_write }));
        }
    }
    if let (Some(staged), Some(object)) = (staged.as_mut(), result.as_object_mut()) {
        object.insert("changes".into(), Value::Array(staged.changes()));
        staged.prune_generated();
        if let Some((_, session)) = &answered {
            staged.remember(session, request.caller.as_deref().unwrap_or("dispatch"));
        }
    }
    Ok(result)
}

/// The run log as text, newest `limit` rows when asked, for the caller's `--stats` and `--savings`.
pub fn run_log(fs: &dyn WorkspaceFiles, settings: &Settings, limit: Option<usize>) -> String {
    let text = read_text(fs, &default_run_log(settings)).unwrap_or_default();
    match limit {
        Some(n) => {
            let lines: Vec<&str> = text.lines().collect();
            lines[lines.len().saturating_sub(n)..].join("\n")
        }
        None => text,
    }
}

/// Warms `pool` again in the shared ledger; false when it was not cold.
pub fn clear_cooldown(runtime: &Handle, ledgers: &dyn Ledgers, pool: &str) -> Result<bool, String> {
    let store = ledgers.open(runtime, "")?;
    let ledger = store.read();
    if let Some(why) = store.last_error() {
        return Err(format!("the pool cooldowns could not be read: {why}"));
    }
    let was_cold = ledger.get(pool).and_then(|e| rca_core::time::parse(&e.cold_until)).is_some_and(|until| until > now());
    let next = rca_ladder::pool_ledger::clear_cooldown(&ledger, pool, now());
    store.write(&next).map_err(|e| e.to_string())?;
    Ok(was_cold)
}
