//! The session rows through the `Host` dispatcher with an in-memory thread set as rca: each reply is read the way the phone reads it, and the
//! agent methods the mapper sent are held to its `USES` list. Given/When/Then in each test.
use orca_core::connection::Connection;
use orca_core::AgentApi;
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Host, HostInfo, ServiceStatus};
use orca_map_agent::testing::{Catalog, Threads};
use orca_map_agent::{body_fingerprint, AgentArea, NOT_OFFERED, USES};
use orca_map_workspace::testing::Workspaces;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

const EVENTS: &str = include_str!("../../../../libraries/agent-api/agent-api-agent/tests/fixtures/thread-events.jsonl");

struct Rig {
    host: Arc<Host>,
    agent: Arc<ScriptedAgent>,
    threads: Threads,
    catalog: Catalog,
    conn: Connection,
    other: Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let files = Workspaces::default();
        files.workspace("main", "root", None).workspace("feat", "worktree", Some("/srv/wt/feat"));
        files.install(&agent);
        let threads = Threads::default();
        threads.install(&agent);
        let catalog = Catalog::default();
        catalog.has(&["claude", "codex"]).skill("workspace", ".claude/skills", "review", Some("Reviews a diff")).skill("workspace", ".codex/skills", "lint", None).skill("home", ".agents/skills", "notes", Some("Keeps notes"));
        catalog.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(AgentArea::default())], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { conn: host.connection("dev", "tok", vec![]), other: host.connection("dev2", "tok2", vec![]), host, agent, threads, catalog, n: 0 }
    }

    fn frames(&mut self, method: &str, params: Value) -> Vec<Value> {
        self.n += 1;
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        self.conn.on_message(&frame).iter().map(|f| serde_json::from_str(f).unwrap()).collect()
    }

    fn other_err(&mut self, method: &str, params: Value) -> (String, String) {
        self.n += 1;
        let frame = json!({"id": format!("o{}", self.n), "method": method, "params": params}).to_string();
        let r: Value = serde_json::from_str(&self.other.on_message(&frame)[0]).unwrap();
        assert_eq!(r["ok"], false, "{method} should be refused: {r}");
        (r["error"]["code"].as_str().unwrap().to_string(), r["error"]["message"].as_str().unwrap().to_string())
    }

    /// An upload the way the phone makes one: a start, appends of at most 384 KiB of bytes each, a commit; the committed path.
    fn upload(&mut self, session: &str, name: &str, bytes: &[u8]) -> Value {
        let id = self.ok("agentSessionAttachment.uploadStart", json!({"sessionId": session, "name": name, "byteLength": bytes.len()}))["uploadId"].clone();
        for (i, chunk) in bytes.chunks(384 * 1024).enumerate() {
            let r = self.ok("agentSessionAttachment.uploadAppend", json!({"uploadId": id, "offset": i * 384 * 1024, "contentBase64": STANDARD.encode(chunk)}));
            assert_eq!(r["receivedBytes"], (i * 384 * 1024 + chunk.len()) as u64);
        }
        self.ok("agentSessionAttachment.uploadCommit", json!({"uploadId": id}))
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.frames(method, params).remove(0)
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert_eq!(r["ok"], true, "{method}: {r}");
        r["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> (String, String) {
        let r = self.call(method, params);
        assert_eq!(r["ok"], false, "{method} should be refused: {r}");
        (r["error"]["code"].as_str().unwrap().to_string(), r["error"]["message"].as_str().unwrap().to_string())
    }

    /// A thread of the double with the recorded events of the fixture (two turns, no turn running).
    fn recorded(&mut self, id: &str) {
        self.agent.request("thread.create", json!({"thread": id, "workspace": "main", "driver": "claudeAgent"})).unwrap();
        for line in EVENTS.lines() {
            let mut event = serde_json::from_str::<Value>(line).unwrap()["event"].clone();
            event.as_object_mut().unwrap().remove("seq");
            self.threads.push(id, event);
        }
    }

    fn idle(&mut self, id: &str) {
        self.agent.request("thread.create", json!({"thread": id, "workspace": "main", "driver": "claudeAgent"})).unwrap();
    }
}

fn env(session: &str, op: &str, fingerprint: &str) -> Value {
    json!({"sessionId": session, "clientOperationId": op, "expectedRuntimeFence": null, "payloadFingerprint": fingerprint})
}

fn body(text: &str) -> Value {
    json!({"kind": "message", "role": "user", "blocks": [{"type": "text", "text": text}]})
}

fn create_params(op: &str, text: &str) -> Value {
    json!({"envelope": env("claude_s1", op, "fp1"), "worktree": "id:main::", "agent": "claude", "firstMessage": {"clientMessageId": "m1", "body": body(text)}})
}

fn sent(rig: &Rig, method: &str) -> Vec<Value> {
    rig.agent.requests().into_iter().filter(|(m, _)| m == method).map(|(_, p)| p).collect()
}

#[test]
fn given_a_new_chat_with_a_first_message_when_created_then_the_thread_runs_it_in_the_workspace_and_the_page_shows_the_message_and_a_retry_runs_nothing_again() {
    // Given a host with a workspace `main`
    let mut rig = Rig::new();
    // When the phone creates a chat for claude there with a first message
    let r = rig.ok("agentSession.create", create_params("op1", "fix the login"));
    // Then the thread is the session, run by claude's driver in main, and its first turn carries the text
    assert_eq!(sent(&rig, "thread.create"), vec![json!({"thread": "claude_s1", "workspace": "main", "driver": "claudeAgent"})]);
    assert_eq!(sent(&rig, "turn.start").iter().map(|p| (p["prompt"].clone(), p["driver"].clone(), p["workspace"].clone())).collect::<Vec<_>>(), vec![(json!("fix the login"), json!("claudeAgent"), json!("main"))]);
    // And the answer is an accepted mutation whose value names the session, the first message and a page holding it
    assert_eq!((r["ok"].clone(), r["replayed"].clone(), r["value"]["sessionId"].clone(), r["value"]["firstMessage"]["clientMessageId"].clone(), r["value"]["firstMessage"]["dispatchState"].clone()), (json!(true), json!(false), json!("claude_s1"), json!("m1"), json!("accepted")));
    let items = r["value"]["page"]["items"].as_array().unwrap();
    assert!(items.iter().any(|i| i["body"]["kind"] == "message" && i["body"]["blocks"][0]["text"] == "fix the login"));
    assert_eq!((r["value"]["page"]["working"].clone(), r["cursor"]["epoch"].clone()), (json!(true), json!("rca")));
    // When the same operation is sent again, Then the recorded answer comes back marked replayed and no second turn started
    let again = rig.ok("agentSession.create", create_params("op1", "fix the login"));
    assert_eq!((again["replayed"].clone(), again["value"]["sessionId"].clone()), (json!(true), json!("claude_s1")));
    assert_eq!(sent(&rig, "turn.start").len(), 1);
    // And the same operation id with another payload is a conflict, which starts nothing either
    let mut other = create_params("op1", "fix the login");
    other["envelope"]["payloadFingerprint"] = "fp-other".into();
    let conflict = rig.ok("agentSession.create", other);
    assert_eq!((conflict["ok"].clone(), conflict["refusal"]["code"].clone()), (json!(false), json!("agent_session_operation_conflict")));
    assert_eq!(sent(&rig, "turn.start").len(), 1);
}

#[test]
fn given_creates_the_host_refuses_when_sent_then_each_is_refused_before_a_thread_exists() {
    // Given a host
    let mut rig = Rig::new();
    // When an agent with no driver, an id of another agent's lane, an unknown worktree and a provider handle to adopt are sent
    let unknown = rig.err("agentSession.create", json!({"envelope": env("gemini_s1", "o1", "f"), "worktree": "main", "agent": "gemini"}));
    let lane = rig.err("agentSession.create", json!({"envelope": env("codex_s1", "o2", "f"), "worktree": "main", "agent": "claude"}));
    let worktree = rig.err("agentSession.create", json!({"envelope": env("claude_s2", "o3", "f"), "worktree": "nowhere", "agent": "claude"}));
    let adopt = rig.err("agentSession.create", json!({"envelope": env("claude_s3", "o4", "f"), "location": {}, "provider": "claude", "agent": "claude", "providerHandle": {"kind": "claude", "sessionId": "x", "leafUuid": null}}));
    // Then each is refused with the host's word, and rca was never asked to create a thread
    assert_eq!(unknown, ("runtime_error".to_string(), "structured_agent_session_unsupported".to_string()));
    assert_eq!(lane.0, "invalid_argument");
    assert_eq!(worktree, ("selector_not_found".to_string(), "selector_not_found".to_string()));
    assert_eq!(adopt, ("runtime_error".to_string(), "structured_agent_session_unsupported".to_string()));
    assert!(sent(&rig, "thread.create").is_empty());
}

#[test]
fn given_an_idle_and_a_running_session_when_sent_to_then_an_idle_one_starts_a_turn_a_running_one_steers_or_queues_and_an_image_is_refused() {
    // Given an idle session and a running one
    let mut rig = Rig::new();
    rig.idle("claude_idle");
    rig.idle("claude_busy");
    rig.agent.request("turn.start", json!({"thread": "claude_busy", "workspace": "main", "driver": "claudeAgent", "prompt": "first"})).unwrap();
    let send = |session: &str, op: &str, extra: Value| {
        let mut p = json!({"envelope": env(session, op, "f"), "body": body("next")});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    // When each is sent to: the idle one plainly with a model, the busy one plainly, and the busy one queue-if-active
    let idle = rig.ok("agentSession.send", send("claude_idle", "a1", json!({"options": {"model": "opus"}})));
    let steer = rig.ok("agentSession.send", send("claude_busy", "b1", json!({})));
    let queued = rig.ok("agentSession.send", send("claude_busy", "b2", json!({"delivery": "queue-if-active"})));
    // Then the idle one started a turn with the model and answers a submission under the operation id
    assert_eq!(sent(&rig, "turn.start").iter().filter(|p| p["thread"] == "claude_idle").map(|p| (p["prompt"].clone(), p["model"].clone())).collect::<Vec<_>>(), vec![(json!("next"), json!("opus"))]);
    assert_eq!((idle["value"]["clientMessageId"].clone(), idle["value"]["submission"]["dispatchState"].clone()), (json!("a1"), json!("accepted")));
    // And the busy one steered the text into its turn and, queue-if-active, queued it behind it with the draft receipt
    assert_eq!(sent(&rig, "turn.steer").len(), 1);
    assert_eq!(steer["value"]["submission"]["clientMessageId"], "b1");
    assert_eq!((queued["value"]["queued"]["state"].clone(), queued["value"]["queued"]["position"].clone(), rig.threads.queued("claude_busy")), (json!("waiting"), json!(1), vec!["next".to_string()]));
    // And a retry of the first send answers the recorded answer and starts no turn
    let retry = rig.ok("agentSession.send", send("claude_idle", "a1", json!({"options": {"model": "opus"}})));
    assert_eq!(retry["replayed"], true);
    assert_eq!(sent(&rig, "turn.start").iter().filter(|p| p["thread"] == "claude_idle").count(), 1);
    // And an image block is refused, and a session nobody created is the host's own refusal
    let mut with_image = send("claude_idle", "a2", json!({}));
    with_image["body"]["blocks"] = json!([{"type": "image-ref", "path": "a.png"}]);
    assert_eq!(rig.err("agentSession.send", with_image).0, "invalid_argument");
    assert_eq!(rig.err("agentSession.send", send("claude_nobody", "z1", json!({}))), ("runtime_error".to_string(), "agent_session_not_found".to_string()));
}

#[test]
fn given_a_running_turn_and_open_prompts_when_cancelled_then_the_turn_is_interrupted_and_a_prompt_is_closed_only_at_the_revision_the_phone_saw() {
    // Given the recorded thread (approval ap1 and question rq1 still open) and a running thread
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    rig.idle("claude_run");
    rig.agent.request("turn.start", json!({"thread": "claude_run", "workspace": "main", "driver": "claudeAgent", "prompt": "go"})).unwrap();
    let cancel = |session: &str, op: &str, extra: Value| {
        let mut p = json!({"envelope": env(session, op, "f")});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    // When the running turn is cancelled, then cancelled again when none runs
    let first = rig.ok("agentSession.cancel", cancel("claude_run", "c1", json!({})));
    let second = rig.ok("agentSession.cancel", cancel("claude_run", "c2", json!({})));
    // Then the first stopped the turn, whose end is in the log, and the second says nothing was cancelled
    assert_eq!((first["value"]["cancelled"].clone(), first["value"]["turnId"].clone(), rig.threads.running("claude_run")), (json!(true), json!("u1"), None));
    assert_eq!(rig.threads.events("claude_run").last().unwrap()["reason"], "interrupted");
    assert_eq!(second["value"]["cancelled"], false);
    assert_eq!(rig.ok("agentSession.history", json!({"sessionId": "claude_run", "direction": "tail"}))["page"]["queuePause"]["reason"], "stopped");
    // When the question is cancelled at a stale revision and then at the one the page showed
    let page = rig.ok("agentSession.history", json!({"sessionId": "claude_rec", "direction": "tail"}));
    let question = page["page"]["items"].as_array().unwrap().iter().find(|i| i["itemId"] == "question:rq1").unwrap().clone();
    let stale = rig.ok("agentSession.cancel", cancel("claude_rec", "c3", json!({"turnId": "u1", "prompt": {"itemId": "question:rq1", "expectedRevision": 1}})));
    let right = rig.ok("agentSession.cancel", cancel("claude_rec", "c4", json!({"turnId": "u1", "prompt": {"itemId": "question:rq1", "expectedRevision": question["revision"]}})));
    // Then the stale one is refused and asked nothing, and the right one dismissed the question
    assert_eq!((stale["ok"].clone(), stale["refusal"]["code"].clone()), (json!(false), json!("agent_session_item_revision_stale")));
    assert_eq!(sent(&rig, "input.dismiss"), vec![json!({"thread": "claude_rec", "request": "rq1"})]);
    assert_eq!(right["value"]["cancelled"], true);
    // And an approval cancelled is declined, and one closed already is refused as resolved
    let approval = page["page"]["items"].as_array().unwrap().iter().find(|i| i["itemId"] == "approval:ap1").unwrap().clone();
    rig.ok("agentSession.cancel", cancel("claude_rec", "c5", json!({"turnId": "u1", "prompt": {"itemId": "approval:ap1", "expectedRevision": approval["revision"]}})));
    assert_eq!(sent(&rig, "approval.answer"), vec![json!({"thread": "claude_rec", "approval": "ap1", "decision": "decline"})]);
    let closed = rig.ok("agentSession.cancel", cancel("claude_rec", "c6", json!({"turnId": "u1", "prompt": {"itemId": "approval:ap1", "expectedRevision": approval["revision"]}})));
    assert_eq!(closed["refusal"]["code"], "agent_session_already_resolved");
    // And a background-task cancel is refused with the host's own word
    assert_eq!(rig.err("agentSession.cancel", cancel("claude_run", "c7", json!({"turnId": "u1", "scope": "background-tasks"}))).1, "agent_session_background_tasks_unsupported");
}

#[test]
fn given_a_long_session_when_its_history_is_paged_then_tail_before_and_after_cover_the_items_once_and_a_cursor_of_no_log_is_a_reset() {
    // Given the recorded thread, which folds to several items
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    let all = rig.ok("agentSession.history", json!({"sessionId": "claude_rec", "direction": "tail", "limit": 200}));
    let ids = |page: &Value| page["items"].as_array().unwrap().iter().map(|i| i["itemId"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    let everything = ids(&all["page"]);
    assert!(everything.len() > 4, "{everything:?}");
    // When the newest three are read, then what is before the oldest of them
    let tail = rig.ok("agentSession.history", json!({"sessionId": "claude_rec", "direction": "tail", "limit": 3}));
    let before = rig.ok("agentSession.history", json!({"sessionId": "claude_rec", "direction": "before", "limit": 200, "cursor": tail["page"]["window"]["nextCursor"]}));
    // Then together they are every item, once and in order, and the tail knows there is more before it and none after
    assert_eq!([ids(&before["page"]), ids(&tail["page"])].concat(), everything);
    assert_eq!((tail["page"]["hasOlder"].clone(), tail["page"]["hasNewer"].clone(), before["page"]["hasOlder"].clone()), (json!(true), json!(false), json!(false)));
    // And the page says what is current: the head, the pending prompts and that no turn works
    assert_eq!((all["page"]["liveCursor"]["sequence"].clone(), all["page"]["working"].clone(), all["page"]["actionablePromptIds"].clone()), (json!(20), json!(false), json!(["approval:ap1", "question:rq1"])));
    // When the phone has read up to seq 17 and asks what is after it
    let after = rig.ok("agentSession.history", json!({"sessionId": "claude_rec", "direction": "after", "cursor": {"epoch": "rca", "sequence": 17}}));
    // Then it gets the items the last three events changed (the second turn and its error) and nothing older
    assert!(ids(&after["page"]).iter().all(|i| i.starts_with("turn:u") || i.starts_with('i')), "{:?}", ids(&after["page"]));
    assert!(ids(&after["page"]).contains(&"turn:u2".to_string()) && !ids(&after["page"]).contains(&"tool:c1".to_string()));
    // When a cursor of another epoch and one past the head are sent, Then each is a reset carrying a tail page
    for (cursor, reason) in [(json!({"epoch": "other", "sequence": 3}), "epoch_changed"), (json!({"epoch": "rca", "sequence": 999}), "cursor_ahead")] {
        let r = rig.ok("agentSession.history", json!({"sessionId": "claude_rec", "direction": "after", "cursor": cursor}));
        assert_eq!((r["ok"].clone(), r["reset"].clone(), r["page"]["direction"].clone()), (json!(false), json!(reason), json!("tail")));
    }
}

#[test]
fn given_open_prompts_when_current_requests_are_read_then_they_come_in_pieces_under_a_version_and_a_changed_log_restarts_the_reader() {
    // Given the recorded thread with two open prompts
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    // When the reader asks with a version it never saw
    let first = rig.ok("agentSession.currentRequests", json!({"sessionId": "claude_rec", "version": "00000000-0000-4000-8000-000000000000"}));
    // Then it is told to restart with the marker holding the version to ask for
    assert_eq!((first["restart"].clone(), first["json"].clone(), first["marker"]["contract"].clone()), (json!(true), json!(""), json!("agent-session.current-requests.v1")));
    let version = first["marker"]["version"].clone();
    // When it asks again with that version
    let piece = rig.ok("agentSession.currentRequests", json!({"sessionId": "claude_rec", "version": version}));
    // Then the text is the JSON of the two open prompts, whole (no next cursor)
    let items: Vec<Value> = serde_json::from_str(piece["json"].as_str().unwrap()).unwrap();
    assert_eq!((items.iter().map(|i| i["itemId"].clone()).collect::<Vec<_>>(), piece["nextCursor"].clone()), (vec![json!("approval:ap1"), json!("question:rq1")], Value::Null));
    assert!(items.iter().all(|i| i["body"]["resolution"]["state"] == "pending"));
    // When the question is answered by someone else, Then the same version is a restart and the new marker lists one prompt
    rig.threads.push("claude_rec", json!({"type": "input.dismissed", "request": "rq1"}));
    let moved = rig.ok("agentSession.currentRequests", json!({"sessionId": "claude_rec", "version": version}));
    assert_eq!(moved["restart"], true);
    let now = rig.ok("agentSession.currentRequests", json!({"sessionId": "claude_rec", "version": moved["marker"]["version"]}));
    assert_eq!(serde_json::from_str::<Vec<Value>>(now["json"].as_str().unwrap()).unwrap().len(), 1);
}

#[test]
fn given_an_item_and_a_conversation_command_when_read_and_sent_then_the_item_is_the_one_asked_for_and_the_command_runs_as_the_prompt_the_cli_reads() {
    // Given the recorded thread and an idle one
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    rig.idle("claude_idle");
    // When an item and a missing item are read
    let found = rig.ok("agentSession.item", json!({"sessionId": "claude_rec", "itemId": "tool:c2"}));
    let missing = rig.ok("agentSession.item", json!({"sessionId": "claude_rec", "itemId": "tool:nope"}));
    // Then the first is the failed tool call with its output and the second is null
    assert_eq!((found["item"]["body"]["state"].clone(), found["item"]["body"]["output"]["head"].clone(), missing["item"].clone()), (json!("failed"), json!("1 failing\n"), Value::Null));
    // When /compact with instructions and /clear are sent
    let compact = rig.ok("agentSession.conversationCommand", json!({"envelope": env("claude_idle", "k1", "f"), "command": "compact", "instructions": "keep the plan"}));
    rig.threads.finish("claude_idle", "completed");
    rig.ok("agentSession.conversationCommand", json!({"envelope": env("claude_idle", "k2", "f"), "command": "clear"}));
    // Then each ran as its text, and the first answers completed
    assert_eq!(sent(&rig, "turn.start").iter().map(|p| p["prompt"].clone()).collect::<Vec<_>>(), vec![json!("/compact keep the plan"), json!("/clear")]);
    assert_eq!((compact["value"]["command"].clone(), compact["value"]["state"].clone()), (json!("compact"), json!("completed")));
    // And a command while a turn runs is refused unless it is sent queued, and then it is queued
    let busy = rig.ok("agentSession.conversationCommand", json!({"envelope": env("claude_idle", "k3", "f"), "command": "clear"}));
    assert_eq!(busy["refusal"]["code"], "agent_session_conflict");
    let queued = rig.ok("agentSession.conversationCommand", json!({"envelope": env("claude_idle", "k4", "f"), "command": "clear", "delivery": "queue-if-active"}));
    assert_eq!(queued["value"]["queued"]["state"], "waiting");
}

#[test]
fn given_a_subscription_when_the_log_grows_then_the_phone_gets_a_snapshot_then_a_batch_of_the_changed_items_and_an_unsubscribe_ends_it() {
    // Given the recorded thread
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    // When the phone subscribes with no cursor
    let opened = rig.frames("agentSession.subscribe", json!({"sessionId": "claude_rec"}));
    // Then the only frame is a streaming snapshot under the request id, carrying a tail page up to the head
    assert_eq!(opened.len(), 1);
    assert_eq!((opened[0]["streaming"].clone(), opened[0]["id"].clone(), opened[0]["result"]["type"].clone(), opened[0]["result"]["page"]["liveCursor"]["sequence"].clone()), (json!(true), json!("r1"), json!("snapshot"), json!(20)));
    // When a turn starts and the assistant answers
    rig.agent.request("turn.start", json!({"thread": "claude_rec", "workspace": "main", "driver": "claudeAgent", "prompt": "again"})).unwrap();
    rig.threads.push("claude_rec", json!({"type": "message.delta", "role": "assistant", "text": "done"}));
    std::thread::sleep(Duration::from_millis(300));
    let frames: Vec<Value> = rig.conn.poll().iter().map(|f| serde_json::from_str(f).unwrap()).collect();
    // Then one batch follows whose cursor is the new head and whose items are only those the new events changed
    assert_eq!(frames.len(), 1, "{frames:?}");
    let batch = &frames[0]["result"];
    assert_eq!((batch["type"].clone(), batch["batch"]["cursor"]["sequence"].clone(), batch["working"].clone()), (json!("batch"), json!(23), json!(true)));
    let changed: Vec<&str> = batch["batch"]["items"].as_array().unwrap().iter().map(|i| i["itemId"].as_str().unwrap()).collect();
    assert!(changed.iter().all(|i| !i.starts_with("tool:")), "{changed:?}");
    assert!(changed.len() >= 2);
    // When the phone unsubscribes by session, Then the stream ends with an end frame and no stream remains
    let ended = rig.call("agentSession.unsubscribe", json!({"sessionId": "claude_rec"}));
    assert_eq!(ended["result"], json!({"unsubscribed": true}));
    assert_eq!(rig.conn.open_streams(), 0);
}

#[test]
fn given_a_cursor_when_subscribed_then_a_current_one_catches_up_with_a_batch_and_one_of_no_log_is_a_reset_and_an_unsubscribe_by_id_ends_only_that_stream() {
    // Given the recorded thread
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    // When the phone subscribes holding seq 17, then with a cursor of another epoch, then past the head
    let caught = rig.frames("agentSession.subscribe", json!({"sessionId": "claude_rec", "cursor": {"epoch": "rca", "sequence": 17}}));
    let other = rig.frames("agentSession.subscribe", json!({"sessionId": "claude_rec", "cursor": {"epoch": "x", "sequence": 3}}));
    let ahead = rig.frames("agentSession.subscribe", json!({"sessionId": "claude_rec", "cursor": {"epoch": "rca", "sequence": 99}}));
    // Then the first is a batch of the items changed after 17 with the head as its cursor, the others resets with the reason
    assert_eq!((caught[0]["result"]["type"].clone(), caught[0]["result"]["batch"]["cursor"]["sequence"].clone()), (json!("batch"), json!(20)));
    assert!(caught[0]["result"]["batch"]["items"].as_array().unwrap().iter().all(|i| i["revision"].as_u64().unwrap() > 17));
    assert_eq!((other[0]["result"]["type"].clone(), other[0]["result"]["reset"].clone()), (json!("reset"), json!("epoch_changed")));
    assert_eq!((ahead[0]["result"]["type"].clone(), ahead[0]["result"]["reset"].clone()), (json!("reset"), json!("cursor_ahead")));
    assert_eq!(rig.conn.open_streams(), 3);
    // When one stream is ended by the id the phone knows it by (its request id)
    rig.call("agentSession.unsubscribe", json!({"sessionId": "claude_rec", "subscriptionId": "r2"}));
    // Then only that one is gone, and a subscribe to a session nobody created is refused
    assert_eq!(rig.conn.open_streams(), 2);
    assert_eq!(rig.err("agentSession.subscribe", json!({"sessionId": "claude_nobody"})), ("runtime_error".to_string(), "agent_session_not_found".to_string()));
}

#[test]
fn given_launches_when_sent_then_a_chat_starts_in_an_existing_workspace_with_the_prompt_and_an_operation_id_makes_it_at_most_once() {
    // Given a host
    let mut rig = Rig::new();
    let launch = json!({"agent": "codex", "operationId": "1730000000000-abc", "target": {"kind": "existing", "worktree": "id:main::"}, "prompt": {"text": "add a test", "delivery": "submit"}});
    // When a launch is replayed before it ran, then launched, then launched again with the same id
    let unknown = rig.err("agent.launchReplay", launch.clone());
    let first = rig.ok("agent.launch", launch.clone());
    let again = rig.ok("agent.launch", launch.clone());
    let replayed = rig.ok("agent.launchReplay", launch.clone());
    // Then the first replay was refused with the host's word, and the launch made one codex-lane session with one turn
    assert_eq!(unknown, ("runtime_error".to_string(), "agent_session_operation_unknown".to_string()));
    let session = first["outcome"]["sessionId"].as_str().unwrap().to_string();
    assert!(session.starts_with("codex_") && first["outcome"]["kind"] == "structured" && first["outcome"]["handle"] == session.as_str());
    assert_eq!((first["worktreeId"].clone(), first["receipt"]["mode"].clone(), first["prompt"]["outcome"].clone()), (json!("main::"), json!("structured"), json!("journaled")));
    assert_eq!(sent(&rig, "turn.start").iter().map(|p| (p["thread"].clone(), p["prompt"].clone(), p["driver"].clone())).collect::<Vec<_>>(), vec![(json!(session), json!("add a test"), json!("codex"))]);
    // And the second launch and the replay answer the first's result, starting nothing
    assert_eq!((again.clone(), replayed), (first.clone(), first));
    assert_eq!(sent(&rig, "thread.create").len(), 1);
    // When the same id comes with another prompt, a draft launch, a target to create and a session id of another lane
    let conflict = rig.err("agent.launch", json!({"agent": "codex", "operationId": "1730000000000-abc", "target": {"kind": "existing", "worktree": "main"}, "prompt": {"text": "other", "delivery": "submit"}}));
    let draft = rig.ok("agent.launch", json!({"agent": "claude", "target": {"kind": "existing", "worktree": "main"}, "prompt": {"text": "later", "delivery": "draft"}}));
    let create = rig.err("agent.launch", json!({"agent": "claude", "target": {"kind": "create-worktree", "create": {}}}));
    let lane = rig.err("agent.launch", json!({"agent": "claude", "sessionId": "codex_x", "target": {"kind": "existing", "worktree": "main"}}));
    // Then the id is a conflict, the draft starts no turn, a target to create is not served here yet and the lane is checked
    assert_eq!(conflict.1, "agent_session_operation_conflict");
    assert_eq!((draft["prompt"]["outcome"].clone(), sent(&rig, "turn.start").len()), (json!("not-delivered"), 1));
    assert_eq!(create.1, "agent_launch_target_unsupported");
    assert_eq!(lane.0, "invalid_argument");
}

#[test]
fn given_a_session_when_held_released_and_read_as_a_native_chat_then_hold_needs_the_session_and_the_chat_is_its_messages_paged_by_offset() {
    // Given the recorded thread
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    // When it is held and released, a missing one held, and the native chat read with a window of two, then before its first row
    let held = rig.ok("agentSession.hold", json!({"sessionId": "claude_rec", "holderId": "h1"}));
    let released = rig.ok("agentSession.release", json!({"sessionId": "claude_rec", "holderId": "h1"}));
    let none = rig.err("agentSession.hold", json!({"sessionId": "claude_nobody", "holderId": "h1"}));
    let page = rig.ok("nativeChat.readSession", json!({"agent": "claude", "sessionId": "claude_rec", "limit": 2}));
    let older = rig.ok("nativeChat.readSession", json!({"agent": "claude", "sessionId": "claude_rec", "limit": 2, "beforeOffset": page["beforeOffset"]}));
    // Then the answers are the host's, the window is the last two messages (the assistant's answer and "Fixed.") and the older page precedes it
    assert_eq!((held, released, none.1), (json!({"held": true}), json!({"released": true}), "agent_session_not_found".to_string()));
    let texts = |p: &Value| p["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap().to_string(), m["blocks"][0]["text"].as_str().unwrap().to_string())).collect::<Vec<_>>();
    assert_eq!(texts(&page), vec![("assistant".to_string(), "I will read the handler.".to_string()), ("assistant".to_string(), "Fixed.".to_string())]);
    assert_eq!((page["hasMore"].clone(), older["hasMore"].clone()), (json!(true), json!(false)));
    assert_eq!(texts(&older), vec![("user".to_string(), "Why does the login fail?".to_string()), ("reasoning".to_string(), "Look at the handler first.".to_string())]);
    assert_eq!(rig.ok("nativeChat.unsubscribe", json!({"subscriptionId": "s"})), json!({"unsubscribed": true}));
}

fn mutation(session: &str, op: &str, extra: Value) -> Value {
    let mut p = json!({"envelope": env(session, op, &format!("fp-{op}"))});
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    p
}

/// A running thread with three queued prompts `one`, `two`, `three` (ids in the order they were queued).
fn queued_rig() -> (Rig, Vec<String>) {
    let mut rig = Rig::new();
    rig.idle("claude_q");
    rig.agent.request("turn.start", json!({"thread": "claude_q", "workspace": "main", "driver": "claudeAgent", "prompt": "first"})).unwrap();
    for (n, text) in ["one", "two", "three"].iter().enumerate() {
        rig.ok("agentSession.send", json!({"envelope": env("claude_q", &format!("add{n}"), "f"), "delivery": "queue-if-active", "body": body(text)}));
    }
    let ids = rig.threads.queued_ids("claude_q");
    (rig, ids)
}

fn cards(page: &Value) -> Vec<(String, u64, String)> {
    page["queuedMessages"].as_array().unwrap().iter().map(|c| (c["messageId"].as_str().unwrap().to_string(), c["position"].as_u64().unwrap(), c["body"]["blocks"][0]["text"].as_str().unwrap().to_string())).collect()
}

#[test]
fn given_queued_prompts_when_a_page_a_catch_up_and_a_stream_are_read_then_each_carries_the_cards_in_order_and_the_pause_the_phone_reads() {
    // Given a running session with three queued prompts
    let (mut rig, ids) = queued_rig();
    // When its history tail and a subscription are read
    let page = rig.ok("agentSession.history", json!({"sessionId": "claude_q", "direction": "tail"}))["page"].clone();
    let opening = rig.frames("agentSession.subscribe", json!({"sessionId": "claude_q"}));
    // Then the cards are the queue in order, the first is the next one sent, and nothing is paused
    let want: Vec<(String, u64, String)> = ids.iter().zip(["one", "two", "three"]).enumerate().map(|(n, (id, t))| (id.clone(), n as u64 + 1, t.to_string())).collect();
    assert_eq!((cards(&page), page["nextQueuedMessageId"].clone(), page["queuePause"].clone()), (want.clone(), json!(ids[0]), Value::Null));
    assert_eq!(cards(&opening[0]["result"]), want);
    // And a prompt queued afterwards arrives in the next batch as the whole list
    rig.ok("agentSession.send", json!({"envelope": env("claude_q", "add9", "f"), "delivery": "queue-if-active", "body": body("four")}));
    std::thread::sleep(Duration::from_millis(300));
    let frames: Vec<Value> = rig.conn.poll().iter().map(|f| serde_json::from_str(f).unwrap()).collect();
    assert_eq!(cards(&frames[0]["result"]).iter().map(|c| c.2.clone()).collect::<Vec<_>>(), vec!["one", "two", "three", "four"]);
    // And after a forced stop holds the queue, no card is next and the pause is published
    rig.threads.hold("claude_q");
    let held = rig.ok("agentSession.history", json!({"sessionId": "claude_q", "direction": "tail"}))["page"].clone();
    assert_eq!((held["nextQueuedMessageId"].clone(), held["queuePause"]["reason"].clone()), (Value::Null, json!("stopped")));
}

#[test]
fn given_queued_prompts_when_moved_then_the_queue_order_is_the_one_asked_for_and_an_unknown_card_or_anchor_is_gone_or_stale() {
    // Given three queued prompts
    let (mut rig, ids) = queued_rig();
    let mv = |rig: &mut Rig, op: &str, extra: Value| rig.ok("agentSession.queuedMessageMove", mutation("claude_q", op, extra))["value"]["status"].clone();
    // When the last goes before the first
    let moved = mv(&mut rig, "m1", json!({"messageId": ids[2], "beforeMessageId": ids[0]}));
    // Then it is first and the others keep their order
    assert_eq!((moved, rig.threads.queued("claude_q")), (json!("moved"), vec!["three".to_string(), "one".into(), "two".into()]));
    // And moving it where it already is changes nothing, and asking again with the same operation answers the recorded move
    assert_eq!(mv(&mut rig, "m2", json!({"messageId": ids[2], "beforeMessageId": ids[0]})), json!("unchanged"));
    assert_eq!(sent(&rig, "queue.move").len(), 1);
    let again = rig.ok("agentSession.queuedMessageMove", mutation("claude_q", "m1", json!({"messageId": ids[2], "beforeMessageId": ids[0]})));
    assert_eq!((again["replayed"].clone(), sent(&rig, "queue.move").len()), (json!(true), 1));
    // And behind an anchor, and to the end with a null anchor
    assert_eq!(mv(&mut rig, "m3", json!({"messageId": ids[2], "afterMessageId": ids[0]})), json!("moved"));
    assert_eq!(rig.threads.queued("claude_q"), vec!["one".to_string(), "three".into(), "two".into()]);
    assert_eq!(mv(&mut rig, "m4", json!({"messageId": ids[0], "beforeMessageId": null})), json!("moved"));
    assert_eq!(rig.threads.queued("claude_q"), vec!["three".to_string(), "two".into(), "one".into()]);
    // And a card the queue lacks is gone and an anchor it lacks is stale, neither moving anything
    let before = sent(&rig, "queue.move").len();
    assert_eq!((mv(&mut rig, "m5", json!({"messageId": "nope", "beforeMessageId": ids[0]})), mv(&mut rig, "m6", json!({"messageId": ids[0], "afterMessageId": "nope"}))), (json!("gone"), json!("stale")));
    assert_eq!(sent(&rig, "queue.move").len(), before);
}

#[test]
fn given_a_queued_prompt_when_updated_then_it_is_saved_only_against_the_fingerprint_the_phone_saw_and_a_lost_card_is_gone() {
    // Given three queued prompts
    let (mut rig, ids) = queued_rig();
    let up = |rig: &mut Rig, op: &str, id: &str, expected: &str, text: &str| rig.ok("agentSession.queuedMessageUpdate", mutation("claude_q", op, json!({"messageId": id, "expectedBodyFingerprint": expected, "text": text})))["value"].clone();
    let (fp_one, fp_uno, fp_eins) = (body_fingerprint("claude_q", "one"), body_fingerprint("claude_q", "uno"), body_fingerprint("claude_q", "eins"));
    // When the first is saved with the fingerprint of the text it shows
    let saved = up(&mut rig, "u1", &ids[0], &fp_one, "uno");
    // Then the queue holds the new text and the answer names its fingerprint
    assert_eq!((saved["status"].clone(), saved["fingerprint"].clone(), rig.threads.queued("claude_q")[0].clone()), (json!("updated"), json!(fp_uno), "uno".to_string()));
    // And the same save again is unchanged, and a save against the old fingerprint is a competing edit that wins nothing
    assert_eq!(up(&mut rig, "u2", &ids[0], &fp_one, "uno")["status"], "unchanged");
    assert_eq!(up(&mut rig, "u3", &ids[0], &fp_one, "eins")["status"], "changed");
    assert_eq!((rig.threads.queued("claude_q")[0].clone(), sent(&rig, "queue.edit").len()), ("uno".to_string(), 1));
    // And an empty text or an image is not editable, and a save with the right fingerprint then works
    assert_eq!(up(&mut rig, "u4", &ids[0], &fp_uno, "")["status"], "not-editable");
    let with_image = rig.ok("agentSession.queuedMessageUpdate", mutation("claude_q", "u5", json!({"messageId": ids[0], "expectedBodyFingerprint": fp_uno, "text": "x", "images": [{"type": "image-ref", "path": "a.png"}]})));
    assert_eq!(with_image["value"]["status"], "not-editable");
    assert_eq!(up(&mut rig, "u6", &ids[0], &fp_uno, "eins")["fingerprint"], json!(fp_eins));
    // And a card that is not queued is gone as missing
    assert_eq!((up(&mut rig, "u7", "nope", &fp_one, "x")["status"].clone(), up(&mut rig, "u8", "nope", &fp_one, "x")["disposition"].clone()), (json!("gone"), json!("missing")));
}

#[test]
fn given_a_queued_prompt_when_an_edit_lease_is_acquired_renewed_and_released_then_the_phone_gets_the_host_s_statuses() {
    // Given three queued prompts
    let (mut rig, ids) = queued_rig();
    let hold = |rig: &mut Rig, action: &str, edit: &str, id: &str, fp: Option<&str>| {
        let mut p = json!({"sessionId": "claude_q", "messageId": id, "editId": edit, "action": action});
        if let Some(f) = fp {
            p["expectedBodyFingerprint"] = f.into();
        }
        rig.ok("agentSession.queuedMessageEditHold", p)
    };
    let fp = body_fingerprint("claude_q", "one");
    // When one editor acquires with the fingerprint it shows, another tries the same card, and one acquires with a stale fingerprint
    let held = hold(&mut rig, "acquire", "e1", &ids[0], Some(&fp));
    let other = hold(&mut rig, "acquire", "e2", &ids[0], Some(&fp));
    let stale = hold(&mut rig, "acquire", "e3", &ids[1], Some(&fp));
    // Then the first holds it for a lease, the second and the third see it changed
    assert_eq!((held["status"].clone(), held["fingerprint"].clone(), held["leaseDurationMs"].clone(), held["remainingMs"].clone()), (json!("held"), json!(fp), json!(30000), json!(30000)));
    assert_eq!((other["status"].clone(), stale["status"].clone()), (json!("changed"), json!("changed")));
    // And renewing needs the lease, releasing frees only the holder's, and a card that is not queued is gone
    assert_eq!(hold(&mut rig, "renew", "e1", &ids[0], None)["status"], "held");
    assert_eq!(hold(&mut rig, "renew", "e2", &ids[0], None)["status"], "expired");
    assert_eq!(hold(&mut rig, "release", "e2", &ids[0], None)["status"], "released");
    assert_eq!(hold(&mut rig, "renew", "e1", &ids[0], None)["status"], "held");
    assert_eq!(hold(&mut rig, "release", "e1", &ids[0], None)["status"], "released");
    assert_eq!(hold(&mut rig, "renew", "e1", &ids[0], None)["status"], "expired");
    assert_eq!(hold(&mut rig, "acquire", "e1", "nope", Some(&fp))["status"], "gone");
}

#[test]
fn given_queued_prompts_when_deleted_sent_now_and_resumed_then_the_queue_and_the_turns_follow_and_a_gone_card_says_why() {
    // Given a running session with three queued prompts
    let (mut rig, ids) = queued_rig();
    // When the second is deleted, then deleted again
    let first = rig.ok("agentSession.queuedMessageDelete", mutation("claude_q", "d1", json!({"messageId": ids[1]})));
    let second = rig.ok("agentSession.queuedMessageDelete", mutation("claude_q", "d2", json!({"messageId": ids[1]})));
    // Then the first removed it, the second says it was withdrawn, and a retry of the first answers the recorded answer
    assert_eq!((first["value"].clone(), rig.threads.queued("claude_q")), (json!({"deleted": true, "messageId": ids[1]}), vec!["one".to_string(), "three".into()]));
    assert_eq!((second["value"]["deleted"].clone(), second["value"]["disposition"].clone()), (json!(false), json!("withdrawn")));
    assert_eq!(rig.ok("agentSession.queuedMessageDelete", mutation("claude_q", "d1", json!({"messageId": ids[1]})))["replayed"], true);
    // And a send now while the turn runs steers the queued prompt into it and takes it out of the queue
    let steer = rig.ok("agentSession.queuedMessageSend", mutation("claude_q", "s1", json!({"messageId": ids[2]})));
    assert_eq!((steer["value"]["submission"]["dispatchState"].clone(), sent(&rig, "turn.steer").last().unwrap()["item"].clone(), rig.threads.queued("claude_q")), (json!("accepted"), json!(ids[2]), vec!["one".to_string()]));
    // And once the turn ended a send now starts the card as the next turn, and the same card sent again says it was sent
    rig.threads.finish("claude_q", "completed");
    rig.ok("agentSession.queuedMessageSend", mutation("claude_q", "s2", json!({"messageId": ids[0]})));
    assert_eq!((sent(&rig, "queue.resume").last().unwrap()["item"].clone(), rig.threads.running("claude_q").is_some(), rig.threads.queued("claude_q")), (json!(ids[0]), true, Vec::<String>::new()));
    let again = rig.ok("agentSession.queuedMessageSend", mutation("claude_q", "s3", json!({"messageId": ids[0]})));
    assert_eq!((again["ok"].clone(), again["refusal"]["code"].clone(), again["refusal"]["message"].clone()), (json!(false), json!("agent_session_operation_invalid"), json!("This queued message was already sent.")));
}

#[test]
fn given_a_held_queue_when_resumed_then_resumed_says_whether_anything_was_held_and_the_queue_is_released() {
    // Given a queue a forced stop holds
    let (mut rig, _) = queued_rig();
    rig.threads.hold("claude_q");
    // When it is resumed, then resumed again
    let first = rig.ok("agentSession.queuedMessagesResume", mutation("claude_q", "r1", json!({})));
    let second = rig.ok("agentSession.queuedMessagesResume", mutation("claude_q", "r2", json!({})));
    // Then the first lifted a hold and the second had none to lift
    assert_eq!((first["value"]["resumed"].clone(), second["value"]["resumed"].clone()), (json!(true), json!(false)));
    let page = rig.ok("agentSession.history", json!({"sessionId": "claude_q", "direction": "tail"}))["page"].clone();
    assert_eq!(page["queuePause"], Value::Null);
}

fn item_of(rig: &mut Rig, session: &str, id: &str) -> Value {
    rig.ok("agentSession.item", json!({"sessionId": session, "itemId": id}))["item"].clone()
}

#[test]
fn given_an_open_approval_when_answered_then_the_option_is_the_decision_rca_gets_and_a_stale_unknown_or_repeated_answer_is_refused_as_the_host_does() {
    // Given the recorded thread whose approval ap1 is open
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    let item = item_of(&mut rig, "claude_rec", "approval:ap1");
    let revision = item["revision"].as_u64().unwrap();
    let options: Vec<String> = item["body"]["options"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap().to_string()).collect();
    let respond = |rig: &mut Rig, op: &str, rev: u64, option: &str| rig.ok("agentSession.respondToApproval", mutation("claude_rec", op, json!({"itemId": "approval:ap1", "expectedRevision": rev, "optionId": option})));
    // When it is answered at a revision the phone did not see, with an option it does not offer, and for an item that is no approval
    let stale = respond(&mut rig, "a1", revision + 1, &options[0]);
    let unknown = respond(&mut rig, "a2", revision, "maybe");
    let not_approval = rig.ok("agentSession.respondToApproval", mutation("claude_rec", "a3", json!({"itemId": "question:rq1", "expectedRevision": 14, "optionId": "x"})));
    // Then each is refused with the host's code and rca was never asked to answer
    assert_eq!((stale["refusal"]["code"].clone(), stale["refusal"]["details"]["currentRevision"].clone()), (json!("agent_session_item_revision_stale"), json!(revision)));
    assert_eq!((unknown["refusal"]["code"].clone(), unknown["refusal"]["details"]["reason"].clone()), (json!("agent_session_operation_invalid"), json!("optionRejected")));
    assert_eq!(not_approval["refusal"]["details"]["reason"], "promptGone");
    assert!(sent(&rig, "approval.answer").is_empty());
    // When it is answered with each offered option in turn the first time at the revision the phone saw
    let accepted = respond(&mut rig, "a4", revision, &options[0]);
    // Then rca got the option as its decision and the answer is the item at its new revision, resolved with that option
    assert_eq!(sent(&rig, "approval.answer"), vec![json!({"thread": "claude_rec", "approval": "ap1", "decision": options[0]})]);
    let value = &accepted["value"];
    assert_eq!((value["itemId"].clone(), value["resolution"]["state"].clone(), value["resolution"]["selectedOptionId"].clone()), (json!("approval:ap1"), json!("resolved"), json!(options[0])));
    assert!(value["revision"].as_u64().unwrap() > revision);
    // And answering again at the old revision is stale, at the new one already resolved, and a retry of the accepted operation is its recorded answer
    assert_eq!(respond(&mut rig, "a5", revision, &options[0])["refusal"]["code"], "agent_session_item_revision_stale");
    assert_eq!(respond(&mut rig, "a6", value["revision"].as_u64().unwrap(), &options[0])["refusal"]["code"], "agent_session_already_resolved");
    assert_eq!(respond(&mut rig, "a4", revision, &options[0])["replayed"], true);
    assert_eq!(sent(&rig, "approval.answer").len(), 1);
}

#[test]
fn given_open_questions_when_answered_with_an_option_or_free_text_then_rca_gets_the_text_and_a_mixed_or_foreign_answer_is_refused() {
    // Given the recorded thread whose question rq1 is open, and a free-text question rq2
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    let q1 = item_of(&mut rig, "claude_rec", "question:rq1");
    let option = q1["body"]["options"][0]["id"].as_str().unwrap().to_string();
    let seq = rig.threads.push("claude_rec", json!({"type": "input.requested", "request": "rq2", "prompt": "Name?", "options": []}));
    let respond = |rig: &mut Rig, op: &str, id: &str, rev: u64, extra: Value| {
        let mut p = mutation("claude_rec", op, json!({"itemId": id, "expectedRevision": rev}));
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        rig.ok("agentSession.respondToQuestion", p)
    };
    // When an option and answers are both given, an option the question lacks is given, and an empty answer is given
    let both = respond(&mut rig, "q1", "question:rq1", q1["revision"].as_u64().unwrap(), json!({"optionId": option, "answers": [{"questionId": "x", "optionIds": [option]}]}));
    let foreign = respond(&mut rig, "q2", "question:rq1", q1["revision"].as_u64().unwrap(), json!({"optionId": "zzz"}));
    let empty = respond(&mut rig, "q3", "question:rq2", seq, json!({"answers": [{"questionId": "x", "optionIds": []}]}));
    // Then each is refused and rca got no answer
    assert_eq!((both["refusal"]["details"]["reason"].clone(), foreign["refusal"]["details"]["reason"].clone(), empty["refusal"]["details"]["reason"].clone()), (json!("optionRejected"), json!("optionRejected"), json!("optionRejected")));
    assert!(sent(&rig, "input.answer").is_empty());
    // When the first is answered by option and the second by free text
    let by_option = respond(&mut rig, "q4", "question:rq1", q1["revision"].as_u64().unwrap(), json!({"answers": [{"questionId": "x", "optionIds": [option]}]}));
    let by_text = respond(&mut rig, "q5", "question:rq2", seq, json!({"answers": [{"questionId": "x", "optionIds": [], "other": "Bob"}]}));
    // Then rca got the option id and the typed text, and each answer is the item resolved
    assert_eq!(sent(&rig, "input.answer").iter().map(|p| (p["request"].clone(), p["answer"].clone())).collect::<Vec<_>>(), vec![(json!("rq1"), json!(option)), (json!("rq2"), json!("Bob"))]);
    assert_eq!((by_option["value"]["resolution"]["state"].clone(), by_text["value"]["resolution"]["state"].clone(), by_text["value"]["resolution"]["selectedOptionId"].clone()), (json!("resolved"), json!("resolved"), json!("other")));
}

/// Bytes that differ along their length, so a window read from the wrong offset shows.
fn bytes_of(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 31 % 251) as u8).collect()
}

#[test]
fn given_a_file_larger_than_one_chunk_when_uploaded_and_committed_then_rca_keeps_the_bytes_under_the_upload_id_and_the_path_reads_back_in_windows() {
    // Given a chat and a 700 KiB file, more than one 384 KiB chunk
    let mut rig = Rig::new();
    rig.idle("claude_a");
    let bytes = bytes_of(700 * 1024);
    // When it is uploaded in chunks and committed
    let done = rig.upload("claude_a", "dir/notes: v1.txt", &bytes);
    // Then the commit names a path in the host's store with the sanitised name and the full length
    let path = done["path"].as_str().unwrap().to_string();
    assert_eq!((done["name"].clone(), done["byteLength"].clone()), (json!("notes_ v1.txt"), json!(bytes.len())));
    assert!(path.contains("/agent-session-attachments/") && path.ends_with("/notes_ v1.txt"), "{path}");
    // And rca received one upload.put under the id in the path, holding all the bytes
    let put = sent(&rig, "upload.put");
    assert_eq!(put.len(), 1);
    assert!(path.contains(put[0]["id"].as_str().unwrap()));
    assert_eq!(STANDARD.decode(put[0]["data"].as_str().unwrap()).unwrap(), bytes);
    // When the text is read in windows from the middle and past the end
    let window = rig.ok("agentSessionAttachment.readTextChunk", json!({"path": path, "offset": 300_000, "length": 4096}));
    let tail = rig.ok("agentSessionAttachment.readTextChunk", json!({"path": path, "offset": bytes.len() - 10, "length": 4096}));
    // Then each window is the bytes at that offset, with the size, a snapshot and eof
    assert_eq!(STANDARD.decode(window["contentBase64"].as_str().unwrap()).unwrap(), bytes[300_000..304_096]);
    assert_eq!((window["size"].clone(), window["bytesRead"].clone(), window["eof"].clone()), (json!(bytes.len()), json!(4096), json!(false)));
    assert_eq!((tail["bytesRead"].clone(), tail["eof"].clone()), (json!(10), json!(true)));
    // And the snapshot of an earlier read is accepted again, while another one is refused as a changed attachment
    let snapshot = window["snapshot"].as_str().unwrap();
    rig.ok("agentSessionAttachment.readTextChunk", json!({"path": path, "offset": 0, "length": 8, "expectedSnapshot": snapshot}));
    let changed = rig.err("agentSessionAttachment.readTextChunk", json!({"path": path, "offset": 0, "length": 8, "expectedSnapshot": "0".repeat(64)}));
    assert_eq!(changed, ("runtime_error".into(), "Attachment changed. Close it and try again.".into()));
}

#[test]
fn given_uploads_the_host_refuses_when_sent_then_each_is_refused_as_the_host_words_it_and_nothing_reaches_rca() {
    // Given a chat and an upload of 10 bytes started on one connection
    let mut rig = Rig::new();
    rig.idle("claude_a");
    let id = rig.ok("agentSessionAttachment.uploadStart", json!({"sessionId": "claude_a", "name": "a.bin", "byteLength": 10}))["uploadId"].as_str().unwrap().to_string();
    let chunk = |offset: u64, n: usize| json!({"uploadId": id, "offset": offset, "contentBase64": STANDARD.encode(vec![7u8; n])});
    // When chunks come out of order, past the declared size, as bad base64, from another connection and too early to commit
    let order = rig.err("agentSessionAttachment.uploadAppend", chunk(3, 2));
    let over = rig.err("agentSessionAttachment.uploadAppend", chunk(0, 11));
    let bad = rig.err("agentSessionAttachment.uploadAppend", json!({"uploadId": id, "offset": 0, "contentBase64": "***"}));
    let theirs = rig.other_err("agentSessionAttachment.uploadAppend", chunk(0, 2));
    rig.ok("agentSessionAttachment.uploadAppend", chunk(0, 4));
    let early = rig.err("agentSessionAttachment.uploadCommit", json!({"uploadId": id}));
    // Then each is the host's message
    assert_eq!(order, ("runtime_error".into(), "Attachment chunk offset is out of order".into()));
    assert_eq!(over, ("runtime_error".into(), "Attachment upload exceeded its declared size".into()));
    assert_eq!(bad, ("invalid_argument".into(), "Attachment chunk must be base64".into()));
    assert_eq!(theirs, ("runtime_error".into(), "Attachment upload was not found".into()));
    assert_eq!(early, ("runtime_error".into(), "Attachment upload is incomplete".into()));
    // And the incomplete commit dropped the upload: it is not found any more, and an abort of it still answers aborted
    assert_eq!(rig.err("agentSessionAttachment.uploadAppend", chunk(4, 1)).1, "Attachment upload was not found");
    assert_eq!(rig.ok("agentSessionAttachment.uploadAbort", json!({"uploadId": id})), json!({"aborted": true}));
    // When a start names an unknown chat, a file over the limit, an empty file or an extra key
    let chat = rig.err("agentSessionAttachment.uploadStart", json!({"sessionId": "claude_nope", "name": "a", "byteLength": 1}));
    let big = rig.err("agentSessionAttachment.uploadStart", json!({"sessionId": "claude_a", "name": "a", "byteLength": orca_map_agent::max_bytes() + 1}));
    let empty = rig.err("agentSessionAttachment.uploadStart", json!({"sessionId": "claude_a", "name": "a", "byteLength": 0}));
    let extra = rig.err("agentSessionAttachment.uploadStart", json!({"sessionId": "claude_a", "name": "a", "byteLength": 1, "mime": "x"}));
    // Then each is refused, and rca never got a put
    assert_eq!(chat, ("runtime_error".into(), "This chat is not on this host".into()));
    assert_eq!(big, ("invalid_argument".into(), "Attachment is too large".into()));
    assert_eq!(empty.0, "runtime_error");
    assert_eq!(extra.0, "invalid_argument");
    assert!(sent(&rig, "upload.put").is_empty());
}

#[test]
fn given_a_started_upload_when_aborted_then_it_is_gone_and_only_eight_uploads_run_at_once() {
    // Given a chat
    let mut rig = Rig::new();
    rig.idle("claude_a");
    let start = |rig: &mut Rig| rig.ok("agentSessionAttachment.uploadStart", json!({"sessionId": "claude_a", "name": "a", "byteLength": 5}))["uploadId"].as_str().unwrap().to_string();
    // When eight uploads run and a ninth starts
    let ids: Vec<String> = (0..8).map(|_| start(&mut rig)).collect();
    let ninth = rig.err("agentSessionAttachment.uploadStart", json!({"sessionId": "claude_a", "name": "a", "byteLength": 5}));
    // Then the ninth is refused until one is aborted, and the aborted one cannot be appended to
    assert_eq!(ninth.1, "Too many attachment uploads are in progress");
    rig.ok("agentSessionAttachment.uploadAbort", json!({"uploadId": ids[0]}));
    assert_eq!(rig.err("agentSessionAttachment.uploadAppend", json!({"uploadId": ids[0], "offset": 0, "contentBase64": "AAAA"})).1, "Attachment upload was not found");
    start(&mut rig);
}

#[test]
fn given_an_uploaded_image_when_read_as_attachment_or_chat_image_then_the_preview_is_the_bytes_with_the_type_of_its_extension_and_anything_else_is_not_an_image() {
    // Given an uploaded png and an uploaded text file
    let mut rig = Rig::new();
    rig.idle("claude_a");
    let png = bytes_of(5000);
    let image = rig.upload("claude_a", "shot.PNG", &png)["path"].clone();
    let text = rig.upload("claude_a", "a.txt", b"hello")["path"].clone();
    // When the image is read by the attachment row and by the chat-image row of a worktree
    let by_attachment = rig.ok("agentSessionAttachment.read", json!({"path": image}));
    let by_worktree = rig.ok("files.readChatImage", json!({"worktree": "id:main::", "path": image}));
    // Then both are the preview the host answers: base64 content, binary, image, the mime of the extension
    for r in [&by_attachment, &by_worktree] {
        assert_eq!((r["isBinary"].clone(), r["isImage"].clone(), r["mimeType"].clone()), (json!(true), json!(true), json!("image/png")));
        assert_eq!(STANDARD.decode(r["content"].as_str().unwrap()).unwrap(), png);
    }
    // When a text file, a path outside the store and an unknown worktree are asked for
    let not_image = rig.err("agentSessionAttachment.read", json!({"path": text}));
    let outside = rig.err("files.readChatImage", json!({"worktree": "id:main::", "path": "/etc/passwd.png"}));
    let wrong_type = rig.err("files.readChatImage", json!({"worktree": "id:main::", "path": text}));
    let worktree = rig.err("files.readChatImage", json!({"worktree": "id:nope::", "path": image}));
    // Then they are refused: not an image (the two rows' own wording), and the unknown worktree as the selector error
    assert_eq!(not_image.1, "Not an attachment image");
    assert_eq!((outside.1.as_str(), wrong_type.1.as_str()), ("not_an_image", "not_an_image"));
    assert_eq!(worktree.1, "selector_not_found");
}

#[test]
fn given_an_image_too_large_for_one_reply_when_previewed_then_it_is_file_too_large_and_a_missing_attachment_is_not_an_image() {
    // Given an uploaded png larger than one reply carries
    let mut rig = Rig::new();
    rig.idle("claude_a");
    let big = rig.upload("claude_a", "big.png", &bytes_of(orca_map_workspace::previewable_limit() + 1))["path"].clone();
    // When it is previewed, and a path of an attachment never uploaded is read
    let too_large = rig.err("agentSessionAttachment.read", json!({"path": big}));
    let missing = rig.err("agentSessionAttachment.read", json!({"path": "/rca/agent-session-attachments/00000000-0000-4000-8000-000000000000/x.png"}));
    // Then the first is file_too_large and the second the uniform not-an-image
    assert_eq!((too_large.1.as_str(), missing.1.as_str()), ("file_too_large", "Not an attachment image"));
}

#[test]
fn given_uploaded_images_when_a_message_names_them_then_the_turn_starts_with_their_ids_and_urls_unknown_paths_and_a_running_chat_are_refused() {
    // Given a chat with an uploaded png
    let mut rig = Rig::new();
    rig.idle("claude_a");
    let path = rig.upload("claude_a", "shot.png", &bytes_of(2000))["path"].as_str().unwrap().to_string();
    let id = path.split('/').nth(3).unwrap().to_string();
    let with = |blocks: Value| json!({"envelope": env("claude_a", "s1", "f"), "body": {"blocks": blocks}});
    // When a message with text and the image is sent
    rig.ok("agentSession.send", with(json!([{"type": "text", "text": "what is this"}, {"type": "image-ref", "path": path}])));
    // Then the turn carries the text and the attachment id
    let turns = sent(&rig, "turn.start");
    assert_eq!((turns[0]["prompt"].clone(), turns[0]["attachments"].clone()), (json!("what is this"), json!([id])));
    // When the chat is running, an url, a path outside the store and an image never uploaded are sent
    let running = rig.err("agentSession.send", json!({"envelope": env("claude_a", "s2", "f"), "body": {"blocks": [{"type": "image-ref", "path": path}]}}));
    rig.threads.finish("claude_a", "completed");
    let url = rig.err("agentSession.send", json!({"envelope": env("claude_a", "s3", "f"), "body": {"blocks": [{"type": "image-ref", "url": "https://x/y.png"}]}}));
    let outside = rig.err("agentSession.send", json!({"envelope": env("claude_a", "s4", "f"), "body": {"blocks": [{"type": "image-ref", "path": "/tmp/x.png"}]}}));
    let never = rig.err("agentSession.send", json!({"envelope": env("claude_a", "s5", "f"), "body": {"blocks": [{"type": "image-ref", "path": "/rca/agent-session-attachments/00000000-0000-4000-8000-000000000000/x.png"}]}}));
    // Then all are refused, and of the four only the never-uploaded one reached rca, which refused it
    assert!(running.1.contains("idle") && url.1.contains("urls") && outside.1.contains("not an attachment"), "{running:?} {url:?} {outside:?}");
    assert_eq!(never.0, "runtime_error");
    assert_eq!(sent(&rig, "turn.start").len(), 2);
}

#[test]
fn given_the_rows_the_area_does_not_offer_when_called_then_each_answers_not_offered_with_its_reason_which_is_the_decisions_files() {
    // Given the decisions file and a host with the area
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/plans/rca-parity/coverage/orca-decisions.json");
    let decisions: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut rig = Rig::new();
    assert!(!NOT_OFFERED.is_empty());
    for row in NOT_OFFERED {
        // When the row is called
        let r = rig.call(row.name, json!({}));
        // Then it answers not_offered with the reason and milestone the decisions file records for it, byte for byte, as a proposed default
        let d = &decisions["decisions"][format!("rpc:{}", row.name)];
        assert_eq!(d["state"], "not-offered", "{}", row.name);
        assert_eq!((r["error"]["code"].clone(), r["error"]["data"].clone()), (json!("not_offered"), json!({"decidedBy": d["decidedBy"]})), "{}", row.name);
        assert_eq!(r["error"]["message"], format!("{} is not offered by this host: {}", row.name, d["reason"].as_str().unwrap()), "{}", row.name);
        assert_eq!(row.reason, d["reason"].as_str().unwrap());
        assert!(row.reason.starts_with("owner-confirmed (decision 7)"), "{}", row.name);
    }
    // And the rows the owner decision names are all among them
    for name in ["orchestration.partyLocation", "orchestration.partyOpen", "orchestration.workerAutoClose", "orchestration.workerKeepOpen", "agentSession.readVisual"] {
        assert!(NOT_OFFERED.iter().any(|r| r.name == name), "{name}");
    }
}

#[test]
fn given_every_row_of_the_area_when_called_then_the_agent_methods_sent_are_exactly_the_ones_listed() {
    // Given each served row called once, the approval and the steer by cancel and send
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    rig.idle("claude_busy");
    rig.ok("agentSession.create", create_params("op1", "go"));
    rig.ok("agentSession.send", json!({"envelope": env("claude_busy", "s0", "f"), "body": body("a")}));
    rig.ok("agentSession.send", json!({"envelope": env("claude_busy", "s1", "f"), "body": body("steer")}));
    rig.ok("agentSession.send", json!({"envelope": env("claude_busy", "s2", "f"), "delivery": "queue-if-active", "body": body("queued")}));
    rig.ok("agentSession.cancel", json!({"envelope": env("claude_busy", "c1", "f")}));
    rig.ok("agentSession.cancel", json!({"envelope": env("claude_rec", "c2", "f"), "turnId": "u1", "prompt": {"itemId": "question:rq1", "expectedRevision": 14}}));
    rig.ok("agentSession.cancel", json!({"envelope": env("claude_rec", "c3", "f"), "turnId": "u1", "prompt": {"itemId": "approval:ap1", "expectedRevision": 9}}));
    rig.idle("claude_idle");
    rig.ok("agentSession.conversationCommand", json!({"envelope": env("claude_idle", "k1", "f"), "command": "clear"}));
    rig.ok("agentSession.history", json!({"sessionId": "claude_rec", "direction": "tail"}));
    rig.ok("agentSession.item", json!({"sessionId": "claude_rec", "itemId": "tool:c1"}));
    rig.ok("agentSession.currentRequests", json!({"sessionId": "claude_rec", "version": "00000000-0000-4000-8000-000000000000"}));
    rig.ok("agentSession.hold", json!({"sessionId": "claude_rec", "holderId": "h"}));
    rig.ok("agentSession.release", json!({"sessionId": "claude_rec", "holderId": "h"}));
    rig.ok("nativeChat.readSession", json!({"agent": "claude", "sessionId": "claude_rec"}));
    rig.frames("agentSession.subscribe", json!({"sessionId": "claude_rec"}));
    rig.ok("agentSession.unsubscribe", json!({"sessionId": "claude_rec"}));
    let shot = rig.upload("claude_busy", "shot.png", b"png")["path"].clone();
    let started = rig.ok("agentSessionAttachment.uploadStart", json!({"sessionId": "claude_busy", "name": "x", "byteLength": 1}));
    rig.ok("agentSessionAttachment.uploadAbort", json!({"uploadId": started["uploadId"]}));
    rig.ok("agentSessionAttachment.read", json!({"path": shot}));
    rig.ok("agentSessionAttachment.readTextChunk", json!({"path": shot, "offset": 0, "length": 3}));
    rig.ok("files.readChatImage", json!({"worktree": "main", "path": shot}));
    rig.ok("agent.launch", json!({"agent": "claude", "operationId": "1730000000000-x", "target": {"kind": "existing", "worktree": "main"}}));
    rig.ok("agentSession.options", json!({"sessionId": "claude_rec"}));
    rig.ok("agentSession.setOption", json!({"envelope": env("claude_idle", "m1", "f"), "key": "model", "value": "opus"}));
    rig.ok("agentSession.modelCatalog", json!({"agent": "claude"}));
    rig.ok("agentSession.commands", json!({"sessionId": "claude_rec"}));
    rig.ok("agentSession.createSupport", json!({"worktree": "main", "agent": "claude"}));
    rig.ok("preflight.detectAgents", json!({}));
    rig.ok("preflight.refreshAgents", json!({}));
    rig.ok("preflight.detectRemoteAgents", json!({"connectionId": rig.catalog.executor()}));
    rig.catalog.has_tools(&["git", "gh"]);
    rig.ok("preflight.check", json!({}));
    rig.ok("clipboard.saveImageAsTempFile", json!({"contentBase64": "AAAA"}));
    rig.catalog.says("hi");
    rig.ok("speech.dictation.setup", json!({"enabled": true, "modelId": "rca-whisper"}));
    rig.ok("speech.dictation.start", json!({"dictationId": "d9"}));
    rig.ok("speech.dictation.chunk", json!({"dictationId": "d9", "audioBase64": pcm_chunk(10), "sampleRate": 16000}));
    rig.ok("speech.dictation.finish", json!({"dictationId": "d9"}));
    rig.ok("skills.discover", json!({"worktreeId": "feat"}));
    rig.ok("aiVault.listSessions", json!({}));
    let (mut q, ids) = queued_rig();
    let fp = body_fingerprint("claude_q", "one");
    q.ok("agentSession.queuedMessageUpdate", mutation("claude_q", "e1", json!({"messageId": ids[0], "expectedBodyFingerprint": fp, "text": "uno"})));
    q.ok("agentSession.queuedMessageMove", mutation("claude_q", "e2", json!({"messageId": ids[2], "beforeMessageId": ids[0]})));
    q.ok("agentSession.queuedMessageEditHold", json!({"sessionId": "claude_q", "messageId": ids[0], "editId": "e", "action": "release"}));
    q.ok("agentSession.queuedMessageDelete", mutation("claude_q", "e3", json!({"messageId": ids[1]})));
    q.ok("agentSession.queuedMessageSend", mutation("claude_q", "e4", json!({"messageId": ids[0]})));
    q.threads.finish("claude_q", "completed");
    q.ok("agentSession.queuedMessageSend", mutation("claude_q", "e5", json!({"messageId": ids[2]})));
    q.ok("agentSession.queuedMessagesResume", mutation("claude_q", "e6", json!({})));
    rig.recorded("claude_open");
    let open = item_of(&mut rig, "claude_open", "question:rq1");
    rig.ok("agentSession.respondToApproval", mutation("claude_open", "e7", json!({"itemId": "approval:ap1", "expectedRevision": 9, "optionId": "accept"})));
    rig.ok("agentSession.respondToQuestion", mutation("claude_open", "e8", json!({"itemId": "question:rq1", "expectedRevision": open["revision"], "optionId": open["body"]["options"][0]["id"]})));
    let (_, second) = two_turns(&mut rig);
    let epoch = rig.ok("agentSession.history", json!({"sessionId": "claude_two", "direction": "tail"}))["page"]["epoch"].clone();
    rig.ok("agentSession.rewind", json!({"envelope": env("claude_two", "rw9", "fp"), "itemId": second, "expectedEpoch": epoch}));
    let mut status = rig.host.connection("dev9", "tok9", vec!["agent-session.structured.v1".to_string()]);
    status_frames(&mut status, "st");
    // When the methods asked of the agent API are collected
    let seen: BTreeSet<String> = rig.agent.requests().into_iter().chain(q.agent.requests()).map(|(m, _)| m).collect();
    // Then they are the list the crate declares, plus the workspace method `repo.list` the target selector reads (which the workspace crate lists)
    let declared: BTreeSet<String> = USES.iter().map(|m| m.name().to_string()).chain(["repo.list".to_string()]).collect();
    assert_eq!(seen, declared);
}

fn catalog_models(rig: &Rig, driver: &str) -> Vec<Value> {
    rig.agent.request("models.list", json!({"driver": driver})).unwrap()["models"].as_array().unwrap().clone()
}

fn default_id(models: &[Value]) -> Value {
    models.iter().find(|m| m["is_default"] == true).unwrap()["id"].clone()
}

#[test]
fn given_a_session_when_its_options_are_read_then_the_models_are_the_drivers_with_their_efforts_and_the_current_model_is_the_catalogs_default_until_one_is_picked() {
    // Given a claude session and the models the catalog lists for its driver
    let mut rig = Rig::new();
    rig.idle("claude_s");
    let models = catalog_models(&rig, "claudeAgent");
    // When the phone reads its options
    let r = rig.ok("agentSession.options", json!({"sessionId": "claude_s"}));
    // Then the models are the catalog's in the host's field names, each with the efforts it lists
    let shown: Vec<(Value, Value, Value)> = r["models"].as_array().unwrap().iter().map(|m| (m["id"].clone(), m["isDefault"].clone(), m["efforts"].clone())).collect();
    assert_eq!(shown, models.iter().map(|m| (m["id"].clone(), m["is_default"].clone(), m["efforts"].clone())).collect::<Vec<_>>());
    let with_effort = models.iter().position(|m| m["default_effort"].is_string()).unwrap();
    assert_eq!(r["models"][with_effort]["defaultEffort"], models[with_effort]["default_effort"]);
    // And the current model is the default, marked as the catalog's own choice, and the session takes images and the two conversation commands
    assert_eq!((r["current"]["model"].clone(), r["current"]["defaulted"].clone()), (default_id(&models), json!(["model"])));
    assert_eq!((r["acceptsImages"].clone(), r["conversationCommands"].clone()), (json!(true), json!(["clear", "compact"])));
    // And a session the phone never named a model for on another driver lists that driver's models
    rig.agent.request("thread.create", json!({"thread": "codex_s", "workspace": "main", "driver": "codex"})).unwrap();
    let codex = rig.ok("agentSession.options", json!({"sessionId": "codex_s"}));
    assert_eq!(codex["models"].as_array().unwrap().iter().map(|m| m["id"].clone()).collect::<Vec<_>>(), catalog_models(&rig, "codex").iter().map(|m| m["id"].clone()).collect::<Vec<_>>());
}

#[test]
fn given_a_model_picked_with_setoption_when_the_next_turn_starts_then_it_runs_on_that_model_and_options_show_it_and_picking_the_default_clears_the_pick() {
    // Given an idle claude session and a model of its catalog that is not the default
    let mut rig = Rig::new();
    rig.idle("claude_s");
    let models = catalog_models(&rig, "claudeAgent");
    let other = models.iter().find(|m| m["is_default"] == false).unwrap()["id"].clone();
    // When the phone picks it
    let r = rig.ok("agentSession.setOption", json!({"envelope": env("claude_s", "o1", "f1"), "key": "model", "value": other}));
    // Then the pick is accepted with the options now in force
    assert_eq!((r["ok"].clone(), r["value"]["key"].clone(), r["value"]["value"].clone(), r["value"]["options"].clone()), (json!(true), json!("model"), other.clone(), json!({"model": other})));
    // And the session's options show it as the model, no longer the catalog's choice
    let shown = rig.ok("agentSession.options", json!({"sessionId": "claude_s"}));
    assert_eq!((shown["current"]["model"].clone(), shown["current"].get("defaulted").cloned()), (other.clone(), None));
    // And a send with no model of its own starts its turn on it
    rig.ok("agentSession.send", json!({"envelope": env("claude_s", "s1", "f"), "body": body("go")}));
    assert_eq!(sent(&rig, "turn.start")[0]["model"], other);
    // And a retry of the pick answers what it answered and stores nothing again
    let again = rig.ok("agentSession.setOption", json!({"envelope": env("claude_s", "o1", "f1"), "key": "model", "value": other}));
    assert_eq!((again["replayed"].clone(), again["value"].clone()), (json!(true), r["value"].clone()));
    // And picking the default model clears the pick, so the options show the default as the catalog's choice again
    let back = default_id(&models);
    rig.threads.finish("claude_s", "completed");
    rig.ok("agentSession.setOption", json!({"envelope": env("claude_s", "o2", "f2"), "key": "model", "value": back}));
    let cleared = rig.ok("agentSession.options", json!({"sessionId": "claude_s"}));
    assert_eq!((cleared["current"]["model"].clone(), cleared["current"]["defaulted"].clone()), (back, json!(["model"])));
    rig.ok("agentSession.send", json!({"envelope": env("claude_s", "s2", "f"), "body": body("again")}));
    assert!(sent(&rig, "turn.start")[1].get("model").is_none());
}

#[test]
fn given_options_the_host_does_not_offer_when_set_then_each_is_refused_and_nothing_is_stored_and_the_turn_runs_on_no_pick() {
    // Given an idle claude session
    let mut rig = Rig::new();
    rig.idle("claude_s");
    // When the phone sets a model the catalog does not list, an effort, and a value that is no string
    let unknown = rig.ok("agentSession.setOption", json!({"envelope": env("claude_s", "o1", "f1"), "key": "model", "value": "gpt-9"}));
    let effort = rig.ok("agentSession.setOption", json!({"envelope": env("claude_s", "o2", "f2"), "key": "effort", "value": "high"}));
    let (code, _) = rig.err("agentSession.setOption", json!({"envelope": env("claude_s", "o3", "f3"), "key": "model", "value": 3}));
    // Then the first two are refused as unsupported options, the last as an invalid value, and no pick is stored
    assert_eq!((unknown["ok"].clone(), unknown["refusal"]["code"].clone()), (json!(false), json!("agent_session_option_unsupported")));
    assert_eq!((effort["ok"].clone(), effort["refusal"]["code"].clone()), (json!(false), json!("agent_session_option_unsupported")));
    assert_eq!(code, "invalid_argument");
    let shown = rig.ok("agentSession.options", json!({"sessionId": "claude_s"}));
    assert_eq!(shown["current"]["defaulted"], json!(["model"]));
    rig.ok("agentSession.send", json!({"envelope": env("claude_s", "s1", "f"), "body": body("go")}));
    assert!(sent(&rig, "turn.start")[0].get("model").is_none());
    // And a session that does not exist is refused as the host words it
    assert_eq!(rig.err("agentSession.options", json!({"sessionId": "claude_none"})).1, "agent_session_not_found");
}

#[test]
fn given_the_models_of_an_agent_when_the_catalog_is_read_then_they_are_the_drivers_and_an_agent_without_a_driver_is_refused() {
    // Given the models the catalog lists for codex
    let mut rig = Rig::new();
    let models = catalog_models(&rig, "codex");
    // When the phone reads the catalog of codex, with or without a session or workspace
    let r = rig.ok("agentSession.modelCatalog", json!({"agent": "codex", "worktree": "main", "savedOnly": true}));
    // Then it is a finished listing of those models, fetched now
    assert_eq!(r["origin"], "probe");
    assert_eq!(r["models"].as_array().unwrap().iter().map(|m| (m["id"].clone(), m["isDefault"].clone())).collect::<Vec<_>>(), models.iter().map(|m| (m["id"].clone(), m["is_default"].clone())).collect::<Vec<_>>());
    assert!(r["fetchedAt"].as_u64().unwrap() > 1_700_000_000_000);
    // And an agent this host has no driver for is refused as a structured session is
    assert_eq!(rig.err("agentSession.modelCatalog", json!({"agent": "gemini"})).1, "structured_agent_session_unsupported");
}

#[test]
fn given_skills_in_the_workspace_and_home_when_the_commands_are_read_then_the_session_lists_the_ones_its_driver_reads_with_their_files() {
    // Given a claude session in main, whose executor has a claude skill, a codex skill and a shared home skill
    let mut rig = Rig::new();
    rig.idle("claude_s");
    rig.agent.request("thread.create", json!({"thread": "codex_s", "workspace": "main", "driver": "codex"})).unwrap();
    // When the phone reads the commands of each session
    let claude = rig.ok("agentSession.commands", json!({"sessionId": "claude_s"}));
    let codex = rig.ok("agentSession.commands", json!({"sessionId": "codex_s"}));
    // Then claude lists the skills of its two directories, the workspace's first, with their descriptions and files, and not the codex one
    assert_eq!(claude["commands"], json!([{"name": "review", "kind": "skill", "description": "Reviews a diff", "path": ".claude/skills/review/SKILL.md"}, {"name": "notes", "kind": "skill", "description": "Keeps notes", "path": "~/.agents/skills/notes/SKILL.md"}]));
    // And codex lists its own, which has no description, and the shared one
    assert_eq!(codex["commands"], json!([{"name": "lint", "kind": "skill", "path": ".codex/skills/lint/SKILL.md"}, {"name": "notes", "kind": "skill", "description": "Keeps notes", "path": "~/.agents/skills/notes/SKILL.md"}]));
    // And the skill commands asked the catalog for the session's workspace
    assert_eq!(sent(&rig, "commands.list").iter().map(|p| (p["driver"].clone(), p["workspace"].clone())).collect::<Vec<_>>(), vec![(json!("claudeAgent"), json!("main")), (json!("codex"), json!("main"))]);
}

#[test]
fn given_the_agent_clis_of_the_executor_when_detection_is_read_then_the_phone_gets_the_ids_of_the_installed_ones_and_a_refresh_asks_the_executor_again() {
    // Given an executor that has claude and codex but not opencode
    let mut rig = Rig::new();
    // When the phone detects the agents, asks for a refresh and detects by repo
    let detected = rig.ok("preflight.detectAgents", json!({}));
    let by_repo = rig.ok("preflight.detectAgents", json!({"repoId": "main"}));
    let refreshed = rig.ok("preflight.refreshAgents", json!({}));
    // Then the ids are the installed agents' in the host's names, and opencode is not among them
    assert_eq!((detected.clone(), by_repo), (json!(["claude", "codex"]), json!(["claude", "codex"])));
    // And the refresh answers the same agents with the path facts the host adds, and the executor was asked to look again
    assert_eq!((refreshed["agents"].clone(), refreshed["addedPathSegments"].clone()), (detected, json!([])));
    assert_eq!((refreshed["shellHydrationOk"].clone(), refreshed["pathSource"].clone(), refreshed["pathFailureReason"].clone()), (json!(true), json!("shell_hydrate"), json!("none")));
    assert_eq!(rig.catalog.detections().iter().map(|p| p["refresh"] == true).collect::<Vec<_>>(), vec![false, false, true]);
    // And once opencode is installed a refresh lists it too
    rig.catalog.has(&["claude", "codex", "opencode"]);
    assert_eq!(rig.ok("preflight.refreshAgents", json!({}))["agents"], json!(["opencode", "claude", "codex"]));
    // And a detection for a project, which the host answers with a launch context of its own, is refused
    assert!(rig.err("preflight.detectAgents", json!({"project": {"repo": "main"}})).1.contains("project"));
}

#[test]
fn given_a_connection_when_remote_agents_are_detected_then_the_executor_of_that_name_answers_and_an_unknown_one_is_the_hosts_dropped_connection() {
    // Given the executor `m1` with claude installed
    let mut rig = Rig::new();
    rig.catalog.has(&["claude"]);
    let executor = rig.catalog.executor();
    // When the phone detects the agents of that connection and of one the host does not know
    let known = rig.ok("preflight.detectRemoteAgents", json!({"connectionId": executor}));
    let (code, message) = rig.err("preflight.detectRemoteAgents", json!({"connectionId": "ghost"}));
    // Then the known one lists claude, and the unknown one is refused in the words the host uses for a dropped connection
    assert_eq!(known, json!(["claude"]));
    assert_eq!((code.as_str(), message.as_str()), ("runtime_error", "Remote connection dropped. Click Reconnect on the SSH target before retrying."));
    assert_eq!(rig.err("preflight.detectRemoteAgents", json!({})).1, "Invalid connection id");
}

#[test]
fn given_agents_the_executor_has_or_lacks_when_create_support_is_read_then_a_chat_is_supported_only_for_an_installed_agent_with_a_driver() {
    // Given an executor with claude and codex, and a workspace main
    let mut rig = Rig::new();
    // When the phone asks whether chats can be created for claude, opencode (not installed) and gemini (no driver)
    let claude = rig.ok("agentSession.createSupport", json!({"worktree": "id:main::", "agent": "claude"}));
    let opencode = rig.ok("agentSession.createSupport", json!({"worktree": "id:main::", "agent": "opencode"}));
    let gemini = rig.ok("agentSession.createSupport", json!({"worktree": "id:main::", "agent": "gemini"}));
    // Then claude is supported, and the others are unsupported for the agent
    assert_eq!(claude, json!({"supported": true}));
    assert_eq!((opencode, gemini), (json!({"supported": false, "reason": "agent"}), json!({"supported": false, "reason": "agent"})));
    // And installing opencode makes it supported, with no answer kept from before
    rig.catalog.has(&["opencode"]);
    assert_eq!(rig.ok("agentSession.createSupport", json!({"worktree": "id:main::", "agent": "opencode"})), json!({"supported": true}));
    // And a workspace the host does not know, or a resume of a terminal session, is refused
    assert_eq!(rig.err("agentSession.createSupport", json!({"worktree": "id:nope::", "agent": "claude"})).0, "selector_not_found");
    assert_eq!(rig.err("agentSession.createSupport", json!({"worktree": "id:main::", "agent": "claude", "resumeFrom": {"providerSessionId": "p", "closingPaneKeys": []}})).1, "structured_agent_session_unsupported");
}

#[test]
fn given_skills_on_the_executor_when_discovered_then_the_roots_are_the_hosts_catalog_for_each_repo_and_the_home_and_each_skill_is_a_row_of_its_root() {
    // Given the skills of the executor: one in .claude/skills and one in .trae/skills of each workspace, one in .codex/skills (no repo root of the host), one in the home
    let mut rig = Rig::new();
    rig.catalog.skill("workspace", ".trae/skills", "fmt", None);
    // When the phone discovers the skills for the worktree feat
    let r = rig.ok("skills.discover", json!({"worktreeId": "feat"}));
    // Then a skill is a row of its root, labelled as the host labels it, with the files named under the repo's path or below ~ for the home
    let row = |file: &str| r["skills"].as_array().unwrap().iter().find(|s| s["skillFilePath"] == file).cloned().unwrap_or_else(|| panic!("{file}: {r}"));
    let review = row("/srv/wt/feat/.claude/skills/review/SKILL.md");
    assert_eq!(
        (review["providers"].clone(), review["sourceKind"].clone(), review["sourceLabel"].clone(), review["rootPath"].clone(), review["directoryPath"].clone(), review["description"].clone()),
        (json!(["claude"]), json!("repo"), json!("Repo feat .claude"), json!("/srv/wt/feat/.claude/skills"), json!("/srv/wt/feat/.claude/skills/review"), json!("Reviews a diff"))
    );
    assert_eq!((row("/srv/wt/feat/.trae/skills/fmt/SKILL.md")["providers"].clone(), row("/srv/wt/feat/.trae/skills/fmt/SKILL.md")["sourceLabel"].clone()), (json!(["agent-skills"]), json!("Repo feat .trae")));
    let notes = row("~/.agents/skills/notes/SKILL.md");
    assert_eq!((notes["sourceKind"].clone(), notes["sourceLabel"].clone(), notes["providers"].clone()), (json!("home"), json!("Agent skills home"), json!(["agent-skills"])));
    assert!(r["skills"].as_array().unwrap().iter().all(|s| s["name"] != "lint"), "{r}");
    // And the sources are the host's 23 home roots and 12 roots of each repo (the asked worktree and the root workspace main), sorted by label
    let sources = r["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 23 + 12 * 2);
    let labels: Vec<String> = sources.iter().map(|s| s["label"].as_str().unwrap().to_lowercase()).collect();
    assert!(labels.windows(2).all(|w| w[0] <= w[1]), "{labels:?}");
    // And a root the executor saw exists, one it did not is missing, and a root holds the owner and providers of its agent
    let by_path = |p: &str| sources.iter().find(|s| s["path"] == p).cloned().unwrap_or_else(|| panic!("{p}"));
    assert_eq!(by_path("~/.agents/skills"), json!({"id": "home-agents", "label": "Agent skills home", "path": "~/.agents/skills", "sourceKind": "home", "providers": ["agent-skills"], "owner": null, "exists": true}));
    assert_eq!((by_path("~/.claude/skills")["exists"].clone(), by_path("~/.claude/skills")["skippedReason"].clone()), (json!(false), json!("missing")));
    assert_eq!((by_path("/srv/wt/feat/.trae/skills")["exists"].clone(), by_path("/srv/wt/feat/.trae/skills")["owner"].clone()), (json!(true), json!("trae")));
    assert_eq!(by_path("~/.codex/plugins/cache")["sourceKind"], "plugin");
    // And a filter by source kind or by name narrows the answer
    let home = rig.ok("skills.discover", json!({"worktreeId": "feat", "sourceKinds": ["home"]}));
    assert_eq!(home["skills"].as_array().unwrap().iter().map(|s| s["name"].clone()).collect::<Vec<_>>(), vec![json!("notes")]);
    assert!(home["sources"].as_array().unwrap().iter().all(|s| s["sourceKind"] == "home"));
    let named = rig.ok("skills.discover", json!({"cwd": "/srv/wt/feat", "names": ["FMT"]}));
    assert!(named["skills"].as_array().unwrap().iter().all(|s| s["name"] == "fmt") && !named["skills"].as_array().unwrap().is_empty());
    // And with no folder every workspace is a repo, a folder no workspace holds is listed unanswered, and an unknown runtime is refused
    assert_eq!(rig.ok("skills.discover", json!({}))["sources"].as_array().unwrap().len(), 23 + 12 * 2);
    let elsewhere = rig.ok("skills.discover", json!({"cwd": "/tmp/elsewhere"}));
    let unanswered = elsewhere["sources"].as_array().unwrap().iter().find(|s| s["path"] == "/tmp/elsewhere/.claude/skills").unwrap().clone();
    assert_eq!((unanswered["exists"].clone(), unanswered["skippedReason"].clone()), (json!(false), json!("unavailable")));
    assert_eq!(rig.err("skills.discover", json!({"runtime": 7})).1, "Invalid option: expected one of \"host\"|\"wsl\"");
    assert_eq!(rig.err("skills.discover", json!({"worktreeId": "nope"})).0, "selector_not_found");
}

#[test]
fn given_the_double_of_the_catalog_when_its_answers_are_read_as_the_contract_then_they_parse_and_every_driver_of_the_contract_has_an_orca_agent() {
    // Given the double's answers to the four methods
    let rig = Rig::new();
    let ask = |method: &'static str, params: Value| rig.agent.request(method, params).unwrap();
    // Then each parses as the contract crate's result, so a drift of the double from the service's shapes fails here
    for driver in agent_api_catalog::DRIVERS {
        serde_json::from_value::<agent_api_catalog::ModelsResult>(ask("models.list", json!({"driver": driver}))).unwrap();
        serde_json::from_value::<agent_api_catalog::CommandsResult>(ask("commands.list", json!({"driver": driver, "workspace": "main"}))).unwrap();
    }
    serde_json::from_value::<agent_api_catalog::DetectResult>(ask("agents.detect", json!({}))).unwrap();
    serde_json::from_value::<agent_api_catalog::SkillsResult>(ask("skills.list", json!({"workspace": "main"}))).unwrap();
    // And each driver the contract names is one this crate maps to an Orca agent
    assert_eq!(orca_map_agent::drivers_served(), agent_api_catalog::DRIVERS.to_vec());
}

#[test]
fn given_cli_sessions_in_the_executors_home_when_the_vault_is_listed_then_each_is_a_scan_row_with_its_resume_line_and_a_matching_fingerprint_is_unchanged() {
    // Given two sessions of the executor
    let mut rig = Rig::new();
    rig.catalog.session("claude", "s1", "/srv/wt/feat", "fix the build", 1_793_494_861_123).session("codex", "c1", "/srv/other", "add a test", 1_000);
    // When the vault is listed, and again scoped to one worktree
    let all = rig.ok("aiVault.listSessions", json!({}));
    let scoped = rig.ok("aiVault.listSessions", json!({"scopePaths": ["/srv/wt/feat"], "scopeOnly": true}));
    // Then the rows carry the host's fields, the resume line, and a path below ~
    assert_eq!(all["issues"], json!([]));
    let first = &all["sessions"][0];
    assert_eq!((first["id"].clone(), first["modifiedAt"].clone(), first["resumeCommand"].clone(), first["filePath"].clone()), (json!("claude:s1"), json!("2026-11-01T01:01:01.123Z"), json!("cd '/srv/wt/feat' && claude --resume 's1'"), json!("~/.claude/sessions/s1.jsonl")));
    assert_eq!(all["sessions"][1]["resumeCommand"], json!("cd '/srv/other' && codex resume 'c1'"));
    assert_eq!(scoped["sessions"].as_array().unwrap().len(), 1);
    // And a request that names the last fingerprint is answered unchanged, a forced one is not, and a bad fingerprint is refused by the schema's words
    let fp = rig.ok("aiVault.listSessions", json!({"afterFingerprint": "0".repeat(64)}))["fingerprint"].clone();
    let same = rig.ok("aiVault.listSessions", json!({"afterFingerprint": fp}));
    assert_eq!((same["unchanged"].clone(), same["sessions"].clone()), (json!(true), json!([])));
    assert!(rig.ok("aiVault.listSessions", json!({"afterFingerprint": fp, "force": true}))["sessions"].as_array().unwrap().len() == 2);
    assert_eq!(rig.err("aiVault.listSessions", json!({"afterFingerprint": "a"})).1, "Invalid string: must match pattern /^[0-9a-f]{64}$/");
    assert_eq!(rig.err("aiVault.listSessions", json!({"limit": 5000})).1, "Limit exceeds maximum");
    // And a resume is prepared with nothing to copy, and a malformed one is refused
    let args = json!({"agent": "codex", "filePath": "~/.codex/sessions/c1.jsonl", "codexHome": null});
    assert_eq!(rig.ok("aiVault.prepareSessionResume", args), json!({"useRealCodexHome": false}));
    assert_eq!(rig.err("aiVault.prepareSessionResume", json!(null)).1, "Invalid input");
}

#[test]
fn an_unsubscribe_names_its_session_the_way_the_schema_and_the_host_do_and_a_chat_unsubscribe_takes_no_other_type() {
    // Given a connection with no stream, When the session id is missing, of the wrong type or not an agent session id, Then each is refused in its own words
    let mut rig = Rig::new();
    assert_eq!(rig.err("agentSession.unsubscribe", json!({})).1, "Invalid input: expected string, received undefined");
    assert_eq!(rig.err("agentSession.unsubscribe", json!({"sessionId": 7})).1, "Invalid input: expected string, received number");
    assert_eq!(rig.err("agentSession.unsubscribe", json!({"sessionId": "a"})).1, "Invalid agent session id");
    // and a native chat unsubscribe with a subscription id of another type is refused, one without any is ended
    assert_eq!(rig.err("nativeChat.unsubscribe", json!({"subscriptionId": 7})).1, "Invalid input: expected string, received number");
    assert_eq!(rig.ok("nativeChat.unsubscribe", json!(null)), json!({"unsubscribed": true}));
}

#[test]
fn given_a_send_the_phone_lost_the_reply_of_when_it_asks_whether_it_was_recorded_then_a_known_message_is_and_an_unknown_one_is_not() {
    // Given a chat created with a first message under the operation id `op1`
    let mut rig = Rig::new();
    rig.ok("agentSession.create", create_params("op1", "fix the login"));
    // When the phone asks whether that message, then another it never sent, was recorded
    let known = rig.ok("agentSession.sendRecorded", json!({"sessionId": "claude_s1", "clientMessageId": "op1"}));
    let unknown = rig.ok("agentSession.sendRecorded", json!({"sessionId": "claude_s1", "clientMessageId": "op-never"}));
    // Then the first is recorded and the second is not
    assert_eq!((known, unknown), (json!({"recorded": true}), json!({"recorded": false})));
    // And a chat the host does not have is refused as every session method refuses it, and a malformed ask by the schema's words
    assert_eq!(rig.err("agentSession.sendRecorded", json!({"sessionId": "claude_none", "clientMessageId": "op1"})).1, "agent_session_not_found");
    for (params, message) in [
        (json!({}), "Invalid input: expected string, received undefined"),
        (json!({"sessionId": "claude_s1"}), "Invalid input: expected string, received undefined"),
        (json!({"sessionId": "", "clientMessageId": "a"}), "Invalid session id"),
        (json!({"sessionId": "claude_s1", "clientMessageId": ""}), "Invalid client message id"),
        (json!({"sessionId": "claude_s1", "clientMessageId": "op1", "extra": 1}), "Unrecognized key: \"extra\""),
    ] {
        assert_eq!(rig.err("agentSession.sendRecorded", params.clone()).1, message, "{params}");
    }
}

#[test]
fn given_a_host_when_the_phone_lists_its_agents_then_each_served_driver_is_an_agent_with_the_capabilities_a_session_here_has() {
    // Given a host serving claude, codex and opencode drivers
    let mut rig = Rig::new();
    // When the phone lists the agents
    let r = rig.ok("agentSession.agents", json!({}));
    // Then every served driver is there once, with the capabilities of this host's sessions
    let names: Vec<&str> = r["agents"].as_array().unwrap().iter().map(|a| a["agent"].as_str().unwrap()).collect();
    assert_eq!(names, ["claude", "codex", "opencode"]);
    assert_eq!(r["agents"][0]["capabilities"], json!({"rewind": true, "compact": true, "threadGoal": false, "contextUsage": false, "imagePrompts": true, "steering": "queue", "approvalEnforcement": "orca"}));
    // And a key the schema lacks is refused in its words
    assert_eq!(rig.err("agentSession.agents", json!({"x": 1})), ("invalid_argument".to_string(), "Unrecognized key: \"x\"".to_string()));
}

#[test]
fn given_a_native_chat_subscription_when_the_chat_grows_then_the_phone_gets_the_tail_snapshot_then_the_changed_rows_with_the_turn_state_and_an_unsubscribe_ends_it() {
    // Given the recorded thread
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    // When the phone subscribes with a window of two
    let opened = rig.frames("nativeChat.subscribe", json!({"agent": "claude", "sessionId": "claude_rec", "limit": 2, "subscriptionId": "s1"}));
    // Then the only frame is the snapshot: the newest two message rows of readSession, a page before them, and no error
    assert_eq!(opened.len(), 1);
    let snap = &opened[0]["result"];
    let read = rig.ok("nativeChat.readSession", json!({"agent": "claude", "sessionId": "claude_rec", "limit": 2}));
    assert_eq!((opened[0]["streaming"].clone(), snap["type"].clone(), snap["messages"].clone(), snap["hasMore"].clone(), snap["beforeOffset"].clone()), (json!(true), json!("snapshot"), read["messages"].clone(), json!(true), read["beforeOffset"].clone()));
    assert!(snap.get("error").is_none() && snap["messages"][0].get("transcriptOrder").is_some(), "{snap}");
    // When a turn starts and the assistant answers
    rig.agent.request("turn.start", json!({"thread": "claude_rec", "workspace": "main", "driver": "claudeAgent", "prompt": "again"})).unwrap();
    rig.threads.push("claude_rec", json!({"type": "message.delta", "role": "assistant", "text": "done"}));
    std::thread::sleep(Duration::from_millis(300));
    let frames: Vec<Value> = rig.conn.poll().iter().map(|f| serde_json::from_str(f).unwrap()).collect();
    // Then one appended frame carries the changed rows and the chat is working
    assert_eq!(frames.len(), 1, "{frames:?}");
    let appended = &frames[0]["result"];
    assert_eq!((appended["type"].clone(), appended["lifecycle"]["state"].clone()), (json!("appended"), json!("working")));
    assert!(appended["messages"].as_array().unwrap().iter().any(|m| m["blocks"][0]["text"] == "done" || m["blocks"][0]["text"] == "again"), "{appended}");
    // When the phone unsubscribes by its token, Then no stream remains
    assert_eq!(rig.ok("nativeChat.unsubscribe", json!({"subscriptionId": "s1"})), json!({"unsubscribed": true}));
    rig.conn.poll();
    assert_eq!(rig.conn.open_streams(), 0);
}

#[test]
fn given_a_chat_the_host_does_not_hold_when_its_transcript_is_read_then_the_answer_is_the_recorded_unavailable_result() {
    // Given a host with no chat, When the phone reads the transcript of an agent and chat it does not hold
    let mut rig = Rig::new();
    let read = rig.ok("nativeChat.readSession", json!({"agent": "a", "sessionId": "a"}));
    // Then the result is the host's own unavailable answer, not a refusal
    assert_eq!(read, json!({"error": "Transcript unavailable"}));
    // And a read that names no agent is refused as invalid
    assert_eq!(rig.err("nativeChat.readSession", json!({"sessionId": "a"})).0, "invalid_argument");
}

#[test]
fn given_an_agent_the_host_does_not_run_or_a_chat_rca_lacks_when_subscribed_then_the_first_is_the_recorded_unavailable_snapshot_and_the_second_waits_in_silence() {
    // Given a host with no chat yet
    let mut rig = Rig::new();
    // When the phone subscribes with an agent the host does not run (the recorded case), and to a chat of a known agent rca has not made
    let unknown = rig.frames("nativeChat.subscribe", json!({"agent": "a", "sessionId": "a"}));
    let waiting = rig.frames("nativeChat.subscribe", json!({"agent": "claude", "sessionId": "claude_later"}));
    // Then the first is the recorded snapshot and the second says nothing
    assert_eq!((unknown.len(), unknown[0]["result"].clone()), (1, json!({"type": "snapshot", "messages": [], "hasMore": false, "error": "Transcript unavailable"})));
    assert!(waiting.is_empty(), "{waiting:?}");
    // When the chat appears and the stream is polled, Then its snapshot follows
    rig.recorded("claude_later");
    std::thread::sleep(Duration::from_millis(300));
    let frames: Vec<Value> = rig.conn.poll().iter().map(|f| serde_json::from_str(f).unwrap()).collect();
    assert_eq!(frames.iter().filter(|f| f["result"]["type"] == "snapshot" && f["result"]["messages"].as_array().is_some_and(|m| !m.is_empty())).count(), 1, "{frames:?}");
}

#[test]
fn given_malformed_native_chat_subscriptions_when_made_then_each_is_refused_with_the_first_issue_of_the_schema() {
    // Given a host
    let mut rig = Rig::new();
    // When the phone subscribes with missing, empty and mistyped fields
    let cases = [
        (json!({}), "Missing agent"),
        (json!({"agent": 7, "sessionId": "x"}), "Missing agent"),
        (json!({"agent": "claude"}), "Missing session id"),
        (json!({"agent": "claude", "sessionId": ""}), "Missing session id"),
        (json!({"agent": "claude", "sessionId": "x", "limit": 0}), "Too small: expected number to be >0"),
        (json!({"agent": "claude", "sessionId": "x", "limit": 1.5}), "Invalid input: expected int, received number"),
        (json!({"agent": "claude", "sessionId": "x", "limit": "3"}), "Invalid input: expected number, received string"),
        (json!({"agent": "claude", "sessionId": "x", "subscriptionId": ""}), "Too small: expected string to have >=1 characters"),
    ];
    // Then each is an invalid_argument error frame with that sentence
    for (params, message) in cases {
        let frames = rig.frames("nativeChat.subscribe", params.clone());
        assert_eq!(frames.len(), 1, "{params}: {frames:?}");
        let text = frames[0]["error"]["message"].as_str().or_else(|| frames[0]["message"].as_str()).unwrap_or_default().to_string();
        assert_eq!(text, message, "{params}: {}", frames[0]);
    }
}

fn status_frames(conn: &mut Connection, id: &str) -> Vec<Value> {
    conn.on_message(&json!({"id": id, "method": "agentSession.subscribeStatus", "params": {}}).to_string()).iter().map(|f| serde_json::from_str(f).unwrap()).collect()
}

#[test]
fn given_a_client_without_the_structured_capability_when_it_subscribes_to_the_status_feed_then_it_is_refused_as_the_recorded_host_refuses_it() {
    // Given a host and a connection that declared no capability
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    // When it subscribes to the status feed
    let frames = rig.frames("agentSession.subscribeStatus", json!({}));
    // Then it gets the recorded refusal, no stream stays open, and rca was not asked to list threads
    assert_eq!(frames.len(), 1, "{frames:?}");
    assert_eq!((frames[0]["error"]["code"].clone(), frames[0]["error"]["message"].clone(), frames[0]["error"]["data"]["refusal"]["details"]["reason"].clone()), (json!("runtime_error"), json!("structured_agent_session_unsupported"), json!("clientCapabilityMissing")));
    assert_eq!(rig.conn.open_streams(), 0);
    assert!(rig.agent.requests().iter().all(|(m, _)| m != "thread.list"));
}

#[test]
fn given_chats_when_a_capable_client_subscribes_to_the_status_feed_then_it_gets_the_naming_and_sessions_snapshots_then_a_status_for_each_chat_that_changes() {
    // Given the recorded chat (it holds open prompts), a chat with no messages, a chat of a driver this host does not run, and a connection that declared the structured capability
    let mut rig = Rig::new();
    rig.recorded("claude_rec");
    rig.idle("claude_new");
    rig.agent.request("thread.create", json!({"thread": "other_x", "workspace": "main", "driver": "someOtherDriver"})).unwrap();
    let mut conn = rig.host.connection("dev3", "tok3", vec!["agent-session.structured.v1".to_string()]);
    // When it subscribes
    let opened = status_frames(&mut conn, "s1");
    // Then the first frame names both chats (null: neither has a title), the second lists them, and the foreign driver is in neither
    assert_eq!(opened.len(), 2, "{opened:?}");
    assert_eq!(opened[0]["result"], json!({"type": "snapshot", "sessions": [], "namingActiveSessionIds": [], "conversationNames": [{"sessionId": "claude_new", "conversationName": null}, {"sessionId": "claude_rec", "conversationName": null}]}));
    let sessions = opened[1]["result"]["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2, "{sessions:?}");
    let by = |id: &str| sessions.iter().find(|s| s["sessionId"] == id).unwrap().clone();
    let rec = by("claude_rec");
    // the recorded chat has an open approval: it needs attention, and carries its last prompt and answer
    assert_eq!((rec["workspaceId"].clone(), rec["agent"].clone(), rec["status"].clone(), rec["latestPrompt"].clone(), rec["lastAssistantMessage"].clone()), (json!("main"), json!("claude"), json!("attention"), json!("Why does the login fail?"), json!("Fixed.")));
    assert_eq!(by("claude_new")["status"], Value::Null);
    // When a turn starts on the empty chat
    rig.agent.request("turn.start", json!({"thread": "claude_new", "workspace": "main", "driver": "claudeAgent", "prompt": "again"})).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let later: Vec<Value> = conn.poll().iter().map(|f| serde_json::from_str(f).unwrap()).collect();
    // Then exactly one status frame follows, for that chat, now working on the prompt
    assert_eq!(later.len(), 1, "{later:?}");
    assert_eq!((later[0]["result"]["type"].clone(), later[0]["result"]["session"]["sessionId"].clone(), later[0]["result"]["session"]["status"].clone(), later[0]["result"]["session"]["latestPrompt"].clone()), (json!("status"), json!("claude_new"), json!("working"), json!("again")));
    // And with nothing more changing, a later tick sends nothing
    std::thread::sleep(Duration::from_millis(300));
    assert!(conn.poll().is_empty());
}

#[test]
fn given_the_host_when_status_is_read_then_it_advertises_the_status_feed_and_structured_capabilities_it_serves() {
    // Given the agent area
    let area = AgentArea::default();
    // When its capabilities are read, Then it names both
    assert_eq!(orca_core::Area::capabilities(&area), ["agent-session.structured.v1", "agent-session.status-feed.v1"]);
}

/// A chat with two finished turns ("one" then "two"), nothing open, and the ids of the two user messages.
fn two_turns(rig: &mut Rig) -> (String, String) {
    rig.idle("claude_two");
    for prompt in ["one", "two"] {
        rig.agent.request("turn.start", json!({"thread": "claude_two", "workspace": "main", "driver": "claudeAgent", "prompt": prompt})).unwrap();
        rig.threads.push("claude_two", json!({"type": "message.delta", "role": "assistant", "text": format!("answer {prompt}")}));
        rig.threads.finish("claude_two", "completed");
    }
    let page = rig.ok("agentSession.history", json!({"sessionId": "claude_two", "direction": "tail"}))["page"].clone();
    let users: Vec<String> = page["items"].as_array().unwrap().iter().filter(|i| i["body"]["kind"] == "message" && i["body"]["role"] == "user").map(|i| i["itemId"].as_str().unwrap().to_string()).collect();
    (users[0].clone(), users[1].clone())
}

#[test]
fn given_two_finished_turns_when_the_phone_rewinds_to_the_second_message_then_the_second_turn_is_rolled_back_the_journal_has_a_new_epoch_and_the_same_operation_replays() {
    // Given a chat of two finished turns
    let mut rig = Rig::new();
    let (_, second) = two_turns(&mut rig);
    let before = rig.ok("agentSession.history", json!({"sessionId": "claude_two", "direction": "tail"}))["page"].clone();
    let epoch = before["epoch"].clone();
    // When the phone rewinds to the second user message
    let params = json!({"envelope": env("claude_two", "rw1", "fp"), "itemId": second, "expectedEpoch": epoch});
    let done = rig.ok("agentSession.rewind", params.clone());
    // Then rca rolled back to the first turn, the answer carries the item and a new epoch, and the history no longer holds the second turn's messages
    assert_eq!(sent(&rig, "thread.rollback"), vec![json!({"thread": "claude_two", "turn": "u1", "restore_files": false})]);
    assert_eq!((done["ok"].clone(), done["replayed"].clone(), done["value"]["itemId"].clone()), (json!(true), json!(false), json!(second)));
    assert_ne!(done["value"]["epoch"], epoch);
    assert_eq!(done["cursor"]["epoch"], done["value"]["epoch"]);
    let after = rig.ok("agentSession.history", json!({"sessionId": "claude_two", "direction": "tail"}))["page"].clone();
    let texts: Vec<String> = after["items"].as_array().unwrap().iter().filter(|i| i["body"]["kind"] == "message").map(|i| i["body"]["blocks"][0]["text"].as_str().unwrap().to_string()).collect();
    assert_eq!(texts, ["one", "answer one"]);
    assert_eq!(after["epoch"], done["value"]["epoch"]);
    // When the same operation is sent again, Then the recorded answer comes back replayed and rca is asked no second time
    let again = rig.ok("agentSession.rewind", params);
    assert_eq!(again["replayed"], json!(true));
    assert_eq!(sent(&rig, "thread.rollback").len(), 1);
}

#[test]
fn given_a_chat_when_a_rewind_cannot_be_done_then_it_is_refused_with_the_hosts_reason_and_rca_is_asked_for_nothing() {
    // Given a chat of two finished turns and the current epoch
    let mut rig = Rig::new();
    let (first, second) = two_turns(&mut rig);
    let epoch = rig.ok("agentSession.history", json!({"sessionId": "claude_two", "direction": "tail"}))["page"]["epoch"].clone();
    let reason = |r: &Value| (r["ok"].clone(), r["refusal"]["code"].clone(), r["refusal"]["message"].clone(), r["refusal"]["details"]["rewindReason"].clone());
    // When the phone names an old epoch, the first turn's message (nothing is kept before it), an item that is no user message, and a missing item
    let stale = rig.ok("agentSession.rewind", json!({"envelope": env("claude_two", "r1", "fp"), "itemId": second, "expectedEpoch": "rca-99"}));
    let opening = rig.ok("agentSession.rewind", json!({"envelope": env("claude_two", "r2", "fp"), "itemId": first, "expectedEpoch": epoch}));
    let not_user = rig.ok("agentSession.rewind", json!({"envelope": env("claude_two", "r3", "fp"), "itemId": "turn:u1", "expectedEpoch": epoch}));
    let missing = rig.ok("agentSession.rewind", json!({"envelope": env("claude_two", "r4", "fp"), "itemId": "i999", "expectedEpoch": epoch}));
    // Then each is a rewind refusal with its reason
    let invalid = |why: &str| (json!(false), json!("agent_session_operation_invalid"), json!(format!("agent_session_rewind:{why}")), json!(why));
    assert_eq!(reason(&stale), invalid("stale-epoch"));
    assert_eq!(reason(&opening), invalid("unsupported"));
    assert_eq!(reason(&not_user), invalid("invalid-target"));
    assert_eq!(reason(&missing), invalid("invalid-target"));
    // And a chat with a running turn is busy
    rig.agent.request("turn.start", json!({"thread": "claude_two", "workspace": "main", "driver": "claudeAgent", "prompt": "three"})).unwrap();
    let busy = rig.ok("agentSession.rewind", json!({"envelope": env("claude_two", "r5", "fp"), "itemId": second, "expectedEpoch": epoch}));
    assert_eq!(reason(&busy), invalid("busy"));
    assert!(sent(&rig, "thread.rollback").is_empty());
}

#[test]
fn given_a_session_id_that_is_no_agent_session_id_when_a_row_takes_it_then_it_is_refused_as_an_invalid_agent_session_id() {
    // Given a host
    let mut rig = Rig::new();
    let envelope = json!({"clientOperationId": "op", "expectedRuntimeFence": 1, "payloadFingerprint": "f", "sessionId": "a"});
    // When the phone releases, sets an option, or asks for a model catalog under an id of no agent, or leaves the id out or sends a number
    let release = rig.err("agentSession.release", json!({"sessionId": "a", "holderId": "h"}));
    let release_number = rig.err("agentSession.release", json!({"sessionId": 7}));
    let release_none = rig.err("agentSession.release", json!({}));
    let option = rig.err("agentSession.setOption", json!({"envelope": envelope, "key": "model", "value": "m"}));
    let catalog = rig.err("agentSession.modelCatalog", json!({"agent": "claude", "sessionId": "a"}));
    // Then the id is the refusal, or the schema's own message when it is no string
    let invalid = ("invalid_argument".to_string(), "Invalid agent session id".to_string());
    assert_eq!((release, option, catalog), (invalid.clone(), invalid.clone(), invalid));
    assert_eq!(release_number.1, "Invalid input: expected string, received number");
    assert_eq!(release_none.1, "Invalid input: expected string, received undefined");
    // And a session id of an agent is released
    assert_eq!(rig.ok("agentSession.release", json!({"sessionId": "claude_a", "holderId": "h"})), json!({"released": true}));
}

#[test]
fn given_detection_inputs_of_the_wrong_type_or_for_a_host_this_machine_is_not_when_agents_are_detected_then_each_is_refused_as_the_host_refuses_it() {
    // Given an executor with agent CLIs
    let mut rig = Rig::new();
    // When the phone detects or refreshes with a number for the worktree, a flag that is no boolean, or another host's id
    let wrong_type = rig.err("preflight.detectAgents", json!({"worktreeId": 7}));
    let wrong_refresh = rig.err("preflight.refreshAgents", json!({"worktreeId": 7}));
    let wrong_flag = rig.err("preflight.refreshAgents", json!({"userInitiated": "yes"}));
    let other_host = rig.err("preflight.detectAgents", json!({"worktreeHostId": "a", "project": {"repo": "a"}}));
    // Then the schema's own message or the host id refusal answers, and this machine's id is accepted
    assert_eq!(wrong_type, ("invalid_argument".into(), "Invalid input: expected string, received number".into()));
    assert_eq!(wrong_refresh, wrong_type);
    assert_eq!(wrong_flag, ("invalid_argument".into(), "Invalid input: expected boolean, received string".into()));
    assert_eq!(other_host, ("invalid_argument".into(), "Invalid host id".into()));
    rig.ok("preflight.refreshAgents", json!({"worktreeHostId": "local", "userInitiated": true}));
}

#[test]
fn given_calls_with_no_session_or_envelope_when_sent_then_each_is_refused_with_the_hosts_schema_words() {
    // Given a host with no session
    let mut rig = Rig::new();
    // When the phone sends each method with no params, Then the schema's words name what is missing
    for method in ["agentSession.history", "agentSession.item", "agentSession.hold", "agentSession.options", "agentSession.commands", "agentSession.currentRequests", "agentSession.subscribe"] {
        assert_eq!(rig.err(method, json!({})).1, "Invalid input: expected string, received undefined", "{method}");
    }
    for method in ["agentSession.send", "agentSession.cancel", "agentSession.setOption", "agentSession.queuedMessageSend", "agentSession.respondToApproval"] {
        assert_eq!(rig.err(method, json!({})).1, "Invalid input: expected object, received undefined", "{method}");
        assert_eq!(rig.err(method, json!({"envelope": "x"})).1, "Invalid input: expected object, received string", "{method}");
    }
    assert_eq!(rig.err("agentSession.create", json!({})).1, "Invalid input");
    assert_eq!(rig.err("agentSession.queuedMessageMove", json!({})).1, "Invalid input");
    assert_eq!(rig.err("agentSession.queuedMessageEditHold", json!({})).1, "Invalid discriminator value. Expected 'acquire' | 'renew' | 'release'");
    assert_eq!(rig.err("agent.launchReplay", json!({})).1, "Unknown TUI agent");
    assert_eq!(rig.err("agentSession.history", json!({"sessionId": 7})).1, "Invalid input: expected string, received number");
}

fn preflight(git: bool, gh: (bool, bool), glab: (bool, bool)) -> Value {
    let none = json!({"configured": false, "authenticated": false, "account": null, "baseUrl": null, "tokenConfigured": false});
    json!({"git": {"installed": git}, "gh": {"installed": gh.0, "authenticated": gh.1}, "glab": {"installed": glab.0, "authenticated": glab.1},
        "bitbucket": {"configured": false, "authenticated": false, "account": null}, "azureDevOps": none, "gitea": none})
}

#[test]
fn given_an_executor_with_git_gh_signed_in_and_glab_signed_out_when_preflight_is_checked_then_the_phone_gets_the_hosts_status_from_one_detection_and_one_sign_in_per_cli() {
    // Given an executor that has git, a signed-in gh and a signed-out glab
    let mut rig = Rig::new();
    rig.catalog.has_tools(&["git", "gh", "glab"]).signed_in(&["github"]);
    // When the phone checks its preflight
    let status = rig.ok("preflight.check", json!({}));
    // Then it is the host's status: the three tools installed, gh signed in, glab not, the other providers not configured
    assert_eq!(status, preflight(true, (true, true), (true, false)));
    // And rca was asked one detection from its kept answer, then each installed CLI's sign-in at the root workspace's host
    assert_eq!(rig.catalog.detections(), [json!({})]);
    assert_eq!(rig.catalog.discovers(), [json!({"workspace": "main", "provider": "github"}), json!({"workspace": "main", "provider": "gitlab"})]);
    // When it is forced, Then the executor is asked again
    rig.ok("preflight.check", json!({"force": true}));
    assert_eq!(rig.catalog.detections().last(), Some(&json!({"refresh": true})));
}

#[test]
fn given_no_forge_cli_or_a_sign_in_that_fails_when_preflight_is_checked_then_nothing_is_signed_in_and_only_installed_clis_are_asked() {
    // Given an executor with none of the tools
    let mut rig = Rig::new();
    // When the phone checks its preflight with no payload
    let none = rig.ok("preflight.check", Value::Null);
    // Then nothing is installed or signed in, and no sign-in was asked
    assert_eq!((none, rig.catalog.discovers()), (preflight(false, (false, false), (false, false)), vec![]));
    // Given gh is installed but its sign-in check fails
    rig.catalog.has_tools(&["git", "gh"]).discover_refused();
    // When checked, Then gh is installed and not signed in, and the answer is still the status
    assert_eq!(rig.ok("preflight.check", json!({})), preflight(true, (true, false), (false, false)));
}

#[test]
fn given_a_force_of_the_wrong_type_or_a_payload_that_is_no_object_when_preflight_is_checked_then_it_is_the_schemas_refusal_and_rca_is_asked_nothing() {
    let mut rig = Rig::new();
    for (params, message) in [
        (json!({"force": "wrong"}), "Invalid input: expected boolean, received string"),
        (json!({"force": null}), "Invalid input: expected boolean, received null"),
        (json!({"force": 1}), "Invalid input: expected boolean, received number"),
        (json!([1]), "Invalid input: expected object, received array"),
    ] {
        // Given a payload the pinned schema refuses, When sent, Then the refusal is its first complaint
        assert_eq!(rig.err("preflight.check", params.clone()), ("invalid_argument".to_string(), message.to_string()), "{params}");
    }
    assert_eq!(rig.catalog.detections(), Vec::<Value>::new());
}

/// The bytes the root workspace's staged file holds, read back from the attachment rca kept under the id in its path.
fn staged_bytes(rig: &Rig, path: &str) -> (Value, Vec<u8>) {
    let id = path.split('/').nth(2).unwrap().trim_start_matches("rca-paste-");
    let got = rig.agent.request("attachment.get", json!({"id": id})).unwrap();
    (got["attachment"]["mime_type"].clone(), STANDARD.decode(got["data"].as_str().unwrap()).unwrap())
}

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];

#[test]
fn given_a_pasted_image_when_saved_as_a_temp_file_then_it_is_kept_and_staged_in_the_root_workspace_named_by_its_time_and_kind() {
    // Given a PNG the phone pasted
    let mut rig = Rig::new();
    // When it is saved as a temp file
    let path = rig.ok("clipboard.saveImageAsTempFile", json!({"contentBase64": STANDARD.encode(PNG)}));
    // Then the answer is its file below the root workspace's staging folder, named as the host names a paste, holding the bytes as a PNG
    let path = path.as_str().unwrap().to_string();
    let parts: Vec<&str> = path.split('/').collect();
    assert_eq!((parts.len(), parts[1], parts[2].starts_with("rca-paste-")), (4, "tmp", true), "{path}");
    assert!(parts[3].starts_with("orca-paste-") && parts[3].ends_with(".png"), "{path}");
    assert_eq!(staged_bytes(&rig, &path), (json!("image/png"), PNG.to_vec()));
    // And rca was asked to keep it, then to stage it in the root workspace
    let asked: Vec<(String, Value)> = rig.agent.requests().into_iter().filter(|(m, _)| m == "attachment.stage").collect();
    assert_eq!(asked, [("attachment.stage".to_string(), json!({"workspace": "main", "ids": [parts[2].trim_start_matches("rca-paste-")], "temp": true}))]);
}

#[test]
fn given_an_image_uploaded_in_chunks_when_committed_then_it_keeps_the_picked_name_and_each_wrong_step_is_the_hosts_refusal() {
    // Given an upload of a picked photo started and sent in two chunks of base64
    let mut rig = Rig::new();
    let data = STANDARD.encode(PNG);
    let (first, second) = data.split_at(8);
    let id = rig.ok("clipboard.startImageUpload", json!({"expectedBase64Length": data.len(), "fileName": "my shot$(x).png"}))["uploadId"].clone();
    let runtime = |m: &str| ("runtime_error".to_string(), m.to_string());
    // When a chunk comes out of order, Then it is refused and nothing is taken
    assert_eq!(rig.err("clipboard.appendImageUploadChunk", json!({"uploadId": id, "offset": 4, "contentBase64": first})), runtime("Clipboard image chunk offset is out of order"));
    assert_eq!(rig.ok("clipboard.appendImageUploadChunk", json!({"uploadId": id, "offset": 0, "contentBase64": first})), json!({"receivedBase64Length": 8}));
    // And another device cannot add to it, and a commit before the last chunk is incomplete and ends the upload
    assert_eq!(rig.other_err("clipboard.appendImageUploadChunk", json!({"uploadId": id, "offset": 8, "contentBase64": second})), runtime("Clipboard image upload was not found"));
    assert_eq!(rig.ok("clipboard.appendImageUploadChunk", json!({"uploadId": id, "offset": 8, "contentBase64": second})), json!({"receivedBase64Length": data.len()}));
    assert_eq!(rig.err("clipboard.appendImageUploadChunk", json!({"uploadId": id, "offset": data.len(), "contentBase64": "AAAA"})), runtime("Clipboard image upload exceeded expected size"));
    // When it is committed
    let path = rig.ok("clipboard.commitImageUpload", json!({"uploadId": id}));
    // Then the file keeps the picked name with what a shell would expand made inert, and holds the bytes
    let path = path.as_str().unwrap().to_string();
    assert!(path.ends_with("/my shot__x_.png"), "{path}");
    assert_eq!(staged_bytes(&rig, &path).1, PNG.to_vec());
    // And the upload is gone once committed, an incomplete one is refused and gone, and an abort of an unknown upload says aborted
    assert_eq!(rig.err("clipboard.commitImageUpload", json!({"uploadId": id})), runtime("Clipboard image upload was not found"));
    let short = rig.ok("clipboard.startImageUpload", json!({"expectedBase64Length": 8}))["uploadId"].clone();
    assert_eq!(rig.err("clipboard.commitImageUpload", json!({"uploadId": short})), runtime("Clipboard image upload is incomplete"));
    assert_eq!(rig.err("clipboard.commitImageUpload", json!({"uploadId": short})), runtime("Clipboard image upload was not found"));
    assert_eq!(rig.ok("clipboard.abortImageUpload", json!({"uploadId": "a"})), json!({"aborted": true}));
    // And an upload for an SSH target is the host's refusal at commit, and a ninth upload in flight is refused
    let remote = rig.ok("clipboard.startImageUpload", json!({"expectedBase64Length": 0, "connectionId": "ssh-1"}))["uploadId"].clone();
    assert_eq!(rig.err("clipboard.commitImageUpload", json!({"uploadId": remote})), runtime("Remote connection dropped. Click Reconnect on the SSH target before retrying."));
    for _ in 0..8 {
        rig.ok("clipboard.startImageUpload", json!({"expectedBase64Length": 4}));
    }
    assert_eq!(rig.err("clipboard.startImageUpload", json!({"expectedBase64Length": 4})), runtime("Too many clipboard image uploads are in progress"));
}

#[test]
fn given_clipboard_requests_that_break_the_pinned_schema_when_sent_then_each_is_its_first_complaint_and_rca_is_asked_nothing() {
    let mut rig = Rig::new();
    let invalid = |m: &str| ("invalid_argument".to_string(), m.to_string());
    for (method, params, message) in [
        ("clipboard.saveImageAsTempFile", Value::Null, "Missing image content"),
        ("clipboard.saveImageAsTempFile", json!({"contentBase64": "a"}), "Clipboard image content must be base64"),
        ("clipboard.saveImageAsTempFile", json!({"contentBase64": "wrong"}), "Clipboard image content must be base64"),
        ("clipboard.saveImageAsTempFile", json!({"contentBase64": "AAAA", "connectionId": ""}), "Too small: expected string to have >=1 characters"),
        ("clipboard.startImageUpload", Value::Null, "Invalid input: expected number, received undefined"),
        ("clipboard.startImageUpload", json!({"expectedBase64Length": "wrong"}), "Invalid input: expected number, received string"),
        ("clipboard.startImageUpload", json!({"expectedBase64Length": 1.5}), "Invalid input: expected int, received number"),
        ("clipboard.startImageUpload", json!({"expectedBase64Length": -1}), "Too small: expected number to be >=0"),
        ("clipboard.startImageUpload", json!({"expectedBase64Length": 25_165_825}), "Clipboard image is too large"),
        ("clipboard.startImageUpload", json!({"expectedBase64Length": 0, "fileName": ""}), "Too small: expected string to have >=1 characters"),
        ("clipboard.appendImageUploadChunk", Value::Null, "Invalid input: expected string, received undefined"),
        ("clipboard.appendImageUploadChunk", json!({"contentBase64": "a", "offset": 0, "uploadId": 7}), "Invalid input: expected string, received number"),
        ("clipboard.appendImageUploadChunk", json!({"contentBase64": "a", "offset": 0, "uploadId": "a"}), "Clipboard image content must be base64"),
        ("clipboard.commitImageUpload", json!({}), "Invalid input: expected string, received undefined"),
        ("clipboard.abortImageUpload", json!({"uploadId": 7}), "Invalid input: expected string, received number"),
    ] {
        // Given a request the schema refuses, When sent, Then the refusal is its first complaint
        assert_eq!(rig.err(method, params.clone()), invalid(message), "{method} {params}");
    }
    // And the largest upload the host takes is started
    assert!(rig.ok("clipboard.startImageUpload", json!({"expectedBase64Length": 25_165_824}))["uploadId"].is_string());
    assert!(rig.agent.requests().iter().all(|(m, _)| !m.starts_with("upload.") && !m.starts_with("attachment.")));
}

fn pcm_chunk(samples: usize) -> String {
    STANDARD.encode(vec![1u8; samples * 2])
}

#[test]
fn given_the_dictation_settings_when_a_phone_lists_sets_up_and_dictates_then_the_chunks_are_kept_in_order_once_and_sent_as_one_wav_for_the_text() {
    // Given a service with a speech engine that hears "open the settings", and a host whose dictation is off
    let mut rig = Rig::new();
    rig.catalog.says("open the settings");
    // When the phone lists the models, and starts a dictation
    let listed = rig.ok("speech.models.list", json!(null));
    let off = rig.err("speech.dictation.start", json!({"dictationId": "d1"}));
    // Then the one model is ready and the dictation is off, so the start is refused as the host refuses it
    assert_eq!(listed["models"], json!([{"id": "rca-whisper", "label": "Whisper (rca)", "provider": "local", "sizeBytes": null, "recommended": true, "status": "ready", "progress": null}]));
    assert_eq!((listed["enabled"].clone(), listed["selectedModelId"].clone(), listed["dictationMode"].clone(), listed["stopAfterSilenceSeconds"].clone()), (json!(false), json!(""), json!("toggle"), json!(60)));
    assert_eq!(off.1, "voice_dictation_disabled");
    // And when it sets the dictation up, and starts, feeds chunks 1, 0, a resend of 0 and 2, and finishes
    let set = rig.ok("speech.dictation.setup", json!({"enabled": true, "modelId": "rca-whisper", "dictationMode": "hold"}));
    let started = rig.ok("speech.dictation.start", json!({"dictationId": "d1"}));
    let sent: Vec<Value> = [1u64, 0, 0, 2].iter().map(|seq| rig.ok("speech.dictation.chunk", json!({"dictationId": "d1", "audioBase64": pcm_chunk(100), "sampleRate": 16000, "seq": seq}))).collect();
    let done = rig.ok("speech.dictation.finish", json!({"dictationId": "d1"}));
    // Then the setup is kept and listed, the dictation says it dedupes resends, and the service heard one WAV of the three chunks once each
    assert_eq!((set["enabled"].clone(), set["selectedModelId"].clone(), set["dictationMode"].clone(), set["dictationHoldToRecord"].clone()), (json!(true), json!("rca-whisper"), json!("hold"), json!(true)));
    assert_eq!(started, json!({"dictationId": "d1", "modelId": "rca-whisper", "dedupesResentChunks": true}));
    assert!(sent.iter().all(|r| r == &json!({"dictationId": "d1"})), "{sent:?}");
    assert_eq!(done, json!({"dictationId": "d1", "text": "open the settings"}));
    let heard = rig.catalog.heard();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].1, "wav");
    assert_eq!(heard[0].0, STANDARD.encode(vec![0u8; 44 + 3 * 200]).len());
    // And a finish with no dictation, a chunk of another dictation, and a cancel of a stale one are told apart
    assert_eq!(rig.err("speech.dictation.finish", json!({"dictationId": "d1"})).1, "dictation_stream_not_started");
    assert_eq!(rig.ok("speech.dictation.cancel", json!({"dictationId": "gone"})), json!({"dictationId": "gone"}));
}

#[test]
fn given_two_phones_setting_up_dictation_at_once_when_both_read_the_kept_voice_settings_first_then_both_changes_are_kept() {
    // Given two connections of one host whose reads of the kept settings overlap
    let rig = Rig::new();
    let agent: Arc<dyn AgentApi> = Arc::new(orca_core::testing::OverlappingReads::over(rig.agent.clone(), Duration::from_millis(300)));
    let host = Host::new("rt".into(), vec![Box::new(AgentArea::default())], agent, Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
    // When one phone turns dictation on and the other picks hold mode at the same moment
    let phones: Vec<_> = [json!({"enabled": true}), json!({"dictationMode": "hold"})]
        .into_iter()
        .map(|params| {
            let mut conn = host.connection("dev", "tok", vec![]);
            std::thread::spawn(move || serde_json::from_str::<Value>(&conn.on_message(&json!({"id": "r1", "method": "speech.dictation.setup", "params": params}).to_string())[0]).unwrap())
        })
        .collect();
    for phone in phones {
        assert_eq!(phone.join().unwrap()["ok"], true);
    }
    // Then the kept voice settings hold both
    let kept = rig.agent.state_get("orca", "settings").unwrap().unwrap();
    assert_eq!((kept["voice"]["enabled"].clone(), kept["voice"]["dictationMode"].clone()), (json!(true), json!("hold")));
}

#[test]
fn given_the_one_model_when_it_is_downloaded_deleted_or_another_is_chosen_then_each_is_answered_as_the_host_answers_a_model_it_cannot_do_that_to() {
    // Given a service with a speech engine
    let mut rig = Rig::new();
    rig.catalog.says("x");
    // When the model is downloaded, another is downloaded, one is deleted, an unknown one is set up, and a dictation starts with no model chosen
    let started = rig.ok("speech.models.download", json!({"modelId": "rca-whisper"}));
    let other = rig.err("speech.models.download", json!({"modelId": "cloud"}));
    let deleted = rig.err("speech.models.delete", json!({"modelId": "rca-whisper"}));
    let unknown = rig.err("speech.dictation.setup", json!({"modelId": "nope"}));
    rig.ok("speech.dictation.setup", json!({"enabled": true}));
    let none = rig.err("speech.dictation.start", json!({"dictationId": "d"}));
    // Then the download starts for the one model only, a delete and an unknown model are refused, and a start needs a model
    assert_eq!(started, json!({"started": true}));
    assert_eq!((other.1.as_str(), deleted.1.as_str(), unknown.1.as_str(), none.1.as_str()), ("voice_model_not_downloadable", "voice_model_delete_failed", "voice_model_unknown", "voice_model_not_selected"));
    // And with no engine at the service no model is listed
    let mut bare = Rig::new();
    assert_eq!(bare.ok("speech.models.list", json!({}))["models"], json!([]));
}

#[test]
fn given_a_saved_default_model_and_permission_when_a_chat_sends_then_the_turn_runs_on_them_unless_the_send_names_a_model() {
    // Given the settings the phone saved: claude's default model and the accept-edits permission
    let mut rig = Rig::new();
    rig.agent.set_state("orca", "settings", json!({"nativeChatPermissionMode": "accept-edits", "nativeChatSessionOptions": {"claude": {"model": "sonnet", "valuesByModel": {}}}}));
    rig.idle("claude_a");
    let send = |op: &str, extra: Value| {
        let mut p = json!({"envelope": env("claude_a", op, "f"), "body": body("go")});
        p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        p
    };
    // When a send names no model, and a later one names opus
    rig.ok("agentSession.send", send("a1", json!({})));
    rig.idle("claude_c");
    rig.ok("agentSession.send", json!({"envelope": env("claude_c", "c1", "f"), "body": body("go"), "options": {"model": "opus"}}));
    // Then the first ran on the saved model and both under the saved permission as rca names it
    let turns = sent(&rig, "turn.start");
    assert_eq!(turns.iter().map(|p| (p["model"].clone(), p["runtime_mode"].clone())).collect::<Vec<_>>(), vec![(json!("sonnet"), json!("auto-accept-edits")), (json!("opus"), json!("auto-accept-edits"))]);
    // And a saved mode rca has no equivalent of (auto) asks for approval rather than granting more
    rig.agent.set_state("orca", "settings", json!({"nativeChatPermissionMode": "auto"}));
    rig.idle("claude_b");
    rig.ok("agentSession.send", json!({"envelope": env("claude_b", "b1", "f"), "body": body("go")}));
    assert_eq!(sent(&rig, "turn.start").last().unwrap()["runtime_mode"], json!("approval-required"));
}

#[test]
fn given_a_chat_when_revealed_then_its_tab_is_kept_in_front_in_its_worktree_and_a_missing_chat_or_agent_is_refused_in_the_hosts_words() {
    // Given an idle chat in the main workspace and a thread of an agent this host does not run
    let mut rig = Rig::new();
    rig.idle("claude_a");
    rig.agent.request("thread.create", json!({"thread": "codex_x", "workspace": "main", "driver": "someOther"})).ok();
    // When the chat is revealed
    let revealed = rig.ok("agentSession.reveal", json!({"sessionId": "claude_a"}));
    // Then the answer names the chat, its workspace id and agent, and the chat's tab is kept and in front
    assert_eq!(revealed, json!({"ok": true, "sessionId": "claude_a", "workspaceId": "main::", "agent": "claude", "readable": true}));
    assert_eq!(rig.agent.state_get("orca", "tabs.extra.main").unwrap().unwrap()[0], json!({"id": "claude_a", "kind": "agent-session", "sessionId": "claude_a", "agent": "claude", "title": "Chat"}));
    assert_eq!(rig.agent.state_get("orca", "tabs.active.main").unwrap(), Some(json!("claude_a")));
    // And a chat the host does not have, one of an agent it cannot open, and a malformed id are refused
    assert_eq!(rig.ok("agentSession.reveal", json!({"sessionId": "claude_gone"})), json!({"ok": false, "refusal": {"code": "agent_session_not_found", "message": "This chat is no longer on this host."}}));
    assert_eq!(rig.ok("agentSession.reveal", json!({"sessionId": "codex_x"}))["refusal"]["code"], json!("structured_agent_session_unsupported"));
    assert_eq!(rig.err("agentSession.reveal", json!({"sessionId": "a"})), ("invalid_argument".to_string(), "Invalid agent session id".to_string()));
}
