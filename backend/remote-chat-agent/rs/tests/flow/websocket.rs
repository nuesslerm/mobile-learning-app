use crate::doubles::{ev, fails, idle, says, Prompt, Run};
use crate::harness::{eventually, get, post, service, service_over, Env, Options, Socket, SECRET};
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

const AT: &str = "2026-01-01T00:00:00.000Z";
const COMMAND_TYPES: [&str; 12] = [
    "thread.create", "thread.delete", "thread.archive", "thread.unarchive", "thread.meta.update", "thread.runtime-mode.set", "thread.interaction-mode.set",
    "thread.turn.start", "thread.turn.interrupt", "thread.approval.respond", "thread.user-input.respond", "thread.session.stop",
];

/// A failure to provoke: the thread, what provokes it, and the error text the turn must report.
type Failure<'a> = (&'a str, Box<dyn Fn() + 'a>, &'a str);

fn model() -> Value {
    json!({ "cli": "opencode", "providerID": "p", "modelID": "m" })
}

fn create(thread: &str) -> Value {
    json!({ "type": "thread.create", "commandId": format!("cmd-create-{thread}"), "threadId": thread, "title": format!("Title of {thread}"), "cwd": "/tmp",
        "modelSelection": model(), "runtimeMode": "full-access", "interactionMode": "default", "createdAt": AT })
}

fn command(kind: &str, thread: &str, extra: Value) -> Value {
    let mut c = json!({ "type": kind, "commandId": format!("cmd-{kind}-{thread}"), "threadId": thread, "createdAt": AT });
    for (key, value) in extra.as_object().unwrap() {
        c[key] = value.clone();
    }
    c
}

fn turn(thread: &str, text: &str, bootstrap: bool, mode: &str) -> Value {
    let mut c = command("thread.turn.start", thread, json!({ "message": { "messageId": format!("msg-{text}"), "role": "user", "text": text }, "runtimeMode": "full-access", "interactionMode": mode }));
    if bootstrap {
        c["bootstrap"] = json!({ "createThread": { "title": "A turn", "cwd": "/tmp", "modelSelection": model(), "runtimeMode": "full-access", "interactionMode": mode } });
    }
    c
}

fn error(code: &str, message: &str) -> Value {
    json!({ "code": code, "message": message })
}

fn is_event(frame: &Value, id: &str, kind: &str) -> bool {
    frame["kind"] == "event" && frame["id"] == id && frame["item"]["kind"] == "event" && (kind.is_empty() || frame["item"]["event"]["type"] == kind)
}

async fn thread(ws: &mut Socket, id: &str) -> Value {
    ws.request(&format!("read-{id}"), "orchestration.getThread", json!({ "threadId": id })).await["result"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_socket_when_envelopes_requests_and_commands_arrive_then_each_is_answered_and_the_thread_view_follows_the_commands() {
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let served = svc.serve();

    // Given a token that is wrong or missing, when a socket is opened, then 401 and no socket; the socket path without an upgrade is
    // a bad request and any other path an ordinary 404.
    assert_eq!(served.connect("?token=wrong").await.err(), Some((401, "unauthorized".to_string())));
    assert_eq!(served.connect("").await.err(), Some((401, "unauthorized".to_string())));
    assert_eq!(svc.send(get(&format!("/ws?token={SECRET}"))).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(svc.send(get("/not-the-socket")).await.status, StatusCode::NOT_FOUND);

    // When the secret is the token, then the first frame is the hello; a ping as text or binary is answered with its time.
    let mut ws = served.connect(&format!("?token={SECRET}")).await.expect("the upgrade is accepted");
    assert_eq!(ws.first().await, json!({ "kind": "hello", "serverVersion": "1.0.0", "protocolVersion": 1, "heartbeatIntervalMs": 15000 }));
    ws.send(json!({ "kind": "ping", "ts": 5 })).await;
    assert_eq!(ws.first().await, json!({ "kind": "pong", "ts": 5 }));
    ws.send_binary(json!({ "kind": "ping", "ts": 7 })).await;
    assert_eq!(ws.first().await, json!({ "kind": "pong", "ts": 7 }));

    // When something that is not an envelope is sent, then bad_envelope carries the reason in serde's words and the socket stays open.
    let not_a_map = |received: &str| format!("invalid type: {received}, expected a map");
    let bad = [
        ("not json".to_string(), "Invalid JSON".to_string()),
        (json!({ "kind": "request" }).to_string(), "missing field `id`".into()),
        (json!({ "kind": "request", "id": "a" }).to_string(), "missing field `method`".into()),
        (json!({ "kind": "request", "id": "", "method": "x", "params": null }).to_string(), "id: invalid length 0, expected a string of at least 1 character".into()),
        (json!({ "kind": "request", "id": "a", "method": "x" }).to_string(), "missing field `params`".into()),
        (json!({ "kind": "subscribe", "id": "s", "method": "orchestration.subscribeShell" }).to_string(), "missing field `params`".into()),
        (json!({ "kind": "unsubscribe" }).to_string(), "missing field `id`".into()),
        (json!({ "kind": "ping", "ts": 1.5 }).to_string(), "ts: invalid type: floating point `1.5`, expected i64".into()),
        (json!({ "kind": "ping" }).to_string(), "missing field `ts`".into()),
        (json!({}).to_string(), "missing field `kind`".into()),
        (json!({ "kind": "nope" }).to_string(), "kind: unknown variant `nope`, expected one of `request`, `subscribe`, `unsubscribe`, `ping`".into()),
        ("[1]".into(), not_a_map("sequence")),
        ("5".into(), not_a_map("integer `5`")),
        ("null".into(), not_a_map("null")),
        ("\"text\"".into(), not_a_map("string \"text\"")),
    ];
    for (text, message) in bad {
        ws.send_text(&text).await;
        assert_eq!(ws.first().await, json!({ "kind": "response", "id": "", "ok": false, "error": error("bad_envelope", &message) }), "{text}");
    }
    ws.send(json!({ "kind": "ping", "ts": -1 })).await;
    assert_eq!(ws.first().await, json!({ "kind": "pong", "ts": -1 }));

    // When methods are requested, then an unknown one is unknown_method, params of the wrong shape are invalid_params, null params
    // list threads, a thread nobody created is not found and its turn diff is empty.
    assert_eq!(ws.request("u1", "nope.method", json!({})).await, json!({ "kind": "response", "id": "u1", "ok": false, "error": error("unknown_method", "Unknown method") }));
    assert!(ws.request("l1", "orchestration.listThreads", Value::Null).await["result"]["threads"].is_array());
    assert_eq!(ws.request("l2", "orchestration.listThreads", json!({ "includeArchived": "yes" })).await["error"], error("invalid_params", "includeArchived: invalid type: string \"yes\", expected a boolean"));
    assert_eq!(ws.request("g1", "orchestration.getThread", json!({ "threadId": "ws-missing" })).await["error"], error("thread_not_found", "Thread not found"));
    assert_eq!(ws.request("g2", "orchestration.getThread", json!({})).await["error"], error("invalid_params", "missing field `threadId`"));
    assert_eq!(ws.request("g3", "orchestration.getTurnDiff", json!({ "threadId": "ws-missing", "turnId": "t" })).await, json!({ "kind": "response", "id": "g3", "ok": true, "result": { "unifiedDiff": "" } }));

    // When a thread is created, then the receipt carries its one event, and the thread is listed and readable; created again it is
    // rejected and nothing is added; sequences rise across threads.
    let receipt = ws.dispatch("c1", create("ws-1")).await["result"].clone();
    assert_eq!((receipt["commandId"].clone(), receipt["status"].clone(), receipt["events"].as_array().unwrap().len()), (json!("cmd-create-ws-1"), json!("accepted"), 1));
    let created = &receipt["events"][0];
    assert_eq!((created["type"].clone(), created["threadId"].clone(), created["actor"].clone(), created["commandId"].clone()), (json!("thread.created"), json!("ws-1"), json!("client"), json!("cmd-create-ws-1")));
    for (key, value) in [("title", json!("Title of ws-1")), ("cwd", json!("/tmp")), ("modelSelection", model()), ("runtimeMode", json!("full-access")), ("interactionMode", json!("default"))] {
        assert_eq!(created["payload"][key], value, "{key}");
    }
    let first_sequence = created["sequence"].as_u64().unwrap();
    let again = ws.dispatch("c2", create("ws-1")).await["result"].clone();
    assert_eq!(again, json!({ "commandId": "cmd-create-ws-1", "status": "rejected", "events": [], "error": error("thread_exists", "thread ws-1 already exists") }));
    let other = ws.dispatch("c3", create("ws-2")).await["result"]["events"][0]["sequence"].as_u64().unwrap();
    assert!(other > first_sequence);
    let listed = ws.request("l3", "orchestration.listThreads", json!({})).await["result"]["threads"].clone();
    assert!(listed.as_array().unwrap().iter().any(|t| t["id"] == "ws-1"));
    let read = thread(&mut ws, "ws-1").await;
    for (key, value) in [("id", json!("ws-1")), ("title", json!("Title of ws-1")), ("cwd", json!("/tmp")), ("modelSelection", model()), ("runtimeMode", json!("full-access")), ("interactionMode", json!("default")),
        ("latestTurn", Value::Null), ("archivedAt", Value::Null), ("deletedAt", Value::Null), ("messages", json!([])), ("proposedPlans", json!([])), ("activities", json!([])), ("session", Value::Null)] {
        assert_eq!(read[key], value, "{key}");
    }

    // When commands break their schema, then invalid_params carries serde's words with the path to the field.
    let mut types = String::new();
    for (i, t) in COMMAND_TYPES.iter().enumerate() {
        types.push_str(&format!("{}`{t}`", if i == 0 { "" } else { ", " }));
    }
    let start = |extra: Value| {
        let mut c = json!({ "type": "thread.turn.start", "commandId": "c", "threadId": "t", "message": { "messageId": "m", "role": "user", "text": "x" }, "runtimeMode": "auto", "interactionMode": "default", "createdAt": AT });
        for (key, value) in extra.as_object().unwrap() {
            c[key] = value.clone();
        }
        c
    };
    let create_with = |key: &str, value: Value| {
        let mut c = json!({ "type": "thread.create", "commandId": "c", "threadId": "t", "title": "T", "modelSelection": { "cli": "opencode" }, "runtimeMode": "auto", "interactionMode": "plan", "createdAt": AT });
        c[key] = value;
        c
    };
    let invalid = [
        (json!({ "type": "thread.nope" }), format!("type: unknown variant `thread.nope`, expected one of {types}")),
        (json!({}), "missing field `type`".into()),
        (json!("str"), "invalid type: string \"str\", expected a map".into()),
        (json!({ "type": "thread.delete" }), "missing field `commandId`".into()),
        (json!({ "type": "thread.delete", "commandId": "c" }), "missing field `threadId`".into()),
        (create_with("title", json!("")), "title: invalid length 0, expected a string of at least 1 character".into()),
        (create_with("modelSelection", json!({ "cli": "x" })), "modelSelection.cli: unknown variant `x`, expected one of `opencode`, `claude`, `codex`".into()),
        (create_with("modelSelection", json!({ "cli": "opencode", "providerID": "" })), "modelSelection.providerID: invalid length 0, expected a string of at least 1 character".into()),
        (create_with("runtimeMode", json!("full")), "runtimeMode: unknown variant `full`, expected one of `approval-required`, `auto-accept-edits`, `auto`, `full-access`".into()),
        (create_with("createdAt", json!("yesterday")), "createdAt: invalid value: string \"yesterday\", expected a UTC date-time such as 2026-01-01T00:00:00.000Z".into()),
        (start(json!({ "message": { "messageId": "m", "role": "assistant", "text": "x" } })), "message.role: unknown variant `assistant`, expected `user`".into()),
        (start(json!({ "message": { "messageId": "m", "role": "user", "text": 1 } })), "message.text: invalid type: integer `1`, expected a string".into()),
        (start(json!({ "bootstrap": {} })), "bootstrap: missing field `createThread`".into()),
        (start(json!({ "bootstrap": { "createThread": { "title": "t" } } })), "bootstrap.createThread: missing field `modelSelection`".into()),
        (command("thread.user-input.respond", "t", json!({ "requestId": "r", "answers": [] })), "answers: invalid type: sequence, expected a map".into()),
        (command("thread.approval.respond", "t", json!({ "requestId": "r", "decision": "maybe" })), "decision: unknown variant `maybe`, expected one of `accept`, `acceptForSession`, `decline`, `cancel`".into()),
        (command("thread.turn.interrupt", "t", json!({ "turnId": 5 })), "turnId: invalid type: integer `5`, expected a string".into()),
        (json!({ "type": "thread.meta.update", "commandId": "c", "threadId": "t", "title": 5 }), "title: invalid type: integer `5`, expected a string".into()),
    ];
    for (bad, message) in invalid {
        assert_eq!(ws.request("x1", "orchestration.dispatchCommand", bad.clone()).await, json!({ "kind": "response", "id": "x1", "ok": false, "error": error("invalid_params", &message) }), "{bad}");
    }

    // When a thread is renamed, archived, unarchived and its modes set, then each event is stored and the read model follows.
    let renamed = ws.dispatch("m1", json!({ "type": "thread.meta.update", "commandId": "cmd-meta", "threadId": "ws-1", "title": "Renamed" })).await["result"]["events"][0].clone();
    assert_eq!((renamed["type"].clone(), renamed["payload"]["title"].clone(), renamed["sequence"].as_u64().unwrap() > other), (json!("thread.meta-updated"), json!("Renamed"), true));
    let archived = ws.dispatch("m2", json!({ "type": "thread.archive", "commandId": "cmd-arch", "threadId": "ws-1" })).await["result"]["events"][0]["type"].clone();
    assert_eq!(archived, "thread.archived");
    let listed = ws.request("l4", "orchestration.listThreads", json!({})).await["result"]["threads"].clone();
    assert!(listed.as_array().unwrap().iter().find(|t| t["id"] == "ws-1").unwrap()["archivedAt"].is_string());
    ws.dispatch("m3", json!({ "type": "thread.unarchive", "commandId": "cmd-unarch", "threadId": "ws-1" })).await;
    ws.dispatch("m4", command("thread.runtime-mode.set", "ws-1", json!({ "runtimeMode": "approval-required" }))).await;
    ws.dispatch("m5", command("thread.interaction-mode.set", "ws-1", json!({ "interactionMode": "plan" }))).await;
    let read = thread(&mut ws, "ws-1").await;
    assert_eq!((read["title"].clone(), read["archivedAt"].clone(), read["runtimeMode"].clone(), read["interactionMode"].clone()), (json!("Renamed"), Value::Null, json!("approval-required"), json!("plan")));

    // When a turn names a thread that does not exist and nothing to create it with, or an interrupt finds no running turn, or an
    // answer names no open request, then the receipt is rejected saying why.
    let orphan = ws.dispatch("t1", turn("ws-never", "x", false, "default")).await["result"].clone();
    assert_eq!(orphan, json!({ "commandId": "cmd-thread.turn.start-ws-never", "status": "rejected", "events": [], "error": error("thread_not_found", "thread ws-never does not exist") }));
    assert_eq!(ws.dispatch("t2", command("thread.turn.interrupt", "ws-1", json!({}))).await["result"]["error"], error("no_active_turn", "thread ws-1 has no running turn"));
    assert_eq!(ws.dispatch("t3", command("thread.approval.respond", "ws-1", json!({ "requestId": "nope", "decision": "accept" }))).await["result"]["error"], error("request_not_open", "request nope is not open on thread ws-1"));

    // When the models of a CLI are asked, then opencode's list is mapped to provider, model and name, and a CLI that is not one is
    // refused; the server probe lists opencode with its models, the selected CLI and the version.
    svc.opencode.respond("GET", "/api/model", 200, json!({ "data": [{ "providerID": "prov", "id": "mod-1", "name": "Model One", "capabilities": {} }, { "providerID": "prov", "id": "mod-2", "name": "Model Two" }] }));
    let models = ws.request("mo1", "models.list", json!({ "cli": "opencode", "directory": "/tmp" })).await;
    assert_eq!(models["result"], json!({ "models": [{ "providerID": "prov", "modelID": "mod-1", "name": "Model One" }, { "providerID": "prov", "modelID": "mod-2", "name": "Model Two" }] }));
    assert_eq!(svc.opencode.find("GET", "/api/model")[0].directory(), Some("/tmp"));
    assert_eq!(ws.request("mo2", "models.list", json!({ "cli": "zzz" })).await["error"], error("invalid_params", "cli: unknown variant `zzz`, expected one of `opencode`, `claude`, `codex`"));
    assert_eq!(ws.request("mo3", "models.list", json!({ "cli": "codex", "directory": "/tmp" })).await["error"]["code"], "models_list_failed");
    env.set("AGENT_CLI", "opencode");
    let probe = ws.request("p1", "server.probe", json!({})).await["result"].clone();
    assert_eq!((probe["availableClis"].clone(), probe["selectedCli"].clone(), probe["version"].clone()), (json!(["opencode"]), json!("opencode"), json!("1.0.0")));
    assert_eq!(
        probe["providers"],
        json!([{ "cli": "opencode", "displayName": "opencode serve, reached over HTTP. No heavy local dependency.", "available": true, "models": [{ "providerID": "prov", "modelID": "mod-1", "name": "Model One" }, { "providerID": "prov", "modelID": "mod-2", "name": "Model Two" }] }])
    );
    ws.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_subscriptions_when_threads_change_turns_run_and_the_service_restarts_then_each_subscriber_sees_its_events_and_the_thread_keeps_what_happened() {
    let env = Env::new(&[]);
    let state = env.dir("state");
    let svc = service(&env, Options { state_dir: Some(state.clone()), ..Options::default() });
    let oc = &svc.opencode;
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;
    let subscribe = |id: &str, method: &str, params: Value| json!({ "kind": "subscribe", "id": id, "method": method, "params": params });

    // Given a thread, when the shell is subscribed, then the summaries arrive (no messages), then synchronized, then every later
    // event of every thread.
    ws.dispatch("c0", create("ws-shell-1")).await;
    ws.send(subscribe("shell", "orchestration.subscribeShell", json!({}))).await;
    let snapshot = ws.next(|f| f["id"] == "shell").await;
    assert_eq!((snapshot["kind"].clone(), snapshot["item"]["kind"].clone()), (json!("event"), json!("snapshot")));
    let summaries = snapshot["item"]["snapshot"].as_array().unwrap();
    let summary = summaries.iter().find(|t| t["id"] == "ws-shell-1").unwrap();
    assert_eq!((summary["title"].clone(), summary["session"].clone()), (json!("Title of ws-shell-1"), Value::Null));
    assert!(summaries.iter().all(|t| t.get("messages").is_none()));
    assert_eq!(ws.next(|f| f["id"] == "shell").await, json!({ "kind": "event", "id": "shell", "item": { "kind": "synchronized" } }));
    ws.dispatch("c1", create("ws-shell-2")).await;
    let live = ws.next(|f| is_event(f, "shell", "thread.created")).await;
    assert_eq!(live["item"]["event"]["threadId"], "ws-shell-2");
    ws.send(json!({ "kind": "unsubscribe", "id": "shell" })).await;

    // When a thread is subscribed, then the whole thread arrives, then synchronized, then that thread's events only; one nobody
    // created gives only synchronized; a sequence to resume after replays what followed it in place of the snapshot.
    ws.dispatch("c2", create("ws-other")).await;
    ws.send(subscribe("thr", "orchestration.subscribeThread", json!({ "threadId": "ws-shell-1" }))).await;
    let whole = ws.next(|f| f["id"] == "thr").await;
    assert_eq!((whole["item"]["kind"].clone(), whole["item"]["snapshot"]["id"].clone(), whole["item"]["snapshot"]["messages"].clone()), (json!("snapshot"), json!("ws-shell-1"), json!([])));
    assert_eq!(ws.next(|f| f["id"] == "thr").await["item"], json!({ "kind": "synchronized" }));
    ws.dispatch("c3", json!({ "type": "thread.meta.update", "commandId": "cmd-other", "threadId": "ws-other", "title": "Not this one" })).await;
    let changed = ws.dispatch("c4", json!({ "type": "thread.meta.update", "commandId": "cmd-this", "threadId": "ws-shell-1", "title": "This one" })).await["result"]["events"][0]["sequence"].clone();
    let only = ws.next(|f| is_event(f, "thr", "")).await;
    assert_eq!((only["item"]["event"]["type"].clone(), only["item"]["event"]["threadId"].clone(), only["item"]["event"]["sequence"].clone()), (json!("thread.meta-updated"), json!("ws-shell-1"), changed));
    assert!(ws.quiet(200).await.iter().all(|f| f["id"] != "thr"));
    ws.send(subscribe("ghost", "orchestration.subscribeThread", json!({ "threadId": "ws-ghost" }))).await;
    assert_eq!(ws.next(|f| f["id"] == "ghost").await, json!({ "kind": "event", "id": "ghost", "item": { "kind": "synchronized" } }));
    let after = ws.dispatch("r0", create("ws-resume")).await["result"]["events"][0]["sequence"].as_u64().unwrap();
    for (i, title) in ["Two", "Three"].iter().enumerate() {
        ws.dispatch(&format!("r{}", i + 1), json!({ "type": "thread.meta.update", "commandId": format!("cmd-r{i}"), "threadId": "ws-resume", "title": title })).await;
    }
    ws.send(subscribe("resume", "orchestration.subscribeThread", json!({ "threadId": "ws-resume", "afterSequence": after }))).await;
    let replayed: Vec<Value> = vec![ws.next(|f| f["id"] == "resume").await, ws.next(|f| f["id"] == "resume").await, ws.next(|f| f["id"] == "resume").await];
    assert_eq!((replayed[0]["item"]["event"]["sequence"].as_u64(), replayed[0]["item"]["event"]["payload"]["title"].clone()), (Some(after + 1), json!("Two")));
    assert_eq!((replayed[1]["item"]["event"]["sequence"].as_u64(), replayed[1]["item"]["event"]["payload"]["title"].clone()), (Some(after + 2), json!("Three")));
    assert_eq!(replayed[2]["item"], json!({ "kind": "synchronized" }));

    // When an id is subscribed twice, then the second replaces the first; an unsubscribe stops a subscription, and one of an id
    // nobody holds is ignored; params that break the schema and a subscription that does not exist are refused.
    ws.send(subscribe("same", "orchestration.subscribeThread", json!({ "threadId": "ws-resume" }))).await;
    ws.next(|f| f["id"] == "same" && f["item"]["kind"] == "synchronized").await;
    ws.send(subscribe("same", "orchestration.subscribeThread", json!({ "threadId": "ws-resume" }))).await;
    ws.next(|f| f["id"] == "same" && f["item"]["kind"] == "synchronized").await;
    ws.send(json!({ "kind": "unsubscribe", "id": "resume" })).await;
    ws.send(json!({ "kind": "unsubscribe", "id": "nobody" })).await;
    ws.request("barrier", "orchestration.listThreads", json!({})).await;
    ws.quiet(50).await;
    ws.dispatch("d1", json!({ "type": "thread.meta.update", "commandId": "cmd-dup", "threadId": "ws-resume", "title": "Once" })).await;
    let arrived = ws.quiet(300).await;
    assert_eq!(arrived.iter().filter(|f| is_event(f, "same", "thread.meta-updated")).count(), 1);
    assert!(!arrived.iter().any(|f| f["id"] == "resume"));
    ws.send(subscribe("bad", "orchestration.subscribeThread", json!({}))).await;
    ws.send(subscribe("unknown", "orchestration.nope", json!({}))).await;
    assert_eq!(ws.next(|f| f["id"] == "bad").await, json!({ "kind": "response", "id": "bad", "ok": false, "error": error("invalid_params", "missing field `threadId`") }));
    assert_eq!(ws.next(|f| f["id"] == "unknown").await, json!({ "kind": "response", "id": "unknown", "ok": false, "error": error("unknown_subscription", "Unknown subscription: orchestration.nope") }));

    // When a subscribed thread is deleted, then its deletion arrives and the subscription completes once; the shell sees it too.
    ws.dispatch("x0", create("ws-delete")).await;
    ws.send(subscribe("del", "orchestration.subscribeThread", json!({ "threadId": "ws-delete" }))).await;
    ws.send(subscribe("shell2", "orchestration.subscribeShell", json!({}))).await;
    ws.next(|f| f["id"] == "del" && f["item"]["kind"] == "synchronized").await;
    ws.next(|f| f["id"] == "shell2" && f["item"]["kind"] == "synchronized").await;
    ws.quiet(50).await;
    ws.dispatch("x1", command("thread.delete", "ws-delete", json!({}))).await;
    let deleted = ws.next(|f| is_event(f, "del", "thread.deleted")).await;
    assert_eq!(deleted["item"]["event"]["payload"]["threadId"], "ws-delete");
    assert_eq!(ws.next(|f| f["kind"] == "complete").await, json!({ "kind": "complete", "id": "del" }));
    ws.next(|f| is_event(f, "shell2", "thread.deleted")).await;
    assert!(ws.quiet(200).await.iter().all(|f| f["kind"] != "complete"));
    ws.send(json!({ "kind": "unsubscribe", "id": "shell2" })).await;

    // Given a turn that creates its thread, when opencode reasons, runs a tool and answers, then the thread streams the user
    // message, the assistant's text once, the activities and a session that runs and settles.
    let tool = |s: &str, status: &str| ev("message.part.updated", json!({ "sessionID": s, "part": { "id": "t1", "type": "tool", "tool": "bash", "callID": "c1", "state": { "status": status, "input": { "command": "ls" }, "output": "a b" }, "messageID": "m", "sessionID": s } }));
    oc.on_prompt(Arc::new(move |p: &Prompt| {
        let s = p.session.as_str();
        vec![
            ev("message.updated", json!({ "sessionID": s, "info": { "id": "u1", "role": "user", "sessionID": s } })),
            ev("message.part.updated", json!({ "sessionID": s, "part": { "id": "echo", "type": "text", "text": p.text(), "messageID": "u1", "sessionID": s } })),
            ev("message.part.updated", json!({ "sessionID": s, "part": { "id": "r1", "type": "reasoning", "text": "hmm", "messageID": "a1", "sessionID": s } })),
            tool(s, "running"),
            tool(s, "completed"),
            ev("message.part.delta", json!({ "sessionID": s, "messageID": "a1", "partID": "p1", "field": "text", "delta": "hello" })),
            ev("message.part.updated", json!({ "sessionID": s, "part": { "id": "p1", "type": "text", "text": "hello back", "messageID": "a1", "sessionID": s } })),
            idle(s),
        ]
    }));
    svc.commands.answer(|r: &Run| (r.program == "git" && r.args == ["diff"]).then(|| r.ok("diff --git a/x b/x\n+changed\n")).flatten());
    ws.send(subscribe("turn", "orchestration.subscribeThread", json!({ "threadId": "ws-turn" }))).await;
    ws.next(|f| f["id"] == "turn").await;
    let receipt = ws.dispatch("tt", turn("ws-turn", "hi there", true, "default")).await["result"].clone();
    assert_eq!(receipt["status"], "accepted");
    assert_eq!(receipt["events"].as_array().unwrap().iter().map(|e| e["type"].clone()).collect::<Vec<_>>(), [json!("thread.created"), json!("thread.message-sent"), json!("thread.turn-start-requested")]);
    let running = ws.next(|f| is_event(f, "turn", "thread.session-set") && f["item"]["event"]["payload"]["session"]["status"] == "running").await;
    assert_eq!(running["item"]["event"]["payload"]["session"]["cli"], "opencode");
    assert!(running["item"]["event"]["payload"]["session"]["activeTurnId"].is_string());
    ws.next(|f| is_event(f, "turn", "thread.message-sent") && f["item"]["event"]["payload"]["message"]["role"] == "assistant").await;
    let ready = ws.next(|f| is_event(f, "turn", "thread.session-set") && f["item"]["event"]["payload"]["session"]["status"] == "ready").await;
    assert_eq!(ready["item"]["event"]["payload"]["session"]["cli"], "opencode");
    let prompt = oc.find("POST", "*/prompt_async").pop().unwrap();
    assert_eq!((prompt.directory(), prompt.body.clone().unwrap()["parts"].clone(), prompt.body.clone().unwrap()["model"].clone()), (Some("/tmp"), json!([{ "type": "text", "text": "hi there" }]), json!({ "providerID": "p", "modelID": "m" })));
    let done = thread(&mut ws, "ws-turn").await;
    assert_eq!(done["messages"].as_array().unwrap().iter().map(|m| (m["role"].clone(), m["text"].clone())).collect::<Vec<_>>(), [(json!("user"), json!("hi there")), (json!("assistant"), json!("hello back"))]);
    assert_eq!((done["latestTurn"]["state"].clone(), done["session"]["status"].clone()), (json!("completed"), json!("ready")));
    let kinds: Vec<String> = done["activities"].as_array().unwrap().iter().map(|a| a["kind"].as_str().unwrap().to_string()).collect();
    assert!(kinds.contains(&"reasoning.delta".to_string()) && kinds.contains(&"tool.started".to_string()) && kinds.contains(&"tool.completed".to_string()), "{kinds:?}");
    let started = done["activities"].as_array().unwrap().iter().find(|a| a["kind"] == "tool.started").unwrap();
    assert_eq!((started["summary"].clone(), started["payload"]["data"].clone()), (json!("bash started"), json!({ "command": "ls" })));
    let completed = done["activities"].as_array().unwrap().iter().find(|a| a["kind"] == "tool.completed").unwrap();
    assert_eq!((completed["summary"].clone(), completed["payload"]["data"].clone()), (json!("bash"), json!({ "output": "a b" })), "{completed}");
    let turn_id = done["latestTurn"]["turnId"].as_str().unwrap().to_string();
    // Then no git ran in its directory (the service holds no working tree: a turn's diff is `turn.diff` at the executor), and the turn's diff stays empty
    let diff = ws.request("diff", "orchestration.getTurnDiff", json!({ "threadId": "ws-turn", "turnId": turn_id })).await["result"]["unifiedDiff"].clone();
    assert!(svc.commands.of("git").is_empty(), "{:?}", svc.commands.of("git").len());
    assert_eq!(diff, "");

    // When opencode asks permission and a question during a turn, then each is an open request on the thread: another turn waits
    // until the question is answered, the approval and the answers reach opencode for their requests, and the turn settles.
    oc.on_prompt(Arc::new(|p: &Prompt| {
        let s = p.session.as_str();
        vec![
            ev("permission.asked", json!({ "id": "perm-1", "sessionID": s, "permission": "bash", "patterns": ["rm -rf"], "metadata": { "command": "rm -rf" } })),
            ev("question.asked", json!({ "id": "q-1", "sessionID": s, "questions": [{ "header": "Pick", "question": "Which?", "options": [{ "label": "A" }], "multiple": true }, { "header": "Why", "question": "Why?" }] })),
        ]
    }));
    oc.forget_requests();
    ws.dispatch("a0", turn("ws-turn", "ask", false, "default")).await;
    let asked = |t: &Value| t["activities"].as_array().unwrap().iter().any(|a| a["kind"] == "user-input.requested");
    let mut waiting = thread(&mut ws, "ws-turn").await;
    for _ in 0..200 {
        if asked(&waiting) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        waiting = thread(&mut ws, "ws-turn").await;
    }
    let approval = waiting["activities"].as_array().unwrap().iter().find(|a| a["kind"] == "approval.requested").expect("the approval is an activity").clone();
    assert_eq!((approval["summary"].clone(), approval["payload"]["requestId"].clone(), approval["payload"]["requestKind"].clone(), approval["payload"]["detail"].clone()), (json!("Command approval requested"), json!("perm-1"), json!("command"), json!("rm -rf")));
    let question = waiting["activities"].as_array().unwrap().iter().find(|a| a["kind"] == "user-input.requested").unwrap().clone();
    assert_eq!(question["payload"]["questions"][0], json!({ "id": "Pick", "header": "Pick", "question": "Which?", "options": [{ "label": "A" }], "multiSelect": true }));
    assert_eq!(question["payload"]["questions"][1]["multiSelect"], false);
    assert_eq!(ws.dispatch("a1", turn("ws-turn", "meanwhile", false, "default")).await["result"]["error"]["code"], "request_pending");
    assert_eq!(ws.dispatch("a2", command("thread.approval.respond", "ws-turn", json!({ "requestId": "perm-1", "decision": "acceptForSession" }))).await["result"]["status"], "accepted");
    assert_eq!(ws.dispatch("a3", command("thread.user-input.respond", "ws-turn", json!({ "requestId": "q-1", "answers": { "Pick": ["A"], "Why": "because" } }))).await["result"]["status"], "accepted");
    eventually("the answers reached opencode", || !oc.find("POST", "/question/q-1/reply").is_empty() && !oc.find("POST", "/permission/perm-1/reply").is_empty()).await;
    assert_eq!(oc.find("POST", "/permission/perm-1/reply")[0].body, Some(json!({ "reply": "always" })));
    assert_eq!(oc.find("POST", "/question/q-1/reply")[0].body, Some(json!({ "answers": [["A"], ["because"]] })));
    assert_eq!(oc.find("POST", "/question/q-1/reply")[0].directory(), Some("/tmp"));

    // When a turn is interrupted while it runs, a second turn arrives meanwhile, or the session is stopped, then opencode is told to
    // abort the session, the second turn is queued, and the stop forgets the session.
    oc.on_prompt(Arc::new(|_: &Prompt| Vec::new()));
    let session = oc.find("POST", "*/prompt_async").pop().unwrap().path.replace("/prompt_async", "");
    ws.dispatch("i0", create("ws-long")).await;
    ws.dispatch("i1", turn("ws-long", "long", false, "default")).await;
    let mut long = thread(&mut ws, "ws-long").await;
    for _ in 0..200 {
        if long["latestTurn"]["state"] == "running" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        long = thread(&mut ws, "ws-long").await;
    }
    let queued = ws.dispatch("i2", turn("ws-long", "later", false, "default")).await["result"]["events"][1].clone();
    assert_eq!((queued["type"].clone(), queued["payload"]["queued"].clone()), (json!("thread.turn-start-requested"), json!(true)));
    eventually("the turn reached opencode", || oc.find("POST", "*/prompt_async").iter().any(|r| r.body.as_ref().unwrap()["parts"][0]["text"] == "long")).await;
    let long_session = oc.find("POST", "*/prompt_async").into_iter().find(|r| r.body.as_ref().unwrap()["parts"][0]["text"] == "long").unwrap().path.replace("/prompt_async", "");
    assert_eq!(ws.dispatch("i3", command("thread.turn.interrupt", "ws-long", json!({}))).await["result"]["status"], "accepted");
    eventually("opencode was told to abort the turn", || !oc.find("POST", &format!("{long_session}/abort")).is_empty()).await;
    ws.dispatch("i4", command("thread.session.stop", "ws-turn", json!({}))).await;
    eventually("opencode was told to stop the session", || !oc.find("POST", &format!("{session}/abort")).is_empty()).await;

    // When a plan-mode turn finishes, then its last text is kept as the proposed plan; when opencode reports an error or ends its
    // event stream, or refuses the session or the prompt, then the session ends in error with the reason.
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "1. do this\n2. then that"), idle(&p.session)]));
    let mut asking = turn("ws-plan", "plan it", true, "plan");
    asking["runtimeMode"] = json!("approval-required");
    asking["bootstrap"]["createThread"]["runtimeMode"] = json!("approval-required");
    oc.forget_requests();
    ws.dispatch("pl", asking).await;
    let settled = |t: &Value| t["session"]["status"].is_string() && t["session"]["status"] != "running" && t["session"]["status"] != "starting";
    let mut planned = thread(&mut ws, "ws-plan").await;
    for _ in 0..300 {
        if settled(&planned) && !planned["proposedPlans"].as_array().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        planned = thread(&mut ws, "ws-plan").await;
    }
    assert_eq!(planned["proposedPlans"][0]["planMarkdown"], "1. do this\n2. then that");
    assert_eq!(oc.find("POST", "*/prompt_async").pop().unwrap().body.unwrap()["agent"], "plan");
    let rules = oc.find("PATCH", "*").pop().unwrap().body.unwrap()["permission"].clone();
    assert_eq!((rules.as_array().unwrap().len(), rules[0].clone(), rules[8].clone()), (9, json!({ "permission": "*", "pattern": "*", "action": "ask" }), json!({ "permission": "question", "pattern": "*", "action": "allow" })));
    let failures: [Failure; 4] = [
        ("ws-error", Box::new(|| oc.on_prompt(Arc::new(|p: &Prompt| vec![fails(&p.session, "rate limited")]))), "rate limited"),
        ("ws-broken", Box::new(|| oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "thinking")]))), "opencode ended its event stream"),
        ("ws-no-session", Box::new(|| oc.respond("POST", "/session", 500, json!({ "error": "no capacity" }))), "gateway.session.create returned no session id for thread ws-no-session"),
        ("ws-refused", Box::new(|| {
            oc.respond("POST", "/session", 200, json!({ "id": "oc-refused" }));
            oc.respond("POST", "/session/oc-refused/prompt_async", 500, json!({ "error": "no" }));
        }), "opencode /session/oc-refused/prompt_async?directory=%2Ftmp responded with 500"),
    ];
    for (id, arrange, reason) in failures {
        arrange();
        ws.send(subscribe(id, "orchestration.subscribeThread", json!({ "threadId": id }))).await;
        ws.next(|f| f["id"] == id && f["item"]["kind"] == "synchronized").await;
        ws.dispatch(id, turn(id, "go", true, "default")).await;
        if id == "ws-broken" {
            let mut said = thread(&mut ws, id).await;
            for _ in 0..300 {
                if said["messages"].as_array().unwrap().iter().any(|m| m["role"] == "assistant") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
                said = thread(&mut ws, id).await;
            }
            oc.end_streams();
        }
        let status = if id == "ws-broken" { "interrupted" } else { "error" };
        let failed = ws.next(|f| is_event(f, id, "thread.session-set") && f["item"]["event"]["payload"]["session"]["status"] == status).await;
        assert_eq!(failed["item"]["event"]["payload"]["session"]["lastError"], reason, "{id}");
        ws.send(json!({ "kind": "unsubscribe", "id": id })).await;
    }
    let mut aborted = thread(&mut ws, "ws-error").await;
    for _ in 0..300 {
        if settled(&aborted) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        aborted = thread(&mut ws, "ws-error").await;
    }
    assert_eq!((aborted["latestTurn"]["state"].clone(), aborted["session"]["status"].clone()), (json!("error"), json!("interrupted")));

    // When a connection drops with subscriptions open, then the service carries on; a quiet connection is pinged on the heartbeat.
    ws.send(subscribe("dropped", "orchestration.subscribeShell", json!({}))).await;
    ws.next(|f| f["id"] == "dropped" && f["item"]["kind"] == "synchronized").await;
    ws.close().await;
    let mut again = served.connect(&format!("?token={SECRET}")).await.unwrap();
    again.first().await;
    let last = again.dispatch("after", create("ws-after-drop")).await["result"]["events"][0]["sequence"].as_u64().unwrap();
    let beating = service(&env, Options { heartbeat: Duration::from_millis(100), ..Options::default() }).serve();
    let mut quiet = beating.connect(&format!("?token={SECRET}")).await.unwrap();
    assert_eq!(quiet.first().await["heartbeatIntervalMs"], 100);
    tokio::time::sleep(Duration::from_millis(450)).await;
    assert!(quiet.pings() >= 2, "pinged {} times", quiet.pings());
    quiet.send(json!({ "kind": "ping", "ts": 9 })).await;
    assert_eq!(quiet.first().await, json!({ "kind": "pong", "ts": 9 }), "the connection survives its heartbeat");

    // Given a thread id that would name another place on disk, when it is created, then the store refuses it.
    assert_eq!(again.dispatch("escape", create("..")).await["error"], error("dispatch_failed", "invalid thread id: \"..\""));

    // Given threads on disk with a turn left running and a log whose last line a crash cut off, when the service starts again over
    // the same state, then they are listed and
    // the turn that was running is interrupted, and a thread's sequence goes on from where it was.
    let log = state.join("threads/ws-after-drop/events.ndjson");
    std::fs::OpenOptions::new().append(true).open(&log).unwrap().write_all(b"{\"eventId\":\"cut").unwrap();
    let restarted = service(&env, Options { state_dir: Some(state.clone()), ..Options::default() }).serve();
    let mut after = restarted.connect(&format!("?token={SECRET}")).await.unwrap();
    after.first().await;
    let listed = after.request("l", "orchestration.listThreads", json!({})).await["result"]["threads"].clone();
    let titles: Vec<(Value, Value)> = listed.as_array().unwrap().iter().map(|t| (t["id"].clone(), t["title"].clone())).collect();
    assert!(titles.contains(&(json!("ws-shell-1"), json!("This one"))) && titles.contains(&(json!("ws-after-drop"), json!("Title of ws-after-drop"))), "{titles:?}");
    let resumed = thread(&mut after, "ws-long").await;
    assert_ne!(resumed["latestTurn"]["state"], "running", "{resumed}");
    let continued = after.dispatch("next", json!({ "type": "thread.meta.update", "commandId": "cmd-continued", "threadId": "ws-after-drop", "title": "Continued" })).await;
    assert_eq!(continued["result"]["events"][0]["sequence"].as_u64(), Some(last + 1));

    // Given a log with a corrupt line before its end, when the service starts over it, then that thread is not served.
    let other = state.join("threads/ws-other/events.ndjson");
    let lines = std::fs::read_to_string(&other).unwrap();
    std::fs::write(&other, format!("not json\n{lines}")).unwrap();
    let corrupt = service(&env, Options { state_dir: Some(state), ..Options::default() }).serve();
    let mut reader = corrupt.connect(&format!("?token={SECRET}")).await.unwrap();
    reader.first().await;
    assert_eq!(reader.request("o", "orchestration.getThread", json!({ "threadId": "ws-other" })).await["ok"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_an_opencode_that_accepts_a_prompt_and_then_stays_silent_when_the_turn_passes_its_deadline_then_the_thread_ends_in_error_opencode_is_told_to_abort_and_no_stream_stays_open() {
    let env = Env::new(&[]);
    let svc = service(&env, Options { hard_timeout_ms: 1_000, ..Options::default() });
    let oc = &svc.opencode;
    oc.on_prompt(Arc::new(|_: &Prompt| Vec::new()));
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;

    // When a turn starts, then opencode is prompted (the arrived probe) and the thread runs.
    ws.dispatch("t", turn("ws-silent", "go", true, "default")).await;
    eventually("the prompt reached opencode", || !oc.find("POST", "*/prompt_async").is_empty()).await;
    let session = oc.find("POST", "*/prompt_async")[0].path.replace("/prompt_async", "");
    assert_eq!(oc.subscribers(), 1);

    // When the deadline passes, then the session is an error naming it, opencode was told to abort, and its stream is closed.
    let mut read = thread(&mut ws, "ws-silent").await;
    for _ in 0..400 {
        if read["session"]["status"] == "error" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        read = thread(&mut ws, "ws-silent").await;
    }
    assert_eq!(read["session"]["status"], "error", "{read}");
    assert!(read["session"]["lastError"].as_str().unwrap().starts_with("hard_timeout"), "{read}");
    eventually("opencode was told to abort", || !oc.find("POST", &format!("{session}/abort")).is_empty()).await;
    eventually("the event stream is closed", || oc.subscribers() == 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_threads_the_engine_owns_when_opencode_goes_down_then_stop_reply_and_interrupt_say_so_instead_of_reporting_success() {
    let env = Env::new(&[]);
    let svc = service(&env, Options { hard_timeout_ms: 60_000, ..Options::default() });
    let oc = &svc.opencode;
    oc.on_prompt(Arc::new(|p: &Prompt| {
        if p.text() == "ask" {
            vec![ev("permission.asked", json!({ "id": "perm-1", "sessionID": p.session, "permission": "bash", "patterns": ["ls"], "metadata": { "command": "ls" } }))]
        } else {
            Vec::new()
        }
    }));
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;
    ws.dispatch("a", turn("ws-run", "run", true, "default")).await;
    ws.dispatch("b", turn("ws-ask", "ask", true, "default")).await;
    let mut asked = thread(&mut ws, "ws-ask").await;
    for _ in 0..300 {
        if asked["activities"].as_array().unwrap().iter().any(|a| a["kind"] == "approval.requested") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        asked = thread(&mut ws, "ws-ask").await;
    }
    eventually("both turns reached opencode", || oc.find("POST", "*/prompt_async").len() == 2).await;
    let run_session = oc.find("POST", "*/prompt_async").into_iter().find(|r| r.body.as_ref().unwrap()["parts"][0]["text"] == "run").unwrap().path.replace("/prompt_async", "");
    let aborts = || oc.find("POST", &format!("{run_session}/abort")).len();
    let stop = || post("/api/control/stop").json(json!({ "sessionId": "ws-run", "directory": "/tmp" }));
    let reply = || post("/api/question/perm-1/reply").json(json!({ "directory": "/tmp", "answers": [["accept"]] }));
    let failed = |t: &Value| t["activities"].as_array().unwrap().iter().filter(|a| a["kind"] == "command-failed").count();

    // Given opencode up, when the running thread is stopped and the open request answered, then 200 and 204 and opencode saw both.
    assert_eq!(svc.send(stop()).await.status, StatusCode::OK);
    assert_eq!(svc.send(reply()).await.status, StatusCode::NO_CONTENT);
    eventually("opencode saw both", || aborts() == 1 && oc.find("POST", "/permission/perm-1/reply").len() == 1).await;
    assert_eq!(failed(&thread(&mut ws, "ws-run").await), 0);

    // Given opencode down, when the same calls are made, then 500 and 502 with the reason, the call was attempted, and the thread
    // records a command-failed activity; an interrupt over the socket is accepted and leaves the same record.
    oc.go_down();
    let stopped = svc.send(stop()).await;
    assert_eq!(stopped.status, StatusCode::INTERNAL_SERVER_ERROR, "{}", stopped.text());
    assert!(stopped.json()["error"].as_str().unwrap().contains("error sending request"), "{}", stopped.text());
    assert_eq!(aborts(), 2);
    let answered = svc.send(reply()).await;
    assert_eq!(answered.status, StatusCode::BAD_GATEWAY, "{}", answered.text());
    assert_eq!(oc.find("POST", "/permission/perm-1/reply").len(), 2);
    assert_eq!(ws.dispatch("i", command("thread.turn.interrupt", "ws-run", json!({}))).await["result"]["status"], "accepted");
    let mut run = thread(&mut ws, "ws-run").await;
    for _ in 0..300 {
        if failed(&run) >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        run = thread(&mut ws, "ws-run").await;
    }
    assert_eq!(failed(&run), 2, "{run}");
    assert!(failed(&thread(&mut ws, "ws-ask").await) >= 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_one_thread_log_that_cannot_be_read_when_the_service_restarts_then_the_other_threads_are_listed_and_a_cut_log_heals_on_the_next_append() {
    let env = Env::new(&[]);
    let state = env.dir("state");
    let first = service(&env, Options { state_dir: Some(state.clone()), hard_timeout_ms: 60_000, ..Options::default() });
    first.opencode.on_prompt(Arc::new(|_: &Prompt| Vec::new()));
    let served = first.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;
    ws.dispatch("c1", create("ws-good")).await;
    ws.dispatch("c2", turn("ws-busy", "go", true, "default")).await;
    eventually("the turn is running", || first.opencode.find("POST", "*/prompt_async").len() == 1).await;

    // Given a log that is not JSON beside the good ones and a turn left running, when the service starts again, then the good threads
    // are listed, the running turn is interrupted, and the unreadable one is refused by name (the arrived probe).
    std::fs::create_dir_all(state.join("threads/ws-bad")).unwrap();
    let good_log = std::fs::read_to_string(state.join("threads/ws-good/events.ndjson")).unwrap();
    std::fs::write(state.join("threads/ws-bad/events.ndjson"), format!("not json\n{good_log}")).unwrap();
    let restarted = service(&env, Options { state_dir: Some(state.clone()), ..Options::default() }).serve();
    let mut after = restarted.connect(&format!("?token={SECRET}")).await.unwrap();
    after.first().await;
    let listed = after.request("l", "orchestration.listThreads", json!({})).await["result"]["threads"].clone();
    let ids: Vec<String> = listed.as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_string()).collect();
    assert!(ids.contains(&"ws-good".to_string()) && ids.contains(&"ws-busy".to_string()), "{ids:?}");
    assert!(!ids.contains(&"ws-bad".to_string()), "{ids:?}");
    assert_ne!(thread(&mut after, "ws-busy").await["latestTurn"]["state"], "running");
    assert_eq!(after.request("bad", "orchestration.getThread", json!({ "threadId": "ws-bad" })).await["ok"], false);

    // Given a log that ends mid-line, when an event is appended and the service starts again, then the thread is still listed.
    std::fs::OpenOptions::new().append(true).open(state.join("threads/ws-good/events.ndjson")).unwrap().write_all(b"{\"eventId\":\"cut").unwrap();
    let again = service(&env, Options { state_dir: Some(state.clone()), ..Options::default() }).serve();
    let mut writer = again.connect(&format!("?token={SECRET}")).await.unwrap();
    writer.first().await;
    let renamed = writer.dispatch("m", json!({ "type": "thread.meta.update", "commandId": "cmd-heal", "threadId": "ws-good", "title": "Healed" })).await;
    assert_eq!(renamed["result"]["status"], "accepted", "{renamed}");
    let last = service(&env, Options { state_dir: Some(state), ..Options::default() }).serve();
    let mut reader = last.connect(&format!("?token={SECRET}")).await.unwrap();
    reader.first().await;
    let listed = reader.request("l2", "orchestration.listThreads", json!({})).await["result"]["threads"].clone();
    let good = listed.as_array().unwrap().iter().find(|t| t["id"] == "ws-good").cloned();
    assert_eq!(good.map(|t| t["title"].clone()), Some(json!("Healed")), "{listed}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_two_turns_that_each_end_in_their_own_idle_when_both_have_run_then_both_completed_and_no_event_stream_stays_open() {
    let env = Env::new(&[]);
    let svc = service(&env, Options { hard_timeout_ms: 60_000, ..Options::default() });
    let oc = &svc.opencode;
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, &p.text()), idle(&p.session)]));
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;
    for (i, text) in ["one", "two"].iter().enumerate() {
        ws.dispatch(&format!("t{i}"), turn("ws-seq", text, i == 0, "default")).await;
        eventually("the turn settled", || oc.find("POST", "*/prompt_async").len() == i + 1).await;
        let mut read = thread(&mut ws, "ws-seq").await;
        for _ in 0..300 {
            if read["session"]["status"] == "ready" && read["messages"].as_array().unwrap().iter().filter(|m| m["role"] == "assistant").count() == i + 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
            read = thread(&mut ws, "ws-seq").await;
        }
        assert_eq!(read["session"]["status"], "ready", "{read}");
    }
    eventually("the event stream is closed", || oc.subscribers() == 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_turn_waiting_on_an_approval_when_the_deadline_passes_then_it_is_not_aborted_and_it_finishes_once_answered() {
    let env = Env::new(&[]);
    let svc = service(&env, Options { hard_timeout_ms: 1_000, ..Options::default() });
    let oc = &svc.opencode;
    oc.on_prompt(Arc::new(|p: &Prompt| vec![ev("permission.asked", json!({ "id": "perm-1", "sessionID": p.session, "kind": "command" }))]));
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;

    // Given a turn parked on an approval, when more than the deadline passes, then the session still runs and opencode was not told to abort.
    ws.dispatch("t", turn("ws-parked", "go", true, "default")).await;
    eventually("the prompt reached opencode", || !oc.find("POST", "*/prompt_async").is_empty()).await;
    let session = oc.find("POST", "*/prompt_async")[0].path.replace("/prompt_async", "");
    tokio::time::sleep(Duration::from_millis(1_600)).await;
    let parked = thread(&mut ws, "ws-parked").await;
    assert_eq!(parked["session"]["status"], "running", "{parked}");
    assert!(oc.find("POST", &format!("{session}/abort")).is_empty());

    // When the approval is answered and the agent finishes, then the turn completes instead of timing out.
    let id = session.rsplit('/').next().unwrap().to_string();
    oc.emit(ev("permission.replied", json!({ "id": "perm-1", "sessionID": id, "reply": "once" })));
    oc.emit(idle(&id));
    let mut read = thread(&mut ws, "ws-parked").await;
    for _ in 0..300 {
        if read["session"]["status"] == "ready" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        read = thread(&mut ws, "ws-parked").await;
    }
    assert_eq!(read["session"]["status"], "ready", "{read}");
    eventually("the event stream is closed", || oc.subscribers() == 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_second_turn_queued_behind_the_first_when_together_they_outlast_the_deadline_then_each_has_its_own_budget() {
    let env = Env::new(&[]);
    let svc = service(&env, Options { hard_timeout_ms: 1_000, ..Options::default() });
    let oc = &svc.opencode;
    oc.on_prompt(Arc::new(|_: &Prompt| Vec::new()));
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;

    // Given two turns queued on one thread, when each takes 700 ms (1.4 s together, over the 1 s deadline), then both complete.
    ws.dispatch("t0", turn("ws-queued", "one", true, "default")).await;
    ws.dispatch("t1", turn("ws-queued", "two", false, "default")).await;
    eventually("both prompts reached opencode", || oc.find("POST", "*/prompt_async").len() == 2).await;
    let session = oc.find("POST", "*/prompt_async")[0].path.replace("/prompt_async", "");
    let id = session.rsplit('/').next().unwrap().to_string();
    tokio::time::sleep(Duration::from_millis(700)).await;
    oc.emit(idle(&id));
    tokio::time::sleep(Duration::from_millis(700)).await;
    oc.emit(idle(&id));
    let mut read = thread(&mut ws, "ws-queued").await;
    for _ in 0..300 {
        if read["session"]["status"] == "ready" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        read = thread(&mut ws, "ws-queued").await;
    }
    assert_eq!(read["session"]["status"], "ready", "{read}");
    assert!(oc.find("POST", &format!("{session}/abort")).is_empty());
    eventually("the event stream is closed", || oc.subscribers() == 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_turn_running_and_an_opencode_that_sends_one_idle_for_two_queued_prompts_when_a_second_turn_is_queued_and_the_run_settles_then_both_turns_are_settled_and_no_stream_stays_open() {
    let env = Env::new(&[]);
    let svc = service(&env, Options { hard_timeout_ms: 60_000, ..Options::default() });
    let oc = &svc.opencode;
    oc.on_prompt(Arc::new(|_: &Prompt| Vec::new()));
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;

    // Given a turn running on a thread, when a second turn is queued and opencode folds both prompts into one run that ends in one idle
    ws.dispatch("t0", turn("ws-folded", "one", true, "default")).await;
    ws.dispatch("t1", turn("ws-folded", "two", false, "default")).await;
    eventually("both prompts reached opencode", || oc.find("POST", "*/prompt_async").len() == 2).await;
    let session = oc.find("POST", "*/prompt_async")[0].path.replace("/prompt_async", "");
    oc.emit(idle(session.rsplit('/').next().unwrap()));

    // Then the thread is settled and the event stream is closed without waiting for a second idle or the deadline.
    eventually("the event stream is closed", || oc.subscribers() == 0).await;
    let read = thread(&mut ws, "ws-folded").await;
    assert_eq!((read["session"]["status"].clone(), read["latestTurn"]["state"].clone()), (json!("ready"), json!("completed")), "{read}");
    assert!(oc.find("POST", &format!("{session}/abort")).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_turn_running_in_an_opencode_session_when_only_the_agent_restarts_then_the_stale_turn_is_aborted_and_a_queued_turn_goes_to_the_same_session() {
    let env = Env::new(&[]);
    let state = env.dir("state");
    let first = service(&env, Options { state_dir: Some(state.clone()), hard_timeout_ms: 60_000, ..Options::default() });
    first.opencode.on_prompt(Arc::new(|_: &Prompt| Vec::new()));
    let oc = Arc::clone(&first.opencode);
    let served = first.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;
    ws.dispatch("t1", turn("ws-restart", "go", true, "default")).await;
    eventually("the first prompt reached opencode", || oc.find("POST", "*/prompt_async").len() == 1).await;
    assert_eq!(oc.find("POST", "*/prompt_async")[0].path, "/session/oc-ses-1/prompt_async");

    // Given the agent restarting alone over the same state and the same opencode, then the stale turn is aborted in the session it ran in.
    let restarted = service_over(&env, Options { state_dir: Some(state), hard_timeout_ms: 60_000, ..Options::default() }, Arc::clone(&oc), Arc::clone(&first.commands), Arc::clone(&first.outbound)).serve();
    let mut after = restarted.connect(&format!("?token={SECRET}")).await.unwrap();
    after.first().await;
    eventually("the restart sweep aborted oc-ses-1", || !oc.find("POST", "/session/oc-ses-1/abort").is_empty()).await;
    assert_eq!(thread(&mut after, "ws-restart").await["latestTurn"]["state"], "interrupted");

    // When a turn is queued on that thread, then its prompt goes to oc-ses-1 and no second session is created (the arrived probe is
    // the prompt itself).
    after.dispatch("t2", turn("ws-restart", "again", false, "default")).await;
    eventually("the queued prompt reached opencode", || oc.find("POST", "*/prompt_async").len() == 2).await;
    assert_eq!(oc.find("POST", "*/prompt_async")[1].path, "/session/oc-ses-1/prompt_async");
    assert_eq!(oc.find("POST", "/session").len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_turn_running_in_an_opencode_session_when_opencode_restarts_and_ends_its_streams_then_the_thread_errors_keeps_the_session_and_the_next_turn_goes_to_it() {
    // Given a turn running in oc-ses-1 that has said something, the thread followed
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let oc = Arc::clone(&svc.opencode);
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "thinking")]));
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;
    ws.send(json!({ "kind": "subscribe", "id": "oc", "method": "orchestration.subscribeThread", "params": { "threadId": "ws-oc-restart" } })).await;
    ws.next(|f| f["id"] == "oc" && f["item"]["kind"] == "synchronized").await;
    ws.dispatch("t1", turn("ws-oc-restart", "go", true, "default")).await;
    let mut said = thread(&mut ws, "ws-oc-restart").await;
    for _ in 0..300 {
        if said["messages"].as_array().unwrap().iter().any(|m| m["role"] == "assistant") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        said = thread(&mut ws, "ws-oc-restart").await;
    }
    // When opencode restarts: every event stream it served ends
    oc.end_streams();
    // Then the thread reports the turn interrupted and still holds oc-ses-1
    let failed = ws.next(|f| is_event(f, "oc", "thread.session-set") && f["item"]["event"]["payload"]["session"]["status"] == "interrupted").await;
    assert_eq!(failed["item"]["event"]["payload"]["session"]["lastError"], "opencode ended its event stream");
    assert_eq!(failed["item"]["event"]["payload"]["session"]["providerSessionId"], "oc-ses-1", "{failed}");
    // And the next turn goes to that session, and no second session is created
    oc.on_prompt(Arc::new(|p: &Prompt| vec![says(&p.session, "back"), idle(&p.session)]));
    ws.dispatch("t2", turn("ws-oc-restart", "again", false, "default")).await;
    eventually("the next prompt reached opencode", || oc.find("POST", "*/prompt_async").len() == 2).await;
    assert_eq!(oc.find("POST", "*/prompt_async")[1].path, "/session/oc-ses-1/prompt_async");
    assert_eq!(oc.find("POST", "/session").len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn given_a_held_subscription_id_when_it_is_replaced_while_the_old_thread_is_busy_then_no_frame_of_the_old_thread_follows_the_new_snapshot() {
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;
    ws.dispatch("ca", create("ws-old")).await;
    ws.dispatch("cb", create("ws-new")).await;
    let subscribe = |thread: &str| json!({ "kind": "subscribe", "id": "x", "method": "orchestration.subscribeThread", "params": { "threadId": thread } });

    for round in 0..40 {
        // Given `x` held on the old thread, when sixty events are committed on it as `x` is subscribed to another thread.
        ws.send(subscribe("ws-old")).await;
        ws.next(|f| f["id"] == "x" && f["item"]["kind"] == "synchronized").await;
        let mut writer = served.connect(&format!("?token={SECRET}")).await.unwrap();
        writer.first().await;
        let busy = tokio::spawn(async move {
            for i in 0..60 {
                writer.dispatch(&format!("w{i}"), json!({ "type": "thread.meta.update", "commandId": format!("cmd-busy-{round}-{i}"), "threadId": "ws-old", "title": format!("Title {round}-{i}") })).await;
            }
        });
        ws.send(subscribe("ws-new")).await;
        busy.await.unwrap();

        // Then the new thread's snapshot arrives (the arrived probe) and nothing of the old thread follows it.
        let frames = ws.quiet(80).await;
        let at = frames.iter().position(|f| f["id"] == "x" && f["item"]["kind"] == "snapshot" && f["item"]["snapshot"]["id"] == "ws-new").expect("the new snapshot arrived");
        let stale: Vec<&Value> = frames[at..].iter().filter(|f| f["id"] == "x" && (f["item"]["event"]["threadId"] == "ws-old" || f["item"]["snapshot"]["id"] == "ws-old")).collect();
        assert!(stale.is_empty(), "round {round}: {} frames of the old thread after the new snapshot: {:?}", stale.len(), stale.first());
        ws.send(json!({ "kind": "unsubscribe", "id": "x" })).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_turn_with_an_image_attachment_when_it_is_dispatched_over_the_socket_then_it_is_refused_by_name_and_the_same_turn_without_one_reaches_opencode() {
    let env = Env::new(&[]);
    let svc = service(&env, Options::default());
    let oc = &svc.opencode;
    let served = svc.serve();
    let mut ws = served.connect(&format!("?token={SECRET}")).await.unwrap();
    ws.first().await;

    // Given a turn whose message carries a picture, when it is dispatched, then the failure names the problem, no thread is made and
    // opencode hears nothing.
    let mut with_picture = turn("ws-picture", "look", true, "default");
    with_picture["message"]["attachments"] = json!([{ "type": "image", "id": "att-1", "name": "a.png", "mimeType": "image/png", "sizeBytes": 4 }]);
    let refused = ws.dispatch("a", with_picture).await;
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["code"], "attachments_unsupported", "{refused}");
    assert_eq!(ws.request("g", "orchestration.getThread", json!({ "threadId": "ws-picture" })).await["error"]["code"], "thread_not_found");
    assert!(oc.find("POST", "*/prompt_async").is_empty());

    // When the same turn is sent without attachments (or with an empty list), then it starts and opencode is prompted.
    let mut plain = turn("ws-picture", "look", true, "default");
    plain["message"]["attachments"] = json!([]);
    assert_eq!(ws.dispatch("b", plain).await["result"]["status"], "accepted");
    eventually("the prompt reached opencode", || oc.find("POST", "*/prompt_async").len() == 1).await;
}
