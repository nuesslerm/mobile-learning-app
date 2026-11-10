//! The ladder's `RunHost` for one run: an attempt is a turn against opencode (or another CLI's turn through it) whose tool calls are folded
//! into events, plus the diff of what the attempt wrote. The cooldown ledger, the run log and the process and file work go through the
//! ports a `HostSetup` carries.

use rca_core::opencode::ToolEvent;
use rca_ladder::pool_ledger::Ledger;
use rca_ladder::run::{AttemptMetrics, AttemptOutput, RunHost};
use rca_ladder::store::CooldownStore;
use rca_service_core::application::log;
use rca_service_core::application::one_shot::{self, Captured, Failure, ToolCall, Turn};
use rca_service_core::application::opencode_client::Client;
use rca_service_core::application::runs::{AttemptHost, HostSetup, RunHosts};
use rca_service_core::domain::{clis, models};
use remote_chat_agent_core::application::ports::{Commands, WorkspaceFiles};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::runtime::Handle;

const DIFF_STAT_TIMEOUT: Duration = Duration::from_secs(10);

/// The `RunHosts` of the service: every run gets a `Host` over the setup's ports.
pub struct Hosts;

impl RunHosts for Hosts {
    fn open<'a>(&self, setup: HostSetup<'a>) -> Box<dyn AttemptHost + 'a> {
        Box::new(Host::new(setup))
    }
}

fn split_model(model: &str) -> Option<(String, String)> {
    let slash = model.find('/').filter(|&i| i >= 1)?;
    Some((model[..slash].to_string(), model[slash + 1..].to_string()))
}

/// Which CLI a model runs on: the one the request pinned, else the default when it takes the model, else claude for an anthropic model
/// and opencode for any other.
fn cli_for_model(model: &str, pinned: Option<&str>, agent_cli: Option<&str>) -> &'static str {
    let default = clis::selected(agent_cli);
    let pick = |id: &str| clis::ALL.iter().map(|(known, _)| *known).find(|known| *known == id).unwrap_or("opencode");
    if let Some(pinned) = pinned {
        return pick(pinned);
    }
    match split_model(model) {
        Some((provider, _)) if !models::compatible(default, &provider) => {
            if provider == "anthropic" {
                "claude"
            } else {
                "opencode"
            }
        }
        _ => default,
    }
}

/// A turn's tool calls as the events a run reports: the snapshots of one call merged (the last name and state, the arguments of the
/// last snapshot that had any), names in lower case, `filechange` as `patch` with one event per changed path, and a `filePath` in every
/// input that names a file.
fn tool_events(calls: &[ToolCall]) -> Vec<ToolEvent> {
    let mut order: Vec<String> = Vec::new();
    let mut merged: HashMap<String, ToolCall> = HashMap::new();
    let mut anonymous = 0;
    for call in calls.iter().filter(|c| c.call_id.is_some() || c.args.is_some()) {
        let key = call.call_id.clone().unwrap_or_else(|| {
            anonymous += 1;
            format!("anonymous:{anonymous}")
        });
        let args = call.args.clone().or_else(|| merged.get(&key).and_then(|prev| prev.args.clone())).or_else(|| Some(Map::new()));
        if !merged.contains_key(&key) {
            order.push(key.clone());
        }
        merged.insert(key, ToolCall { args, ..call.clone() });
    }
    let mut events = Vec::new();
    for key in order {
        let call = &merged[&key];
        let status = match call.state {
            "done" => "completed",
            "failed" => "error",
            _ => "other",
        };
        let args = call.args.clone().unwrap_or_default();
        let lower = call.name.to_lowercase();
        let tool = if lower == "filechange" { "patch".to_string() } else { lower.clone() };
        if lower == "filechange" {
            if let Some(changes) = args.get("changes").and_then(Value::as_array) {
                for change in changes {
                    if let Some(path) = change.get("path").filter(|p| p.is_string()) {
                        let mut input = args.clone();
                        input.insert("filePath".into(), path.clone());
                        events.push(ToolEvent { tool: tool.clone(), status, input, error: None });
                    }
                }
                continue;
            }
        }
        let file_path = ["filePath", "file_path"].iter().find_map(|k| args.get(*k).filter(|v| !v.is_null())).cloned();
        let mut input = args;
        if let Some(path) = file_path {
            input.insert("filePath".into(), path);
        }
        events.push(ToolEvent { tool, status, input, error: None });
    }
    events
}

fn touched_files(events: &[ToolEvent]) -> Vec<String> {
    let mut seen = HashSet::new();
    events
        .iter()
        .filter(|e| matches!(e.tool.as_str(), "edit" | "write" | "patch" | "multiedit") && e.status == "completed")
        .filter_map(|e| e.input.get("filePath").and_then(Value::as_str))
        .filter(|p| seen.insert((*p).to_string()))
        .map(String::from)
        .collect()
}

/// `git diff --stat` of the files a run wrote, unscoped when none was reported so a change made through `bash` still shows up.
async fn diff_stat(commands: &dyn Commands, cwd: &str, files: &[String]) -> Option<String> {
    let mut args = vec!["-C", cwd, "diff", "--stat"];
    if !files.is_empty() {
        args.push("--");
        args.extend(files.iter().map(String::as_str));
    }
    match commands.run("git", &args, None, Some(DIFF_STAT_TIMEOUT)).await {
        Ok(out) => Some(out.stdout.trim().to_string()),
        Err(e) => {
            log::warn!("gitDiffStat.failed", cwd = cwd, err = e.message);
            None
        }
    }
}

struct Host<'a> {
    /// The runtime the blocking run calls back into for its network and process work.
    runtime: &'a Handle,
    client: &'a Client,
    commands: &'a dyn Commands,
    fs: &'a dyn WorkspaceFiles,
    prompt: String,
    cwd: String,
    pinned_cli: Option<String>,
    /// `AGENT_CLI`: the CLI a model of no known provider runs on.
    agent_cli: Option<String>,
    ledger: Box<dyn CooldownStore>,
    run_log_path: PathBuf,
    /// The diff of each attempt that ran.
    diffs: HashMap<usize, String>,
    last_attempt: Option<usize>,
    session: Option<String>,
    agent: Option<String>,
    variant: Option<String>,
    title: Option<String>,
    auto: bool,
    attach: Vec<String>,
    command: Option<String>,
    /// The opencode session and the token usage of each attempt that answered.
    sessions: HashMap<usize, String>,
    usages: HashMap<usize, one_shot::Usage>,
    /// The turn of each attempt as `opencode run --format json` prints it.
    events: HashMap<usize, Vec<Value>>,
}

/// `opencode run -f`: each staged file as a file part of the message, by its `file://` url and its media type.
fn file_parts(cwd: &str, attach: &[String]) -> Vec<Value> {
    attach
        .iter()
        .map(|path| {
            let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
            let mime = match Path::new(path).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("png") => "image/png",
                Some("jpg" | "jpeg") => "image/jpeg",
                Some("gif") => "image/gif",
                Some("webp") => "image/webp",
                Some("pdf") => "application/pdf",
                _ => "text/plain",
            };
            json!({ "type": "file", "mime": mime, "filename": name, "url": format!("file://{}", Path::new(cwd).join(path).display()) })
        })
        .collect()
}

/// The session's permission rules, as `opencode run` creates its session: nobody is there to answer a question or to plan, so those
/// are denied. What opencode would ask (env files, other directories, repeated calls) is answered when it asks: approved with `--auto`,
/// rejected without it.
fn permission_rules() -> Value {
    json!([{ "permission": "question", "action": "deny", "pattern": "*" }, { "permission": "plan_enter", "action": "deny", "pattern": "*" }, { "permission": "plan_exit", "action": "deny", "pattern": "*" }])
}

fn now() -> i64 {
    log::now_ms() as i64
}

impl RunHost for Host<'_> {
    fn now(&mut self) -> i64 {
        now()
    }

    fn read_ledger(&mut self) -> Ledger {
        self.ledger.read()
    }

    fn write_ledger(&mut self, ledger: &Ledger) {
        if let Err(e) = self.ledger.write(ledger) {
            log::warn!("runOnce.poolLedgerWriteFailed", err = e.to_string());
        }
    }

    fn append_run_log(&mut self, entry: &Map<String, Value>) {
        let mut line = Value::Object(entry.clone()).to_string();
        line.push('\n');
        if let Err(e) = self.fs.append(&self.run_log_path, line.as_bytes()) {
            log::warn!("runOnce.runLogAppendFailed", path = self.run_log_path.to_string_lossy(), err = e);
        }
    }

    fn diff_stat(&mut self, _cwd: &str, _touched: &[String]) -> Option<String> {
        self.last_attempt.and_then(|i| self.diffs.get(&i).cloned())
    }

    fn bad_output(&mut self, _text: &str) -> bool {
        false
    }

    fn attempt(&mut self, index: usize, model: &str) -> AttemptOutput {
        self.last_attempt = Some(index);
        let cli = cli_for_model(model, self.pinned_cli.as_deref(), self.agent_cli.as_deref());
        let mut captured = Captured::default();
        let mut error = None;
        if !clis::is_available(cli) {
            let available = clis::available();
            let mut rebuild: Vec<&str> = available.clone();
            if !rebuild.contains(&cli) {
                rebuild.push(cli);
            }
            error = Some((
                "crash".to_string(),
                format!("Agent CLI \"{cli}\" is not available in this image (its dependency was not installed). Available: {}. Rebuild with --build-arg AGENT_CLIS={} to include it.", available.join(", "), rebuild.join(",")),
            ));
        } else {
            let permission = Some(permission_rules());
            let turn = Turn { cwd: self.cwd.clone(), model: split_model(model), session_id: self.session.clone(), agent: self.agent.clone(), variant: self.variant.clone(), title: self.title.clone(), permission, file_parts: file_parts(&self.cwd, &self.attach), command: self.command.clone(), approve_asks: self.auto, ..Turn::default() };
            match self.runtime.block_on(one_shot::run_capturing(self.client, &self.prompt, &turn, None, &mut captured)) {
                Ok(reply) => {
                    self.sessions.insert(index, reply.session_id);
                    self.events.insert(index, std::mem::take(&mut captured.events));
                    if let Some(usage) = reply.usage {
                        self.usages.insert(index, usage);
                    }
                }
                Err(failure) => {
                    let message = failure.message();
                    log::warn!("runOnce.turnFailed", cli = cli, attempt = index, message = message);
                    let class = if matches!(failure, Failure::Stall { .. }) { "timeout" } else { "crash" };
                    error = Some((class.to_string(), message));
                }
            }
        }
        let events = tool_events(&captured.tools);
        if let Some(stat) = self.runtime.block_on(diff_stat(self.commands, &self.cwd, &touched_files(&events))).filter(|s| !s.is_empty()) {
            self.diffs.insert(index, stat);
        }
        AttemptOutput { text: captured.text, tool_events: events, structured: true, error }
    }

    fn metrics(&mut self, _index: usize) -> Option<AttemptMetrics> {
        None
    }
}

impl<'a> Host<'a> {
    fn new(setup: HostSetup<'a>) -> Self {
        Host {
            runtime: setup.runtime,
            client: setup.client,
            commands: setup.commands,
            fs: setup.fs,
            prompt: setup.prompt,
            cwd: setup.cwd,
            pinned_cli: setup.pinned_cli,
            agent_cli: setup.agent_cli,
            ledger: setup.ledger,
            run_log_path: setup.run_log_path,
            diffs: HashMap::new(),
            last_attempt: None,
            session: setup.session,
            agent: setup.agent,
            variant: setup.variant,
            title: setup.title,
            auto: setup.auto,
            attach: setup.attach,
            command: setup.command,
            sessions: HashMap::new(),
            usages: HashMap::new(),
            events: HashMap::new(),
        }
    }
}

impl AttemptHost for Host<'_> {
    fn diff(&self, attempt: usize) -> Option<String> {
        self.diffs.get(&attempt).cloned()
    }

    fn session(&self, attempt: usize) -> Option<String> {
        self.sessions.get(&attempt).cloned()
    }

    fn events(&self, attempt: usize) -> Option<Vec<Value>> {
        self.events.get(&attempt).cloned()
    }

    fn usage(&self, attempt: usize) -> Option<one_shot::Usage> {
        self.usages.get(&attempt).cloned()
    }
}
