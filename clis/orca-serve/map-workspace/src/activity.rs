//! What happens in the worktrees while nobody looks, over rca's journal, panes and sleeping terminals: `notifications.getMissedSince`,
//! `notifications.subscribe` and `.unsubscribe` (the journal of `service.notifications`, entries in the host's own words), `agentActivity.setRead`
//! and `.clear` (the marks of `service.panes`), `worktree.ps` (the worktree rows with the live terminals of each and its agents as the pane list
//! shows them) and `worktree.sleep` and `.activate` (`terminal.sleep` and `terminal.wake`, with the agents of the worktree stopped on a sleep and its
//! finished panes read on an activation). A pane is a thread (`thread:<id>`); a worktree's agents are the threads that run in its workspace.
use crate::agent::{ask, refusal};
use crate::repos::{object, repo_key};
use crate::target::{id_of, targets, Target};
use crate::worktrees::{checkouts, find, record, selector, Checkout};
use agent_api_agent::lifecycle::THREAD_FORCE_STOP;
use agent_api_agent::THREAD_LIST;
use agent_api_service::notifications::{self, cause};
use agent_api_service::panes::{self, PaneState};
use agent_api_terminal::{LIST as TERMINAL_LIST, SLEEP, SLEEPING, WAKE};
use base64::Engine;
use orca_core::area::Out;
use orca_core::wire::{received, Failure, RUNTIME_ERROR};
use orca_core::{AgentApi, AgentError, Ctx, EventStream, Subscription};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::Duration;

const DEFAULT_PS_LIMIT: u64 = 200;
/// The snapshot ids of the replies the host remembers rows for, so a delta can name the rows a caller already holds.
const RECENT_SNAPSHOTS: usize = 16;
/// How much `lastOutputAt` is rounded down for a caller that polls with snapshot ids: the id then survives a terminal's output.
const COARSE_OUTPUT_MS: u64 = 30_000;
const SUBSCRIPTION_PREFIX: &str = "notifications-";
const POLL_WAIT: Duration = Duration::from_millis(1);
const MAX_EVENTS: usize = 256;
const SOURCE: &str = "agent-task-complete";
const NAVIGATION: [&str; 4] = ["caller", "host", "clients", "all"];

/// One checkout of a repo with the rca workspace it runs in: the repo's own for the main checkout, the managed worktree's for another, none for a
/// checkout git lists that rca does not manage.
pub(crate) struct Placed {
    pub repo: Target,
    pub checkout: Checkout,
    pub workspace: Option<String>,
    pub id: String,
}

fn placement(all: &[Target], repo: &Target, checkout: Checkout) -> Placed {
    let workspace = if checkout.is_main { Some(repo.workspace.clone()) } else { all.iter().find(|t| t.kind == "worktree" && t.root_path == checkout.path).map(|t| t.workspace.clone()) };
    Placed { id: id_of(&repo.workspace, &checkout.path), repo: repo.clone(), checkout, workspace }
}

fn placed(agent: &dyn AgentApi) -> Result<Vec<Placed>, Failure> {
    let all = targets(agent)?;
    let mut out = Vec::new();
    for repo in all.iter().filter(|t| t.kind == "root") {
        for checkout in checkouts(agent, &repo.workspace)? {
            out.push(placement(&all, repo, checkout));
        }
    }
    Ok(out)
}

fn placed_by_selector(agent: &dyn AgentApi, params: &Value) -> Result<Placed, Failure> {
    let (repo, checkout) = find(agent, selector(params)?)?;
    Ok(placement(&targets(agent)?, &repo, checkout))
}

fn optional_bool(params: &Value, key: &str) -> Result<Option<bool>, Failure> {
    match params.get(key) {
        None | Some(Value::Null) if params.get(key).is_none() => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", received(other)))),
        None => Ok(None),
    }
}

fn literal_true(params: &Value, key: &str) -> Result<bool, Failure> {
    match params.get(key) {
        None => Ok(false),
        Some(Value::Bool(true)) => Ok(true),
        Some(_) => Err(Failure::invalid("Invalid input: expected true")),
    }
}

/// A request to the service whose refusal reads as the host's `runtime_error` with the service's sentence.
fn runtime(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { message, .. } => Failure::new(RUNTIME_ERROR, message),
        failed => refusal(failed),
    }
}

fn int_field(params: &Value, key: &str) -> Result<u64, Failure> {
    match params.get(key) {
        None => Err(Failure::invalid("Invalid input: expected number, received undefined")),
        Some(Value::Number(n)) => match (n.as_u64(), n.as_i64()) {
            (Some(v), _) => Ok(v),
            (_, Some(_)) => Err(Failure::invalid(format!("{key} must be a non-negative integer"))),
            _ => Err(Failure::invalid("Invalid input: expected int, received number")),
        },
        Some(other) => Err(Failure::invalid(format!("Invalid input: expected number, received {}", received(other)))),
    }
}

fn delivered(params: &Value) -> Result<Option<usize>, Failure> {
    let Some(list) = params.get("deliveredPushes") else { return Ok(None) };
    let Value::Array(items) = list else { return Err(Failure::invalid(format!("Invalid input: expected array, received {}", received(list)))) };
    if items.len() > 256 {
        return Err(Failure::invalid("Too big: expected array to have <=256 items"));
    }
    for item in items {
        let Value::Object(fields) = item else { return Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(item)))) };
        for (key, max) in [("notificationId", 2048), ("notificationEpoch", 128)] {
            match fields.get(key) {
                Some(Value::String(s)) if (1..=max).contains(&s.chars().count()) => {}
                Some(Value::String(s)) if s.is_empty() => return Err(Failure::invalid("Too small: expected string to have >=1 characters")),
                Some(Value::String(_)) => return Err(Failure::invalid(format!("Too big: expected string to have <={max} characters"))),
                Some(other) => return Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other)))),
                None => return Err(Failure::invalid("Invalid input: expected string, received undefined")),
            }
        }
        int_field(item, "notificationSeq")?;
    }
    Ok(Some(items.len()))
}

/// The worktree a workspace's checkout is, by workspace name; nothing for a workspace that is no checkout of a repo.
fn worktree_ids(agent: &dyn AgentApi) -> BTreeMap<String, String> {
    placed(agent).map(|all| all.into_iter().filter_map(|p| p.workspace.map(|w| (w, p.id))).collect()).unwrap_or_default()
}

fn state_of(cause: &str) -> &'static str {
    match cause {
        cause::APPROVAL_REQUESTED => "blocked",
        cause::INPUT_REQUESTED => "waiting",
        _ => "done",
    }
}

/// An entry of the journal (`seq`, `id`, `cause`, `title`, `body`, `thread_id`, `workspace`, `at_ms`) as the host dispatches it to a phone. No
/// desktop window is in front of this host, so the person is away from it.
fn host_notification(entry: &Value, epoch: &str, worktrees: &BTreeMap<String, String>) -> Value {
    let mut out = Map::new();
    out.insert("type".into(), "notification".into());
    out.insert("source".into(), SOURCE.into());
    out.insert("title".into(), entry["title"].clone());
    out.insert("body".into(), entry["body"].clone());
    if let Some(worktree) = entry["workspace"].as_str().and_then(|w| worktrees.get(w)) {
        out.insert("worktreeId".into(), worktree.clone().into());
    }
    if let Some(thread) = entry["thread_id"].as_str() {
        out.insert("paneKey".into(), panes::pane_key(thread).into());
    }
    out.insert("notificationId".into(), entry["id"].clone());
    out.insert("notificationSeq".into(), entry["seq"].clone());
    out.insert("notificationEpoch".into(), epoch.into());
    out.insert("emittedAt".into(), entry["at_ms"].clone());
    out.insert("agentState".into(), state_of(entry["cause"].as_str().unwrap_or_default()).into());
    out.insert("desktopAway".into(), true.into());
    Value::Object(out)
}

pub(crate) fn missed_since(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let last_seen = int_field(params, "lastSeenSeq")?;
    let epoch = crate::target::text(params, "epoch");
    if let Some(other) = params.get("epoch").filter(|e| !e.is_string()) {
        return Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other))));
    }
    optional_bool(params, "includeDesktopSuppressed")?;
    let pushes = delivered(params)?;
    let mut asked = json!({"after_seq": last_seen});
    if let Some(epoch) = epoch {
        asked["epoch"] = epoch.into();
    }
    let answer = ask(agent, notifications::SINCE.name(), asked)?;
    let current = answer["epoch"].as_str().unwrap_or_default().to_string();
    let worktrees = worktree_ids(agent);
    let rows: Vec<Value> = answer["notifications"].as_array().map(|all| all.iter().map(|n| host_notification(n, &current, &worktrees)).collect()).unwrap_or_default();
    let mut reply = Map::new();
    reply.insert("notifications".into(), rows.into());
    reply.insert("epoch".into(), current.into());
    if pushes.is_some() {
        // No push is ever delivered from this host, so none of the phone's delivered pushes was dismissed.
        reply.insert("dismissedPushes".into(), json!([]));
    }
    Ok(Value::Object(reply))
}

struct Feed {
    id: String,
    events: Box<dyn EventStream>,
    agent: std::sync::Arc<dyn AgentApi>,
    epoch: String,
    ready: bool,
    over: bool,
}

pub(crate) fn open_feed(ctx: &mut Ctx, params: &Value) -> Result<Box<dyn Subscription>, Failure> {
    if !params.is_null() && !params.is_object() {
        return Err(Failure::invalid(format!("Invalid input: expected object, received {}", received(params))));
    }
    optional_bool(params, "includeDesktopSuppressed")?;
    let (answer, events) = ctx.agent_handle.follow(notifications::SUBSCRIBE.name(), notifications::TOPIC, json!({}), "after_seq").map_err(runtime)?;
    let id = ctx.subscription_id(SUBSCRIPTION_PREFIX);
    Ok(Box::new(Feed { id, events, agent: ctx.agent_handle.clone(), epoch: answer["epoch"].as_str().unwrap_or_default().to_string(), ready: false, over: false }))
}

impl Subscription for Feed {
    fn id(&self) -> &str {
        &self.id
    }

    fn poll(&mut self) -> Vec<Value> {
        let mut out = Vec::new();
        if !self.ready {
            self.ready = true;
            out.push(json!({"type": "ready", "subscriptionId": self.id, "epoch": self.epoch}));
        }
        let mut taken = Vec::new();
        while !self.over && taken.len() < MAX_EVENTS {
            match self.events.next(POLL_WAIT) {
                Ok(Some(event)) if event.kind == notifications::kind::POSTED => taken.push(event),
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(_) => {
                    self.over = true;
                    out.push(json!({"type": "end"}));
                }
            }
        }
        if !taken.is_empty() {
            let worktrees = worktree_ids(self.agent.as_ref());
            for event in taken {
                let mut entry = event.data.clone();
                entry["seq"] = event.seq.into();
                out.push(host_notification(&entry, &self.epoch, &worktrees));
            }
        }
        out
    }

    fn end(&mut self) -> Vec<Value> {
        if self.over {
            return Vec::new();
        }
        self.over = true;
        vec![json!({"type": "end"})]
    }

    fn poll_out(&mut self) -> Vec<Out> {
        self.poll().into_iter().map(|e| Out::Text(e.to_string())).collect()
    }
}

pub(crate) fn unsubscribe(ctx: &mut Ctx, params: &Value) -> Result<Value, Failure> {
    match params.get("subscriptionId") {
        Some(Value::String(id)) if !id.is_empty() => {
            ctx.subscriptions.cancel(id);
            Ok(json!({"unsubscribed": true}))
        }
        _ => Err(Failure::invalid("Missing subscriptionId")),
    }
}

fn pane_keys(params: &Value) -> Result<Vec<String>, Failure> {
    let list = match params.get("paneKeys") {
        None => return Err(Failure::invalid("Invalid input: expected array, received undefined")),
        Some(Value::Array(items)) => items,
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected array, received {}", received(other)))),
    };
    if list.is_empty() {
        return Err(Failure::invalid("Too small: expected array to have >=1 items"));
    }
    if list.len() > panes::MAX_MARKED {
        return Err(Failure::invalid(format!("Too big: expected array to have <={} items", panes::MAX_MARKED)));
    }
    list.iter()
        .map(|key| match key {
            Value::String(s) if s.is_empty() => Err(Failure::invalid("Too small: expected string to have >=1 characters")),
            Value::String(s) if s.chars().count() > panes::MAX_KEY => Err(Failure::invalid(format!("Too big: expected string to have <={} characters", panes::MAX_KEY))),
            Value::String(s) => Ok(s.clone()),
            other => Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other)))),
        })
        .collect()
}

fn mark(agent: &dyn AgentApi, keys: Vec<String>, how: &str) -> Result<Value, Failure> {
    ask(agent, panes::MARK.name(), json!({"panes": keys, "mark": how}))?;
    Ok(json!({"applied": true}))
}

pub(crate) fn set_read(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let keys = pane_keys(params)?;
    let read = match params.get("read") {
        Some(Value::Bool(b)) => *b,
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected boolean, received {}", received(other)))),
        None => return Err(Failure::invalid("Invalid input: expected boolean, received undefined")),
    };
    mark(agent, keys, if read { "read" } else { "unread" })
}

pub(crate) fn clear(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    mark(agent, pane_keys(params)?, "clear")
}

/// The terminals of a workspace; none when no executor is attached for it, which says nothing is running there that this host can see.
fn terminals_of(agent: &dyn AgentApi, workspace: &str) -> Result<Vec<Value>, Failure> {
    match agent.request(TERMINAL_LIST.name(), json!({"workspace": workspace})) {
        Ok(answer) => Ok(answer["terminals"].as_array().cloned().unwrap_or_default()),
        Err(AgentError::Refused { code, .. }) if code == "NO_EXECUTOR" => Ok(Vec::new()),
        Err(other) => Err(refusal(other)),
    }
}

fn worktree_status(live: usize, agents: &[Value]) -> &'static str {
    let rank = |s: &str| match s {
        "permission" => 4,
        "working" => 3,
        "done" => 2,
        "active" => 1,
        _ => 0,
    };
    let mut best = if live > 0 { "active" } else { "inactive" };
    for agent in agents.iter().filter(|a| a["activity"]["listed"] != false) {
        let seen = match agent["state"].as_str().unwrap_or_default() {
            "working" => "working",
            "waiting" | "blocked" => "permission",
            _ => "done",
        };
        if rank(seen) > rank(best) {
            best = seen;
        }
    }
    best
}

fn agent_state(state: PaneState) -> &'static str {
    match state {
        PaneState::Working => "working",
        PaneState::Waiting => "waiting",
        PaneState::Blocked => "blocked",
        _ => "done",
    }
}

fn list_status(state: PaneState) -> &'static str {
    match state {
        PaneState::Working => "working",
        PaneState::Waiting => "waiting",
        PaneState::Blocked => "blocked",
        PaneState::Done => "done",
        PaneState::Failed => "failed",
        PaneState::Interrupted => "interrupted",
        PaneState::Idle => "idle",
    }
}

/// The agents of one workspace: its panes that ever ran a turn, as the host's agent rows with the verdict of the pane list.
fn agent_rows(all: &[panes::PaneView], workspace: Option<&str>) -> Vec<Value> {
    let Some(workspace) = workspace else { return Vec::new() };
    let mut rows: Vec<&panes::PaneView> = all.iter().filter(|p| p.workspace.as_deref() == Some(workspace) && p.state != PaneState::Idle).collect();
    rows.sort_by_key(|p| p.state_started_at_ms);
    rows.into_iter()
        .map(|p| {
            let activity = if p.listed {
                json!({"listed": true, "status": list_status(p.state), "unread": p.unread, "clearable": p.clearable, "sortAt": p.sort_at_ms, "title": p.title})
            } else {
                json!({"listed": false})
            };
            json!({
                "paneKey": p.pane, "parentPaneKey": null, "state": agent_state(p.state), "agentType": null, "prompt": "", "taskTitle": null,
                "displayName": null, "conversationName": p.title, "lastAssistantMessage": null, "toolName": null, "toolInput": null,
                "interrupted": p.state == PaneState::Interrupted, "stateStartedAt": p.state_started_at_ms, "updatedAt": p.state_started_at_ms, "activity": activity,
            })
        })
        .collect()
}

fn summary(agent: &dyn AgentApi, platform: &str, p: &Placed, repo_name: &Value, all_panes: &[panes::PaneView]) -> Result<Value, Failure> {
    let base = record(agent, &p.repo, &p.checkout)?;
    let terminals = match &p.workspace {
        Some(w) => terminals_of(agent, w)?,
        None => Vec::new(),
    };
    let running: Vec<&Value> = terminals.iter().filter(|t| t["status"] == "running").collect();
    let last_output = running.iter().filter_map(|t| t["updated_at"].as_u64()).max();
    let agents = agent_rows(all_panes, p.workspace.as_deref());
    let unread_agent = agents.iter().any(|a| a["activity"]["unread"] == true);
    let mut row = Map::new();
    let mut put = |k: &str, v: Value| {
        row.insert(k.into(), v);
    };
    put("workspaceKind", "git".into());
    put("worktreeId", p.id.clone().into());
    put("repoId", p.repo.workspace.clone().into());
    put("terminalPlatform", platform.into());
    put("repo", repo_name.clone());
    put("path", p.checkout.path.clone().into());
    put("branch", base["branch"].clone());
    put("isArchived", base["isArchived"].clone());
    put("isMainWorktree", p.checkout.is_main.into());
    put("hasHostSidebarActivity", false.into());
    put("defaultBranch", (p.checkout.is_main && !p.checkout.branch.is_empty()).into());
    if p.checkout.branch.is_empty() && !p.checkout.head.is_empty() {
        put("detachedHead", true.into());
    }
    put("parentWorktreeId", Value::Null);
    put("childWorktreeIds", json!([]));
    put("displayName", base["displayName"].clone());
    put("workspaceStatus", base.get("workspaceStatus").cloned().unwrap_or_else(|| "in-progress".into()));
    put("sortOrder", base["sortOrder"].clone());
    if let Some(order) = base.get("manualOrder") {
        put("manualOrder", order.clone());
    }
    let activity_at = [base["lastActivityAt"].as_u64(), last_output, agents.iter().filter_map(|a| a["updatedAt"].as_u64()).max()].into_iter().flatten().max().unwrap_or(0);
    put("lastActivityAt", activity_at.into());
    put("linkedIssue", base["linkedIssue"].clone());
    let pr = match &base["linkedPR"] {
        Value::Number(n) => json!({"number": n, "state": "unknown"}),
        other => other.clone(),
    };
    put("linkedPR", pr);
    for key in ["linkedLinearIssue", "linkedGitLabMR", "linkedGitLabIssue"] {
        put(key, base.get(key).cloned().unwrap_or(Value::Null));
    }
    put("linkedItems", base.get("linkedItems").cloned().unwrap_or_else(|| json!([])));
    put("comment", base["comment"].clone());
    put("isPinned", base["isPinned"].clone());
    put("isActive", false.into());
    put("unread", (base["isUnread"] == true || unread_agent).into());
    put("liveTerminalCount", running.len().into());
    put("hasAttachedPty", (!running.is_empty()).into());
    put("unverifiableTerminalCount", 0.into());
    put("lastOutputAt", last_output.map_or(Value::Null, Value::from));
    put("preview", "".into());
    put("status", worktree_status(running.len(), &agents).into());
    put("agents", agents.into());
    Ok(Value::Object(row))
}

fn row_order(a: &Value, b: &Value) -> std::cmp::Ordering {
    let flag = |row: &Value, key: &str| row[key] == true;
    let last = |row: &Value| row["lastOutputAt"].as_i64().unwrap_or(-1);
    flag(b, "isPinned")
        .cmp(&flag(a, "isPinned"))
        .then(flag(b, "unread").cmp(&flag(a, "unread")))
        .then(last(b).cmp(&last(a)))
        .then(b["liveTerminalCount"].as_u64().cmp(&a["liveTerminalCount"].as_u64()))
        .then(a["path"].as_str().cmp(&b["path"].as_str()))
}

fn snapshot_id(serialized: &str) -> String {
    let digest = Sha256::digest(serialized.as_bytes());
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest).chars().take(32).collect()
}

static RECENT: Mutex<Vec<(String, Vec<String>)>> = Mutex::new(Vec::new());

fn remember(id: &str, hashes: &[String]) {
    let mut recent = RECENT.lock().unwrap_or_else(|p| p.into_inner());
    recent.retain(|(known, _)| known != id);
    recent.push((id.to_string(), hashes.to_vec()));
    if recent.len() > RECENT_SNAPSHOTS {
        recent.remove(0);
    }
}

/// The reply of a caller that sends snapshot ids: the whole reply with its id, `unchanged` when it already holds this content, or the rows as
/// numbers where the base reply it names had the same row.
fn resolve_snapshot(result: Value, after: Option<&str>, delta: bool) -> Value {
    let rows = result["worktrees"].as_array().cloned().unwrap_or_default();
    let hashes: Vec<String> = rows.iter().map(|r| snapshot_id(&r.to_string())).collect();
    let mut shell = result.as_object().cloned().unwrap_or_default();
    shell.insert("worktrees".into(), Value::Array(hashes.iter().map(|h| Value::String(h.clone())).collect()));
    let id = snapshot_id(&Value::Object(shell).to_string());
    let base = if delta { after.and_then(|a| RECENT.lock().unwrap_or_else(|p| p.into_inner()).iter().find(|(known, _)| known == a).map(|(_, h)| h.clone())) } else { None };
    remember(&id, &hashes);
    if after == Some(id.as_str()) {
        return json!({"unchanged": true, "snapshotId": id});
    }
    if let (Some(after), Some(base)) = (after, base) {
        let mut envelope = result.as_object().cloned().unwrap_or_default();
        envelope.remove("worktrees");
        envelope.insert("snapshotId".into(), id.into());
        envelope.insert("baseSnapshotId".into(), after.into());
        let from_base: Vec<Value> = rows.iter().zip(&hashes).map(|(row, hash)| base.iter().position(|b| b == hash).map_or_else(|| row.clone(), Value::from)).collect();
        envelope.insert("rowsFromBase".into(), from_base.into());
        return Value::Object(envelope);
    }
    let mut full = result.as_object().cloned().unwrap_or_default();
    full.insert("snapshotId".into(), id.into());
    Value::Object(full)
}

pub(crate) fn ps(agent: &dyn AgentApi, platform: &str, params: &Value) -> Result<Value, Failure> {
    let limit = match params.get("limit") {
        None => DEFAULT_PS_LIMIT as f64,
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected number, received {}", received(other)))),
    };
    let after = match params.get("afterSnapshotId") {
        None => None,
        Some(Value::Null) => Some(None),
        Some(Value::String(s)) if s.is_empty() => return Err(Failure::invalid("Too small: expected string to have >=1 characters")),
        Some(Value::String(s)) if s.chars().count() > 128 => return Err(Failure::invalid("Too big: expected string to have <=128 characters")),
        Some(Value::String(s)) => Some(Some(s.as_str())),
        Some(other) => return Err(Failure::invalid(format!("Invalid input: expected string, received {}", received(other)))),
    };
    let delta = literal_true(params, "acceptRowDelta")?;
    literal_true(params, "supportsWorktreeVisibilitySourceDefaults")?;
    literal_true(params, "includeAgentWait")?;
    if limit.fract() != 0.0 || limit <= 0.0 {
        return Err(Failure::new("invalid_limit", "invalid_limit"));
    }
    let all_panes: Vec<panes::PaneView> = serde_json::from_value(ask(agent, panes::LIST.name(), json!({}))?["panes"].clone()).map_err(|e| Failure::new(RUNTIME_ERROR, e.to_string()))?;
    let worktrees = placed(agent)?;
    let mut repo_names = BTreeMap::new();
    let mut rows = Vec::new();
    for p in &worktrees {
        let repo = p.repo.workspace.as_str();
        let name = match repo_names.entry(repo) {
            Entry::Occupied(saved) => saved.into_mut(),
            Entry::Vacant(slot) => {
                let mut meta = object(agent, &repo_key(repo))?;
                slot.insert(meta.remove("displayName").unwrap_or_else(|| repo.into()))
            }
        };
        rows.push(summary(agent, platform, p, name, &all_panes)?);
    }
    rows.sort_by(row_order);
    let total = rows.len();
    let legacy = after.is_none() && !delta;
    if !legacy {
        for row in &mut rows {
            row["preview"] = "".into();
            if let Some(at) = row["lastOutputAt"].as_u64() {
                row["lastOutputAt"] = (at / COARSE_OUTPUT_MS * COARSE_OUTPUT_MS).into();
            }
        }
        rows.sort_by(row_order);
    }
    let truncated = (total as f64) > limit;
    rows.truncate(limit as usize);
    let result = json!({"worktrees": rows, "hostScope": {"hostIds": [], "omittedHostIds": ["local"]}, "totalCount": total, "truncated": truncated});
    if legacy {
        return Ok(result);
    }
    Ok(resolve_snapshot(result, after.flatten(), delta))
}

/// The sleeping workspaces by name.
fn sleeping(agent: &dyn AgentApi) -> Result<BTreeSet<String>, Failure> {
    let answer = ask(agent, SLEEPING.name(), json!({}))?;
    Ok(answer["sleeping"].as_array().map(|all| all.iter().filter_map(|s| s["workspace"].as_str().map(str::to_string)).collect()).unwrap_or_default())
}

/// Stops the turns that run in `workspace` and holds their queues, as the host stops the agents of a worktree it puts to sleep.
fn stop_agents(agent: &dyn AgentApi, workspace: &str) -> Result<(), Failure> {
    let listed = ask(agent, THREAD_LIST.name(), json!({}))?;
    for thread in listed["threads"].as_array().into_iter().flatten().filter(|t| t["workspace"] == workspace && t["status"] == "running") {
        if let Some(id) = thread["id"].as_str() {
            ask(agent, THREAD_FORCE_STOP.name(), json!({"thread": id}))?;
        }
    }
    Ok(())
}

pub(crate) fn sleep(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let target = placed_by_selector(agent, params)?;
    let chats = optional_bool(params, "markChatsAsleep")?.unwrap_or(true);
    if let Some(workspace) = &target.workspace {
        if chats {
            stop_agents(agent, workspace)?;
        }
        agent.request(SLEEP.name(), json!({"workspace": workspace})).map_err(runtime)?;
    }
    Ok(json!({"worktreeId": target.id}))
}

pub(crate) fn activate(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let target = placed_by_selector(agent, params)?;
    optional_bool(params, "notifyClients")?;
    match params.get("navigation") {
        None => {}
        Some(Value::String(s)) if NAVIGATION.contains(&s.as_str()) => {}
        Some(_) => return Err(Failure::invalid("Invalid option: expected one of \"caller\"|\"host\"|\"clients\"|\"all\"")),
    }
    if let Some(workspace) = &target.workspace {
        let shown: Vec<panes::PaneView> = serde_json::from_value(ask(agent, panes::LIST.name(), json!({"workspace": workspace}))?["panes"].clone()).map_err(|e| Failure::new(RUNTIME_ERROR, e.to_string()))?;
        let unread: Vec<String> = shown.into_iter().filter(|p| p.unread).map(|p| p.pane).collect();
        if !unread.is_empty() {
            ask(agent, panes::MARK.name(), json!({"panes": unread, "mark": "read"}))?;
        }
        if sleeping(agent)?.contains(workspace) {
            agent.request(WAKE.name(), json!({"workspace": workspace})).map_err(runtime)?;
        }
    }
    Ok(json!({"repoId": target.repo.workspace, "worktreeId": target.id, "activated": true, "sleepingAgentWake": "not-applicable"}))
}
