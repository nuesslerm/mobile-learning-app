//! `workspaceCleanup.browse`: Orca normalises the saved filters and sort field by field, so a newer client's value never rejects the whole `ui.set`.
use crate::ui_values::{as_record, execution_host_id};
use serde_json::{json, Map, Value};

const TRI: &[&str] = &["any", "only", "exclude"];
const PRESENCE: &[&str] = &["any", "some", "none"];
const IDLE: &[&str] = &["last-visited", "last-activity", "created"];
const AGENT_STATES: &[&str] = &["idle", "working", "permission"];
const GIT_STATES: &[&str] = &["clean", "unknown", "dirty", "unpushed"];
const REVIEW_STATES: &[&str] = &["merged", "closed", "unknown", "draft", "open"];
const PROVIDERS: &[&str] = &["github", "gitlab", "bitbucket", "azure-devops", "gitea", "unsupported"];
const TICKET_SOURCES: &[&str] = &["work-item", "linear", "issue"];
const BLOCKER_MODES: &[&str] = &["any-of", "none-of"];
const BLOCKERS: &[&str] = &[
    "main-worktree", "folder-repo", "active-workspace", "pinned", "live-agent", "running-terminal", "dirty-editor-buffer", "recent-visible-context", "terminal-liveness-unknown", "ssh-disconnected",
    "unpushed-commits", "dirty-files", "ignored-files", "unknown-base", "git-status-error", "dismissed",
];
const SORT_FIELDS: &[&str] = &[
    "last-activity", "last-visited", "created", "size", "name", "repo", "path", "host", "workspace-status", "agent", "git", "ahead", "behind", "branch", "review", "ticket", "local-context", "blocker-count",
];

fn string(v: Option<&Value>) -> Value {
    json!(v.and_then(Value::as_str).unwrap_or(""))
}

fn boolean(v: Option<&Value>, fallback: bool) -> Value {
    json!(v.and_then(Value::as_bool).unwrap_or(fallback))
}

fn pick(v: Option<&Value>, allowed: &[&str], fallback: &str) -> Value {
    json!(v.and_then(Value::as_str).filter(|s| allowed.contains(s)).unwrap_or(fallback))
}

/// null stays null; a non-number is the default (null); a number is raised to at least 0.
fn nullable_min0(v: Option<&Value>) -> Value {
    match v {
        Some(Value::Null) => Value::Null,
        Some(n) if n.is_number() => crate::ui_values::num(n.as_f64().unwrap_or(0.0).max(0.0)),
        _ => Value::Null,
    }
}

fn enum_list(v: Option<&Value>, allowed: &[&str]) -> Value {
    let mut out: Vec<&str> = Vec::new();
    for s in v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        if allowed.contains(&s) && !out.contains(&s) {
            out.push(s);
        }
    }
    json!(out)
}

fn string_list(v: Option<&Value>) -> Value {
    let mut out: Vec<&str> = Vec::new();
    for s in v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|s| !s.is_empty()) {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    json!(out)
}

fn host_ids(v: Option<&Value>) -> Value {
    let mut out: Vec<String> = Vec::new();
    for id in v.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter_map(execution_host_id) {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    json!(out)
}

fn filters(v: Option<&Value>) -> Value {
    let raw = as_record(v);
    let group = |k: &str| as_record(raw.get(k));
    let (activity, size, status, agent, git, review, ticket, context, location, safety) =
        (group("activity"), group("size"), group("status"), group("agent"), group("git"), group("review"), group("ticket"), group("context"), group("location"), group("safety"));
    let tri = |m: &Map<String, Value>, k: &str| pick(m.get(k), TRI, "any");
    json!({
        "query": string(raw.get("query")),
        "activity": {"idleSignal": pick(activity.get("idleSignal"), IDLE, "last-visited"), "idleMinDays": nullable_min0(activity.get("idleMinDays")), "neverVisited": boolean(activity.get("neverVisited"), false)},
        "size": {"minBytes": nullable_min0(size.get("minBytes")), "maxBytes": nullable_min0(size.get("maxBytes")), "includeUnsized": boolean(size.get("includeUnsized"), true)},
        "status": {"workspaceStatuses": string_list(status.get("workspaceStatuses")), "matchStatusless": boolean(status.get("matchStatusless"), true), "archived": tri(&status, "archived"), "pinned": tri(&status, "pinned"), "unread": tri(&status, "unread"), "comment": tri(&status, "comment")},
        "agent": {"states": enum_list(agent.get("states"), AGENT_STATES), "retainedDoneAgents": tri(&agent, "retainedDoneAgents")},
        "git": {"states": enum_list(git.get("states"), GIT_STATES), "minAhead": nullable_min0(git.get("minAhead")), "minBehind": nullable_min0(git.get("minBehind")), "branchQuery": string(git.get("branchQuery")), "prunable": tri(&git, "prunable"), "locked": tri(&git, "locked")},
        "review": {"presence": pick(review.get("presence"), PRESENCE, "any"), "states": enum_list(review.get("states"), REVIEW_STATES), "providers": enum_list(review.get("providers"), PROVIDERS)},
        "ticket": {"presence": pick(ticket.get("presence"), PRESENCE, "any"), "sources": enum_list(ticket.get("sources"), TICKET_SOURCES)},
        "context": {"presence": pick(context.get("presence"), PRESENCE, "any"), "completelyEmpty": boolean(context.get("completelyEmpty"), false)},
        "location": {"hostIds": host_ids(location.get("hostIds")), "repoIds": string_list(location.get("repoIds")), "pathPrefix": string(location.get("pathPrefix"))},
        "safety": {"blockers": enum_list(safety.get("blockers"), BLOCKERS), "blockerMode": pick(safety.get("blockerMode"), BLOCKER_MODES, "none-of"), "dismissed": tri(&safety, "dismissed")},
    })
}

/// A missing value is the default browse state; anything else is normalised key by key.
pub(crate) fn browse(v: &Value) -> Value {
    let raw = as_record(if v.is_null() { None } else { Some(v) });
    let sort = as_record(raw.get("sort"));
    json!({
        "version": 1,
        "filters": filters(raw.get("filters")),
        "sort": {"field": pick(sort.get("field"), SORT_FIELDS, "last-activity"), "direction": pick(sort.get("direction"), &["asc", "desc"], "asc")},
    })
}
