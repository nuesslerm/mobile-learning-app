//! `terminal` end to end over the agent socket, against the real `rca-exec` binary serving a temporary git repository: the shell is the
//! executor's, a pty on its machine. The flows show the round trip a phone makes (open, write, output as bytes by offset, resize, close), the
//! output that survives an executor reconnect (the executor's channel is cut at the TCP level and reopened by `--reconnect`, the shell keeps
//! running and the stream resumes by byte offset), a reader older than the kept tail, what the executor refuses, and every recorded
//! exchange of `agent-api-terminal` replayed as recorded.

use crate::harness::{service, Env, Options, Service, SECRET};
use crate::mapper::{Agent, WAIT};
use agent_api_proto::Event;
use agent_api_terminal::b64;
use exec_testkit::{RcaExec, TempRepo};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

/// A TCP forwarder between the executor and the service whose connections can be cut without a WebSocket close, as a network drop is.
struct Proxy {
    port: u16,
    open: Arc<Mutex<Vec<tokio::task::AbortHandle>>>,
}

impl Proxy {
    async fn to(service: u16) -> Proxy {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let open: Arc<Mutex<Vec<tokio::task::AbortHandle>>> = Arc::default();
        let kept = open.clone();
        tokio::spawn(async move {
            while let Ok((mut from, _)) = listener.accept().await {
                let Ok(mut upstream) = TcpStream::connect(("127.0.0.1", service)).await else { continue };
                let task = tokio::spawn(async move {
                    let _ = tokio::io::copy_bidirectional(&mut from, &mut upstream).await;
                });
                kept.lock().unwrap().push(task.abort_handle());
            }
        });
        Proxy { port, open }
    }

    fn sever(&self) {
        for task in self.open.lock().unwrap().drain(..) {
            task.abort();
        }
    }
}

struct Rig {
    _svc: Service,
    _exec: RcaExec,
    proxy: Proxy,
    port: u16,
    token: String,
    _repo: TempRepo,
    _dir: tempfile::TempDir,
}

async fn rig(env: &Env) -> Rig {
    let repo = TempRepo::new();
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap().join("worktrees");
    std::fs::create_dir(&base).unwrap();
    let svc = service(env, Options::default());
    let port = svc.listen().await;
    let proxy = Proxy::to(port).await;
    let flags = ["--worktree-base", base.to_str().unwrap(), "--reconnect"];
    let url = format!("http://127.0.0.1:{}", proxy.port);
    let exec = tokio::task::block_in_place(|| RcaExec::serve_with(&url, "w1", &repo, SECRET, &flags));
    let token = svc.mint("terminal flow").await;
    Rig { _svc: svc, _exec: exec, proxy, port, token, _repo: repo, _dir: dir }
}

impl Rig {
    async fn agent(&self) -> Agent {
        Agent::open(self.port, &self.token).await
    }
}

async fn ok(agent: &mut Agent, method: &str, params: Value) -> Value {
    let answer = agent.ask(method, params).await;
    assert!(answer.get("result").is_some(), "{method} failed: {answer}");
    answer["result"].clone()
}

/// Reads events into `seen` until `ready` holds for them.
async fn read_until(agent: &mut Agent, seen: &mut Vec<Event>, what: &str, ready: impl Fn(&[Event]) -> bool) {
    let read = async {
        while !ready(seen) {
            seen.push(agent.event().await);
        }
    };
    if tokio::time::timeout(WAIT, read).await.is_err() {
        panic!("{what}; saw {:?}", seen.iter().map(|e| (e.seq, e.kind.clone(), text_of(std::slice::from_ref(e)))).collect::<Vec<_>>());
    }
}

fn bytes_of(events: &[Event]) -> Vec<u8> {
    events.iter().filter(|e| e.kind == "terminal.output" || e.kind == "terminal.snapshot").flat_map(|e| b64::decode(e.data["data"].as_str().unwrap()).unwrap()).collect()
}

fn text_of(events: &[Event]) -> String {
    String::from_utf8_lossy(&bytes_of(events)).into_owned()
}

/// Every output starts where the one before ended, and `seq` is where it ends: no gap and no repeat.
fn assert_contiguous(events: &[Event]) {
    let mut cursor: Option<u64> = None;
    for e in events.iter().filter(|e| e.kind == "terminal.output" || e.kind == "terminal.snapshot") {
        let (start, end) = if e.kind == "terminal.snapshot" { (e.data["start_offset"].as_u64().unwrap(), e.data["offset"].as_u64().unwrap()) } else { (e.data["offset"].as_u64().unwrap(), e.seq) };
        assert_eq!((e.seq, cursor.unwrap_or(start)), (end, start), "{} is not where the stream stood", e.kind);
        cursor = Some(end);
    }
}

async fn eventually_ok(what: &str, mut attempt: impl AsyncFnMut() -> bool) {
    for _ in 0..300 {
        if attempt().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{what}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_terminal_opened_over_the_socket_when_commands_are_typed_resized_and_closed_then_the_shell_of_the_executor_answers_in_bytes_numbered_by_offset_without_the_executors_secret() {
    // Given the service, the real executor and a mapper that opened a terminal and attached to it
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    let opened = ok(&mut agent, "terminal.open", json!({"workspace": "w1", "terminal": "t1", "cols": 100, "rows": 30})).await;
    assert_eq!((opened["status"].clone(), opened["cols"].clone(), opened["rows"].clone(), opened["opened"].clone()), (json!("running"), json!(100), json!(30), json!(true)));
    let attached = ok(&mut agent, "terminal.attach", json!({"workspace": "w1", "terminal": "t1"})).await;
    assert_eq!((attached["start_offset"].clone(), attached["reset"].clone()), (json!(0), json!(false)));
    let mut seen = Vec::new();
    // When a command whose output differs from what is typed is written, bytes no text can hold are printed, and the pty is resized and asked its size
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "echo hello-$((40+2))\n"})).await;
    read_until(&mut agent, &mut seen, "the computed output", |e| text_of(e).contains("hello-42")).await;
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "printf '\\377\\000done\\n'\n"})).await;
    read_until(&mut agent, &mut seen, "the binary output", |e| bytes_of(e).windows(6).any(|w| w == [0xff, 0x00, b'd', b'o', b'n', b'e'])).await;
    assert_eq!(ok(&mut agent, "terminal.resize", json!({"workspace": "w1", "terminal": "t1", "cols": 101, "rows": 33})).await, json!({"cols": 101, "rows": 33}));
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "stty size\n"})).await;
    read_until(&mut agent, &mut seen, "the size the shell sees", |e| text_of(e).contains("33 101")).await;
    // Then the first event is the snapshot, the stream has no gap and no repeat, and a terminal list shows the new size
    assert_eq!(seen[0].kind, "terminal.snapshot");
    assert_contiguous(&seen);
    let listed = ok(&mut agent, "terminal.list", json!({"workspace": "w1"})).await;
    assert_eq!((listed["terminals"][0]["cols"].clone(), listed["terminals"][0]["rows"].clone()), (json!(101), json!(33)));
    // And the executor's own secret is not in the shell's environment, and a cwd outside the root is refused by the executor
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "env; echo end-of-env\n"})).await;
    read_until(&mut agent, &mut seen, "the environment", |e| text_of(e).contains("end-of-env\r\n")).await;
    assert!(!text_of(&seen).contains(SECRET));
    let outside = agent.ask("terminal.open", json!({"workspace": "w1", "terminal": "t2", "cwd": "../.."})).await;
    assert_eq!(outside["error"]["code"], json!("PATH_OUTSIDE_ROOT"));
    // When the terminal is closed, Then the reader is told, the terminal is gone and its shell is not running
    ok(&mut agent, "terminal.close", json!({"workspace": "w1", "terminal": "t1"})).await;
    read_until(&mut agent, &mut seen, "the close", |e| e.last().is_some_and(|e| e.kind == "terminal.closed")).await;
    let after = agent.ask("terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "x"})).await;
    assert_eq!(after["error"]["code"], json!("UNKNOWN_TERMINAL"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_an_attached_terminal_when_the_executors_connection_drops_and_comes_back_then_the_same_shell_goes_on_and_the_stream_resumes_by_byte_offset_with_what_it_printed_meanwhile() {
    // Given an attached terminal whose shell has printed once and the process id of that shell
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    let opened = ok(&mut agent, "terminal.open", json!({"workspace": "w1", "terminal": "t1"})).await;
    ok(&mut agent, "terminal.attach", json!({"workspace": "w1", "terminal": "t1"})).await;
    let mut seen = Vec::new();
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "echo before-$((1+1))\n"})).await;
    read_until(&mut agent, &mut seen, "the first output", |e| text_of(e).contains("before-2")).await;
    // When a background job is told to print in a second, and the connection is cut at the TCP level
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "(sleep 1; echo late-$((2+2))) &\n"})).await;
    rig.proxy.sever();
    // Then the service has no executor for a moment, and then the executor attached again by itself
    let mut back = false;
    eventually_ok("the executor reattaches", async || {
        let again = agent.ask("terminal.list", json!({"workspace": "w1"})).await;
        back = again.get("result").is_some();
        back
    })
    .await;
    // And the shell that printed during the outage is the same one, the reader gets that output after the reconnect, and a command typed now is answered
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "echo after-$((3+3))\n"})).await;
    read_until(&mut agent, &mut seen, "the output after the reconnect", |e| text_of(e).contains("late-4") && text_of(e).contains("after-6")).await;
    assert_contiguous(&seen);
    let listed = ok(&mut agent, "terminal.list", json!({"workspace": "w1"})).await;
    assert_eq!(listed["terminals"][0]["pid"], opened["pid"]);
    assert_eq!(text_of(&seen).matches("before-2\r\n").count(), 1, "the output before the drop is not repeated");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_more_output_than_the_service_keeps_when_a_reader_resumes_from_the_start_then_it_gets_a_reset_snapshot_of_the_tail_and_a_reader_at_the_end_gets_only_what_follows() {
    // Given a terminal that printed 400000 bytes, more than the 256 KiB the service keeps
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    ok(&mut agent, "terminal.open", json!({"workspace": "w1", "terminal": "t1"})).await;
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "head -c 400000 /dev/zero | tr '\\0' 'x'; echo; echo finished-$((5+5))\n"})).await;
    let mut tail = Vec::new();
    eventually_ok("the output reached the service", async || {
        let info = ok(&mut agent, "terminal.list", json!({"workspace": "w1"})).await;
        tail.clear();
        info["terminals"][0]["offset"].as_u64().unwrap() > 400_000
    })
    .await;
    // When a reader resumes from offset 0
    let attached = ok(&mut agent, "terminal.attach", json!({"workspace": "w1", "terminal": "t1", "after_offset": 0})).await;
    read_until(&mut agent, &mut tail, "the snapshot", |e| !e.is_empty()).await;
    // Then the answer says reset, the first event is a reset snapshot of exactly the kept tail that ends where the stream stood, and the start is not 0
    assert_eq!(attached["reset"], json!(true));
    let snapshot = &tail[0];
    assert_eq!((snapshot.kind.as_str(), snapshot.data["reset"].clone()), ("terminal.snapshot", json!(true)));
    assert!(snapshot.data["start_offset"].as_u64().unwrap() > 0);
    assert_eq!(b64::decode(snapshot.data["data"].as_str().unwrap()).unwrap().len(), agent_api_terminal::RETAIN);
    assert_eq!(snapshot.seq, snapshot.data["offset"].as_u64().unwrap());
    // And a reader that already had everything gets nothing but the next output
    let end = ok(&mut agent, "terminal.list", json!({"workspace": "w1"})).await["terminals"][0]["offset"].as_u64().unwrap();
    let mut other = rig.agent().await;
    ok(&mut other, "terminal.attach", json!({"workspace": "w1", "terminal": "t1", "after_offset": end})).await;
    ok(&mut agent, "terminal.write", json!({"workspace": "w1", "terminal": "t1", "data": "echo next-$((6+6))\n"})).await;
    let mut next = Vec::new();
    read_until(&mut other, &mut next, "the next output", |e| text_of(e).contains("next-12")).await;
    assert!(next.iter().all(|e| e.kind == "terminal.output") && next[0].data["offset"].as_u64().unwrap() >= end);
}

/// A fixture's exchange compared as recorded, on a rig of its own: the terminals it sets up are the only ones in its service.
async fn replay(fixture: &agent_api_proto::testing::Fixture) {
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    for exchange in &fixture.exchanges {
        for before in &exchange.before {
            let answered = agent.ask(before["method"].as_str().unwrap(), before["params"].clone()).await;
            assert!(answered.get("result").is_some(), "{}: the setup failed: {answered}", exchange.name);
        }
        let answer = agent.ask(exchange.request["method"].as_str().unwrap(), exchange.request["params"].clone()).await;
        let mut answer = answer;
        answer["id"] = exchange.response["id"].clone();
        assert_eq!(exchange.masked(answer), exchange.masked(exchange.response.clone()), "{}: {}", fixture.method, exchange.name);
        // The exchange's actions are among the ones the method declares
        let declared: Vec<&str> = rca_group_terminal::SERVED.iter().find(|s| s.method.name() == fixture.method).map(|s| s.actions.to_vec()).unwrap();
        assert!(exchange.actions.iter().all(|a| declared.contains(&a.as_str())), "{}: {} calls {:?}, the table declares {declared:?}", fixture.method, exchange.name, exchange.actions);
        // Each exchange starts from a clean service: the terminal it left is closed
        for t in ["t1", "t2"] {
            let _ = agent.ask("terminal.close", json!({"workspace": "w1", "terminal": t})).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn every_method_of_the_terminal_contract_has_recorded_exchanges_that_the_service_answers_as_recorded_over_the_real_executor() {
    // Given the recorded exchanges of the terminal crate and its methods
    let recorded = agent_api_terminal::testing::fixtures();
    let mut named: Vec<&str> = recorded.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    let mut methods: Vec<&str> = agent_api_terminal::METHODS.iter().map(|m| m.name()).collect();
    methods.sort_unstable();
    // Then each method has one fixture, and every exchange asks for its own method
    assert_eq!(named, methods);
    assert!(recorded.iter().all(|f| f.exchanges.iter().all(|e| e.request["method"] == f.method.as_str() && !e.name.is_empty())));
    // And each is answered as recorded by the service in front of the real executor
    for fixture in &recorded {
        replay(fixture).await;
    }
}
