//! Orca's terminal rows over rca's terminal group: `terminal.list`, `send`, `close`, `clearBuffer` and `rename`. An Orca terminal handle is
//! `<workspace>:<terminal>` (a terminal's name never holds a colon), a worktree selector names the workspace to list, and nothing here touches a
//! pty. The streaming rows (`terminal.subscribe`, replay by byte offset) need the port to carry events and stay pending.
use agent_api_proto::Method as AgentMethod;
use agent_api_terminal::{code, CLEAR, CLOSE, LIST, RENAME, WRITE};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{Area, AgentApi, AgentError, Ctx, Kind, Method};
use serde_json::{json, Value};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const TERMINAL_LIST: &str = "terminal.list";
pub const TERMINAL_SEND: &str = "terminal.send";
pub const TERMINAL_CLOSE: &str = "terminal.close";
pub const TERMINAL_CLEAR_BUFFER: &str = "terminal.clearBuffer";
pub const TERMINAL_RENAME: &str = "terminal.rename";

/// The agent methods this crate sends: a flow test holds the list to the methods really sent.
pub const USES: &[AgentMethod] = &[LIST, WRITE, CLOSE, CLEAR, RENAME];

const METHODS: &[Method] = &[
    Method { name: TERMINAL_LIST, kind: Kind::Unary },
    Method { name: TERMINAL_SEND, kind: Kind::Unary },
    Method { name: TERMINAL_CLOSE, kind: Kind::Unary },
    Method { name: TERMINAL_CLEAR_BUFFER, kind: Kind::Unary },
    Method { name: TERMINAL_RENAME, kind: Kind::Unary },
];

const INTERRUPT: &str = "\u{3}";

pub struct TerminalArea;

impl Area for TerminalArea {
    fn name(&self) -> &'static str {
        "terminal"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        let agent = ctx.agent;
        match method {
            TERMINAL_LIST => list(agent, params),
            TERMINAL_SEND => send(agent, params),
            TERMINAL_CLOSE => close(agent, params),
            TERMINAL_CLEAR_BUFFER => clear(agent, params),
            TERMINAL_RENAME => rename(agent, params),
            other => Err(Failure::new(orca_core::wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}

fn refusal(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { code, message } => match code.as_str() {
            code::UNKNOWN_TERMINAL => Failure::new("terminal_handle_stale", "terminal_handle_stale"),
            code::TERMINAL_EXITED => Failure::new("terminal_not_writable", "terminal_not_writable"),
            "UNKNOWN_WORKSPACE" => Failure::new("selector_not_found", "selector_not_found"),
            "INVALID_PARAMS" | "INVALID_ARGS" => Failure::invalid(message),
            _ => Failure::new(RUNTIME_ERROR, message),
        },
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

fn ask(agent: &dyn AgentApi, method: AgentMethod, params: Value) -> Result<Value, Failure> {
    agent.request(method.name(), params).map_err(refusal)
}

fn handle_of(workspace: &str, terminal: &str) -> String {
    format!("{workspace}:{terminal}")
}

/// The workspace and terminal a handle names.
fn parse(params: &Value) -> Result<(String, String), Failure> {
    let handle = params.get("terminal").and_then(Value::as_str).filter(|h| !h.is_empty()).ok_or_else(|| Failure::invalid("Missing terminal handle"))?;
    match handle.rsplit_once(':') {
        Some((w, t)) if !w.is_empty() && !t.is_empty() => Ok((w.to_string(), t.to_string())),
        _ => Err(Failure::new("terminal_handle_stale", "terminal_handle_stale")),
    }
}

fn addressed(workspace: &str, terminal: &str) -> Value {
    json!({"workspace": workspace, "terminal": terminal})
}

fn row(target: &orca_map_workspace::Target, t: &Value) -> Value {
    let name = t["terminal"].as_str().unwrap_or_default();
    let running = t["status"] == "running";
    json!({
        "handle": handle_of(&target.workspace, name),
        "worktreeId": target.id,
        "worktreePath": target.root_path,
        "title": t["title"],
        "connected": running,
        "writable": running,
        "lastOutputAt": t["updated_at"],
        "preview": "",
    })
}

fn list(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let selected = params.get("worktree").and_then(Value::as_str).filter(|s| !s.is_empty());
    let targets = match selected {
        Some(selector) => vec![orca_map_workspace::resolve(agent, selector)?],
        None => orca_map_workspace::targets(agent)?,
    };
    let wanted: Option<Vec<&str>> = params.get("handles").and_then(Value::as_array).map(|h| h.iter().filter_map(Value::as_str).collect());
    let mut terminals = Vec::new();
    for target in &targets {
        let listed = ask(agent, LIST, json!({"workspace": target.workspace}))?;
        terminals.extend(listed["terminals"].as_array().into_iter().flatten().map(|t| row(target, t)));
    }
    if let Some(wanted) = wanted {
        terminals.retain(|t| wanted.contains(&t["handle"].as_str().unwrap_or_default()));
    }
    let total = terminals.len();
    let limit = params.get("limit").and_then(Value::as_u64).map_or(total, |l| l as usize);
    terminals.truncate(limit);
    Ok(json!({"hostScope": {"hostIds": ["local"], "omittedHostIds": []}, "terminals": terminals, "topologyRevisions": {}, "totalCount": total, "truncated": total > limit}))
}

fn send(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (workspace, terminal) = parse(params)?;
    let flag = |k: &str| params.get(k).is_some_and(|v| !v.is_null() && v != &json!(false));
    let mut data = String::new();
    if flag("interrupt") {
        data.push_str(INTERRUPT);
    }
    data.push_str(params.get("text").and_then(Value::as_str).unwrap_or_default());
    if flag("enter") {
        data.push('\r');
    }
    if data.is_empty() {
        return Err(Failure::invalid("Missing input: pass text, enter or interrupt"));
    }
    let mut request = addressed(&workspace, &terminal);
    request["data"] = json!(data);
    let written = ask(agent, WRITE, request)?;
    Ok(json!({"send": {"handle": handle_of(&workspace, &terminal), "accepted": true, "bytesWritten": written["bytes"]}}))
}

fn close(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (workspace, terminal) = parse(params)?;
    ask(agent, CLOSE, addressed(&workspace, &terminal))?;
    Ok(json!({"close": {"handle": handle_of(&workspace, &terminal), "closed": true}}))
}

/// The host words a clear of a handle it does not hold as a plain error, not as a stale handle.
fn clear(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let gone = |f: Failure| if f.code == "terminal_handle_stale" { Failure::new(RUNTIME_ERROR, "terminal_not_found") } else { f };
    let (workspace, terminal) = parse(params).map_err(gone)?;
    ask(agent, CLEAR, addressed(&workspace, &terminal)).map_err(gone)?;
    Ok(json!({"clear": {"handle": handle_of(&workspace, &terminal), "cleared": true}}))
}

/// A null or empty title resets to the terminal's own name, as the host's reset does.
fn rename(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let (workspace, terminal) = parse(params)?;
    let title = match params.get("title") {
        Some(Value::String(t)) if !t.is_empty() => t.clone(),
        Some(Value::String(_) | Value::Null) => terminal.clone(),
        _ => return Err(Failure::invalid("Missing --title (pass empty string or null to reset)")),
    };
    let mut request = addressed(&workspace, &terminal);
    request["title"] = json!(title);
    ask(agent, RENAME, request)?;
    Ok(json!({"rename": {"handle": handle_of(&workspace, &terminal), "title": title}}))
}
