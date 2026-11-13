//! The phone's agent sessions through the chain: a chat is an rca thread, a message a turn of the scripted CLI whose file the real rca-exec
//! writes, and what the phone reads (the stream, the history, the queue, the prompts) is what rca logged. A row counts only when the agent
//! method it maps to ran in its call's window and was answered, and a refusal is shown to have reached the service or to have asked it nothing.
use crate::rig::*;
use base64::Engine;
use exec_testkit::script::{Script, Turn};
use exec_testkit::Case;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const WS: &str = "w1";
// A loaded hosted shard (two cores, six shards) timed out here once at 30 s; the wait costs nothing when the start is prompt.
const SETTLE: Duration = Duration::from_secs(120);

fn said(text: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string()
}

/// A turn that says one thing, writes `path` through the executor's MCP tool and says another.
fn writes(path: &str, content: &str) -> Turn {
    Turn::replaying(&[said("writing the note")]).then_calling("k1", "write_file", json!({"path": path, "content": content, "create_dirs": true})).then_line(&said("done"))
}

/// A turn that asks the person through the service's MCP tool, and writes `path` once it is answered.
fn asking(tool: &str, args: Value, path: &str) -> Turn {
    Turn::replaying(&[said("asking")]).then_calling("ask1", tool, args).then_calling("w1", "write_file", json!({"path": path, "content": "after the answer", "create_dirs": true})).then_line(&said("done"))
}

fn env(session: &str, op: &str) -> Value {
    json!({"sessionId": session, "clientOperationId": op, "expectedRuntimeFence": null, "payloadFingerprint": format!("fp-{op}")})
}

fn body(text: &str) -> Value {
    json!({"kind": "message", "role": "user", "blocks": [{"type": "text", "text": text}]})
}

fn mutation(session: &str, op: &str, extra: Value) -> Value {
    let mut p = json!({"envelope": env(session, op)});
    p.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    p
}

pub(crate) fn answered(case: &Case, method: &str) {
    let window = last_window(case);
    assert!(window.iter().any(|(m, ok)| m == method && *ok), "{method} did not answer in the call's window: {window:?}");
}

pub(crate) fn asked_nothing_of(case: &Case, method: &str) {
    let window = last_window(case);
    assert!(window.iter().all(|(m, _)| m != method), "{method} was asked although the phone's call was refused: {window:?}");
}

pub(crate) fn executor_ran(case: &Case, tool: &str) -> bool {
    case.calls().last().unwrap()["executor"].as_array().unwrap().iter().any(|h| h["tool"] == tool && h["ok"] == true)
}

pub(crate) fn create_params(session: &str, op: &str, first: Option<&str>) -> Value {
    let mut p = json!({"envelope": env(session, op), "worktree": format!("id:{WS}::"), "agent": "claude"});
    if let Some(text) = first {
        p["firstMessage"] = json!({"clientMessageId": format!("m-{op}"), "body": body(text)});
    }
    p
}

fn items(page: &Value) -> Vec<Value> {
    page["items"].as_array().cloned().unwrap_or_default()
}

fn tail(phone: &mut Phone, case: &mut Case, chain: &Chain, session: &str) -> Value {
    let r = phone.call(case, chain, &["rpc:agentSession.history"], "agentSession.history", json!({"sessionId": session, "direction": "tail", "limit": 200}));
    answered(case, "thread.history");
    r["result"]["page"].clone()
}

fn texts(page: &Value, role: &str) -> Vec<String> {
    items(page).iter().filter(|i| i["body"]["kind"] == "message" && i["body"]["role"] == role).map(|i| i["body"]["blocks"][0]["text"].as_str().unwrap_or_default().to_string()).collect()
}

/// What the tool call `call` of the thread printed, as rca logged it (the answer a prompt tool returned to the CLI).
fn tool_output(rca: &mut Direct, thread: &str, call: &str) -> String {
    let mut out = String::new();
    wait_until("the tool's output", || {
        out = rca.events(thread, "tool.output").iter().filter(|e| e["call"] == call).filter_map(|e| e["data"].as_str().map(str::to_string)).collect::<Vec<_>>().join("");
        !out.is_empty()
    });
    out
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + SETTLE;
    while !done() {
        assert!(Instant::now() < deadline, "{what} never happened");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The prompts the CLI was started with, in order: the argument that carries each (the text it was sent).
fn started_with(chain: &Chain, texts: &[&str]) -> Vec<String> {
    chain.cli.invocations("claude").iter().map(|start| texts.iter().find(|t| start.argv.iter().any(|a| a.contains(**t))).map(|t| t.to_string()).unwrap_or_default()).collect()
}

#[test]
fn a_message_runs_a_turn_whose_file_rca_exec_writes_and_the_phone_reads_the_run_on_its_stream_its_history_and_its_items() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a CLI whose turn writes notes/a.txt through its MCP tool, a paired phone and a chat it created
        let chain = Chain::start(None);
        chain.cli.script("claude", &Script::of(vec![writes("notes/a.txt", "hello from the model")]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        let made = phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", create_params("claude_s1", "c1", None));
        answered(case, "thread.create");
        assert_eq!(made["result"]["ok"], true, "{made}");
        // When it opens the stream and sends a message that the turn answers
        let (stream, snapshot) = phone.subscribe(case, &chain, &["rpc:agentSession.subscribe"], "agentSession.subscribe", json!({"sessionId": "claude_s1"}));
        assert_eq!(snapshot["result"]["type"], "snapshot", "{snapshot}");
        let sent = phone.call_settled(case, &chain, &["rpc:agentSession.send"], "agentSession.send", json!({"envelope": env("claude_s1", "s1"), "body": body("write the note")}), |_| {
            rca.wait_events("claude_s1", "turn.finished", 1);
        });
        // Then rca started the turn, the executor wrote the file the CLI named, and the phone was told the send went through
        assert_eq!(sent["result"]["ok"], true, "{sent}");
        answered(case, "turn.start");
        assert!(executor_ran(case, "write_file"), "{:?}", case.calls().last());
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/a.txt")).unwrap(), "hello from the model", "{}", chain.evidence());
        // And the stream delivered a batch whose items are the run (the turn finished) under the request id the phone opened it with
        let mut finished = false;
        for _ in 0..40 {
            let frame = phone.next_frame(&stream);
            assert_eq!(frame["streaming"], true, "{frame}");
            let batch = &frame["result"]["batch"];
            finished = items(batch).iter().any(|i| i["body"]["kind"] == "turn" && i["body"]["state"] == "completed");
            if finished {
                break;
            }
        }
        assert!(finished, "the stream never showed the turn completed");
        // And the history is the conversation: what the phone said, what the model said, the tool call it made and the turn's end
        let page = tail(&mut phone, case, &chain, "claude_s1");
        assert_eq!(texts(&page, "user"), ["write the note"]);
        assert!(texts(&page, "assistant").iter().any(|t| t.contains("writing the note")), "{page}");
        let tool = items(&page).into_iter().find(|i| i["body"]["kind"] == "tool-call").expect("the tool call is an item");
        assert_eq!(tool["body"]["name"], "write_file");
        let read = phone.call(case, &chain, &["rpc:agentSession.item"], "agentSession.item", json!({"sessionId": "claude_s1", "itemId": tool["itemId"]}));
        answered(case, "thread.history");
        assert_eq!(read["result"]["item"]["body"]["callId"], tool["body"]["callId"]);
        // When the phone reads the chat as a native chat, holds and releases it, and asks for its open prompts
        let native = phone.call(case, &chain, &["rpc:nativeChat.readSession"], "nativeChat.readSession", json!({"agent": "claude", "sessionId": "claude_s1", "limit": 50}));
        assert!(native["result"]["messages"].as_array().unwrap().iter().any(|m| m["blocks"][0]["text"] == "write the note"), "{native}");
        let held = phone.call(case, &chain, &["rpc:agentSession.hold"], "agentSession.hold", json!({"sessionId": "claude_s1", "holderId": "h1"}));
        let released = phone.call(case, &chain, &["rpc:agentSession.release"], "agentSession.release", json!({"sessionId": "claude_s1", "holderId": "h1"}));
        let none = phone.call(case, &chain, &["rpc:agentSession.currentRequests"], "agentSession.currentRequests", json!({"sessionId": "claude_s1", "version": "00000000-0000-4000-8000-000000000000"}));
        // Then the answers are the host's: held, released, and a marker for a list with no open prompt
        assert_eq!((held["result"].clone(), released["result"].clone()), (json!({"held": true}), json!({"released": true})));
        assert_eq!(none["result"]["restart"], true, "{none}");
        // When the phone unsubscribes by the stream's id, Then it is acknowledged and the stream is gone
        let ended = phone.call(case, &chain, &["rpc:agentSession.unsubscribe"], "agentSession.unsubscribe", json!({"sessionId": "claude_s1", "subscriptionId": stream}));
        assert_eq!(ended["result"], json!({"unsubscribed": true}));
        let native_end = phone.call(case, &chain, &["rpc:nativeChat.unsubscribe"], "nativeChat.unsubscribe", json!({"subscriptionId": "s"}));
        assert_eq!(native_end["result"], json!({"unsubscribed": true}));
    });
}

#[test]
fn a_chat_created_with_a_message_and_a_launch_each_run_a_turn_once_and_a_retry_of_either_runs_nothing_again() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain, a CLI that writes a file in each of its two turns, and a paired phone
        let chain = Chain::start(None);
        chain.cli.script("claude", &Script::of(vec![writes("created.txt", "from a create"), writes("launched.txt", "from a launch")]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        // When a chat is created with its first message, and the same create is sent again
        let params = create_params("claude_c1", "k1", Some("make created.txt"));
        let created = phone.call_settled(case, &chain, &["rpc:agentSession.create"], "agentSession.create", params.clone(), |_| {
            rca.wait_events("claude_c1", "turn.finished", 1);
        });
        // Then the turn ran in the executor's repository and the file is there
        assert_eq!(created["result"]["ok"], true, "{created}");
        answered(case, "turn.start");
        assert!(executor_ran(case, "write_file"));
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("created.txt")).unwrap(), "from a create");
        let again = phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", params);
        asked_nothing_of(case, "turn.start");
        assert_eq!((again["result"]["replayed"].clone(), created["result"]["replayed"].clone()), (json!(true), json!(false)));
        assert_eq!(again["result"]["value"], created["result"]["value"], "a retry answers what the first did");
        assert_eq!(chain.cli.invocations("claude").len(), 1);
        // When a launch with a prompt is sent, replayed and sent again
        let launch = json!({"agent": "claude", "operationId": "1730000000000-abc", "target": {"kind": "existing", "worktree": format!("id:{WS}::")}, "prompt": {"text": "make launched.txt", "delivery": "submit"}});
        let launched = phone.call_settled(case, &chain, &["rpc:agent.launch"], "agent.launch", launch.clone(), |reply| {
            let session = reply["result"]["outcome"]["sessionId"].as_str().expect("the launch names its session").to_string();
            rca.wait_events(&session, "turn.finished", 1);
        });
        let replayed = phone.call(case, &chain, &["rpc:agent.launchReplay"], "agent.launchReplay", launch.clone());
        let twice = phone.call(case, &chain, &["rpc:agent.launch"], "agent.launch", launch);
        // Then the launch made a claude chat whose turn wrote its file, and the replay and the second launch answer the first's result and start nothing
        assert!(launched["result"]["outcome"]["sessionId"].as_str().unwrap().starts_with("claude_"));
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("launched.txt")).unwrap(), "from a launch", "{}", chain.evidence());
        assert_eq!((replayed["result"].clone(), twice["result"].clone()), (launched["result"].clone(), launched["result"].clone()));
        asked_nothing_of(case, "turn.start");
        assert_eq!(chain.cli.invocations("claude").len(), 2);
    });
}

#[test]
fn a_turn_that_asks_for_approval_and_a_question_waits_until_the_phone_answers_and_a_stale_answer_asks_rca_nothing() {
    exec_testkit::case!(SUITE, |case| {
        // Given a CLI whose turns ask the person through the service's MCP tools before they write: an approval, then a question
        let chain = Chain::start(None);
        chain.cli.script("claude", &Script::of(vec![asking("approval_prompt", json!({"tool_name": "write_file", "input": {"path": "a.txt"}}), "a.txt"), asking("ask_user", json!({"question": "which one?", "options": ["a", "b"]}), "b.txt")]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", create_params("claude_p1", "c1", None));
        // When the phone sends a message and the turn asks for approval
        phone.call(case, &chain, &["rpc:agentSession.send"], "agentSession.send", json!({"envelope": env("claude_p1", "s1"), "body": body("go")}));
        let approval = rca.wait_events("claude_p1", "approval.requested", 1).remove(0)["approval"].as_str().unwrap().to_string();
        std::thread::sleep(Duration::from_millis(500));
        // Then the turn is held: the file is not written, and the phone's current requests list the approval
        assert!(!chain.repo.path().join("a.txt").exists(), "the turn ran past the approval");
        let page = tail(&mut phone, case, &chain, "claude_p1");
        let item = items(&page).into_iter().find(|i| i["itemId"] == format!("approval:{approval}")).expect("the approval is an item");
        assert_eq!(page["actionablePromptIds"], json!([format!("approval:{approval}")]));
        let marker = phone.call(case, &chain, &["rpc:agentSession.currentRequests"], "agentSession.currentRequests", json!({"sessionId": "claude_p1", "version": "00000000-0000-4000-8000-000000000000"}));
        let listed = phone.call(case, &chain, &["rpc:agentSession.currentRequests"], "agentSession.currentRequests", json!({"sessionId": "claude_p1", "version": marker["result"]["marker"]["version"]}));
        let open: Vec<Value> = serde_json::from_str(listed["result"]["json"].as_str().unwrap()).unwrap();
        assert_eq!(open.iter().map(|i| i["itemId"].clone()).collect::<Vec<_>>(), [json!(format!("approval:{approval}"))]);
        // When it answers at a revision it did not see, Then the host refuses as stale after rca was asked nothing and the turn is still held
        let revision = item["revision"].as_u64().unwrap();
        let stale = phone.call(case, &chain, &["rpc:agentSession.respondToApproval"], "agentSession.respondToApproval", mutation("claude_p1", "a0", json!({"itemId": format!("approval:{approval}"), "expectedRevision": revision + 99, "optionId": "accept"})));
        assert_eq!(stale["result"]["refusal"]["code"], "agent_session_item_revision_stale", "{stale}");
        asked_nothing_of(case, "approval.answer");
        assert!(rca.events("claude_p1", "approval.answered").is_empty());
        // When it accepts at the revision it saw
        let accepted = phone.call_settled(case, &chain, &["rpc:agentSession.respondToApproval"], "agentSession.respondToApproval", mutation("claude_p1", "a1", json!({"itemId": format!("approval:{approval}"), "expectedRevision": revision, "optionId": "accept"})), |_| {
            rca.wait_events("claude_p1", "turn.finished", 1);
        });
        // Then rca took the answer, the turn went on and the executor wrote, and the log says what was decided
        assert_eq!(accepted["result"]["value"]["resolution"]["state"], "resolved", "{accepted}");
        answered(case, "approval.answer");
        assert!(executor_ran(case, "write_file"));
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("a.txt")).unwrap(), "after the answer");
        assert_eq!(rca.events("claude_p1", "approval.answered").len(), 1);
        assert!(tool_output(&mut rca, "claude_p1", "ask1").contains("allow"), "the tool was not told to allow");
        // When the next turn asks a question and the phone answers with an option
        phone.call(case, &chain, &["rpc:agentSession.send"], "agentSession.send", json!({"envelope": env("claude_p1", "s2"), "body": body("again")}));
        let question = rca.wait_events("claude_p1", "input.requested", 1).remove(0)["request"].as_str().unwrap().to_string();
        std::thread::sleep(Duration::from_millis(300));
        assert!(!chain.repo.path().join("b.txt").exists(), "the turn ran past the question");
        let page = tail(&mut phone, case, &chain, "claude_p1");
        let item = items(&page).into_iter().find(|i| i["itemId"] == format!("question:{question}")).expect("the question is an item");
        let answer = phone.call_settled(case, &chain, &["rpc:agentSession.respondToQuestion"], "agentSession.respondToQuestion", mutation("claude_p1", "q1", json!({"itemId": format!("question:{question}"), "expectedRevision": item["revision"], "optionId": "b"})), |_| {
            rca.wait_events("claude_p1", "turn.finished", 2);
        });
        // Then rca took the option as the answer's text and the turn wrote after it
        assert_eq!(answer["result"]["value"]["resolution"]["selectedOptionId"], "b", "{answer}");
        answered(case, "input.answer");
        assert_eq!(std::fs::read_to_string(chain.repo.path().join("b.txt")).unwrap(), "after the answer", "{}", chain.evidence());
        assert!(tool_output(&mut rca, "claude_p1", "ask1").contains('b'), "the tool's result did not carry the option");
    });
}

#[test]
fn queued_messages_are_edited_moved_and_deleted_on_rcas_queue_and_a_cancelled_turn_holds_the_queue_until_the_phone_resumes_it_in_the_order_it_set() {
    exec_testkit::case!(SUITE, |case| {
        // Given a turn that stays running, three queued messages, and CLI turns that only answer afterwards
        let chain = Chain::start(None);
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("working")]).hanging(), Turn::replaying(&[said("ok")]), Turn::replaying(&[said("ok")]), Turn::replaying(&[said("ok")])]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        let s = "claude_q1";
        phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", create_params(s, "c1", None));
        phone.call(case, &chain, &["rpc:agentSession.send"], "agentSession.send", json!({"envelope": env(s, "s0"), "body": body("first-job")}));
        wait_until("the first turn", || chain.cli.invocations("claude").len() == 1);
        for (n, text) in ["alpha-one", "beta-two", "gamma-three", "delta-four"].iter().enumerate() {
            let queued = phone.call(case, &chain, &["rpc:agentSession.send"], "agentSession.send", json!({"envelope": env(s, &format!("q{n}")), "delivery": "queue-if-active", "body": body(text)}));
            assert_eq!(queued["result"]["ok"], true, "{queued}");
            answered(case, "queue.add");
        }
        let cards = |page: &Value| -> Vec<(String, String)> { page["queuedMessages"].as_array().unwrap().iter().map(|c| (c["messageId"].as_str().unwrap().to_string(), c["body"]["blocks"][0]["text"].as_str().unwrap().to_string())).collect() };
        let page = tail(&mut phone, case, &chain, s);
        let queue = cards(&page);
        assert_eq!(queue.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["alpha-one", "beta-two", "gamma-three", "delta-four"]);
        let id = |text: &str| queue.iter().find(|c| c.1 == text).unwrap().0.clone();
        // When the phone moves the third before the first, edits the second against the fingerprint it shows, and takes an edit lease on the fourth and releases it
        let moved = phone.call(case, &chain, &["rpc:agentSession.queuedMessageMove"], "agentSession.queuedMessageMove", mutation(s, "m1", json!({"messageId": id("gamma-three"), "beforeMessageId": id("alpha-one")})));
        answered(case, "queue.move");
        assert_eq!(moved["result"]["value"]["status"], "moved", "{moved}");
        let fingerprint = orca_map_agent::body_fingerprint(s, "beta-two");
        let updated = phone.call(case, &chain, &["rpc:agentSession.queuedMessageUpdate"], "agentSession.queuedMessageUpdate", mutation(s, "u1", json!({"messageId": id("beta-two"), "expectedBodyFingerprint": fingerprint, "text": "beta-edited"})));
        answered(case, "queue.edit");
        assert_eq!(updated["result"]["value"]["status"], "updated", "{updated}");
        let held = phone.call(case, &chain, &["rpc:agentSession.queuedMessageEditHold"], "agentSession.queuedMessageEditHold", json!({"sessionId": s, "messageId": id("delta-four"), "editId": "e1", "action": "acquire", "expectedBodyFingerprint": orca_map_agent::body_fingerprint(s, "delta-four")}));
        assert_eq!(held["result"]["status"], "held", "{held}");
        let freed = phone.call(case, &chain, &["rpc:agentSession.queuedMessageEditHold"], "agentSession.queuedMessageEditHold", json!({"sessionId": s, "messageId": id("delta-four"), "editId": "e1", "action": "release"}));
        assert_eq!(freed["result"]["status"], "released", "{freed}");
        // Then rca's queue is the order and the text the phone set
        assert_eq!(cards(&tail(&mut phone, case, &chain, s)).iter().map(|c| c.1.clone()).collect::<Vec<_>>(), ["gamma-three", "alpha-one", "beta-edited", "delta-four"]);
        // When it deletes the first card of the queue that is left, Then rca no longer holds it
        let deleted = phone.call(case, &chain, &["rpc:agentSession.queuedMessageDelete"], "agentSession.queuedMessageDelete", mutation(s, "d1", json!({"messageId": id("alpha-one")})));
        answered(case, "queue.delete");
        assert_eq!(deleted["result"]["value"]["deleted"], true, "{deleted}");
        // When it cancels the running turn
        let cancelled = phone.call_settled(case, &chain, &["rpc:agentSession.cancel"], "agentSession.cancel", mutation(s, "x1", json!({})), |_| {
            rca.wait_events(s, "turn.finished", 1);
        });
        // Then rca interrupted it (the log's turn end says so), and the queue is held: no queued message started
        assert_eq!(cancelled["result"]["value"]["cancelled"], true, "{cancelled}");
        answered(case, "thread.forceStop");
        assert_eq!(rca.events(s, "turn.finished")[0]["reason"], "interrupted");
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(chain.cli.invocations("claude").len(), 1, "a message of the queue started although the stop holds it");
        assert_eq!(tail(&mut phone, case, &chain, s)["queuePause"]["reason"], "stopped");
        // When it sends one queued message now, and then resumes the queue
        let now = phone.call_settled(case, &chain, &["rpc:agentSession.queuedMessageSend"], "agentSession.queuedMessageSend", mutation(s, "n1", json!({"messageId": id("delta-four")})), |_| {
            wait_until("delta-four's turn", || chain.cli.invocations("claude").len() >= 2);
        });
        assert_eq!(now["result"]["ok"], true, "{now}");
        let resumed = phone.call_settled(case, &chain, &["rpc:agentSession.queuedMessagesResume"], "agentSession.queuedMessagesResume", mutation(s, "r1", json!({})), |_| {
            wait_until("the rest of the queue", || chain.cli.invocations("claude").len() >= 4);
        });
        assert_eq!(resumed["result"]["ok"], true, "{resumed}");
        // Then the CLI was started with the prompts in the order the phone set: the first job, the one sent now, then the queue as moved, edited and pruned
        assert_eq!(started_with(&chain, &["first-job", "alpha-one", "beta-edited", "beta-two", "gamma-three", "delta-four"]), ["first-job", "delta-four", "gamma-three", "beta-edited"], "{}", chain.evidence());
    });
}

#[test]
fn an_uploaded_image_is_read_back_byte_for_byte_and_the_message_that_names_it_reaches_the_workspace_where_rca_exec_stages_it() {
    exec_testkit::case!(SUITE, |case| {
        // Given a chat and a png of more than one 384 KiB chunk, and a CLI whose turn only answers
        let chain = Chain::start(None);
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("seen")])]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        let png: Vec<u8> = (0..500 * 1024u32).map(|i| (i * 31 % 251) as u8).collect();
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        let s = "claude_a1";
        phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", create_params(s, "c1", None));
        // When it uploads the file in chunks and commits it
        let start = phone.call(case, &chain, &["rpc:agentSessionAttachment.uploadStart"], "agentSessionAttachment.uploadStart", json!({"sessionId": s, "name": "shot.png", "byteLength": png.len()}));
        let upload = start["result"]["uploadId"].clone();
        for (n, chunk) in png.chunks(384 * 1024).enumerate() {
            let r = phone.call(case, &chain, &["rpc:agentSessionAttachment.uploadAppend"], "agentSessionAttachment.uploadAppend", json!({"uploadId": upload, "offset": n * 384 * 1024, "contentBase64": b64(chunk)}));
            assert_eq!(r["result"]["receivedBytes"], (n * 384 * 1024 + chunk.len()) as u64, "{r}");
        }
        let committed = phone.call(case, &chain, &["rpc:agentSessionAttachment.uploadCommit"], "agentSessionAttachment.uploadCommit", json!({"uploadId": upload}));
        answered(case, "upload.put");
        let path = committed["result"]["path"].as_str().expect("the commit names the path").to_string();
        // Then the preview the phone reads is the bytes it sent, through the attachment row and the chat-image row, and a window of the text row is the same bytes
        let by_attachment = phone.call(case, &chain, &["rpc:agentSessionAttachment.read"], "agentSessionAttachment.read", json!({"path": path}));
        answered(case, "attachment.get");
        let by_chat = phone.call(case, &chain, &["rpc:files.readChatImage"], "files.readChatImage", json!({"worktree": format!("id:{WS}::"), "path": path}));
        answered(case, "attachment.get");
        let decode = |r: &Value| base64::engine::general_purpose::STANDARD.decode(r["result"]["content"].as_str().unwrap_or_else(|| panic!("{r}"))).unwrap();
        assert_eq!((decode(&by_attachment), decode(&by_chat)), (png.clone(), png.clone()));
        assert_eq!(by_chat["result"]["mimeType"], "image/png");
        let window = phone.call(case, &chain, &["rpc:agentSessionAttachment.readTextChunk"], "agentSessionAttachment.readTextChunk", json!({"path": path, "offset": 400_000, "length": 4096}));
        answered(case, "attachment.get");
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(window["result"]["contentBase64"].as_str().unwrap()).unwrap(), png[400_000..404_096]);
        // And a text file is not an image, which the chat-image row refuses after it asked rca for the bytes
        let note = phone.call(case, &chain, &[], "agentSessionAttachment.uploadStart", json!({"sessionId": s, "name": "n.txt", "byteLength": 5}));
        phone.call(case, &chain, &[], "agentSessionAttachment.uploadAppend", json!({"uploadId": note["result"]["uploadId"], "offset": 0, "contentBase64": b64(b"hello")}));
        let text_path = phone.call(case, &chain, &[], "agentSessionAttachment.uploadCommit", json!({"uploadId": note["result"]["uploadId"]}))["result"]["path"].clone();
        let refused = phone.refused(case, &chain, &["rpc:files.readChatImage"], "files.readChatImage", json!({"worktree": format!("id:{WS}::"), "path": text_path}));
        assert_eq!(refused["error"]["message"], "not_an_image");
        // When an upload is started and aborted, Then it is gone: it cannot be appended to
        let spare = phone.call(case, &chain, &["rpc:agentSessionAttachment.uploadStart"], "agentSessionAttachment.uploadStart", json!({"sessionId": s, "name": "spare.bin", "byteLength": 10}));
        let aborted = phone.call(case, &chain, &["rpc:agentSessionAttachment.uploadAbort"], "agentSessionAttachment.uploadAbort", json!({"uploadId": spare["result"]["uploadId"]}));
        assert_eq!(aborted["result"], json!({"aborted": true}));
        let gone = phone.refused(case, &chain, &["rpc:agentSessionAttachment.uploadAppend"], "agentSessionAttachment.uploadAppend", json!({"uploadId": spare["result"]["uploadId"], "offset": 0, "contentBase64": b64(b"0123456789")}));
        assert_eq!(gone["error"]["message"], "Attachment upload was not found");
        // When a message names the image
        let id = path.split('/').nth(3).unwrap().to_string();
        phone.call_settled(case, &chain, &["rpc:agentSession.send"], "agentSession.send", json!({"envelope": env(s, "s1"), "body": {"blocks": [{"type": "text", "text": "what is this"}, {"type": "image-ref", "path": path}]}}), |_| {
            rca.wait_events(s, "turn.finished", 1);
        });
        // Then rca-exec staged the image in the workspace under the upload's id, byte for byte, and the CLI was started with the text
        let staged = chain.repo.path().join(format!(".rca/attachments/{id}/shot.png"));
        assert_eq!(std::fs::read(&staged).unwrap_or_else(|e| panic!("{}: {e}\n{}", staged.display(), chain.evidence())), png);
        assert!(chain.cli.invocations("claude").iter().any(|start| start.argv.iter().any(|a| a.contains("what is this"))));
    });
}

#[test]
fn a_conversation_command_reaches_the_cli_as_the_prompt_it_reads_and_runs_as_a_turn_of_the_chat() {
    exec_testkit::case!(SUITE, |case| {
        // Given a chat whose CLI answers each turn
        let chain = Chain::start(None);
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("compacted")]), Turn::replaying(&[said("cleared")])]));
        let mut phone = Phone::connect(case, &chain);
        let mut rca = chain.direct();
        let s = "claude_k1";
        phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", create_params(s, "c1", None));
        // When the phone sends /compact with instructions and then /clear, each once the turn before it ended
        let compact = phone.call_settled(case, &chain, &["rpc:agentSession.conversationCommand"], "agentSession.conversationCommand", mutation(s, "k1", json!({"command": "compact", "instructions": "keep the plan"})), |_| {
            rca.wait_events(s, "turn.finished", 1);
        });
        answered(case, "turn.start");
        let clear = phone.call_settled(case, &chain, &["rpc:agentSession.conversationCommand"], "agentSession.conversationCommand", mutation(s, "k2", json!({"command": "clear"})), |_| {
            rca.wait_events(s, "turn.finished", 2);
        });
        // Then the CLI was started with each command as its prompt, in order, and the host's answers name the command
        assert_eq!((compact["result"]["value"]["command"].clone(), clear["result"]["value"]["command"].clone()), (json!("compact"), json!("clear")), "{compact} {clear}");
        assert_eq!(started_with(&chain, &["/compact keep the plan", "/clear"]), ["/compact keep the plan", "/clear"]);
        // And a command the host does not know is refused before any turn starts
        let starts = chain.cli.invocations("claude").len();
        let unknown = phone.refused(case, &chain, &["rpc:agentSession.conversationCommand"], "agentSession.conversationCommand", mutation(s, "k3", json!({"command": "reboot"})));
        assert_eq!(unknown["error"]["code"], "invalid_argument", "{unknown}");
        asked_nothing_of(case, "turn.start");
        assert_eq!(chain.cli.invocations("claude").len(), starts);
    });
}
