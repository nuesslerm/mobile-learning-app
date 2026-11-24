//! The run-centric rows of a thread projection (pin `orchestrationV2.ts`, `IdAllocator`): the ids a run, its attempt, its root node and its provider
//! thread get from the run's ordinal, and the rows derived from the runs a thread holds, as the recorded v0.1.9 server answers them. Pure.
use serde_json::{json, Map, Value};

const CLAUDE_CAPABILITIES: &str = include_str!("../data/claude-capabilities.json");

/// `encodeURIComponent`: what an id embedded in another id is written as.
pub fn enc(text: &str) -> String {
    text.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
        out
    })
}

pub fn run_id(thread: &str, ordinal: u64) -> String {
    format!("run:thread:{thread}:ordinal:{ordinal}")
}

pub fn attempt_id(run: &str) -> String {
    format!("run-attempt:run:{}:attempt:1", enc(run))
}

pub fn node_id(run: &str) -> String {
    format!("node:run:{}:root", enc(run))
}

/// A thread's one provider thread is named by the thread's first run.
pub fn provider_thread_id(instance: &str, first_run: &str) -> String {
    format!("provider-thread:provider:{instance}:native-thread:pending{}", enc(&format!(":{first_run}")))
}

pub fn scope_id(thread: &str) -> String {
    format!("checkpoint-scope:thread:{thread}:name:root")
}

pub fn session_id(instance: &str, thread: &str, uuid: &str) -> String {
    format!("provider-session:provider-instance:{instance}:thread:{thread}:{uuid}")
}

/// The session a provider that could not be opened is listed under.
pub fn shared_session_id(instance: &str) -> String {
    format!("provider-session:provider-instance:{instance}:shared")
}

pub fn handoff_id(instance: &str, thread: &str, uuid: &str) -> String {
    format!("context-handoff:thread:{thread}:from-provider-instance:{instance}:to-provider-instance:{instance}:{uuid}")
}

pub fn message_item_id(message: &str) -> String {
    format!("turn-item:message:{message}")
}

pub fn signal_item_id(run: &str, signal: &str) -> String {
    format!("turn-item:run:{}:signal:{signal}", enc(run))
}

/// A turn item's ordinal: the run's ordinal in the millions, then its place among the run's items.
pub fn item_ordinal(run_ordinal: u64, nth: u64) -> u64 {
    run_ordinal * 1_000_000 + nth
}

/// A run that has left the queue: a turn was started for it.
pub fn is_started(run: &Value) -> bool {
    !run["startedAt"].is_null()
}

fn text(run: &Value, key: &str) -> String {
    run[key].as_str().unwrap_or("").to_string()
}

fn ordinal(run: &Value) -> u64 {
    run["ordinal"].as_u64().unwrap_or(0)
}

/// The rows a thread's runs stand for. `runs` are the run entities in creation order; a cancelled run left its attempt and node behind with the queue.
pub struct Rows {
    pub runs: Vec<Value>,
    pub attempts: Vec<Value>,
    pub nodes: Vec<Value>,
    pub checkpoint_scopes: Vec<Value>,
    pub provider_sessions: Vec<Value>,
    pub provider_threads: Vec<Value>,
    pub context_handoffs: Vec<Value>,
    pub active_provider_thread: Value,
}

pub fn derive(thread: &str, shell: &Map<String, Value>, runs: &[Value], workspace: Option<&str>) -> Rows {
    let first = runs.first();
    let instance = first.map_or_else(String::new, |r| text(r, "providerInstanceId"));
    let provider_thread = first.map(|r| provider_thread_id(&instance, &text(r, "id")));
    let pt = provider_thread.clone().unwrap_or_default();
    let queue = {
        let mut queued: Vec<&Value> = runs.iter().filter(|r| r["status"] == "queued").collect();
        queued.sort_by(|a, b| a["queueRank"].as_f64().partial_cmp(&b["queueRank"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
        queued.iter().map(|r| text(r, "id")).collect::<Vec<_>>()
    };
    let run_rows: Vec<Value> = runs
        .iter()
        .map(|r| {
            let id = text(r, "id");
            let mut row = json!({
                "activeAttemptId": attempt_id(&id), "checkpointId": null, "completedAt": r["completedAt"], "contextHandoffId": null, "id": id,
                "modelSelection": r["modelSelection"], "ordinal": r["ordinal"], "providerInstanceId": r["providerInstanceId"], "providerThreadId": pt,
                "queuePosition": queue.iter().position(|q| *q == id).map_or(Value::Null, |i| json!(i + 1)),
                "requestedAt": r["requestedAt"], "rootNodeId": node_id(&id), "startedAt": r["startedAt"], "status": r["status"], "threadId": thread, "userMessageId": r["userMessageId"],
            });
            if let Some(held) = r.get("queueHeld") {
                row["queueHeld"] = held.clone();
            }
            row
        })
        .collect();
    let live: Vec<&Value> = runs.iter().filter(|r| r["status"] != "cancelled").collect();
    let attempts = live
        .iter()
        .map(|r| {
            let id = text(r, "id");
            let status = if matches!(r["status"].as_str(), Some("queued" | "preparing")) { json!("pending") } else { r["status"].clone() };
            let mut row = json!({"attemptOrdinal": 1, "completedAt": r["completedAt"], "id": attempt_id(&id), "providerInstanceId": r["providerInstanceId"], "providerThreadId": pt, "providerTurnId": null, "reason": "initial", "rootNodeId": node_id(&id), "runId": id, "startedAt": r["startedAt"], "status": status});
            if is_started(r) {
                row["nativeThreadId"] = r["nativeThreadId"].clone();
            }
            row
        })
        .collect();
    let mut node_order = live.clone();
    node_order.sort_by_key(|r| (is_started(r), r["startedAt"].as_str().unwrap_or("").to_string(), ordinal(r)));
    let nodes = node_order
        .iter()
        .map(|r| {
            let id = text(r, "id");
            let status = if matches!(r["status"].as_str(), Some("queued" | "preparing")) { json!("pending") } else { r["status"].clone() };
            json!({"checkpointScopeId": if r["status"] == "preparing" { Value::Null } else { json!(scope_id(thread)) }, "completedAt": r["completedAt"], "countsForRun": true, "id": node_id(&id), "kind": "root_turn", "nativeItemRef": null, "parentNodeId": null, "providerThreadId": pt, "providerTurnId": null, "rootNodeId": node_id(&id), "runId": id, "runtimeRequestId": null, "startedAt": r["startedAt"], "status": status, "threadId": thread})
        })
        .collect();
    let scope_owner = runs.iter().rev().find(|r| r["status"] != "preparing").filter(|r| r["status"] != "cancelled");
    let checkpoint_scopes = scope_owner
        .map(|r| {
            let id = text(r, "id");
            vec![json!({"advancesAppRunCount": true, "createdAt": first.map_or(Value::Null, |f| f["requestedAt"].clone()), "cwd": workspace, "id": scope_id(thread), "kind": "root_run", "nodeId": node_id(&id), "ordinalWithinParent": 0, "parentScopeId": null, "providerThreadId": pt, "runId": id, "threadId": thread})]
        })
        .unwrap_or_default();
    let started: Vec<&Value> = runs.iter().filter(|r| is_started(r)).collect();
    let (provider_sessions, provider_threads) = match started.first() {
        None if first.is_some_and(|f| f["openFailed"] == true) => {
            let head = first.unwrap_or(&Value::Null);
            let stamp = head["completedAt"].clone();
            let unopened = json!({"appThreadId": thread, "contextUsage": null, "createdAt": stamp, "driver": instance, "firstRunOrdinal": head["ordinal"], "forkedFrom": null, "goal": null, "handoffIds": [], "id": pt, "lastRunOrdinal": runs.last().map_or(Value::Null, |r| r["ordinal"].clone()), "nativeConversationHeadRef": null, "nativeMetadata": null, "nativeThreadRef": null, "ownerNodeId": null, "pendingBackgroundTasks": [], "providerInstanceId": instance, "providerSessionId": shared_session_id(&instance), "status": "not_loaded", "updatedAt": stamp});
            (vec![], vec![unopened])
        }
        None => (vec![], vec![]),
        Some(head) => {
            let session = session_id(&instance, thread, &text(head, "sessionUuid"));
            let latest = started.last().copied().unwrap_or(head);
            let model = shell["modelSelection"]["model"].clone();
            let capabilities: Value = serde_json::from_str(CLAUDE_CAPABILITIES).unwrap_or(Value::Null);
            let sessions = vec![json!({"capabilities": capabilities, "createdAt": head["startedAt"], "cwd": workspace, "driver": instance, "extensionStatuses": [], "id": session, "lastError": null, "model": model, "providerInstanceId": instance, "status": "ready", "updatedAt": latest["startedAt"]})];
            let threads = vec![json!({"appThreadId": thread, "contextUsage": null, "createdAt": head["startedAt"], "driver": instance, "firstRunOrdinal": head["ordinal"], "forkedFrom": null, "goal": null, "handoffIds": [], "id": pt, "lastRunOrdinal": latest["ordinal"], "nativeConversationHeadRef": null, "nativeMetadata": null, "nativeThreadRef": {"driver": instance, "nativeId": head["nativeThreadId"], "strength": "strong"}, "ownerNodeId": null, "pendingBackgroundTasks": [], "providerInstanceId": instance, "providerSessionId": session, "status": "active", "updatedAt": latest["startedAt"]})];
            (sessions, threads)
        }
    };
    let context_handoffs = runs
        .iter()
        .filter_map(|r| {
            let h = r.get("handoff").filter(|h| h.is_object())?;
            Some(json!({"coveredRunOrdinals": {"from": h["from"], "to": h["to"]}, "createdAt": r["startedAt"], "createdByProviderInstanceId": null, "fromProviderThreadIds": [pt], "id": handoff_id(&instance, thread, h["uuid"].as_str().unwrap_or("")), "status": "ready", "strategy": "delta_since_target_last_seen", "summaryMessageId": null, "summaryText": "", "targetRunId": r["id"], "threadId": thread, "toProviderThreadId": pt, "transferId": null, "updatedAt": r["startedAt"]}))
        })
        .collect();
    let unopened = first.is_some_and(|f| f["openFailed"] == true);
    let active_provider_thread = if started.is_empty() && !unopened { Value::Null } else { json!(pt) };
    Rows { runs: run_rows, attempts, nodes, checkpoint_scopes, provider_sessions, provider_threads, context_handoffs, active_provider_thread }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_embedded_in_another_is_percent_encoded_once_and_the_provider_thread_is_named_by_the_first_run() {
        // Given the first run of a thread, When the ids derived from it are written, Then they are the recorded ones
        let run = run_id("agent-thread", 1);
        assert_eq!(run, "run:thread:agent-thread:ordinal:1");
        assert_eq!(attempt_id(&run), "run-attempt:run:run%3Athread%3Aagent-thread%3Aordinal%3A1:attempt:1");
        assert_eq!(node_id(&run), "node:run:run%3Athread%3Aagent-thread%3Aordinal%3A1:root");
        assert_eq!(provider_thread_id("claudeAgent", &run), "provider-thread:provider:claudeAgent:native-thread:pending%3Arun%3Athread%3Aagent-thread%3Aordinal%3A1");
        assert_eq!(signal_item_id(&run, "interrupt-request"), "turn-item:run:run%3Athread%3Aagent-thread%3Aordinal%3A1:signal:interrupt-request");
        assert_eq!(enc("a b/é"), "a%20b%2F%C3%A9");
    }

    #[test]
    fn a_run_that_failed_to_open_lists_its_provider_thread_not_loaded_with_no_session_and_a_preparing_run_has_no_checkpoint_scope() {
        // Given a first run whose provider never opened and a second one waiting for its workspace
        let run = |n: u64, status: &str, open_failed: bool| json!({"id": run_id("t", n), "ordinal": n, "status": status, "providerInstanceId": "codex", "startedAt": null, "completedAt": "2026-11-01T00:00:00.000Z", "requestedAt": "2026-11-01T00:00:00.000Z", "openFailed": open_failed});
        let rows = derive("t", &Map::new(), &[run(1, "failed", true), run(2, "preparing", false)], None);
        // When the rows are derived, Then the provider thread is not loaded under the shared session, no session is open, and only the first node has the scope
        let thread = &rows.provider_threads[0];
        assert_eq!((thread["status"].as_str(), thread["providerSessionId"].as_str(), thread["lastRunOrdinal"].as_u64(), rows.provider_sessions.len()), (Some("not_loaded"), Some("provider-session:provider-instance:codex:shared"), Some(2), 0));
        assert_eq!((rows.nodes[0]["checkpointScopeId"].is_string(), rows.nodes[1]["checkpointScopeId"].is_null(), rows.attempts[1]["status"].as_str()), (true, true, Some("pending")));
        assert_eq!(rows.checkpoint_scopes[0]["runId"], run_id("t", 1));
        assert!(rows.active_provider_thread.is_string());
    }
}
