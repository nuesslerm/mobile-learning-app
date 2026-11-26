//! `session.tabs.*`: a worktree's terminals as the phone's tabs, then the kept layout's (`orca_map_workspace::layout`); no browser tabs or groups,
//! the foreground tab is phone state under `tabs.active.<workspace>`, and a paired device is a mobile client, so a close needs no reason.
use crate::{ask, handle_of};
use agent_api_terminal::{CLOSE, LIST, OPEN};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{AgentApi, Ctx, Subscription};
use orca_map_workspace::{layout, Target};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const SESSION_TABS_LIST: &str = "session.tabs.list";
pub const SESSION_TABS_ACTIVATE: &str = "session.tabs.activate";
pub const SESSION_TABS_CLOSE: &str = "session.tabs.close";
pub const SESSION_TABS_CREATE_TERMINAL: &str = "session.tabs.createTerminal";
pub const SESSION_TABS_SUBSCRIBE: &str = "session.tabs.subscribe";
pub const SESSION_TABS_UNSUBSCRIBE: &str = "session.tabs.unsubscribe";

const POLL: Duration = Duration::from_millis(250);
const SCOPE: &str = "orca";
static PUBLICATION_EPOCH: LazyLock<u128> =
    LazyLock::new(|| SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
static SNAPSHOT_VERSION: AtomicU64 = AtomicU64::new(1);

fn selector(params: &Value) -> Result<&str, Failure> {
    params.get("worktree").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing worktree selector"))
}

fn tab_id(params: &Value) -> Result<&str, Failure> {
    params.get("tabId").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing tab id"))
}

fn active_key(workspace: &str) -> String {
    format!("tabs.active.{workspace}")
}

fn active_of(agent: &dyn AgentApi, workspace: &str) -> Option<String> {
    agent.state_get(SCOPE, &active_key(workspace)).ok().flatten().and_then(|v| v.as_str().map(str::to_string))
}

fn set_active(agent: &dyn AgentApi, workspace: &str, tab: Option<&str>) -> Result<(), Failure> {
    orca_map_state::keep(agent, &active_key(workspace), json!(tab))
}

fn terminals(agent: &dyn AgentApi, workspace: &str) -> Result<Vec<Value>, Failure> {
    Ok(ask(agent, LIST, json!({"workspace": workspace}))?["terminals"].as_array().cloned().unwrap_or_default())
}

/// The foreground tab: the one the phone chose while it still exists, else the first running terminal, else the first tab.
fn foreground(list: &[Value], extras: &[Value], chosen: Option<String>) -> Option<String> {
    let name = |t: &Value| t["terminal"].as_str().unwrap_or_default().to_string();
    let id = |t: &Value| t["id"].as_str().unwrap_or_default().to_string();
    chosen
        .filter(|c| list.iter().any(|t| name(t) == *c) || extras.iter().any(|t| id(t) == *c))
        .or_else(|| list.iter().find(|t| t["status"] == "running").or(list.first()).map(name))
        .or_else(|| extras.first().map(id))
}

fn snapshot(agent: &dyn AgentApi, target: &Target, chosen: Option<String>) -> Result<Value, Failure> {
    let list = terminals(agent, &target.workspace)?;
    let extras = layout::kept(agent, &target.workspace)?;
    let active = foreground(&list, &extras, chosen);
    let mut tabs: Vec<Value> = list
        .iter()
        .map(|t| {
            let name = t["terminal"].as_str().unwrap_or_default();
            let mut tab = json!({"type": "terminal", "id": name, "title": t["title"], "parentTabId": name, "leafId": name, "isActive": active.as_deref() == Some(name)});
            if t["status"] == "running" {
                tab["ptyId"] = json!(handle_of(&target.workspace, name));
            }
            tab
        })
        .collect();
    tabs.extend(extras.iter().map(|t| layout::project(target, t, active.as_deref())));
    let version = SNAPSHOT_VERSION.fetch_add(1, Ordering::Relaxed);
    let active_type = active.as_ref().and_then(|a| tabs.iter().find(|t| t["id"] == a.as_str())).map(|t| t["type"].clone());
    Ok(json!({"worktree": target.id, "publicationEpoch": format!("rca:{}:{}", *PUBLICATION_EPOCH, target.workspace), "snapshotVersion": version, "activeGroupId": null, "activeTabId": active, "activeTabType": active_type, "tabs": tabs}))
}

fn read(agent: &dyn AgentApi, target: &Target) -> Result<Value, Failure> {
    snapshot(agent, target, active_of(agent, &target.workspace))
}

pub(crate) fn list(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let target = orca_map_workspace::resolve(agent, selector(params)?)?;
    layout::discover_mobile_chats(agent, &target.workspace)?;
    read(agent, &target)
}

/// The tab must be one of the worktree's terminals or of its kept layout.
fn existing(agent: &dyn AgentApi, target: &Target, tab: &str) -> Result<(), Failure> {
    if terminals(agent, &target.workspace)?.iter().any(|t| t["terminal"] == tab) || layout::kept(agent, &target.workspace)?.iter().any(|t| t["id"] == tab) { Ok(()) } else { Err(Failure::new(RUNTIME_ERROR, "tab_not_found")) }
}

pub(crate) fn activate(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let selected = selector(params)?;
    let tab = tab_id(params)?;
    let target = orca_map_workspace::resolve(agent, selected)?;
    existing(agent, &target, tab)?;
    set_active(agent, &target.workspace, Some(tab))?;
    snapshot(agent, &target, Some(tab.to_string()))
}

pub(crate) fn close(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let selected = selector(params)?;
    let tab = tab_id(params)?;
    let target = orca_map_workspace::resolve(agent, selected)?;
    existing(agent, &target, tab)?;
    if layout::kept(agent, &target.workspace)?.iter().any(|row| row["id"] == tab && row["kind"] == "browser") {
        orca_map_browser::close_page(agent, &target.workspace, tab)?;
    }
    if layout::close(agent, &target.workspace, tab)? {
        return Ok(json!({"closed": true}));
    }
    ask(agent, CLOSE, json!({"workspace": target.workspace, "terminal": tab}))?;
    if active_of(agent, &target.workspace).as_deref() == Some(tab) {
        set_active(agent, &target.workspace, None)?;
    }
    Ok(json!({"closed": true}))
}

/// A name rca takes (at most 48 of letters, digits, `_`, `.`, `-`), the first of `terminal-N` no terminal of the workspace has.
fn free_name(list: &[Value]) -> String {
    (1..).map(|n| format!("terminal-{n}")).find(|name| !list.iter().any(|t| t["terminal"] == name.as_str())).unwrap_or_default()
}

pub(crate) fn create_terminal(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let target = orca_map_workspace::resolve(agent, selector(params)?)?;
    let name = free_name(&terminals(agent, &target.workspace)?);
    let mut open = json!({"workspace": target.workspace, "terminal": name});
    for (from, to) in [("cwd", "cwd"), ("title", "title")] {
        if let Some(v) = params.get(from).and_then(Value::as_str).filter(|v| !v.is_empty()) {
            open[to] = json!(v);
        }
    }
    if let Some(env) = params.get("env").and_then(Value::as_object) {
        let strings: serde_json::Map<String, Value> = env.iter().filter(|(_, v)| v.is_string()).map(|(k, v)| (k.clone(), v.clone())).collect();
        open["env"] = Value::Object(strings);
    }
    ask(agent, OPEN, open)?;
    if let Some(command) = params.get("command").and_then(Value::as_str).filter(|c| !c.is_empty()) {
        ask(agent, agent_api_terminal::WRITE, json!({"workspace": target.workspace, "terminal": name, "data": format!("{command}\r")}))?;
    }
    set_active(agent, &target.workspace, Some(&name))?;
    let result = snapshot(agent, &target, Some(name.clone()))?;
    let tab = result["tabs"].as_array().into_iter().flatten().find(|t| t["id"] == name.as_str()).cloned().unwrap_or(Value::Null);
    Ok(json!({"tab": tab, "publicationEpoch": result["publicationEpoch"], "snapshotVersion": result["snapshotVersion"]}))
}

/// The id a subscription is known by: the worktree's id keys it, so the unsubscribe computes the same one.
fn base(connection: &str, worktree_id: &str) -> String {
    format!("session.tabs:{connection}:{worktree_id}")
}

fn cancel_tabs(ctx: &mut Ctx, worktree_id: &str, subscription_id: Option<&str>) -> bool {
    let base = base(ctx.connection_id, worktree_id);
    match subscription_id {
        Some(id) => ctx.subscriptions.cancel(&format!("{base}:{id}")),
        None => ctx.subscriptions.cancel_prefix(&base) > 0,
    }
}

pub(crate) fn unsubscribe(ctx: &mut Ctx, params: &Value) -> Result<Value, Failure> {
    let selected = selector(params)?;
    let subscription_id = params.get("subscriptionId").and_then(Value::as_str);
    if let Some(id) = selected.strip_prefix("id:") {
        if cancel_tabs(ctx, id, subscription_id) {
            return Ok(json!({"unsubscribed": true}));
        }
        // Picker ids can name the parent repo while the stream is keyed by the managed workspace.
        if let Ok(target) = orca_map_workspace::resolve(ctx.agent, selected) {
            cancel_tabs(ctx, &target.id, subscription_id);
        }
    } else {
        let target = orca_map_workspace::resolve(ctx.agent, selected)?;
        cancel_tabs(ctx, &target.id, subscription_id);
    }
    Ok(json!({"unsubscribed": true}))
}

struct TabsStream {
    id: String,
    agent: Arc<dyn AgentApi>,
    target: Target,
    last: Option<Value>,
    asked: Option<Instant>,
}

pub(crate) fn open(ctx: &mut Ctx, params: &Value) -> Result<Box<dyn Subscription>, Failure> {
    let agent = ctx.agent_handle.clone();
    let target = orca_map_workspace::resolve(agent.as_ref(), selector(params)?)?;
    layout::discover_mobile_chats(agent.as_ref(), &target.workspace)?;
    let id = format!("{}:{}", base(ctx.connection_id, &target.id), ctx.request_id);
    Ok(Box::new(TabsStream { id, agent, target, last: None, asked: None }))
}

impl Subscription for TabsStream {
    fn id(&self) -> &str {
        &self.id
    }

    /// The first poll sends the snapshot, a later one `updated` only when the tabs or the foreground tab moved.
    fn poll(&mut self) -> Vec<Value> {
        if self.asked.is_some_and(|t| t.elapsed() < POLL) {
            return Vec::new();
        }
        self.asked = Some(Instant::now());
        let Ok(mut frame) = read(self.agent.as_ref(), &self.target) else { return Vec::new() };
        let content = json!([frame["tabs"], frame["activeTabId"]]);
        if self.last.as_ref() == Some(&content) {
            return Vec::new();
        }
        let kind = if self.last.is_none() { "snapshot" } else { "updated" };
        self.last = Some(content);
        frame["type"] = json!(kind);
        vec![frame]
    }

    fn end(&mut self) -> Vec<Value> {
        vec![json!({"type": "end"})]
    }
}
