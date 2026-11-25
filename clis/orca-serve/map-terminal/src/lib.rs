//! Orca's terminal rows over rca's terminal group: `terminal.list`, `send`, `close`, `clearBuffer` and `rename`. An Orca terminal handle is
//! `<workspace>:<terminal>` (a terminal's name never holds a colon), a worktree selector names the workspace to list, and nothing here touches a
//! pty except through `terminal.subscribe`, which reads the tail and follows rca's attach stream.
use agent_api_proto::Method as AgentMethod;
use agent_api_terminal::{b64, code, ATTACH, CLEAR, CLOSE, LIST, OPEN, READ, RENAME, RESIZE, WRITE};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{Area, AgentApi, AgentError, Ctx, Kind, Method};
use serde_json::{json, Value};

#[allow(dead_code)] // the stream units that follow call these
mod replay;
pub mod binary;
pub mod resume;
mod subscribe;
mod tabs;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const TERMINAL_LIST: &str = "terminal.list";
pub const TERMINAL_SEND: &str = "terminal.send";
pub const TERMINAL_CLOSE: &str = "terminal.close";
pub const TERMINAL_CLEAR_BUFFER: &str = "terminal.clearBuffer";
pub const TERMINAL_RENAME: &str = "terminal.rename";
pub const TERMINAL_UPDATE_VIEWPORT: &str = "terminal.updateViewport";
pub const TERMINAL_SET_DISPLAY_MODE: &str = "terminal.setDisplayMode";
pub const TERMINAL_ACK_OUTPUT: &str = "terminal.ackOutput";
pub const TERMINAL_FOCUS: &str = "terminal.focus";
pub const TERMINAL_READ: &str = "terminal.read";
pub const TERMINAL_SUBSCRIBE: &str = "terminal.subscribe";
pub const TERMINAL_UNSUBSCRIBE: &str = "terminal.unsubscribe";
pub const TERMINAL_OPEN_INPUT_SESSION: &str = "terminal.openInputSession";
pub use tabs::{SESSION_TABS_ACTIVATE, SESSION_TABS_CLOSE, SESSION_TABS_CREATE_TERMINAL, SESSION_TABS_LIST, SESSION_TABS_SUBSCRIBE, SESSION_TABS_UNSUBSCRIBE};

/// The agent methods this crate sends: a flow test holds the list to the methods really sent.
pub const USES: &[AgentMethod] = &[LIST, WRITE, CLOSE, CLEAR, RENAME, RESIZE, OPEN, READ, ATTACH, orca_map_state::STATE_SET, orca_map_workspace::layout::THREAD_LIST];

const METHODS: &[Method] = &[
    Method { name: TERMINAL_LIST, kind: Kind::Unary },
    Method { name: TERMINAL_SEND, kind: Kind::Unary },
    Method { name: TERMINAL_CLOSE, kind: Kind::Unary },
    Method { name: TERMINAL_CLEAR_BUFFER, kind: Kind::Unary },
    Method { name: TERMINAL_RENAME, kind: Kind::Unary },
    Method { name: TERMINAL_UPDATE_VIEWPORT, kind: Kind::Unary },
    Method { name: TERMINAL_SET_DISPLAY_MODE, kind: Kind::Unary },
    Method { name: TERMINAL_ACK_OUTPUT, kind: Kind::Unary },
    Method { name: TERMINAL_FOCUS, kind: Kind::Unary },
    Method { name: TERMINAL_READ, kind: Kind::Unary },
    Method { name: TERMINAL_OPEN_INPUT_SESSION, kind: Kind::Unary },
    Method { name: TERMINAL_SUBSCRIBE, kind: Kind::Streaming },
    Method { name: TERMINAL_UNSUBSCRIBE, kind: Kind::Unary },
    Method { name: SESSION_TABS_LIST, kind: Kind::Unary },
    Method { name: SESSION_TABS_ACTIVATE, kind: Kind::Unary },
    Method { name: SESSION_TABS_CLOSE, kind: Kind::Unary },
    Method { name: SESSION_TABS_CREATE_TERMINAL, kind: Kind::Unary },
    Method { name: SESSION_TABS_SUBSCRIBE, kind: Kind::Streaming },
    Method { name: SESSION_TABS_UNSUBSCRIBE, kind: Kind::Unary },
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

    fn open(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Box<dyn orca_core::Subscription>, Failure> {
        match method {
            SESSION_TABS_SUBSCRIBE => tabs::open(ctx, params),
            TERMINAL_SUBSCRIBE => subscribe::open(ctx, params),
            other => Err(Failure::new(orca_core::wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        let agent = ctx.agent;
        match method {
            SESSION_TABS_LIST => tabs::list(agent, params),
            SESSION_TABS_ACTIVATE => tabs::activate(agent, params),
            SESSION_TABS_CLOSE => tabs::close(agent, params),
            SESSION_TABS_CREATE_TERMINAL => tabs::create_terminal(agent, params),
            SESSION_TABS_UNSUBSCRIBE => tabs::unsubscribe(ctx, params),
            TERMINAL_LIST => list(agent, params),
            TERMINAL_SEND => send(agent, params),
            TERMINAL_CLOSE => close(agent, params),
            TERMINAL_CLEAR_BUFFER => clear(agent, params),
            TERMINAL_RENAME => rename(agent, params),
            TERMINAL_UPDATE_VIEWPORT => update_viewport(agent, params),
            TERMINAL_SET_DISPLAY_MODE => set_display_mode(agent, params),
            TERMINAL_ACK_OUTPUT => ack_output(ctx.device_id, params),
            TERMINAL_FOCUS => focus(agent, params),
            TERMINAL_READ => read(agent, params),
            TERMINAL_UNSUBSCRIBE => subscribe::unsubscribe(ctx, params),
            TERMINAL_OPEN_INPUT_SESSION => open_input_session(params),
            other => Err(Failure::new(orca_core::wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}

pub(crate) fn refusal(error: AgentError) -> Failure {
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

pub(crate) fn ask(agent: &dyn AgentApi, method: AgentMethod, params: Value) -> Result<Value, Failure> {
    agent.request(method.name(), params).map_err(refusal)
}

pub(crate) fn handle_of(workspace: &str, terminal: &str) -> String {
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

/// A send that guards on the terminal's state is not accepted, rather than refused, when no such terminal is open: the host's answer, no bytes written.
fn send(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    match deliver(agent, params) {
        Err(f) if f.code == "terminal_handle_stale" && ["requireAgentStatus", "requireStartupDialog", "expectedIncarnationId"].iter().any(|k| params.get(*k).is_some_and(|v| !v.is_null())) => {
            Ok(json!({"send": {"handle": params["terminal"], "accepted": false, "bytesWritten": 0}}))
        }
        sent => sent,
    }
}

fn deliver(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
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

const MAX_COLS: u64 = 1000;
const MAX_ROWS: u64 = 500;

fn handle_param(params: &Value) -> Result<(), Failure> {
    params.get("terminal").and_then(Value::as_str).filter(|h| !h.is_empty()).map(|_| ()).ok_or_else(|| Failure::invalid("Missing terminal handle"))
}

/// The `client` of a viewport call: an id, and a type of `mobile` or `desktop` when one is sent.
fn client_param(params: &Value, required: bool) -> Result<(), Failure> {
    match params.get("client") {
        None | Some(Value::Null) if !required => Ok(()),
        Some(Value::Object(c)) => {
            if c.get("id").and_then(Value::as_str).is_none_or(str::is_empty) {
                return Err(Failure::invalid("Missing client ID"));
            }
            match c.get("type") {
                None | Some(Value::Null) => Ok(()),
                Some(Value::String(t)) if t == "mobile" || t == "desktop" => Ok(()),
                _ => Err(Failure::invalid("Invalid option: expected one of \"mobile\"|\"desktop\"")),
            }
        }
        _ => Err(Failure::invalid("Invalid input: expected object, received undefined")),
    }
}

fn dimension(viewport: &Value, key: &str, min: u64, max: u64) -> Result<u64, Failure> {
    match viewport.get(key) {
        Some(Value::Number(n)) => match n.as_u64() {
            Some(v) if v < min => Err(Failure::invalid(format!("Too small: expected number to be >={min}"))),
            Some(v) if v > max => Err(Failure::invalid(format!("Too big: expected number to be <={max}"))),
            Some(v) => Ok(v),
            None => Err(Failure::invalid("Invalid input: expected int, received number")),
        },
        _ => Err(Failure::invalid("Invalid input: expected number, received undefined")),
    }
}

/// The viewport the phone fits a terminal to, as rca's pty can take it (rca stops at 1000 columns, the host at 1024).
fn viewport_param(params: &Value) -> Result<(u64, u64), Failure> {
    let viewport = params.get("viewport").filter(|v| v.is_object()).ok_or_else(|| Failure::invalid("Invalid input: expected object, received undefined"))?;
    Ok((dimension(viewport, "cols", 20, 1024)?.min(MAX_COLS), dimension(viewport, "rows", 8, MAX_ROWS)?))
}

/// The host answers a handle it holds no live pty for as `no_connected_pty`, whatever is wrong with the handle.
fn live(agent: &dyn AgentApi, params: &Value) -> Result<(String, String), Failure> {
    let gone = || Failure::new(RUNTIME_ERROR, "no_connected_pty");
    let (workspace, terminal) = parse(params).map_err(|_| gone())?;
    let listed = ask(agent, LIST, json!({"workspace": workspace})).map_err(|_| gone())?;
    let running = listed["terminals"].as_array().into_iter().flatten().any(|t| t["terminal"] == terminal.as_str() && t["status"] == "running");
    if running { Ok((workspace, terminal)) } else { Err(gone()) }
}

fn resize(agent: &dyn AgentApi, workspace: &str, terminal: &str, (cols, rows): (u64, u64)) -> Result<(), Failure> {
    let mut request = addressed(workspace, terminal);
    request["cols"] = json!(cols);
    request["rows"] = json!(rows);
    ask(agent, RESIZE, request).map(|_| ())
}

/// rca resizes its one pty, so the fit is always applied; it keeps no layout sequence, which the phone treats as optional.
fn update_viewport(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    handle_param(params)?;
    client_param(params, true)?;
    let size = viewport_param(params)?;
    let (workspace, terminal) = live(agent, params)?;
    resize(agent, &workspace, &terminal, size)?;
    Ok(json!({"updated": true, "applied": true}))
}

/// `auto` fits the pty to the phone; `desktop` has no desktop size to restore, so it only proves the terminal is live.
fn set_display_mode(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    handle_param(params)?;
    let mode = match params.get("mode").and_then(Value::as_str) {
        Some(m @ ("auto" | "desktop")) => m,
        _ => return Err(Failure::invalid("Invalid option: expected one of \"auto\"|\"desktop\"")),
    };
    client_param(params, false)?;
    let size = if params.get("viewport").is_some_and(|v| !v.is_null()) { Some(viewport_param(params)?) } else { None };
    let (workspace, terminal) = live(agent, params)?;
    if let (true, Some(size)) = (mode == "auto", size) {
        resize(agent, &workspace, &terminal, size)?;
    }
    Ok(json!({"mode": mode}))
}

/// Credit counts only for a live stream token of this device; an unknown or ended one answers false.
fn ack_output(device: &str, params: &Value) -> Result<Value, Failure> {
    let Some(token) = params.get("streamToken").and_then(Value::as_str).filter(|t| !t.is_empty()) else {
        return Err(Failure::invalid("Missing stream token"));
    };
    let units = match params.get("units") {
        Some(Value::Number(n)) => match n.as_i64() {
            Some(u) if u > 0 => u as u64,
            Some(_) => return Err(Failure::invalid("Too small: expected number to be >0")),
            None => return Err(Failure::invalid("Invalid input: expected int, received number")),
        },
        _ => return Err(Failure::invalid("Invalid input: expected number, received undefined")),
    };
    let credited = resume::registry().lock().unwrap_or_else(|e| e.into_inner()).ack(token, device, units);
    Ok(json!({"credited": credited}))
}

/// There is no desktop pane to reveal, so the answer says nothing was navigated.
fn focus(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    handle_param(params)?;
    if let Some(n) = params.get("navigation").filter(|n| !n.is_null()) {
        if !matches!(n.as_str(), Some("caller" | "host")) {
            return Err(Failure::invalid("Invalid option: expected one of \"caller\"|\"host\""));
        }
    }
    let (workspace, terminal) = parse(params)?;
    let stale = || Failure::new("terminal_handle_stale", "terminal_handle_stale");
    let target = orca_map_workspace::targets(agent)?.into_iter().find(|t| t.workspace == workspace).ok_or_else(stale)?;
    let listed = ask(agent, LIST, json!({"workspace": workspace}))?;
    if !listed["terminals"].as_array().into_iter().flatten().any(|t| t["terminal"] == terminal.as_str()) {
        return Err(stale());
    }
    Ok(json!({"focus": {"handle": handle_of(&workspace, &terminal), "tabId": terminal, "worktreeId": target.id, "navigated": false}}))
}

/// The phone falls back to unordered input on a null session, which `terminal.send` serves; no ordering ledger is kept.
fn open_input_session(params: &Value) -> Result<Value, Failure> {
    handle_param(params)?;
    let client = params.get("client").and_then(Value::as_object).ok_or_else(|| Failure::invalid("Invalid input: expected object, received undefined"))?;
    if client.get("id").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Err(Failure::invalid("Missing client ID"));
    }
    if client.get("type").and_then(Value::as_str) != Some("mobile") {
        return Err(Failure::invalid("Invalid input: expected \"mobile\""));
    }
    Ok(json!({"inputSessionId": null}))
}

/// Lines of at most this many are kept when the phone names no limit.
const DEFAULT_LINES: usize = 200;

/// The text of terminal bytes without its escape sequences (CSI, OSC and two-byte ones) and carriage returns, one string per line.
fn lines_of(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    for n in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' || (n == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {}
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    let mut lines: Vec<String> = out.split('\n').map(str::to_string).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// The last lines the service keeps of a terminal, from a cursor (a byte offset) when the phone holds one. A screen read has no screen to read.
fn read(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    handle_param(params)?;
    let cursor = match params.get("cursor") {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => Some(n.as_u64().ok_or_else(|| Failure::invalid("Invalid input: expected number, received NaN"))?),
        Some(_) => return Err(Failure::invalid("Invalid input: expected number, received NaN")),
    };
    let screen = match params.get("screen") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(true)) => true,
        Some(_) => return Err(Failure::invalid("Invalid input: expected true")),
    };
    if screen && cursor.is_some() {
        return Err(Failure::invalid("Cursor cannot be combined with a screen read"));
    }
    let limit = params.get("limit").and_then(Value::as_f64).filter(|l| l.is_finite() && *l >= 1.0).map_or(DEFAULT_LINES, |l| l as usize);
    let (workspace, terminal) = parse(params)?;
    let mut request = addressed(&workspace, &terminal);
    if let Some(after) = cursor {
        request["after_offset"] = json!(after);
    }
    let read = ask(agent, READ, request)?;
    let bytes = read["data"].as_str().and_then(b64::decode).unwrap_or_default();
    let (mut tail, source) = if screen { (Vec::new(), "screen-unavailable") } else { (lines_of(&bytes), "stream") };
    let dropped = tail.len().saturating_sub(limit);
    tail.drain(..dropped);
    let exited = read["status"] != "running";
    let (start, end) = (read["start_offset"].as_u64().unwrap_or(0), read["offset"].as_u64().unwrap_or(0));
    Ok(json!({"terminal": {
        "handle": handle_of(&workspace, &terminal),
        "status": if exited { "exited" } else { "running" },
        "tail": tail,
        "truncated": dropped > 0 || read["reset"] == true,
        "nextCursor": end.to_string(),
        "oldestCursor": start.to_string(),
        "latestCursor": end.to_string(),
        "returnedLineCount": tail.len(),
        "source": source,
    }}))
}
