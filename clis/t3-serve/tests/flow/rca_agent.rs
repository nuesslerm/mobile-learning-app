//! `RcaAgent`, the `AgentApi` over rca's agent API, against a stand-in rca that speaks the contract's own frames (the events are written by
//! `agent-api-agent`, the answers are the ones the contract crates recorded): a turn is `thread.create`, `turn.start` and `thread.subscribe`
//! followed through the thread's events, a thread's past is `thread.history`, an interrupt is `turn.interrupt`, and project files are
//! `file.list` and `file.read`.
use super::support::*;
use agent_api_agent::{FinishReason, HistoryResult, OutputStream, Role, Status, ThreadEvent, ThreadView};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use t3_core::ports::{agent_codes, AgentApi, AgentError, AgentEvent, AgentEventKind, AgentThread, AgentTurn, ClientActivity, Finish, Follow, Op};
use t3_serve::adapters::rca::{agent_event, needed_groups, RcaAgent, UNLISTED, USES};

const BEARER: &str = "flow-rca-bearer";
const WORKSPACE: &str = "w1";
/// The newest event the stand-in's thread has before the turn: the turn's own events come after it.
const BEFORE: u64 = 4;
const TURN: &str = "u9";

type Script = Box<dyn Fn(&Value) -> Vec<Value> + Send + Sync>;
type Seen<T> = Arc<Mutex<Vec<T>>>;

/// A stand-in rca: one frame list per request, the bearer of each handshake and each request it saw.
struct Rca {
    port: u16,
    bearers: Seen<String>,
    requests: Seen<Value>,
}

#[allow(clippy::result_large_err)] // tungstenite fixes the callback's error type
fn rca(script: Script) -> Rca {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (bearers, requests): (Seen<String>, Seen<Value>) = Default::default();
    let (record, asked) = (Arc::clone(&bearers), Arc::clone(&requests));
    let script: Arc<Script> = Arc::new(script);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (record, asked, script) = (Arc::clone(&record), Arc::clone(&asked), Arc::clone(&script));
            std::thread::spawn(move || {
                let callback = |req: &tungstenite::handshake::server::Request, res| {
                    let bearer = req.headers().get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                    record.lock().unwrap().push(bearer.clone());
                    if bearer == format!("Bearer {BEARER}") {
                        return Ok(res);
                    }
                    Err(tungstenite::http::Response::builder().status(401).body(None).unwrap())
                };
                let Ok(mut socket) = tungstenite::accept_hdr(stream, callback) else { return };
                while let Ok(tungstenite::Message::Text(text)) = socket.read() {
                    let request: Value = serde_json::from_str(&text).unwrap();
                    asked.lock().unwrap().push(request.clone());
                    for frame in script(&request) {
                        let _ = socket.send(tungstenite::Message::Text(frame.to_string().into()));
                    }
                }
            });
        }
    });
    Rca { port, bearers, requests }
}

impl Rca {
    fn agent(&self) -> RcaAgent {
        RcaAgent::new(&format!("http://127.0.0.1:{}", self.port), BEARER, WORKSPACE).unwrap()
    }

    /// The methods asked so far, in order.
    fn methods(&self) -> Vec<String> {
        self.requests.lock().unwrap().iter().map(|r| r["method"].as_str().unwrap().to_string()).collect()
    }

    fn asked(&self, method: &str) -> Vec<Value> {
        self.requests.lock().unwrap().iter().filter(|r| r["method"] == method).map(|r| r["params"].clone()).collect()
    }
}

fn answer(request: &Value, result: Value) -> Value {
    json!({"id": request["id"], "result": result})
}

fn refuse(request: &Value, code: &str, message: &str) -> Value {
    json!({"id": request["id"], "error": {"code": code, "message": message}})
}

fn event(turn: &str, seq: u64, event: ThreadEvent) -> Value {
    event.into_event("t1", Some(turn), seq).to_value()
}

fn view(last_seq: u64) -> ThreadView {
    ThreadView { id: "t1".into(), title: "t1".into(), workspace: Some(WORKSPACE.into()), driver: Some("claudeAgent".into()), model: None, status: Status::Idle, turn: None, last_seq, created_at: "2026-11-01T00:00:00Z".into(), updated_at: "2026-11-01T00:00:00Z".into() }
}

/// An rca whose thread `t1` has `BEFORE` events and whose turn `TURN` has emitted `events` once it is started, as the thread's log: a
/// subscription replays what the log has after the number it names.
fn turning(events: Vec<ThreadEvent>) -> Rca {
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rca(Box::new(move |request| match request["method"].as_str().unwrap() {
        "thread.create" => vec![answer(request, view(BEFORE).to_result())],
        "turn.start" => {
            started.store(true, std::sync::atomic::Ordering::SeqCst);
            vec![answer(request, json!({"thread": "t1", "turn": TURN}))]
        }
        "thread.subscribe" => {
            let after = request["params"]["after_seq"].as_u64().unwrap();
            let mut frames = vec![answer(request, json!({"topic": "thread/t1", "after_seq": after, "last_seq": BEFORE}))];
            if started.load(std::sync::atomic::Ordering::SeqCst) {
                // A late event of an earlier turn comes first: the log has it, and it is not this turn's
                let log = std::iter::once(event("u8", BEFORE + 1, ThreadEvent::MessageDelta { role: Role::Assistant, text: "from the last turn".into() })).chain(events.iter().enumerate().map(|(i, e)| event(TURN, BEFORE + 2 + i as u64, e.clone())));
                frames.extend(log.filter(|frame| frame["event"]["seq"].as_u64().unwrap() > after));
            }
            frames
        }
        "turn.interrupt" => vec![answer(request, json!({"thread": "t1", "turn": TURN}))],
        other => vec![refuse(request, "INVALID_PARAMS", &format!("{other} is not scripted"))],
    }))
}

fn thread() -> AgentThread {
    AgentThread { thread_id: "t1".into(), instance_id: "claudeAgent".into(), model: Some("m1".into()) }
}

fn turn() -> AgentTurn {
    AgentTurn { thread_id: "t1".into(), instance_id: "claudeAgent".into(), model: Some("m1".into()), prompt: "say hi".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into() , attachments: vec![]}
}

/// Every event the thread sends after `after` until the sink has seen `count` of them, or what stopped the follow.
fn followed(agent: &RcaAgent, after: u64, count: usize) -> (Result<(), AgentError>, Vec<AgentEvent>) {
    let mut seen = Vec::new();
    let result = agent.follow("t1", after, &mut |e| {
        seen.push(e);
        if seen.len() == count {
            Follow::Stop
        } else {
            Follow::Continue
        }
    });
    (result, seen)
}

#[test]
fn a_turn_is_created_started_and_followed_in_the_contracts_words_and_every_event_reaches_t3_as_what_it_shows() {
    // Given an rca whose turn thinks, runs a command that prints on both streams, changes a file, then answers
    let args = json!({"command": "ls"});
    let events = vec![
        ThreadEvent::TurnStarted {},
        ThreadEvent::MessageDelta { role: Role::User, text: "say hi".into() },
        ThreadEvent::ReasoningDelta { text: "look first".into() },
        ThreadEvent::ToolStarted { call: "c1".into(), tool: "run_command".into(), args: args.clone() },
        ThreadEvent::ToolOutput { call: "c1".into(), stream: OutputStream::Stdout, data: "a.txt\n".into() },
        ThreadEvent::ToolOutput { call: "c1".into(), stream: OutputStream::Stderr, data: "warn\n".into() },
        ThreadEvent::ToolFinished { call: "c1".into(), ok: true, result: json!({"exit_code": 0}) },
        ThreadEvent::FileChanged { path: "a.txt".into(), op: agent_api_agent::FileOp::Edit },
        ThreadEvent::Usage(Default::default()),
        ThreadEvent::MessageDelta { role: Role::Assistant, text: "Done: a.txt".into() },
        ThreadEvent::TurnFinished { reason: FinishReason::Completed },
    ];
    let count = events.len() + 1;
    let rca = turning(events);
    let agent = rca.agent();
    // When t3 makes the thread, starts the turn and follows the thread from the number the thread had
    let after = agent.ensure_thread(&thread()).unwrap();
    let started = agent.start_turn(&turn()).unwrap();
    let (result, seen) = followed(&agent, after, count);
    // Then the thread was at its newest event and the turn is the one rca started
    assert_eq!((after, started.as_str(), result), (BEFORE, TURN, Ok(())));
    // And t3 saw each event as what it shows: the other turn's text first, then the turn's own, the user's message and the unshown ones named
    let on_turn = |seq: u64, kind: AgentEventKind| AgentEvent { seq: BEFORE + seq, turn: Some(TURN.into()), kind };
    assert_eq!(seen[0], AgentEvent { seq: BEFORE + 1, turn: Some("u8".into()), kind: AgentEventKind::Text("from the last turn".into()) });
    assert_eq!(
        seen[1..],
        [
            on_turn(2, AgentEventKind::TurnStarted),
            on_turn(3, AgentEventKind::UserText("say hi".into())),
            on_turn(4, AgentEventKind::Reasoning("look first".into())),
            on_turn(5, AgentEventKind::ToolStarted { id: "c1".into(), name: "run_command".into(), input: args }),
            on_turn(6, AgentEventKind::ToolOutput { id: "c1".into(), data: "a.txt\n".into() }),
            on_turn(7, AgentEventKind::ToolOutput { id: "c1".into(), data: "warn\n".into() }),
            on_turn(8, AgentEventKind::ToolFinished { id: "c1".into(), ok: true }),
            on_turn(9, AgentEventKind::Other),
            on_turn(10, AgentEventKind::Other),
            on_turn(11, AgentEventKind::Text("Done: a.txt".into())),
            on_turn(12, AgentEventKind::TurnFinished(Finish::Completed)),
        ]
    );
    // And rca was asked to create the thread, start the turn and follow the thread from its newest event, each with the request's fields and nothing else
    assert_eq!(rca.methods(), ["thread.create", "turn.start", "thread.subscribe"]);
    assert_eq!(rca.asked("thread.create"), [json!({"thread": "t1", "workspace": WORKSPACE, "driver": "claudeAgent", "model": "m1"})]);
    assert_eq!(rca.asked("turn.start"), [json!({"thread": "t1", "workspace": WORKSPACE, "driver": "claudeAgent", "model": "m1", "prompt": "say hi", "runtime_mode": "full-access", "interaction_mode": "default"})]);
    assert_eq!(rca.asked("thread.subscribe"), [json!({"thread": "t1", "after_seq": BEFORE})]);
    // And every socket was opened with the device bearer and nothing else
    assert!(rca.bearers.lock().unwrap().iter().all(|b| b == &format!("Bearer {BEARER}")));
}

#[test]
fn the_events_of_the_contracts_two_recorded_turns_each_read_as_what_t3_shows_and_a_kind_outside_the_contract_is_an_error() {
    // Given the events the contract recorded for two turns: every one of the thirteen kinds, the second turn only a failure
    let recorded = agent_api_agent::testing::thread_events();
    // When each is read as t3 does
    let read: Vec<AgentEvent> = recorded.iter().map(|e| agent_event(e).unwrap_or_else(|e| panic!("{}", e.message()))).collect();
    // Then each keeps its number and turn, those t3 shows nothing of are `Other` and no other is
    let unshown = ["file.changed", "approval.requested", "input.requested", "plan.updated", "usage"];
    for (wire, got) in recorded.iter().zip(&read) {
        assert_eq!((wire.seq, &wire.turn), (got.seq, &got.turn));
        assert_eq!(got.kind == AgentEventKind::Other, unshown.contains(&wire.kind.as_str()), "{}", wire.kind);
    }
    // And the user's message and the assistant's are told apart, and each turn's last event says how it ended
    let texts: BTreeSet<&str> = read.iter().filter_map(|e| if let AgentEventKind::UserText(t) = &e.kind { Some(t.as_str()) } else { None }).collect();
    let user_wire: BTreeSet<&str> = recorded.iter().filter(|e| e.kind == "message.delta" && e.data["role"] == "user").filter_map(|e| e.data["text"].as_str()).collect();
    assert_eq!(texts, user_wire);
    let ends: Vec<(Option<String>, AgentEventKind)> = read.iter().filter(|e| matches!(e.kind, AgentEventKind::TurnFinished(_))).map(|e| (e.turn.clone(), e.kind.clone())).collect();
    assert_eq!(ends, [(Some("u1".into()), AgentEventKind::TurnFinished(Finish::Completed)), (Some("u2".into()), AgentEventKind::TurnFinished(Finish::Failed))]);
    // And an event of a kind the contract does not have is refused saying so
    let mut stranger = recorded[0].clone();
    stranger.kind = "item.completed".into();
    assert!(agent_event(&stranger).unwrap_err().message().contains("item.completed is not a thread event"));
    // And every kind the queue, request and lifecycle groups write on the thread's topic is read as no change of the turn, never as a failure
    for kind in agent_api_agent::queue::events::KINDS.iter().chain(&agent_api_agent::requests::events::KINDS).chain(&agent_api_agent::lifecycle::events::KINDS) {
        let mut own = recorded[0].clone();
        own.kind = (*kind).into();
        assert_eq!(agent_event(&own).unwrap().kind, AgentEventKind::Other, "{kind}");
    }
}

#[test]
fn a_turn_rca_refuses_is_rcas_own_message_a_thread_it_does_not_have_is_no_thread_and_an_interrupt_with_nothing_to_stop_is_not_an_error() {
    // Given an rca whose thread already runs a turn, one that has never heard of the thread, and ones that answer an interrupt in each way
    let busy = rca(Box::new(|request| vec![refuse(request, "TURN_RUNNING", "thread t1 already has a running turn")]));
    let unknown = rca(Box::new(|request| vec![refuse(request, "THREAD_NOT_FOUND", "no thread has the id t1")]));
    // When a turn is started on the busy one, the unknown one is followed and asked for its history
    let refused = busy.agent().start_turn(&turn()).unwrap_err();
    let (followed_unknown, history_unknown) = (unknown.agent().follow("t1", 0, &mut |_| Follow::Stop), unknown.agent().history("t1", 0, 50));
    // Then the refusal is rca's message, and the unknown thread is no thread for both
    assert_eq!(refused, AgentError::Failed("thread t1 already has a running turn".into()));
    assert_eq!((followed_unknown, history_unknown.map(drop)), (Err(AgentError::NoThread), Err(AgentError::NoThread)));
    // And only a refusal of an interrupt that means something else is an error, and rca was asked for the thread alone
    for (code, expected) in [(None, Ok(())), (Some("NO_TURN"), Ok(())), (Some("THREAD_NOT_FOUND"), Ok(())), (Some("INVALID_PARAMS"), Err(AgentError::Failed("thread is required".into())))] {
        let rca = rca(Box::new(move |request| match code {
            None => vec![answer(request, json!({"thread": "t1", "turn": TURN}))],
            Some(code) => vec![refuse(request, code, "thread is required")],
        }));
        assert_eq!(rca.agent().interrupt("t1"), expected, "{code:?}");
        assert_eq!(rca.asked("turn.interrupt"), [json!({"thread": "t1"})]);
    }
}

#[test]
fn a_threads_past_is_read_page_by_page_from_the_number_asked_for_in_the_contracts_events() {
    // Given an rca holding the events the contract recorded for thread t1, that answers history the way the contract defines it
    let recorded = agent_api_agent::testing::thread_events();
    let log = recorded.clone();
    let rca = rca(Box::new(move |request| {
        let (after, limit) = (request["params"]["after_seq"].as_u64().unwrap(), request["params"]["limit"].as_u64().unwrap() as usize);
        let rest: Vec<_> = log.iter().filter(|e| e.seq > after).cloned().collect();
        let page = HistoryResult::new(&rest[..rest.len().min(limit)], log.len() as u64, rest.len() > limit);
        vec![answer(request, serde_json::to_value(page).unwrap())]
    }));
    // When t3 reads the thread after number 15 with a page of three, then after the page's last number
    let first = rca.agent().history("t1", 15, 3).unwrap();
    let second = rca.agent().history("t1", first.events.last().unwrap().seq, 3).unwrap();
    // Then the first page is the three events after 15 with more to come, and the second the last two, each as t3 reads it
    let seqs = |h: &t3_core::ports::History| h.events.iter().map(|e| e.seq).collect::<Vec<_>>();
    assert_eq!((seqs(&first), first.more, first.last_seq), ([16, 17, 18].to_vec(), true, recorded.len() as u64));
    assert_eq!((seqs(&second), second.more), ([19, 20].to_vec(), false));
    assert_eq!(first.events[0], agent_event(&recorded[15]).unwrap());
    // And rca was asked for each page by thread, number and size, and nothing else
    assert_eq!(rca.asked("thread.history"), [json!({"thread": "t1", "after_seq": 15, "limit": 3}), json!({"thread": "t1", "after_seq": 18, "limit": 3})]);
}

#[test]
fn the_groups_a_mapper_needs_are_those_of_the_methods_it_really_sends() {
    // Given an rca that answers every method the mapper might send, each as the contract recorded or defines it
    let status = agent_api_service::testing::fixtures().iter().find(|f| f.method == agent_api_service::STATUS.name()).and_then(|f| f.exchanges.iter().find(|e| e.succeeds())).unwrap().response["result"].clone();
    let entries = json!({"entries": []});
    let rca = rca(Box::new(move |request| match request["method"].as_str().unwrap() {
        "service.status" => vec![answer(request, status.clone())],
        "thread.create" => vec![answer(request, view(BEFORE).to_result())],
        "turn.start" => vec![answer(request, json!({"thread": "t1", "turn": TURN})), event(TURN, BEFORE + 1, ThreadEvent::TurnFinished { reason: FinishReason::Completed })],
        "thread.subscribe" => vec![answer(request, json!({"topic": "thread/t1", "after_seq": BEFORE, "last_seq": BEFORE})), event(TURN, BEFORE + 1, ThreadEvent::TurnFinished { reason: FinishReason::Completed })],
        "thread.history" => vec![answer(request, json!({"events": [], "last_seq": BEFORE, "more": false}))],
        "turn.interrupt" => vec![answer(request, json!({"thread": "t1", "turn": TURN}))],
        "file.list" => vec![answer(request, entries.clone())],
        "file.read" => vec![answer(request, json!({"content": "x", "total_lines": 1, "truncated": false}))],
        "terminal.list" => vec![answer(request, json!({"workspace": "w1", "terminals": []}))],
        "terminal.attach" => vec![answer(request, json!({"workspace": "w1", "terminal": "t1.a", "after_offset": null, "start_offset": 0, "offset": 0, "reset": false, "cols": 80, "rows": 24, "status": "running", "exit_code": null}))],
        "git.subscribe" => vec![answer(request, json!({"workspace": "w1", "seq": 0})), agent_api_git::status_event(1, "w1", &json!({"branch": "main", "files": []})).to_value()],
        "clone.subscribe" => vec![answer(request, json!({"workspace": "w1", "seq": 0})), agent_api_hosted::clone_event(1, "w1", "c1", "running", "octo/hello", json!({})).to_value()],
        "schedule.subscribe" => vec![answer(request, json!({"topic": "schedules", "tasks": [], "seq": 0, "reset": false})), agent_api_schedules::watch::webhook_event(1, "s1", "rejected_token", 5).to_value()],
        "preview.subscribe" => vec![answer(request, json!({"topic": "preview", "epoch": "e1", "seq": 0, "reset": false})), agent_api_preview::watch::closed_event(1, "e1", "t1", "tab-1", 5).to_value()],
        "provider.auth.subscribe" => vec![answer(request, agent_api_accounts::subscribe_result("auth-1", 1)), agent_api_accounts::auth_event(1, &ended_login()).to_value()],
        other if USES.iter().any(|m| m.name() == other) => vec![answer(request, json!({}))],
        other => vec![refuse(request, "INVALID_PARAMS", &format!("{other} is not scripted"))],
    }));
    let agent = rca.agent();
    // When every operation of the port is run once, and the start check
    agent.check_agent_api().unwrap();
    agent.ensure_thread(&thread()).unwrap();
    agent.start_turn(&turn()).unwrap();
    agent.follow("t1", BEFORE, &mut |_| Follow::Stop).unwrap();
    agent.history("t1", 0, 10).unwrap();
    agent.interrupt("t1").unwrap();
    agent.list_dir("", "").unwrap();
    agent.read_file("", "a").unwrap();
    agent.setup_events("/work/proj", 0, Duration::from_millis(50)).unwrap();
    agent.terminal_events("", "t1.a", None, Duration::from_millis(50)).unwrap();
    agent.terminals().unwrap();
    agent.git_follow("/work/proj", &mut |status| {
        assert_eq!(status.map(|s| s["branch"].clone()), Some(json!("main")));
        Follow::Stop
    })
    .unwrap();
    agent.auth_follow("auth-1", &mut |_| Follow::Stop).unwrap();
    agent.preview_follow("e1", 0, &mut |event| {
        assert_eq!(event.map(|e| e["type"].clone()), Some(json!("preview.closed")));
        Follow::Stop
    })
    .unwrap();
    agent.schedule_follow(&mut |event| {
        assert_eq!(event.map(|e| (e["type"].clone(), e["outcome"].clone(), e["seq"].clone())), Some((json!("schedule.webhook"), json!("rejected_token"), json!(1))));
        Follow::Stop
    })
    .unwrap();
    agent.clone_follow("/work/proj", &mut |clone| {
        assert_eq!(clone.map(|c| (c["clone"].clone(), c["state"].clone(), c["seq"].clone())), Some((json!("c1"), json!("running"), json!(1))));
        Follow::Stop
    })
    .unwrap();
    for op in every_op() {
        agent.run(&op).unwrap_or_else(|e| panic!("{}: {}", op.method(), e.message()));
    }
    // Then the methods rca was asked are exactly the ones the mapper declares it uses, so a call added without the declaration fails here
    let sent: BTreeSet<String> = rca.methods().into_iter().collect();
    let declared: BTreeSet<String> = USES.iter().map(|m| m.name().to_string()).collect();
    assert_eq!(sent, declared);
    // And the groups it needs at startup are those of those methods and no others, each once
    let groups: Vec<&str> = needed_groups().iter().map(|g| g.as_str()).collect();
    let expected: BTreeSet<&str> = USES.iter().map(|m| m.group()).filter(|g| !UNLISTED.contains(g)).map(|g| g.as_str()).collect();
    assert!(USES.iter().any(|m| UNLISTED.contains(&m.group())), "an unlisted group is one the mapper does call");
    assert_eq!((groups.len(), groups.iter().copied().collect::<BTreeSet<_>>()), (expected.len(), expected));
}

#[test]
fn a_directory_a_repository_and_a_workspace_are_asked_in_the_contracts_words_and_the_refusals_t3_tells_apart_keep_their_codes() {
    // Given an rca that makes a directory once, refuses its name the second time, and knows a workspace once
    let made = Arc::new(Mutex::new(false));
    let seen = made.clone();
    let rca = rca(Box::new(move |request| match request["method"].as_str().unwrap() {
        "file.mkdir" if std::mem::replace(&mut *seen.lock().unwrap(), true) => vec![refuse(request, "ALREADY_EXISTS", "taken")],
        "workspace.create" => vec![refuse(request, "WORKSPACE_EXISTS", "workspace project-p1 exists")],
        "workspace.remove" => vec![refuse(request, "UNKNOWN_WORKSPACE", "no workspace project-p1")],
        "file.mkdir" | "git.init" => vec![answer(request, json!({"path": "x", "created": true}))],
        other => vec![refuse(request, "INVALID_PARAMS", &format!("{other} is not scripted"))],
    }));
    let agent = rca.agent();
    // When a directory is claimed twice, a repository is made with a first file, and a workspace is made and forgotten
    agent.run(&Op::FileMkdir { path: ".rca/projects/demo".into(), parents: false }).unwrap();
    let taken = agent.run(&Op::FileMkdir { path: ".rca/projects/demo".into(), parents: true }).unwrap_err();
    agent.run(&Op::GitInit { path: ".rca/projects/demo".into(), initial_branch: None, files: vec![("README.md".into(), "# demo\n".into())], message: Some("Initial commit".into()) }).unwrap();
    let exists = agent.run(&Op::WorkspaceCreate { workspace: "project-p1".into(), path: "demo".into() }).unwrap_err();
    let unknown = agent.run(&Op::WorkspaceRemove { workspace: "project-p1".into() }).unwrap_err();
    // Then rca was asked in the contract's words, in the mapper's workspace, with what the request leaves out left out
    assert_eq!(rca.asked("file.mkdir"), [json!({"workspace": WORKSPACE, "path": ".rca/projects/demo", "parents": false}), json!({"workspace": WORKSPACE, "path": ".rca/projects/demo", "parents": true})]);
    assert_eq!(rca.asked("git.init"), [json!({"workspace": WORKSPACE, "path": ".rca/projects/demo", "files": [{"path": "README.md", "content": "# demo\n"}], "message": "Initial commit"})]);
    assert_eq!((rca.asked("workspace.create"), rca.asked("workspace.remove")), ([json!({"workspace": "project-p1", "path": "demo"})].to_vec(), [json!({"workspace": "project-p1"})].to_vec()));
    // And each refusal keeps the code the mapper acts on, and those codes are the contract's: the ones its recordings carry
    let codes: BTreeSet<String> = agent_api_workspace::testing::group_fixtures("files-edit").iter().chain(&agent_api_workspace::testing::group_fixtures("workspace")).flat_map(|f| f.exchanges.iter()).filter_map(|e| e.response["error"]["code"].as_str().map(str::to_string)).collect();
    for (error, code) in [(taken, agent_codes::ALREADY_EXISTS), (exists, agent_codes::WORKSPACE_EXISTS), (unknown, agent_codes::UNKNOWN_WORKSPACE)] {
        assert!(matches!(&error, AgentError::Refused { code: got, .. } if got == code), "{error:?}");
        assert!(codes.contains(code), "{code} is in no recorded exchange of the workspace groups");
    }
    assert!(codes.contains(agent_codes::NOT_FOUND) && codes.contains(agent_codes::PATH_OUTSIDE_ROOT), "{codes:?}");
}

/// One of every request the port makes, over the shapes of the agent API's contract.
fn every_op() -> Vec<Op> {
    let t = || "t1".to_string();
    let hosted = agent_api_hosted::all_methods().into_iter().filter(|m| USES.contains(m)).map(|m| Op::Hosted { method: m.name(), root: "/work/proj".into(), params: json!({"repo": "octo/hello"}) });
    let mut ops: Vec<Op> = vec![
        Op::ApprovalAnswer { thread: t(), approval: "a1".into(), decision: "accept_for_session".into() },
        Op::InputAnswer { thread: t(), request: "r1".into(), answer: "yes".into() },
        Op::InputDismiss { thread: t(), request: "r1".into() },
        Op::QueueAdd { thread: t(), item: "q1".into(), prompt: "later".into() },
        Op::QueueEdit { thread: t(), item: "q1".into(), prompt: "sooner".into() },
        Op::QueueMove { thread: t(), item: "q1".into(), before: Some("q2".into()) },
        Op::QueueDelete { thread: t(), item: "q1".into() },
        Op::QueueResume { thread: t(), item: None },
        Op::TurnSteer { thread: t(), turn: Some(TURN.into()), item: Some("q1".into()) },
        Op::ThreadArchive { thread: t(), archived: true },
        Op::ThreadDelete { thread: t(), only_if_unused: true },
        Op::ThreadFork { thread: t(), fork: "f1".into(), turn: None, title: None },
        Op::ThreadSettle { thread: t(), state: "snoozed", until_ms: Some(2), at_ms: None },
        Op::ThreadSearch { query: "needle".into(), limit: 5 },
        Op::ThreadFamily { thread: t() },
        Op::ThreadForceStop { thread: t(), turn: None },
        Op::ItemGet { thread: t(), seq: 3 },
        Op::WorkflowStop { thread: t(), turn: TURN.into(), workflow: None },
        Op::ThreadMergeBack { thread: t(), into: "p1".into(), turn: None },
        Op::ThreadRollback { thread: t(), turn: TURN.into(), restore_files: false },
        Op::ThreadDetach { thread: t(), session: "s1".into() },
        Op::TurnRetry { thread: t(), turn: TURN.into() },
        Op::ThreadShelf { shelf: "settled", before: Some((5, Some("x".into()))), limit: 10 },
        Op::StateGet { scope: "t3".into(), key: "k".into() },
        Op::StateSet { scope: "t3".into(), key: "k".into(), value: json!(1) },
        Op::FileStat { root: "/work/proj".into(), path: "a.txt".into() },
        Op::FileWrite { root: "/work/proj".into(), path: "a.txt".into(), content: "new\n".into() },
        Op::FileBrowse { root: String::new(), path: "/shared".into(), show_hidden: false },
        Op::FileMkdir { path: ".rca/projects/demo".into(), parents: false },
        Op::GitInit { path: ".rca/projects/demo".into(), initial_branch: Some("main".into()), files: vec![("README.md".into(), "# demo\n".into())], message: Some("Initial commit".into()) },
        Op::WorkspaceCreate { workspace: "project-p1".into(), path: ".rca/projects/demo".into() },
        Op::WorkspaceRemove { workspace: "project-p1".into() },
        Op::FileSearch { root: "/work/proj".into(), query: "main".into(), limit: 5 },
        Op::TurnDiff { root: "/work/proj".into(), base: "HEAD".into() },
        Op::ThreadDiff { root: "/work/proj".into() },
        Op::WorktreeCreate { root: "/work/proj".into(), worktree: "wt".into(), branch: "feature/x".into(), base: Some("main".into()) },
        Op::WorktreeSetupCancel { root: "/work/proj".into() },
        Op::TerminalOpen { root: "/work/proj".into(), terminal: "t1.a".into(), cwd: "/work/proj".into(), cols: Some(100), rows: Some(30), env: vec![("TERM".into(), "xterm".into())] },
        Op::TerminalWrite { root: "/work/proj".into(), terminal: "t1.a".into(), data: "ls\n".into() },
        Op::TerminalResize { root: "/work/proj".into(), terminal: "t1.a".into(), cols: 120, rows: 40 },
        Op::TerminalClear { root: "/work/proj".into(), terminal: "t1.a".into() },
        Op::TerminalClose { root: "/work/proj".into(), terminal: "t1.a".into() },
        Op::GitStatus { root: "/work/proj".into() },
        Op::GitUpstream { root: "/work/proj".into() },
        Op::GitDiff { root: "/work/proj".into(), base: Some("HEAD".into()), head: None, path: Some("a.txt".into()) },
        Op::GitRefs { root: "/work/proj".into(), kind: Some("branch"), limit: 50 },
        Op::GitHistory { root: "/work/proj".into(), rev: Some("abc".into()), limit: 1 },
        Op::GitCheckout { root: "/work/proj".into(), target: "origin/x".into(), track: true },
        Op::GitCreateRef { root: "/work/proj".into(), name: "feature/y".into(), from: None, checkout: true },
        Op::GitCreateRef { root: "/work/proj".into(), name: "t3code/pr-12/x".into(), from: Some("abc1234".into()), checkout: false },
        Op::GitFetch { root: "/work/proj".into(), refspecs: vec!["+refs/pull/12/head:refs/rca/pr/12/head".into()] },
        Op::GitCompare { root: "/work/proj".into(), base: "main".into(), head: "feat".into(), limit: 100 },
        Op::WorktreeList { root: "/work/proj".into() },
        Op::GitPull { root: "/work/proj".into() },
        Op::GitBlob { root: "/work/proj".into(), rev: "HEAD".into(), path: "a.txt".into() },
        Op::GitStage { root: "/work/proj".into(), paths: vec![".".into()] },
        Op::GitCommit { root: "/work/proj".into(), message: "m".into() },
        Op::GitPush { root: "/work/proj".into(), set_upstream: true },
        Op::AccountsList,
        Op::AccountsRefresh { provider: Some("codex".into()) },
        Op::AccountsSelect { provider: "codex".into() },
        Op::AuthStart { provider: "codex".into(), method: Some("device_code".into()) },
        Op::AuthRespond { auth: "auth-1".into(), text: "code".into() },
        Op::AuthComplete { auth: "auth-1".into(), timeout_ms: 1000 },
        Op::AuthCancel { auth: "auth-1".into() },
        Op::AccountsLogout { provider: "codex".into() },
        Op::UsageSummary,
        Op::ResetCredit { provider: "codex".into() },
        Op::ClientActivity(ClientActivity { client: "phone-1".into(), kind: "mobile".into(), visible: true, focused: true, recently_interacted: false, app_state: None, low_power_mode: None, scopes: vec!["server-config".into()], ttl_ms: None, observed_at_ms: 5 }),
    ];
    ops.extend(hosted);
    // every method of the schedules group the mapper uses: the save in a project's workspace, the rest on a schedule by id
    let by_id = agent_api_schedules::METHODS.iter().filter(|m| **m != agent_api_schedules::UPSERT && USES.contains(m)).map(|m| Op::Schedule { method: m.name(), root: None, params: json!({"id": "s1", "enabled": true}) });
    ops.extend(agent_api_attachments::METHODS.iter().filter(|m| USES.contains(m)).map(|m| Op::Attachment { method: m.name(), params: json!({"id": "att-1"}) }));
    ops.extend(agent_api_preview::sessions::METHODS.iter().chain(agent_api_preview::watch::METHODS).filter(|m| USES.contains(m)).map(|m| Op::Preview { method: m.name(), params: json!({"thread": "t1", "tab": "tab-1"}) }));
    ops.push(Op::Schedule { method: agent_api_schedules::UPSERT.name(), root: Some("/work/proj".into()), params: json!({"title": "t", "prompt": "p", "enabled": true, "schedule": {"type": "webhook"}, "driver": "claudeAgent"}) });
    ops.extend(by_id);
    ops
}

fn ended_login() -> agent_api_accounts::AuthView {
    agent_api_accounts::AuthView { auth: "auth-1".into(), provider: "codex".into(), method: "device_code".into(), status: agent_api_accounts::AuthStatus::Succeeded, url: None, code: None, error: None }
}

#[test]
fn a_login_is_followed_from_its_first_state_to_the_one_that_ends_it_and_a_refusal_keeps_the_code_the_service_gave() {
    // Given an rca whose login auth-1 shows its prompt, then succeeds, and a login it does not know
    let pending = agent_api_accounts::AuthView { status: agent_api_accounts::AuthStatus::Pending, url: Some("https://login.example.test/device".into()), code: Some("ABCD-1234".into()), ..ended_login() };
    let (first, last) = (pending.clone(), ended_login());
    let rca = rca(Box::new(move |request| {
        let params = &request["params"];
        match (request["method"].as_str().unwrap(), params["auth"].as_str()) {
            ("provider.auth.subscribe", Some("auth-1")) => vec![answer(request, agent_api_accounts::subscribe_result("auth-1", 2)), agent_api_accounts::auth_event(1, &first).to_value(), agent_api_accounts::auth_event(2, &last).to_value()],
            ("provider.auth.subscribe", _) => vec![refuse(request, "UNKNOWN_AUTH", "no such login")],
            _ => vec![refuse(request, "INVALID_PARAMS", "not scripted")],
        }
    }));
    let agent = rca.agent();
    // When the login is followed to its end, and one that does not exist is followed
    let mut seen: Vec<Value> = Vec::new();
    agent.auth_follow("auth-1", &mut |view| {
        seen.push(view);
        Follow::Continue
    })
    .unwrap();
    let unknown = agent.auth_follow("auth-9", &mut |_| Follow::Continue).unwrap_err();
    // Then it gave the prompt and then the end, and stopped by itself after the end; the unknown one is refused with the service's code
    assert_eq!(seen.iter().map(|v| v["status"].clone()).collect::<Vec<_>>(), [json!("pending"), json!("succeeded")]);
    assert_eq!(seen[0]["code"], json!("ABCD-1234"));
    assert!(matches!(unknown, AgentError::Refused { ref code, .. } if code == "UNKNOWN_AUTH"), "{unknown:?}");
}

fn recorded(method: &str, name: &str) -> (Value, Value) {
    let fixtures = agent_api_workspace::testing::fixtures();
    let exchange = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == method && e.name.contains(name)).unwrap_or_else(|| panic!("no recorded {method} exchange named like {name}"));
    (exchange.request["params"].clone(), exchange.response.clone())
}

#[test]
fn project_files_are_listed_dirs_first_and_read_through_the_workspace_and_what_the_executor_refuses_comes_back_as_it_said_it() {
    // Given the exchanges the contract recorded for file.list and file.read, replayed by an rca for the paths they name
    let (list_params, list_answer) = recorded("file.list", "lists a directory");
    let (_, read_answer) = recorded("file.read", "reads a window");
    let (_, missing) = recorded("file.read", "relays the executor's refusal");
    let rca = rca(Box::new(move |request| {
        let frame = match (request["method"].as_str().unwrap(), request["params"]["path"].as_str().unwrap()) {
            ("file.list", path) if path == list_params["path"] => list_answer.clone(),
            ("file.list", "mixed") => json!({"id": 0, "result": {"entries": [{"name": "z.txt", "kind": "file", "size": 1}, {"name": "b", "kind": "dir", "size": 0}, {"name": "a.txt", "kind": "file", "size": 2}, {"name": "a", "kind": "dir", "size": 0}]}}),
            ("file.read", "missing.txt") => missing.clone(),
            ("file.read", "big.txt") => json!({"id": 0, "result": {"content": "head", "total_lines": 9000, "truncated": true}}),
            ("file.read", _) => read_answer.clone(),
            (method, path) => json!({"id": 0, "error": {"code": "INVALID_PARAMS", "message": format!("{method} {path} is not scripted")}}),
        };
        let mut frame = frame;
        frame["id"] = request["id"].clone();
        vec![frame]
    }));
    let agent = rca.agent();
    // When a directory, a mixed one, a file, a file the executor refuses and a file longer than a read are asked for
    let listed = agent.list_dir("", "src").unwrap();
    let mixed: Vec<(String, bool)> = agent.list_dir("", "mixed").unwrap().into_iter().map(|e| (e.name, e.directory)).collect();
    let read = agent.read_file("", "notes.txt").unwrap();
    let refused = agent.read_file("", "missing.txt").unwrap_err().message();
    let long = agent.read_file("", "big.txt").unwrap_err().message();
    // Then the entries are the recorded ones, directories come first by name, the file is the recorded content, and the refusal is the executor's own message
    assert_eq!(listed.iter().map(|e| (e.name.as_str(), e.directory, e.size)).collect::<Vec<_>>(), [("a.txt", false, 8), ("b.rs", false, 13)]);
    assert_eq!(mixed, [("a".to_string(), true), ("b".to_string(), true), ("a.txt".to_string(), false), ("z.txt".to_string(), false)]);
    assert_eq!(read, recorded("file.read", "reads a window").1["result"]["content"].as_str().unwrap());
    assert_eq!(refused, recorded("file.read", "relays the executor's refusal").1["error"]["message"].as_str().unwrap());
    assert!(long.contains("9000 lines"), "{long}");
}

#[test]
fn a_project_path_is_asked_for_under_its_root_in_the_executor_and_a_root_of_nothing_is_the_executors_own() {
    // Given an rca that lists whatever it is asked for as empty
    let rca = rca(Box::new(|request| vec![answer(request, json!({"entries": []}))]));
    let agent = rca.agent();
    // When directories are listed under a project root, with and without slashes, and under no root
    for (root, path) in [("/work/proj", ""), ("/work/proj/", "/src"), ("/work/proj", "src/a"), ("", "/"), ("", "/etc")] {
        agent.list_dir(root, path).unwrap();
    }
    // Then each asks the workspace for the path under the root, and no root asks for the executor's own root or the path under it
    let paths: Vec<Value> = rca.asked("file.list").into_iter().map(|p| p["path"].clone()).collect();
    assert_eq!(paths, ["/work/proj", "/work/proj/src", "/work/proj/src/a", ".", "etc"]);
    assert!(rca.asked("file.list").iter().all(|p| p["workspace"] == WORKSPACE));
}

#[test]
fn a_bearer_rca_refuses_ends_every_call_with_unauthorized_and_an_rca_that_is_down_with_a_connection_error() {
    // Given an rca that accepts one bearer, an agent holding another, and a port nothing listens on
    let rca = rca(Box::new(|request| vec![answer(request, json!({"entries": []}))]));
    let wrong = RcaAgent::new(&format!("http://127.0.0.1:{}", rca.port), "not-the-bearer", WORKSPACE).unwrap();
    let down = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        RcaAgent::new(&format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port()), BEARER, WORKSPACE).unwrap()
    };
    // When each is asked for a thread, a turn, an interrupt, a follow, a history and a listing
    let results = [
        wrong.ensure_thread(&thread()).unwrap_err().message(),
        wrong.start_turn(&turn()).unwrap_err().message(),
        wrong.interrupt("t1").unwrap_err().message(),
        wrong.follow("t1", 0, &mut |_| Follow::Stop).unwrap_err().message(),
        wrong.history("t1", 0, 10).map(drop).unwrap_err().message(),
        wrong.list_dir("", "").unwrap_err().message(),
    ];
    let unreachable = down.list_dir("", "").unwrap_err().message();
    // Then the refused ones say UNAUTHORIZED and never got a request through, and the one with nothing listening says it could not connect
    assert!(results.iter().all(|r| r.contains("UNAUTHORIZED")), "{results:?}");
    assert!(rca.methods().is_empty());
    assert!(unreachable.contains("CONNECT"), "{unreachable}");
}

#[test]
fn a_client_activity_is_sent_as_the_params_the_contract_recorded_and_one_the_contract_refuses_is_never_sent() {
    // Given the exchanges the contract recorded for service.clientActivity, replayed by an rca
    let fixtures = agent_api_service::testing::fixtures();
    let exchanges: Vec<_> = fixtures.iter().filter(|f| f.method == "service.clientActivity").flat_map(|f| f.exchanges.clone()).collect();
    let kept = exchanges.iter().find(|e| e.succeeds()).unwrap().clone();
    let recorded = kept.request["params"].clone();
    let rca = rca(Box::new(move |request| vec![answer(request, kept.response["result"].clone())]));
    // When t3 reports the activity the recorded request describes, and one with an empty client
    let list = |key: &str| recorded[key].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>();
    let report = |client: &str| Op::ClientActivity(ClientActivity {
        client: client.into(),
        kind: recorded["kind"].as_str().unwrap().into(),
        visible: recorded["visible"].as_bool().unwrap(),
        focused: recorded["focused"].as_bool().unwrap(),
        recently_interacted: recorded["recently_interacted"].as_bool().unwrap(),
        app_state: None,
        low_power_mode: None,
        scopes: list("scopes"),
        ttl_ms: recorded["ttl_ms"].as_u64(),
        observed_at_ms: recorded["observed_at_ms"].as_u64().unwrap(),
    });
    let done = rca.agent().run(&report(recorded["client"].as_str().unwrap())).unwrap();
    let said = rca.agent().run(&report("")).unwrap_err().message();
    // Then rca was asked once with exactly the recorded params, the answer is the recorded result, and the one with no client never left t3
    assert_eq!(rca.asked("service.clientActivity")[0], recorded);
    assert_eq!(done, exchanges.iter().find(|e| e.succeeds()).unwrap().response["result"]);
    assert_eq!(rca.asked("service.clientActivity").len(), 1);
    assert!(said.contains("client is a name of 1 to 128"), "{said}");
}

/// The worktrees `worktree.list` recorded: the repository's own tree first, then a worktree.
fn recorded_worktrees() -> Value {
    let fixtures = agent_api_workspace::testing::group_fixtures("worktrees");
    let exchange = fixtures.iter().flat_map(|f| &f.exchanges).find(|e| e.request["method"] == "worktree.list" && e.succeeds()).expect("a recorded worktree.list");
    exchange.response["result"].clone()
}

#[test]
fn a_directory_in_a_worktree_is_asked_for_in_that_worktrees_workspace_and_any_other_is_under_the_configured_one() {
    // Given an rca that lists the worktrees the contract recorded and answers every file request with a recorded-shape result
    let worktrees = recorded_worktrees();
    let paths: Vec<String> = worktrees["worktrees"].as_array().unwrap().iter().map(|w| w["path"].as_str().unwrap().to_string()).collect();
    let (main, linked) = (paths[0].clone(), paths[1].clone());
    let name = linked.rsplit('/').next().unwrap().to_string();
    let rca = rca(Box::new(move |request| match request["method"].as_str().unwrap() {
        "worktree.list" => vec![answer(request, worktrees.clone())],
        "file.stat" => vec![answer(request, json!({"kind": "file", "size": 1, "modified_ms": 0}))],
        "file.write" => vec![answer(request, json!({"bytes": 1}))],
        "file.search" => vec![answer(request, json!({"entries": [], "truncated": false}))],
        "turn.diff" | "thread.diff" => vec![answer(request, json!({"diff": ""}))],
        "worktree.create" => vec![answer(request, json!({}))],
        _ => vec![answer(request, json!({"entries": []}))],
    }));
    let agent = rca.agent();
    // When files are listed, written and searched, and diffs taken, in the worktree, below it, in the repository's own tree and elsewhere
    agent.list_dir(&linked, "src").unwrap();
    agent.list_dir(&format!("{linked}/sub"), "").unwrap();
    agent.list_dir(&main, "src").unwrap();
    agent.list_dir("/elsewhere", "src").unwrap();
    agent.run(&Op::FileWrite { root: linked.clone(), path: "a.txt".into(), content: "x".into() }).unwrap();
    agent.run(&Op::FileSearch { root: linked.clone(), query: "main".into(), limit: 5 }).unwrap();
    agent.run(&Op::FileSearch { root: "/elsewhere".into(), query: "main".into(), limit: 5 }).unwrap();
    agent.run(&Op::ThreadDiff { root: linked.clone() }).unwrap();
    agent.run(&Op::TurnDiff { root: format!("{linked}/sub"), base: "HEAD".into() }).unwrap();
    // Then a path in the worktree is asked for by the worktree's workspace (named by its directory) and relative to it, any other under the
    // configured workspace as the project's own directory, and a write makes its parents
    let at = |method: &str| -> Vec<(String, Option<String>)> { rca.asked(method).iter().map(|p| (p["workspace"].as_str().unwrap().to_string(), p["path"].as_str().map(str::to_string))).collect() };
    let own = |path: &str| (WORKSPACE.to_string(), Some(path.to_string()));
    assert_eq!(at("file.list"), [(name.clone(), Some("src".into())), (name.clone(), Some("sub".into())), own(&format!("{main}/src")), own("/elsewhere/src")]);
    assert_eq!(rca.asked("file.write"), [json!({"workspace": name, "path": "a.txt", "content": "x", "create_dirs": true})]);
    assert_eq!(at("file.search"), [(name.clone(), None), own("/elsewhere")]);
    assert_eq!(at("thread.diff"), [(name.clone(), None)]);
    assert_eq!(at("turn.diff"), [(name.clone(), Some("sub".into()))]);
    // And the worktrees were asked of the configured workspace, once for each request that has a directory
    assert!(rca.asked("worktree.list").iter().all(|p| p == &json!({"workspace": WORKSPACE})));
}

#[test]
fn the_setup_events_of_a_worktree_are_those_rca_replays_after_its_answer_and_a_worktree_without_a_setup_has_none() {
    // Given an rca that, asked to follow the setup of the worktree the contract recorded, answers and then sends its events as the contract maps them
    let linked = recorded_worktrees()["worktrees"][1]["path"].as_str().unwrap().to_string();
    let name = linked.rsplit('/').next().unwrap().to_string();
    let executor_events = [exec_git::SetupEvent::Started { steps: 1 }, exec_git::SetupEvent::StepStarted { step: 0, command: "pnpm i".into() }, exec_git::SetupEvent::StepOutput { step: 0, stream: "stdout".into(), data: "ok\n".into() }, exec_git::SetupEvent::StepFinished { step: 0, exit_code: 0 }, exec_git::SetupEvent::Finished { ok: true }];
    let (worktrees, workspace_name) = (recorded_worktrees(), name.clone());
    let wire: Vec<Value> = executor_events.iter().enumerate().map(|(i, e)| agent_api_workspace::worktrees::setup_event(&name, i as u64 + 1, &serde_json::to_value(e).unwrap()).to_value()).collect();
    let rca = rca(Box::new(move |request| match request["method"].as_str().unwrap() {
        "worktree.list" => vec![answer(request, worktrees.clone())],
        "worktree.setup.subscribe" if request["params"]["workspace"] == workspace_name.as_str() => {
            let after = request["params"]["after_seq"].as_u64().unwrap();
            let mut frames = vec![answer(request, json!({"workspace": workspace_name, "after_seq": after, "last_seq": 5, "running": false}))];
            frames.extend(wire.iter().filter(|e| e["event"]["seq"].as_u64().unwrap() > after).cloned());
            frames
        }
        "worktree.setup.subscribe" => vec![refuse(request, "NO_SETUP", "no setup ran in worktree")],
        other => vec![refuse(request, "INVALID_PARAMS", &format!("{other} is not scripted"))],
    }));
    let agent = rca.agent();
    // When the setup is read from the start, from the third event, and for a directory that is no worktree
    let all = agent.setup_events(&linked, 0, Duration::from_millis(500)).unwrap();
    let rest = agent.setup_events(&linked, 3, Duration::from_millis(500)).unwrap();
    let none = agent.setup_events("/elsewhere", 0, Duration::from_millis(500)).unwrap();
    // Then every event arrives in order with its contract kind and fields, the later read has only the later ones, and no setup is none ran
    assert_eq!((all.ran, all.events.iter().map(|e| (e.seq, e.kind.as_str())).collect::<Vec<_>>()), (true, vec![(1, "setup.started"), (2, "step.started"), (3, "step.output"), (4, "step.finished"), (5, "setup.finished")]));
    assert_eq!((all.events[1].data["command"].clone(), all.events[4].data["ok"].clone()), (json!("pnpm i"), json!(true)));
    assert_eq!(rest.events.iter().map(|e| e.seq).collect::<Vec<_>>(), [4, 5]);
    assert_eq!((none.ran, none.events.len()), (false, 0));
    // And rca was asked to follow the setup of the worktree's workspace from the number given
    assert_eq!(rca.asked("worktree.setup.subscribe")[..2], [json!({"workspace": name, "after_seq": 0}), json!({"workspace": name, "after_seq": 3})]);
}

#[test]
fn a_git_request_for_a_directory_of_a_worktree_is_asked_of_that_worktrees_workspace_with_the_params_the_contract_names_and_a_blob_comes_back_as_text() {
    // Given an rca with a worktree workspace wt at /base/wt, whose blob is the bytes of "hi\n"
    let rca = rca(Box::new(|request| match request["method"].as_str().unwrap() {
        "worktree.list" => vec![answer(request, json!({"worktrees": [{"path": "/work/proj", "is_main": true}, {"path": "/base/wt", "is_main": false}]}))],
        "git.blob" => vec![answer(request, json!({"rev": "HEAD", "path": "a.txt", "size": 3, "data": "aGkK", "truncated": false, "binary": false}))],
        "git.pull" => vec![refuse(request, "INVALID_ARGS", "this repository has no remote configured")],
        _ => vec![answer(request, json!({}))],
    }));
    let agent = rca.agent();
    // When a status is asked for a directory below the worktree and for the project's own, a blob is read in the worktree, and a pull is refused
    agent.run(&Op::GitStatus { root: "/base/wt/sub".into() }).unwrap();
    agent.run(&Op::GitStatus { root: "/work/proj".into() }).unwrap();
    let blob = agent.run(&Op::GitBlob { root: "/base/wt".into(), rev: "HEAD".into(), path: "a.txt".into() }).unwrap();
    let pull = agent.run(&Op::GitPull { root: "/work/proj".into() }).unwrap_err();
    // Then the worktree's request names it and the project's names the configured workspace, the blob has its text beside the contract's data, and the refusal keeps its code
    assert_eq!(rca.asked("git.status"), [json!({"workspace": "wt"}), json!({"workspace": WORKSPACE})]);
    assert_eq!((blob["text"].clone(), blob["size"].clone()), (json!("hi\n"), json!(3)));
    assert_eq!(pull, AgentError::Refused { code: "INVALID_ARGS".into(), message: "this repository has no remote configured".into() });
}
