//! The recorded exchanges of the agent API and the executor contract (`libraries/agent-api/*/tests/fixtures`) against the service, over real
//! WebSockets: a mapper's request on `/agent/v1/socket` must be answered as recorded, and the executor attached on `/exec/v2/executors` must be
//! asked for exactly the actions the registered groups declare for the method. The executor side replays the same files in
//! `clis/rca-exec/tests/flow/contracts.rs`.

use crate::harness::{agent_api, service, Env, Options, SECRET};
use agent_api_proto::testing::{Exchange, Fixture};
use rca_cli_driver::testing::CliScript;
use exec_proto::testing::Fixture as ExecFixture;
use futures_util::{SinkExt, StreamExt};
use rca_cli_driver::domain::{translate, Driver, ExecEvent};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

/// The fixtures of the groups that share one service when replayed: what one leaves behind does not change what another answers.
fn agent_fixtures() -> Vec<Fixture> {
    [agent_api_service::testing::fixtures(), agent_api_state::testing::fixtures(), agent_api_workspace::testing::fixtures()].concat()
}

/// The fixtures of `agent.threads`, each replayed on a service of its own: `thread.list` is recorded as the list of a service that holds no other thread.
fn thread_fixtures() -> Vec<Fixture> {
    agent_api_agent::testing::fixtures()
}

/// The fixtures of the sub-groups of the thread surface (`agent.requests`, `agent.queue`, `agent.lifecycle`), each replayed on a service of its own.
fn sub_group_fixtures() -> Vec<Fixture> {
    ["requests", "queue", "lifecycle"].into_iter().flat_map(agent_api_agent::testing::group_fixtures).collect()
}

/// The fixtures of `workspace`, `files.edit` and `workspace.worktrees`, each replayed on a service of its own: a workspace or worktree one makes
/// changes what another answers.
fn workspace_group_fixtures() -> Vec<Fixture> {
    ["workspace", "files-edit", "worktrees"].into_iter().flat_map(agent_api_workspace::testing::group_fixtures).collect()
}

/// The fixtures of `schedules` and `schedules.watch`, each replayed on a service of its own: the flow's clock stands still, so the times are the recorded ones.
fn schedule_fixtures() -> Vec<Fixture> {
    agent_api_schedules::testing::fixtures()
}

fn exec_fixtures() -> Vec<ExecFixture> {
    [exec_fs::testing::fixtures(), exec_proc::testing::fixtures(), exec_git::testing::fixtures()].concat()
}

fn contract_methods() -> BTreeSet<&'static str> {
    [agent_api_service::METHODS, agent_api_state::METHODS, agent_api_state::watch::METHODS, agent_api_workspace::METHODS, agent_api_workspace::edit::METHODS, agent_api_workspace::worktrees::METHODS, agent_api_agent::METHODS].into_iter().flatten().map(|m| m.name()).collect()
}

/// The executor actions the groups the service registers declare for `method`.
fn declared_actions(method: &str) -> Vec<&'static str> {
    let api = agent_api(&Env::detached()).groups;
    let declared = api.served().find(|s| s.method.name() == method).map(|s| s.actions.to_vec()).unwrap_or_default();
    declared
}

#[test]
fn every_method_of_the_contract_crates_has_recorded_exchanges_and_a_success_that_calls_the_declared_actions() {
    // Given the methods the contract crates define, the fixtures they ship and the groups the service registers
    let recorded = [agent_fixtures(), thread_fixtures(), workspace_group_fixtures()].concat();
    // Then each method has one fixture, and no fixture names a method the contract does not
    let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    assert_eq!(named, contract_methods().into_iter().collect::<Vec<_>>());
    for fixture in &recorded {
        // And each exchange asks for that method, one succeeds at least, and a success calls the actions the table declares, a refusal
        // calls the first of them or none (the service answered before it asked the executor, or the executor refused the first)
        assert!(fixture.exchanges.iter().all(|e| e.request["method"] == fixture.method.as_str()), "{}", fixture.method);
        assert!(fixture.exchanges.iter().any(Exchange::succeeds), "{} has no exchange that succeeds", fixture.method);
        let declared = declared_actions(&fixture.method);
        for exchange in &fixture.exchanges {
            if exchange.succeeds() {
                assert_eq!(exchange.actions, declared, "{}: {}", fixture.method, exchange.name);
            } else {
                let is_prefix = exchange.actions.len() <= declared.len() && exchange.actions.iter().zip(&declared).all(|(a, d)| a == d);
                assert!(is_prefix, "{}: {} calls {:?}, the table declares {declared:?}", fixture.method, exchange.name, exchange.actions);
            }
        }
    }
}

#[test]
fn every_method_of_the_thread_sub_groups_has_recorded_exchanges_that_call_the_actions_the_table_declares() {
    // Given the methods of the three sub-groups, the fixtures they ship and the groups the service registers
    let recorded = sub_group_fixtures();
    let methods: BTreeSet<&str> = [agent_api_agent::requests::METHODS, agent_api_agent::queue::METHODS, agent_api_agent::lifecycle::METHODS].into_iter().flatten().map(|m| m.name()).collect();
    // Then each method has one fixture, no fixture names another, and each exchange calls the declared actions or none
    let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    assert_eq!(named, methods.into_iter().collect::<Vec<_>>());
    for fixture in &recorded {
        let declared = declared_actions(&fixture.method);
        assert!(fixture.exchanges.iter().all(|e| e.request["method"] == fixture.method.as_str()), "{}", fixture.method);
        for exchange in &fixture.exchanges {
            assert!(exchange.actions == declared || (!exchange.succeeds() && exchange.actions.is_empty()), "{}: {} calls {:?}, the table declares {declared:?}", fixture.method, exchange.name, exchange.actions);
        }
    }
}

#[test]
fn every_method_of_the_schedules_groups_has_recorded_exchanges_that_call_no_executor_action_but_the_checkpoint_of_a_turn_run_now_starts() {
    // Given the methods of the two schedules groups, the fixtures they ship and the groups the service registers
    let recorded = schedule_fixtures();
    let methods: BTreeSet<&str> = [agent_api_schedules::METHODS, agent_api_schedules::watch::METHODS].into_iter().flatten().map(|m| m.name()).collect();
    // Then each method has one fixture, no fixture names another, and the table declares no executor action for any but `schedule.runNow`, which starts a turn and so takes its checkpoint first
    let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    assert_eq!(named, methods.into_iter().collect::<Vec<_>>());
    for fixture in &recorded {
        let expected: Vec<&str> = if fixture.method == "schedule.runNow" { vec!["git_checkpoint"] } else { vec![] };
        assert_eq!(declared_actions(&fixture.method), expected, "{}", fixture.method);
        // And a refusal calls none, a success the declared ones
        assert!(fixture.exchanges.iter().all(|e| if e.succeeds() { e.actions == expected } else { e.actions.is_empty() }), "{}", fixture.method);
    }
}

/// The name the exec-recordings cassette gives each kind of neutral event.
fn kind_of(event: &ExecEvent) -> &'static str {
    match event {
        ExecEvent::Text(_) => "text",
        ExecEvent::Reasoning(_) => "reasoning",
        ExecEvent::ToolStarted { .. } => "tool.started",
        ExecEvent::ToolFinished { .. } => "tool.finished",
        ExecEvent::Done => "done",
        ExecEvent::Error(_) => "error",
        ExecEvent::Session(_) => "session",
    }
}

#[test]
fn every_recording_of_the_scripted_cli_translates_to_the_neutral_events_its_index_expects() {
    // Given the recordings of each CLI's stream-json that the scripted CLI replays, and the index that says what each should become
    let recordings = exec_testkit::recordings::all();
    assert!(!recordings.is_empty());
    for recording in recordings {
        // When every line of one goes through the translation the service applies to the real CLI's output
        let driver = Driver::parse(&recording.driver).unwrap_or_else(|| panic!("{}: {} is no driver of the service", recording.name, recording.driver));
        let events: Vec<String> = recording.lines.iter().flat_map(|line| translate(driver, line)).map(|event| kind_of(&event).to_string()).collect();
        // Then the neutral events are the ones the index lists, in order
        assert_eq!(events, recording.expect, "{}", recording.name);
    }
}

#[test]
fn every_contract_crate_that_ships_recordings_has_them_among_the_inputs_of_this_flow() {
    // Given the contract crates that keep recorded exchanges, and this package's manifest
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let crates = std::fs::read_dir(root.join("../../../libraries/agent-api")).unwrap().flatten();
    for krate in crates {
        // When an exchange of one is edited, Then the flow reruns only if its directory is an input of the flow task
        let name = krate.file_name().to_string_lossy().into_owned();
        for dir in ["tests/fixtures", "tests/cassettes"].iter().filter(|d| krate.path().join(d).is_dir()) {
            assert!(manifest.contains(&format!("libraries/agent-api/{name}/{dir}/**")), "{name}'s {dir} are not in [package.metadata.flow] inputs");
        }
    }
    // And the scripted CLI's cassette set, which the translation test above reads, is an input too
    let kit = exec_testkit::recordings::set_dir();
    let dir = kit.parent().unwrap().strip_prefix(exec_testkit::recordings::root()).unwrap().display().to_string();
    assert!(manifest.contains(&format!("libraries/agent-api/exec-testkit/{dir}/**")), "the exec-testkit cassettes are not in [package.metadata.flow] inputs");
}

type Socket = WebSocketStream<TcpStream>;

async fn open_agent(port: u16, bearer: &str) -> Socket {
    let mut request = format!("ws://127.0.0.1:{port}/agent/v1/socket").into_client_request().unwrap();
    request.headers_mut().insert("authorization", format!("Bearer {bearer}").parse().unwrap());
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.expect("the service is listening");
    tokio_tungstenite::client_async(request, tcp).await.expect("the agent socket opens").0
}

async fn ask(socket: &mut Socket, request: &Value) -> Value {
    socket.send(Message::Text(request.to_string().into())).await.unwrap();
    loop {
        if let Message::Text(text) = socket.next().await.expect("an answer").unwrap() {
            let frame: Value = serde_json::from_str(&text).unwrap();
            // The events of a stream an earlier request opened arrive between answers: they carry no id
            if frame.get("event").is_none() {
                return frame;
            }
        }
    }
}

/// An executor attached for `workspace` that answers a request from the recorded exchange with the same tool and args, and notes the tool.
async fn attach_recorded_executor(port: u16, workspace: &str, recorded: Vec<ExecFixture>, seen: Arc<Mutex<Vec<String>>>) {
    let mut request = format!("ws://127.0.0.1:{port}/exec/v2/executors?workspace={workspace}").into_client_request().unwrap();
    request.headers_mut().insert("x-bot-secret", SECRET.parse().unwrap());
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (mut socket, _) = tokio_tungstenite::client_async(request, tcp).await.expect("the executor channel opens");
    tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let request: Value = serde_json::from_str(&text).unwrap();
            let id = request["id"].clone();
            // `cancel` is what a dropped call sends, after the answer to the request that dropped it may have been read: not an action of a method
            if request["tool"] != "cancel" {
                seen.lock().unwrap().push(request["tool"].as_str().unwrap_or_default().to_string());
            }
            let answer = recorded.iter().flat_map(|f| &f.exchanges).find(|e| e.request["tool"] == request["tool"] && e.request["args"] == request["args"]);
            let frames = match answer {
                Some(exchange) => exchange.stream.iter().map(|s| json!({"id": id, "stream": s["stream"], "data": s["data"]})).chain(exchange.events.iter().enumerate().map(|(n, event)| json!({"id": id, "seq": n + 1, "event": event}))).chain([{
                    let mut response = exchange.response.clone();
                    response["id"] = id.clone();
                    response
                }]).collect::<Vec<_>>(),
                None => vec![json!({"id": id, "ok": false, "error": {"code": "UNRECORDED", "message": format!("no recorded exchange for {request}")}})],
            };
            for frame in frames {
                let _ = socket.send(Message::Text(frame.to_string().into())).await;
            }
        }
    });
}

/// Replays `recorded` on a service of its own: an executor that answers from the recorded executor exchanges attached for each workspace the
/// recorded listing names, and a CLI that stays up once started (the turns of `agent.threads` end only when a request ends them).
async fn replay(env: &Env, recorded: &[&Fixture], workspaces: &[String]) {
    // A service of its own has no schedule an earlier one kept on the same home
    let _ = std::fs::remove_file(env.home.join(".local/state/rca/schedules.json"));
    let svc = service(env, Options::default());
    svc.spawner.script("claude", CliScript { hold: true, ..CliScript::default() });
    let port = svc.listen().await;
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    for workspace in workspaces {
        attach_recorded_executor(port, workspace, exec_fixtures(), seen.clone()).await;
    }
    let token = svc.mint("contracts").await;
    let mut socket = open_agent(port, &token).await;
    for exchange in recorded.iter().flat_map(|f| &f.exchanges) {
        // When the exchange's setup requests, then its request, are sent on one socket
        for before in &exchange.before {
            let answered = ask(&mut socket, before).await;
            assert!(answered.get("result").is_some(), "{}: the setup request failed: {answered}", exchange.name);
        }
        let asked_before = seen.lock().unwrap().len();
        let answer = ask(&mut socket, &exchange.request).await;
        // Then the answer is the recorded one, and the executor was asked for exactly the actions the exchange declares, in order
        assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}", exchange.name);
        assert_eq!(seen.lock().unwrap()[asked_before..], exchange.actions[..], "{}", exchange.name);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_recorded_request_is_answered_as_recorded_and_the_executor_is_asked_for_exactly_the_declared_actions() {
    // Given the recorded exchanges of every group, and the workspaces the recorded listing names
    let env = Env::new(&[]);
    let recorded = agent_fixtures();
    let listing = recorded.iter().find(|f| f.method == agent_api_workspace::LIST.name()).and_then(|f| f.exchanges.iter().find(|e| e.succeeds())).expect("a recorded listing");
    let workspaces: Vec<String> = listing.response["result"]["workspaces"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect();
    assert!(!workspaces.is_empty());
    // When the groups that share a service are replayed on one, and each fixture of agent.threads on a service of its own
    replay(&env, &recorded.iter().collect::<Vec<_>>(), &workspaces).await;
    let threads = thread_fixtures();
    for fixture in &threads {
        replay(&env, &[fixture], &workspaces).await;
    }
    // And so is each fixture of the sub-groups of the thread surface, on a service of its own
    for fixture in &sub_group_fixtures() {
        replay(&env, &[fixture], &workspaces).await;
    }
    // And so is each fixture of workspace, files.edit and workspace.worktrees
    for fixture in &workspace_group_fixtures() {
        replay(&env, &[fixture], &workspaces).await;
    }
    // And so is each fixture of the schedules groups
    for fixture in &schedule_fixtures() {
        replay(&env, &[fixture], &workspaces).await;
    }
    // Then every exchange was answered as recorded (the assertions are in the replay)
    assert!(threads.iter().flat_map(|f| &f.exchanges).any(|e| e.request["method"] == "turn.start" && e.succeeds()), "a turn started in a recorded exchange");
}
