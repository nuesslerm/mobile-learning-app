//! `agent.threads` end to end over the agent API socket (`/agent/v1/socket`): a mapper creates a thread, follows it and starts a turn, and the
//! scripted CLI of `exec-testkit` (installed as `claude` on the service's `PATH`, started by the service's real process adapter) writes a file
//! through its MCP tool, which the service forwards over the executor channel to the real `rca-exec` binary serving a temporary git repository.
//! A file in that repository was written by `rca-exec`; the service has no way to write one. The same flows prove who the socket lets in: a
//! device bearer minted with the service secret opens it, and opens nothing else.

use crate::harness::{service, service_spawning, Env, Options, Service, SECRET};
use crate::mapper::{handshake, Agent};
use agent_api_proto::Event;
use exec_testkit::script::{Script, Turn};
use exec_testkit::{RcaExec, ScriptedCli, TempRepo};
use rca_service::infrastructure::agent_socket::QUEUE;
use remote_chat_agent_host::Processes;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;

pub(crate) fn seqs(events: &[Event]) -> Vec<u64> {
    events.iter().map(|e| e.seq).collect()
}

pub(crate) fn kinds(events: &[Event]) -> Vec<&str> {
    events.iter().map(|e| e.kind.as_str()).collect()
}

pub(crate) fn said(text: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string()
}

/// Puts the scripted CLIs first on the `PATH` of this process, where the service's process adapter finds `claude`, and puts the old one back.
pub(crate) struct Path(OsString);

impl Path {
    pub(crate) fn first(cli: &ScriptedCli) -> Path {
        let old = Path(std::env::var_os("PATH").unwrap_or_default());
        std::env::set_var("PATH", cli.path());
        old
    }
}

impl Drop for Path {
    fn drop(&mut self) {
        std::env::set_var("PATH", &self.0);
    }
}

/// The service that starts real processes, the scripted CLI it finds, the repository and the real `rca-exec` serving it as workspace `w1`.
pub(crate) struct Chain {
    _svc: Service,
    pub(crate) port: u16,
    pub(crate) cli: ScriptedCli,
    pub(crate) repo: TempRepo,
    pub(crate) exec: Option<RcaExec>,
    token: String,
    _path: Path,
}

pub(crate) async fn chain(env: &Env) -> Chain {
    chain_with(env, Options::default()).await
}

async fn chain_with(env: &Env, options: Options) -> Chain {
    let cli = ScriptedCli::install();
    let path = Path::first(&cli);
    let repo = TempRepo::new();
    let svc = service_spawning(env, options, Arc::new(Processes::at(env.providers_root().to_string_lossy())));
    let port = svc.listen().await;
    let exec = tokio::task::block_in_place(|| RcaExec::serve(&format!("http://127.0.0.1:{port}"), "w1", &repo, SECRET));
    let token = svc.mint("flow mapper").await;
    Chain { _svc: svc, port, cli, repo, exec: Some(exec), token, _path: path }
}

impl Chain {
    pub(crate) fn service(&self) -> &Service {
        &self._svc
    }

    /// The model the `git.ai` group asks.
    pub(crate) fn models(&self) -> Arc<rca_group_accounts::testing::ScriptedModelApi> {
        self._svc.models.clone()
    }

    pub(crate) async fn agent(&self) -> Agent {
        Agent::open(self.port, &self.token).await
    }

    pub(crate) fn script(&self, turns: Vec<Turn>) {
        self.cli.script("claude", &Script::of(turns));
    }
}

pub(crate) async fn create(agent: &mut Agent, thread: &str) {
    let made = agent.ask("thread.create", json!({"thread": thread, "title": "Notes", "workspace": "w1", "driver": "claudeAgent"})).await;
    assert!(made.get("result").is_some(), "{made}");
}

pub(crate) async fn start_turn(agent: &mut Agent, thread: &str) -> Value {
    agent.ask("turn.start", json!({"thread": thread, "workspace": "w1", "driver": "claudeAgent", "prompt": "make a note"})).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_scripted_cli_that_writes_a_file_through_its_tool_when_a_turn_runs_over_the_agent_socket_then_the_file_is_in_the_repository_and_the_events_arrive_in_seq_order() {
    // Given the service, the real executor serving a repository, a CLI whose turn writes notes/a.txt through the MCP tool, and a mapper following a new thread
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let write = json!({"path": "notes/a.txt", "content": "hello from the model", "create_dirs": true});
    chain.script(vec![Turn::replaying(&[said("writing the note")]).then_calling("k1", "write_file", write.clone()).then_line(&said("done"))]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    let followed = agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    assert_eq!(followed["result"], json!({"topic": "thread/t1", "after_seq": 0, "last_seq": 0}));
    // When a turn starts
    let started = start_turn(&mut agent, "t1").await;
    let events = agent.events_through("turn.finished").await;
    // Then the file is in the repository, written by the executor, and not in the directory the service runs from
    assert_eq!(std::fs::read_to_string(chain.repo.path().join("notes/a.txt")).unwrap(), "hello from the model");
    assert_eq!(chain.repo.git(&["status", "--porcelain"]), "?? notes/");
    assert!(!env.home.join("notes").exists());
    // And the events are the turn's, in the order they happened, each of the turn that started, with strictly increasing seq
    let turn = started["result"]["turn"].as_str().unwrap().to_string();
    assert_eq!(started["result"]["thread"], json!("t1"));
    assert_eq!(kinds(&events), ["turn.started", "message.delta", "message.delta", "tool.started", "tool.output", "tool.finished", "file.changed", "message.delta", "turn.finished"]);
    assert!(events.iter().all(|e| e.turn.as_deref() == Some(turn.as_str()) && e.thread.as_deref() == Some("t1")));
    assert!(seqs(&events).windows(2).all(|pair| pair[0] < pair[1]), "{:?}", seqs(&events));
    assert_eq!((events[1].data["role"].clone(), events[1].data["text"].clone()), (json!("user"), json!("make a note")));
    assert_eq!((events[3].data["tool"].clone(), events[3].data["args"].clone()), (json!("write_file"), write));
    assert_eq!((events[5].data["ok"].clone(), events[6].data["path"].clone(), events[6].data["op"].clone()), (json!(true), json!("notes/a.txt"), json!("write")));
    assert_eq!(events[8].data["reason"], json!("completed"));
    // And the CLI was started once, in the service's directory, with its own tools off and the thread's executor as its only tool server
    let starts = chain.cli.invocations("claude");
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].cwd, env.home.to_string_lossy());
    let argv = &starts[0].argv;
    let after = |flag: &str| argv[argv.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag} is not in {argv:?}")) + 1].clone();
    assert!(after("--mcp-config").contains(&format!("http://127.0.0.1:{}/mcp/t1", chain.port)) && after("--disallowedTools").contains("Bash"), "{argv:?}");
    assert!(argv.contains(&"--strict-mcp-config".to_string()) && argv.contains(&"make a note".to_string()));
    // And the thread is idle again with the newest seq, and its history and transcript read the same events from the engine
    let thread = agent.ask("thread.get", json!({"thread": "t1"})).await;
    assert_eq!((thread["result"]["thread"]["status"].clone(), thread["result"]["thread"]["last_seq"].clone()), (json!("idle"), json!(events.last().unwrap().seq)));
    let history = agent.ask("thread.history", json!({"thread": "t1"})).await;
    assert_eq!(history["result"]["events"].as_array().unwrap().iter().map(|e| e["seq"].as_u64().unwrap()).collect::<Vec<_>>(), seqs(&events));
    let transcript = agent.ask("thread.transcript", json!({"thread": "t1"})).await;
    let rows: Vec<&str> = transcript["result"]["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert_eq!(rows, ["message", "message", "tool", "file", "message"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_socket_that_drops_mid_turn_when_a_new_one_subscribes_after_the_last_seq_it_saw_then_the_two_together_have_every_event_once_in_order() {
    // Given a turn that says one thing, waits, and says another, and a mapper that follows it
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("one")]).then_sleeping(1500).then_line(&said("two"))]);
    let mut first = chain.agent().await;
    create(&mut first, "t1").await;
    first.ask("thread.subscribe", json!({"thread": "t1"})).await;
    start_turn(&mut first, "t1").await;
    let mut before_drop = Vec::new();
    while !before_drop.last().is_some_and(|e: &Event| e.kind == "message.delta" && e.data["role"] == json!("assistant")) {
        before_drop.extend(first.events_through("message.delta").await);
    }
    let last_seen = before_drop.last().unwrap().seq;
    // When the socket drops while the CLI waits, and a new one subscribes after the last seq it saw
    drop(first);
    let mut second = chain.agent().await;
    let resumed = second.ask("thread.subscribe", json!({"thread": "t1", "after_seq": last_seen})).await;
    let rest = second.events_through("turn.finished").await;
    // Then the new socket's events are the ones after it, starting at the very next, with no duplicate of what the first saw
    assert_eq!(resumed["result"]["after_seq"], json!(last_seen));
    assert!(rest.iter().all(|e| e.seq > last_seen), "{:?} after {last_seen}", seqs(&rest));
    // And the two together are exactly the thread's log: no event lost, none twice
    let all = second.ask("thread.history", json!({"thread": "t1"})).await;
    let logged: Vec<u64> = all["result"]["events"].as_array().unwrap().iter().map(|e| e["seq"].as_u64().unwrap()).collect();
    let together: Vec<u64> = seqs(&before_drop).into_iter().chain(seqs(&rest)).collect();
    assert_eq!(together, logged);
    assert_eq!(kinds(&rest).last(), Some(&"turn.finished"));
    assert!(rest.iter().any(|e| e.kind == "message.delta" && e.data["text"] == json!("two")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_thread_with_more_events_than_a_socket_queue_holds_when_a_new_socket_subscribes_from_the_start_then_every_event_arrives_once_in_order_and_the_socket_stays_open() {
    // Given a finished turn whose CLI said more lines than the frames a socket may have waiting
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let lines: Vec<String> = (0..QUEUE + 500).map(|n| said(&format!("line {n}"))).collect();
    chain.script(vec![Turn::replaying(&lines)]);
    let mut first = chain.agent().await;
    create(&mut first, "t1").await;
    start_turn(&mut first, "t1").await;
    let mut status = json!(null);
    for _ in 0..600 {
        status = first.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"].clone();
        if status == json!("idle") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(status, json!("idle"));
    // When a new socket subscribes from the start, and asks something else before it reads any event
    let mut late = chain.agent().await;
    let followed = late.ask("thread.subscribe", json!({"thread": "t1", "after_seq": 0})).await;
    let last = followed["result"]["last_seq"].as_u64().unwrap();
    let still_open = late.ask("thread.get", json!({"thread": "t1"})).await;
    let mut replayed = Vec::new();
    while replayed.last().is_none_or(|event: &Event| event.seq < last) {
        replayed.push(late.event().await);
    }
    // Then the events are the turn's, each once and in order, to the newest, with the lines as the CLI said them
    assert_eq!(replayed.len(), lines.len() + 3);
    assert!(seqs(&replayed).windows(2).all(|pair| pair[0] < pair[1]) && replayed.last().map(|e| e.seq) == Some(last));
    let said_lines: Vec<&Value> = replayed.iter().filter(|e| e.kind == "message.delta" && e.data["role"] == json!("assistant")).map(|e| &e.data["text"]).collect();
    assert_eq!(said_lines, (0..lines.len()).map(|n| json!(format!("line {n}"))).collect::<Vec<_>>().iter().collect::<Vec<_>>());
    // And the socket was not closed for the length of the replay: it answered meanwhile and answers after
    assert_eq!(still_open["result"]["thread"]["last_seq"], json!(last));
    assert_eq!(late.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_running_turn_when_a_second_starts_and_the_first_is_interrupted_then_the_second_is_refused_and_the_first_ends_interrupted_and_frees_the_thread() {
    // Given a CLI that says it is working and stays up
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("working")]).hanging()]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let started = start_turn(&mut agent, "t1").await;
    let turn = started["result"]["turn"].as_str().unwrap().to_string();
    let mut seen = agent.events_through("message.delta").await;
    seen.extend(agent.events_through("message.delta").await);
    // When a second turn starts on the thread, and an interrupt names a turn that is not running
    let busy = start_turn(&mut agent, "t1").await;
    let wrong = agent.ask("turn.interrupt", json!({"thread": "t1", "turn": "turn-0"})).await;
    // Then the second is refused, the wrong interrupt is NO_TURN, and the turn still runs
    assert_eq!(busy["error"]["code"], json!("TURN_RUNNING"));
    assert_eq!(wrong["error"]["code"], json!("NO_TURN"));
    assert_eq!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("running"));
    // When the running turn is interrupted
    let stopped = agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    let end = agent.events_through("turn.finished").await;
    // Then the answer names it and the events end with its finish as interrupted, and the thread is idle and takes a new turn
    assert_eq!(stopped["result"], json!({"thread": "t1", "turn": turn}));
    assert_eq!(end.last().unwrap().data["reason"], json!("interrupted"));
    assert!(end.iter().all(|e| e.turn.as_deref() == Some(turn.as_str())));
    assert_eq!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
    assert_eq!(agent.ask("turn.interrupt", json!({"thread": "t1"})).await["error"]["code"], json!("NO_TURN"));
    let again = start_turn(&mut agent, "t1").await;
    assert!(again["result"]["turn"].is_string() && again["result"]["turn"] != json!(turn), "{again}");
    for _ in 0..200 {
        if chain.cli.invocations("claude").len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(chain.cli.invocations("claude").len(), 2);
    agent.ask("turn.interrupt", json!({"thread": "t1"})).await;
    assert!(seen.len() >= 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_command_running_at_the_executor_when_the_executor_process_dies_then_the_turn_ends_failed_with_the_executor_gone() {
    // Given a CLI whose command the executor is still running when the executor is killed
    let env = Env::process(&[]);
    let mut chain = chain(&env).await;
    chain.script(vec![Turn::replaying(&[said("starting")]).then_calling("k1", "run_command", json!({"command": "sleep 3"})).then_line(&said("never said"))]);
    let mut agent = chain.agent().await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    start_turn(&mut agent, "t1").await;
    agent.events_through("tool.started").await;
    // When the executor process is killed
    drop(chain.exec.take());
    let end = agent.events_through("turn.finished").await;
    // Then the turn recorded the executor going away and ended failed
    let error = end.iter().find(|e| e.kind == "error").unwrap_or_else(|| panic!("no error event in {:?}", kinds(&end)));
    assert_eq!(error.data["code"], json!("EXECUTOR_GONE"));
    assert_eq!(end.last().unwrap().data["reason"], json!("failed"));
    assert_eq!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
}

async fn http(port: u16, method: &str, path: &str, headers: &[(&str, String)], body: Option<Value>) -> (u16, Value) {
    let mut request = reqwest::Client::new().request(method.parse().unwrap(), format!("http://127.0.0.1:{port}{path}"));
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.expect("the service answers");
    (response.status().as_u16(), response.json().await.unwrap_or(Value::Null))
}

fn secret() -> Vec<(&'static str, String)> {
    vec![("x-bot-secret", SECRET.to_string())]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_an_operator_with_the_service_secret_when_a_bearer_is_minted_listed_and_revoked_then_it_opens_the_socket_until_it_is_revoked_and_only_its_digest_is_kept() {
    // Given the service
    let env = Env::process(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    // When the operator mints a bearer for a label, one without a label, and lists them
    let (status, minted) = http(port, "POST", "/agent/v1/bearers", &secret(), Some(json!({"label": "phone"}))).await;
    let (no_label, _) = http(port, "POST", "/agent/v1/bearers", &secret(), Some(json!({"label": "  "}))).await;
    let (_, listed) = http(port, "GET", "/agent/v1/bearers", &secret(), None).await;
    // Then the mint answers 201 with the token once, a blank label is refused, and the list names the bearer and never a token
    let token = minted["token"].as_str().unwrap().to_string();
    assert_eq!((status, no_label), (201, 400));
    assert!(token.starts_with("rcab_") && token.len() > 60, "{token}");
    assert_eq!(listed["bearers"], json!([{"id": minted["id"], "label": "phone", "createdAt": minted["createdAt"]}]));
    assert!(!listed.to_string().contains(&token));
    // And the file on the volume holds the digest of the token and not the token, and nobody but the service's user reads it
    let path = env.home.join(".local/state/rca/device-bearers.json");
    let kept = std::fs::read_to_string(&path).unwrap();
    assert!(kept.contains(&rca_agent_core::token_digest(&token)) && !kept.contains(&token), "{kept}");
    assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&path).unwrap().permissions()) & 0o777, 0o600);
    // And the bearer opens the socket, also on a service started again over the same volume
    assert!(Agent::open(port, &token).await.ask("service.status", json!({})).await.get("result").is_some());
    let restarted = service(&env, Options::default());
    let restarted_port = restarted.listen().await;
    assert!(Agent::open(restarted_port, &token).await.ask("service.status", json!({})).await.get("result").is_some());
    // When the operator revokes it, and revokes it again
    let id = minted["id"].as_str().unwrap();
    let (revoked, answer) = http(port, "DELETE", &format!("/agent/v1/bearers/{id}"), &secret(), None).await;
    let (again, gone) = http(port, "DELETE", &format!("/agent/v1/bearers/{id}"), &secret(), None).await;
    // Then the first says so and the second finds nothing, and the token opens the socket no longer, a made-up one never did
    assert_eq!((revoked, answer), (200, json!({"revoked": id})));
    assert_eq!((again, gone["code"].clone()), (404, json!("NOT_FOUND")));
    assert_eq!(handshake(port, "/agent/v1/socket", &[("authorization", format!("Bearer {token}"))]).await.err(), Some(401));
    assert_eq!(handshake(port, "/agent/v1/socket", &[("authorization", "Bearer rcab_made_up".to_string())]).await.err(), Some(401));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_device_bearer_when_it_is_presented_to_the_bearer_endpoints_and_the_executor_channel_then_each_answers_401_and_nothing_changed() {
    // Given two bearers, a phone's and a watch's, and the service secret as the operator's
    let env = Env::process(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    let phone = svc.mint("phone").await;
    let watch = svc.mint("watch").await;
    let (_, before) = http(port, "GET", "/agent/v1/bearers", &secret(), None).await;
    let watch_id = before["bearers"][1]["id"].as_str().unwrap().to_string();
    assert_eq!(before["bearers"].as_array().unwrap().len(), 2);
    // When the phone's bearer is presented, as a bearer token and as the secret header, to mint, list and revoke
    for (how, header) in [("bearer", ("authorization", format!("Bearer {phone}"))), ("header", ("x-bot-secret", phone.clone()))] {
        let headers = [header];
        let minted = http(port, "POST", "/agent/v1/bearers", &headers, Some(json!({"label": "stolen"}))).await;
        let listed = http(port, "GET", "/agent/v1/bearers", &headers, None).await;
        let revoked = http(port, "DELETE", &format!("/agent/v1/bearers/{watch_id}"), &headers, None).await;
        // Then each is the service's own 401, and no token is in any answer
        for (what, (status, body)) in [("mint", minted), ("list", listed), ("revoke", revoked)] {
            assert_eq!((status, body["code"].clone()), (401, json!("UNAUTHORIZED")), "{what} as {how}: {body}");
            assert!(!body.to_string().contains("rcab_"));
        }
    }
    // And the secret on the same routes works, and shows the bearers are the two there were: nothing was minted, nothing revoked
    let (_, after) = http(port, "GET", "/agent/v1/bearers", &secret(), None).await;
    assert_eq!(after, before);
    assert!(Agent::open(port, &watch).await.ask("service.status", json!({})).await.get("result").is_some());
    // When the bearer is presented to the executor channel, as a bearer token and as the secret header
    for header in [("authorization", format!("Bearer {phone}")), ("x-bot-secret", phone.clone())] {
        // Then the channel is refused with 401
        assert_eq!(handshake(port, "/exec/v2/executors?workspace=w9", &[header]).await.err(), Some(401));
    }
    // And the secret opens the channel, which proves the route is there and that the refusals above attached nothing: w9 is listed only now
    let mut agent = Agent::open(port, &phone).await;
    assert_eq!(agent.ask("workspace.list", json!({})).await["result"], json!({"workspaces": []}));
    let _executor = handshake(port, "/exec/v2/executors?workspace=w9", &[("x-bot-secret", SECRET.to_string())]).await.expect("the secret opens the channel");
    assert_eq!(agent.ask("workspace.list", json!({})).await["result"], json!({"workspaces": ["w9"]}));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_the_service_secret_when_it_is_presented_to_the_agent_socket_then_it_is_refused_in_both_forms_and_a_service_with_no_secret_mints_nothing() {
    // Given the service, and one started with no secret configured
    let env = Env::process(&[]);
    let svc = service(&env, Options::default());
    let port = svc.listen().await;
    // When the secret is presented to the socket as a bearer token and as the header, Then it is refused with 401, though the secret is what mints bearers
    for header in [("authorization", format!("Bearer {SECRET}")), ("x-bot-secret", SECRET.to_string())] {
        assert_eq!(handshake(port, "/agent/v1/socket", &[header]).await.err(), Some(401));
    }
    assert_eq!(http(port, "POST", "/agent/v1/bearers", &secret(), Some(json!({"label": "ok"}))).await.0, 201);
    let open = service(&env, Options { secret: None, ..Options::default() });
    let open_port = open.listen().await;
    // And with no secret configured nobody mints, lists or revokes, and the socket stays shut
    for (method, path, body) in [("POST", "/agent/v1/bearers", Some(json!({"label": "x"}))), ("GET", "/agent/v1/bearers", None), ("DELETE", "/agent/v1/bearers/x", None)] {
        let (status, answer) = http(open_port, method, path, &[], body.clone()).await;
        let (with_header, _) = http(open_port, method, path, &[("x-bot-secret", String::new())], body).await;
        assert_eq!((status, answer["code"].clone(), with_header), (401, json!("UNAUTHORIZED"), 401), "{method} {path}");
    }
    assert_eq!(handshake(open_port, "/agent/v1/socket", &[]).await.err(), Some(401));
}

/// The same chain after its service and executor were stopped and started again over the same home, repository and bearer.
async fn restarted(old: Chain, env: &Env, options: Options) -> Chain {
    let Chain { _svc, cli, repo, exec, token, _path, .. } = old;
    drop(exec);
    drop(_svc);
    let svc = service_spawning(env, options, Arc::new(Processes::at(env.providers_root().to_string_lossy())));
    let port = svc.listen().await;
    let exec = tokio::task::block_in_place(|| RcaExec::serve(&format!("http://127.0.0.1:{port}"), "w1", &repo, SECRET));
    Chain { _svc: svc, port, cli, repo, exec: Some(exec), token, _path }
}

fn opencode_said(session: &str, text: &str) -> String {
    json!({"type": "text", "timestamp": 1, "sessionID": session, "part": {"text": text}}).to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_an_opencode_thread_whose_turn_named_a_session_when_the_service_and_executor_restart_then_the_next_turn_resumes_that_session() {
    // Given an opencode thread whose first turn printed lines carrying the CLI's own session id, and a second turn scripted behind it
    let env = Env::process(&[]);
    let state = env.home.join("rca-state");
    std::fs::create_dir_all(&state).unwrap();
    let options = || Options { state_dir: Some(state.clone()), ..Options::default() };
    let chain = chain_with(&env, options()).await;
    chain.cli.script("opencode", &Script::of(vec![
        Turn::replaying(&[opencode_said("ses_resume_1", "first answer")]),
        Turn::replaying(&[opencode_said("ses_resume_1", "second answer")]),
    ]));
    let mut agent = chain.agent().await;
    let made = agent.ask("thread.create", json!({"thread": "t1", "title": "Notes", "workspace": "w1", "driver": "opencode"})).await;
    assert!(made.get("result").is_some(), "{made}");
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let first = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "opencode", "prompt": "one"})).await;
    assert!(first.get("result").is_some(), "{first}");
    agent.events_through("turn.finished").await;
    // When the service and the executor restart over the same home and a second turn starts on the same thread
    drop(agent);
    let chain = restarted(chain, &env, options()).await;
    let mut agent = chain.agent().await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let second = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "opencode", "prompt": "two"})).await;
    assert!(second.get("result").is_some(), "{second}");
    history_with_finishes(&mut agent, "t1", 2).await;
    // Then the first run of the CLI started a conversation of its own and the second one went on in the session the first named
    let starts = chain.cli.invocations("opencode");
    assert_eq!(starts.len(), 2, "{starts:?}");
    let session_of = |argv: &Vec<String>| argv.iter().position(|a| a == "--session").map(|i| argv[i + 1].clone());
    assert_eq!((session_of(&starts[0].argv), session_of(&starts[1].argv)), (None, Some("ses_resume_1".to_string())), "{:?}", starts.iter().map(|s| s.argv.clone()).collect::<Vec<_>>());
}

async fn history_kinds(agent: &mut Agent, thread: &str) -> Vec<(String, Value)> {
    let history = agent.ask("thread.history", json!({"thread": thread})).await;
    history["result"]["events"].as_array().unwrap().iter().map(|e| (e["type"].as_str().unwrap().to_string(), if e["type"] == "error" { e["message"].clone() } else { e["reason"].clone() })).collect()
}

/// The history of the thread once it holds `count` finished turns (a subscription replays the finishes of before).
async fn history_with_finishes(agent: &mut Agent, thread: &str, count: usize) -> Vec<(String, Value)> {
    for _ in 0..300 {
        let kinds = history_kinds(agent, thread).await;
        if kinds.iter().filter(|(kind, _)| kind == "turn.finished").count() >= count {
            return kinds;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the thread never held {count} finished turns");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_an_opencode_turn_interrupted_by_a_restart_with_a_prompt_queued_behind_it_then_the_queue_runs_in_the_kept_session_and_a_continue_resumes_it_too() {
    // Given an opencode thread whose first turn announced its session and is still running, with one more prompt queued behind it
    let env = Env::process(&[]);
    let state = env.home.join("rca-state");
    std::fs::create_dir_all(&state).unwrap();
    let options = || Options { state_dir: Some(state.clone()), ..Options::default() };
    // The first service runs on a runtime of its own that is shut down to crash it: the turn it runs and the queue behind it record nothing more
    let first_run = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let chain = std::thread::scope(|scope| {
        scope.spawn(|| first_run.block_on(async {
            let chain = chain_with(&env, options()).await;
            chain.cli.script("opencode", &Script::of(vec![
                Turn::replaying(&[opencode_said("ses_queue_1", "working")]).then_sleeping(60_000),
                Turn::replaying(&[opencode_said("ses_queue_1", "queued answer")]),
                Turn::replaying(&[opencode_said("ses_queue_1", "continued")]),
            ]));
            let mut agent = chain.agent().await;
            let made = agent.ask("thread.create", json!({"thread": "t1", "title": "Notes", "workspace": "w1", "driver": "opencode"})).await;
            assert!(made.get("result").is_some(), "{made}");
            agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
            let first = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "opencode", "prompt": "one"})).await;
            assert!(first.get("result").is_some(), "{first}");
            agent.events_through("message.delta").await;
            agent.events_through("message.delta").await;
            let queued = agent.ask("queue.add", json!({"thread": "t1", "item": "q1", "prompt": "two"})).await;
            assert!(queued.get("result").is_some(), "{queued}");
            drop(agent);
            chain
        })).join().unwrap()
    });
    // When the service crashes and the service and the executor start again over the same state dir
    first_run.shutdown_background();
    let chain = restarted(chain, &env, options()).await;
    let mut agent = chain.agent().await;
    let kinds_seen = history_with_finishes(&mut agent, "t1", 2).await;
    // Then the first turn is reported finished as interrupted, the queued prompt ran after the restart, and the thread is idle
    let finishes: Vec<Value> = kinds_seen.iter().filter(|(kind, _)| kind == "turn.finished").map(|(_, reason)| reason.clone()).collect();
    assert_eq!(finishes, [json!("interrupted"), json!("completed")], "{kinds_seen:?}");
    assert!(kinds_seen.iter().any(|(kind, _)| kind == "queue.started"), "{kinds_seen:?}");
    assert_eq!(agent.ask("thread.get", json!({"thread": "t1"})).await["result"]["thread"]["status"], json!("idle"));
    // And the queued prompt's CLI went on in the first turn's session
    let session_of = |argv: &Vec<String>| argv.iter().position(|a| a == "--session").map(|i| argv[i + 1].clone());
    let starts = chain.cli.invocations("opencode");
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert_eq!((session_of(&starts[0].argv), session_of(&starts[1].argv)), (None, Some("ses_queue_1".to_string())));
    // When a follow-up prompt "continue" starts a turn
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    let again = agent.ask("turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "opencode", "prompt": "continue"})).await;
    assert!(again.get("result").is_some(), "{again}");
    history_with_finishes(&mut agent, "t1", 3).await;
    // Then its CLI resumes the same session
    let starts = chain.cli.invocations("opencode");
    assert_eq!(starts.len(), 3, "{starts:?}");
    assert_eq!(session_of(&starts[2].argv), Some("ses_queue_1".to_string()), "{:?}", starts[2].argv);
}
