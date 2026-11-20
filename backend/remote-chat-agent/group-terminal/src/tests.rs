//! The terminal group over an executor registry and a double of an executor that owns ptys: what each method asks of the executor, how output
//! is numbered and replayed, what an attach older than the kept tail is given, how the pump attaches again from its offset after the
//! executor's channel is replaced, and how a terminal the service lost is adopted.
use crate::{Clock, MemorySleep, SleepStore, TerminalGroup};
use agent_api_proto::{Event, Method};
use agent_api_terminal::{self as api, b64};
use async_trait::async_trait;
use rca_agent_core::{AgentApi, Build, Connection, EventSink, Group};
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError, Streamed};
use rca_exec_channel::{Executors, Registry};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

const BUILD: Build<'static> = Build { version: "1", commit: "c" };

struct Tick(AtomicU64);

impl Clock for Tick {
    fn now_ms(&self) -> u64 {
        self.0.fetch_add(1, Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Pty {
    out: Vec<u8>,
    base: u64,
    exited: Option<i32>,
    cols: u16,
    rows: u16,
}

/// An executor with ptys: what it was asked, and what each pty has produced. A channel is one connection to it, so replacing the channel
/// of the executor leaves its ptys running.
#[derive(Default)]
struct Host {
    ptys: Mutex<std::collections::BTreeMap<String, Pty>>,
    asked: Mutex<Vec<(String, Value)>>,
    produced: watch::Sender<u64>,
}

impl Host {
    fn produce(&self, pty: &str, bytes: &[u8]) {
        self.ptys.lock().unwrap().get_mut(pty).unwrap().out.extend_from_slice(bytes);
        self.produced.send_modify(|v| *v += 1);
    }

    fn attaches(&self) -> Vec<Value> {
        self.asked.lock().unwrap().iter().filter(|(t, _)| t == "pty_attach").map(|(_, a)| a["after_offset"].clone()).collect()
    }

    fn pty_name(&self) -> String {
        self.ptys.lock().unwrap().keys().next().cloned().unwrap()
    }
}

struct Connection1 {
    host: Arc<Host>,
    closed: watch::Sender<bool>,
}

impl Connection1 {
    fn new(host: &Arc<Host>) -> Arc<Connection1> {
        Arc::new(Connection1 { host: host.clone(), closed: watch::channel(false).0 })
    }
}

fn done(result: Value) -> Result<ActionOutput, RemoteError> {
    Ok(ActionOutput { result, output: String::new() })
}

#[async_trait]
impl ExecutorChannel for Connection1 {
    async fn call(&self, tool: &str, args: Value) -> Result<ActionOutput, RemoteError> {
        if self.is_closed() {
            return Err(RemoteError::ExecutorGone);
        }
        self.host.asked.lock().unwrap().push((tool.to_string(), args.clone()));
        let name = args["pty"].as_str().unwrap_or_default().to_string();
        let mut ptys = self.host.ptys.lock().unwrap();
        match tool {
            "pty_open" if ptys.contains_key(&name) => Err(RemoteError::Action { code: "ALREADY_EXISTS".into(), message: "exists".into() }),
            "pty_open" => {
                let (cols, rows) = (args["cols"].as_u64().unwrap_or(80) as u16, args["rows"].as_u64().unwrap_or(24) as u16);
                ptys.insert(name.clone(), Pty { cols, rows, ..Pty::default() });
                done(json!({"pty": name, "pid": 4242, "cwd": args["cwd"].as_str().unwrap_or("/repo"), "cols": cols, "rows": rows}))
            }
            "pty_write" => {
                let Some(pty) = ptys.get_mut(&name) else { return Err(RemoteError::Action { code: "NOT_FOUND".into(), message: "no pty".into() }) };
                pty.out.extend_from_slice(format!("got:{}", args["data"].as_str().unwrap()).as_bytes());
                drop(ptys);
                self.host.produced.send_modify(|v| *v += 1);
                done(json!({"bytes": args["data"].as_str().unwrap().len()}))
            }
            "pty_resize" => {
                let pty = ptys.get_mut(&name).unwrap();
                (pty.cols, pty.rows) = (args["cols"].as_u64().unwrap() as u16, args["rows"].as_u64().unwrap() as u16);
                done(json!({"cols": pty.cols, "rows": pty.rows}))
            }
            "pty_close" => match ptys.remove(&name) {
                Some(_) => done(json!({"closed": true, "exit_code": 129})),
                None => Err(RemoteError::Action { code: "NOT_FOUND".into(), message: "no pty".into() }),
            },
            "pty_list" => done(json!({"ptys": ptys.iter().map(|(n, p)| json!({"pty": n, "pid": 1, "cwd": "/repo", "cols": p.cols, "rows": p.rows, "offset": p.base + p.out.len() as u64, "base_offset": p.base, "exited": p.exited.is_some(), "exit_code": p.exited})).collect::<Vec<_>>()})),
            other => panic!("unexpected action {other}"),
        }
    }

    async fn call_stream(&self, tool: &str, args: Value, stream: mpsc::UnboundedSender<Streamed>) -> Result<ActionOutput, RemoteError> {
        assert_eq!(tool, "pty_attach");
        if self.is_closed() {
            return Err(RemoteError::ExecutorGone);
        }
        self.host.asked.lock().unwrap().push((tool.to_string(), args.clone()));
        let name = args["pty"].as_str().unwrap().to_string();
        let mut sent = args["after_offset"].as_u64().unwrap_or(0);
        let mut produced = self.host.produced.subscribe();
        loop {
            if self.is_closed() {
                return done(json!({"pty": name, "offset": sent, "exited": false, "exit_code": null, "detached": true}));
            }
            let (chunk, exited) = {
                let ptys = self.host.ptys.lock().unwrap();
                let Some(pty) = ptys.get(&name) else { return Err(RemoteError::Action { code: "NOT_FOUND".into(), message: "no pty".into() }) };
                let from = (sent - pty.base) as usize;
                (pty.out[from..].to_vec(), pty.exited)
            };
            if !chunk.is_empty() {
                let _ = stream.send(Streamed::Bytes { offset: sent, data: chunk.clone() });
                sent += chunk.len() as u64;
                continue;
            }
            if let Some(code) = exited {
                return done(json!({"pty": name, "offset": sent, "exited": true, "exit_code": code, "detached": false}));
            }
            let mut closed = self.closed.subscribe();
            tokio::select! {
                _ = produced.changed() => {}
                _ = closed.wait_for(|c| *c) => return done(json!({"pty": name, "offset": sent, "exited": false, "exit_code": null, "detached": true})),
            }
        }
    }

    fn close(&self, _code: u16) {
        self.closed.send_replace(true);
    }

    fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    async fn closed(&self) {
        let _ = self.closed.subscribe().wait_for(|c| *c).await;
    }
}

#[derive(Default)]
struct Pushed(Mutex<Vec<Event>>);

#[async_trait]
impl EventSink for Pushed {
    fn send(&self, event: &Event) -> bool {
        self.0.lock().unwrap().push(event.clone());
        true
    }

    async fn send_replay(&self, event: &Event) -> bool {
        self.send(event)
    }
}

impl Pushed {
    fn events(&self) -> Vec<Event> {
        self.0.lock().unwrap().clone()
    }

    /// The bytes of the output events and snapshots so far, in order.
    fn text(&self) -> String {
        let bytes: Vec<u8> = self.events().iter().filter(|e| e.kind == "terminal.output" || e.kind == "terminal.snapshot").flat_map(|e| b64::decode(e.data["data"].as_str().unwrap()).unwrap()).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    async fn until(&self, what: &str, ready: impl Fn(&[Event]) -> bool) {
        for _ in 0..400 {
            if ready(&self.events()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("{what}: {:?}", self.events().iter().map(|e| (e.seq, e.kind.clone())).collect::<Vec<_>>());
    }
}

struct Rig {
    api: AgentApi,
    host: Arc<Host>,
    executors: Arc<Registry>,
    channel: Arc<Connection1>,
}

fn rig() -> Rig {
    rig_keeping(Arc::new(MemorySleep::default()), Arc::new(Host::default()))
}

/// A service whose sleeping workspaces are in `store` and whose executor is `host`: a second rig over the same two is a service that restarted.
fn rig_keeping(store: Arc<dyn SleepStore>, host: Arc<Host>) -> Rig {
    let channel = Connection1::new(&host);
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "w1", channel.clone()).unwrap();
    let group: Arc<dyn Group> = Arc::new(TerminalGroup::new(executors.clone(), Arc::new(Tick(AtomicU64::new(1000)))).with_sleep_store(store));
    Rig { api: AgentApi::new(vec![group]).unwrap(), host, executors, channel }
}

impl Rig {
    async fn ask(&self, method: Method, params: Value) -> Value {
        self.ask_on(method, params).await.0
    }

    async fn ask_on(&self, method: Method, params: Value) -> (Value, Arc<Pushed>) {
        let pushed = Arc::new(Pushed::default());
        let connection = Connection::new(pushed.clone());
        let answer = self.api.dispatch_on(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string(), &connection).await;
        for stream in connection.take_following() {
            tokio::spawn(stream.job);
        }
        (answer, pushed)
    }

    async fn open(&self, terminal: &str) -> Value {
        let answer = self.ask(api::OPEN, json!({"workspace": "w1", "terminal": terminal, "cols": 100, "rows": 30})).await;
        assert!(answer.get("result").is_some(), "{answer}");
        answer["result"].clone()
    }
}

fn code(answer: &Value) -> Value {
    answer["error"]["code"].clone()
}

#[tokio::test]
async fn given_an_opened_terminal_when_input_is_written_then_the_attached_stream_carries_the_output_numbered_by_byte_offset_and_a_second_attach_from_an_offset_gets_the_rest() {
    // Given a terminal opened with a size, and an attach that asks for the stream from the start
    let rig = rig();
    let opened = rig.open("t1").await;
    assert_eq!((opened["opened"].clone(), opened["cols"].clone(), opened["rows"].clone(), opened["status"].clone(), opened["offset"].clone()), (json!(true), json!(100), json!(30), json!("running"), json!(0)));
    let (attached, first) = rig.ask_on(api::ATTACH, json!({"workspace": "w1", "terminal": "t1"})).await;
    assert_eq!((attached["result"]["start_offset"].clone(), attached["result"]["offset"].clone(), attached["result"]["reset"].clone()), (json!(0), json!(0), json!(false)));
    // When two inputs are written
    assert_eq!(rig.ask(api::WRITE, json!({"workspace": "w1", "terminal": "t1", "data": "ls\n"})).await["result"], json!({"bytes": 3}));
    rig.ask(api::WRITE, json!({"workspace": "w1", "terminal": "t1", "data": "pwd\n"})).await;
    first.until("the echoes", |e| e.last().is_some_and(|e| e.seq == 15)).await;
    // Then the stream is the snapshot (empty) and the outputs whose seq is the offset after their last byte, and the bytes are the executor's
    let events = first.events();
    assert_eq!(events[0].kind, "terminal.snapshot");
    assert_eq!(first.text(), "got:ls\ngot:pwd\n");
    let seqs: Vec<u64> = events.iter().filter(|e| e.kind == "terminal.output").map(|e| e.seq).collect();
    assert_eq!(*seqs.last().unwrap(), 15);
    // And an attach after offset 7 gets only what came after, as output and no snapshot
    let (again, second) = rig.ask_on(api::ATTACH, json!({"workspace": "w1", "terminal": "t1", "after_offset": 7})).await;
    assert_eq!((again["result"]["start_offset"].clone(), again["result"]["reset"].clone()), (json!(7), json!(false)));
    second.until("the tail", |e| e.last().is_some_and(|e| e.seq == 15)).await;
    assert_eq!((second.text(), second.events().iter().any(|e| e.kind == "terminal.snapshot")), ("got:pwd\n".to_string(), false));
    // And a read after offset 7 answers the same rest as one reply
    let read = rig.ask(api::READ, json!({"workspace": "w1", "terminal": "t1", "after_offset": 7})).await;
    assert_eq!((read["result"]["start_offset"].clone(), read["result"]["offset"].clone(), b64::decode(read["result"]["data"].as_str().unwrap()).unwrap()), (json!(7), json!(15), b"got:pwd\n".to_vec()));
}

#[tokio::test]
async fn given_a_stream_that_outgrew_the_kept_tail_when_a_reader_resumes_from_before_it_then_it_is_handed_a_reset_snapshot_of_the_tail_and_one_ahead_of_the_stream_is_refused() {
    // Given a terminal whose pty produced more than the service keeps
    let rig = rig();
    rig.open("t1").await;
    let pty = rig.host.pty_name();
    let big = vec![b'x'; api::RETAIN + 5000];
    rig.host.produce(&pty, &big);
    let end = big.len() as u64;
    let seen = rig.ask(api::LIST, json!({"workspace": "w1"})).await;
    assert_eq!(seen["result"]["terminals"][0]["terminal"], json!("t1"));
    for _ in 0..400 {
        if rig.ask(api::LIST, json!({"workspace": "w1"})).await["result"]["terminals"][0]["offset"] == json!(end) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // When a reader resumes from offset 10 and another from the end
    let (old, behind) = rig.ask_on(api::ATTACH, json!({"workspace": "w1", "terminal": "t1", "after_offset": 10})).await;
    let (_, current) = rig.ask_on(api::ATTACH, json!({"workspace": "w1", "terminal": "t1", "after_offset": end})).await;
    // Then the first is answered reset, with a snapshot that starts at the oldest kept byte and ends at the stream's end, and the second gets none
    assert_eq!((old["result"]["reset"].clone(), old["result"]["start_offset"].clone()), (json!(true), json!(end - api::RETAIN as u64)));
    behind.until("the snapshot", |e| !e.is_empty()).await;
    let snapshot = &behind.events()[0];
    assert_eq!((snapshot.kind.as_str(), snapshot.data["reset"].clone(), snapshot.data["offset"].clone(), snapshot.seq), ("terminal.snapshot", json!(true), json!(end), end));
    assert_eq!(b64::decode(snapshot.data["data"].as_str().unwrap()).unwrap().len(), api::RETAIN);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(current.events().is_empty());
    // And an offset past the end is refused with its code
    assert_eq!(code(&rig.ask(api::ATTACH, json!({"workspace": "w1", "terminal": "t1", "after_offset": end + 1})).await), json!(api::code::OFFSET_AHEAD));
}

#[tokio::test]
async fn given_an_attached_reader_when_the_executors_channel_is_replaced_then_the_pump_attaches_again_from_the_offset_it_reached_and_the_stream_goes_on_without_a_gap_or_a_repeat() {
    // Given a terminal with output, an attached reader and the executor's first channel
    let rig = rig();
    rig.open("t1").await;
    let pty = rig.host.pty_name();
    let (_, reader) = rig.ask_on(api::ATTACH, json!({"workspace": "w1", "terminal": "t1"})).await;
    rig.host.produce(&pty, b"before|");
    reader.until("the first output", |e| e.iter().any(|e| e.seq == 7)).await;
    // When the executor reconnects (a second channel replaces the first, closing it) and the pty produces more
    let second = Connection1::new(&rig.host);
    rig.executors.open_executor("e1", "w1", second.clone()).unwrap();
    rig.channel.close(4001);
    rig.host.produce(&pty, b"after");
    reader.until("the output after the reconnect", |e| e.iter().any(|e| e.seq == 12)).await;
    // Then the reader saw every byte once, in order, and the executor was attached again after offset 7
    assert_eq!(reader.text(), "before|after");
    assert_eq!(rig.host.attaches(), [json!(0), json!(7)]);
    let offsets: Vec<u64> = reader.events().iter().filter(|e| e.kind == "terminal.output").map(|e| e.data["offset"].as_u64().unwrap()).collect();
    assert_eq!(offsets, [0, 7]);
}

#[tokio::test]
async fn given_a_shell_that_exits_when_input_is_written_or_the_terminal_is_closed_then_the_reader_is_told_the_exit_and_the_close_and_input_is_refused() {
    // Given an attached terminal whose shell then exits with 3
    let rig = rig();
    rig.open("t1").await;
    let pty = rig.host.pty_name();
    let (_, reader) = rig.ask_on(api::ATTACH, json!({"workspace": "w1", "terminal": "t1"})).await;
    rig.host.ptys.lock().unwrap().get_mut(&pty).unwrap().exited = Some(3);
    rig.host.produce(&pty, b"bye");
    // Then the reader is told the exit code, input is refused with TERMINAL_EXITED, and the terminal's status says exited
    reader.until("the exit", |e| e.iter().any(|e| e.kind == "terminal.exited")).await;
    let exited = reader.events().into_iter().find(|e| e.kind == "terminal.exited").unwrap();
    assert_eq!((exited.data["exit_code"].clone(), exited.seq), (json!(3), 3));
    assert_eq!(code(&rig.ask(api::WRITE, json!({"workspace": "w1", "terminal": "t1", "data": "x"})).await), json!(api::code::TERMINAL_EXITED));
    assert_eq!(rig.ask(api::LIST, json!({"workspace": "w1"})).await["result"]["terminals"][0]["status"], json!("exited"));
    // When it is closed, Then the reader gets closed and the terminal is gone
    assert_eq!(rig.ask(api::CLOSE, json!({"workspace": "w1", "terminal": "t1"})).await["result"], json!({"workspace": "w1", "terminal": "t1"}));
    reader.until("the close", |e| e.last().is_some_and(|e| e.kind == "terminal.closed")).await;
    assert_eq!(code(&rig.ask(api::WRITE, json!({"workspace": "w1", "terminal": "t1", "data": "x"})).await), json!(api::code::UNKNOWN_TERMINAL));
}

#[tokio::test]
async fn given_a_terminal_when_it_is_resized_renamed_and_cleared_then_the_executor_is_told_the_metadata_stream_carries_each_change_and_the_reader_sees_the_clear() {
    // Given a metadata subscription and a reader
    let rig = rig();
    let (subscribed, meta) = rig.ask_on(api::SUBSCRIBE, json!({"workspace": "w1"})).await;
    assert_eq!((subscribed["result"]["terminals"].clone(), subscribed["result"]["seq"].clone(), subscribed["result"]["reset"].clone()), (json!([]), json!(0), json!(false)));
    rig.open("t1").await;
    let (_, reader) = rig.ask_on(api::ATTACH, json!({"workspace": "w1", "terminal": "t1"})).await;
    // When it is resized, renamed and cleared
    assert_eq!(rig.ask(api::RESIZE, json!({"workspace": "w1", "terminal": "t1", "cols": 120, "rows": 40})).await["result"], json!({"cols": 120, "rows": 40}));
    assert_eq!(rig.ask(api::RENAME, json!({"workspace": "w1", "terminal": "t1", "title": "build"})).await["result"]["title"], json!("build"));
    rig.ask(api::WRITE, json!({"workspace": "w1", "terminal": "t1", "data": "a"})).await;
    reader.until("the echo", |e| e.iter().any(|e| e.kind == "terminal.output")).await;
    rig.ask(api::CLEAR, json!({"workspace": "w1", "terminal": "t1"})).await;
    // Then the executor got the new size, a form feed after the clear, and the reader saw the clear
    assert!(rig.host.asked.lock().unwrap().contains(&("pty_resize".to_string(), json!({"pty": rig.host.pty_name(), "cols": 120, "rows": 40}))));
    assert!(rig.host.asked.lock().unwrap().iter().any(|(t, a)| t == "pty_write" && a["data"] == "\u{c}"));
    reader.until("the clear", |e| e.iter().any(|e| e.kind == "terminal.cleared")).await;
    // And the metadata stream numbered an upsert for each change from 1, the last one with the final size and title
    meta.until("the changes", |e| e.len() >= 3).await;
    let events = meta.events();
    assert!(events.iter().enumerate().all(|(i, e)| e.seq == i as u64 + 1 && e.kind == "terminal.upsert"));
    let last = events.last().unwrap();
    assert_eq!((last.data["info"]["cols"].clone(), last.data["info"]["title"].clone()), (json!(120), json!("build")));
    // And a subscribe after a seq it already holds replays only the later events
    let (_, resumed) = rig.ask_on(api::SUBSCRIBE, json!({"workspace": "w1", "after_seq": 1})).await;
    resumed.until("the replay", |e| !e.is_empty()).await;
    assert_eq!(resumed.events()[0].seq, 2);
}

#[tokio::test]
async fn given_ptys_the_service_does_not_know_when_terminals_are_listed_then_those_of_the_workspace_are_adopted_with_their_tail_refilled_and_those_of_another_are_not() {
    // Given an executor holding a pty of w1's tag with output, and one of another workspace
    let rig = rig();
    rig.open("keep").await;
    let name = rig.host.pty_name();
    rig.host.produce(&name, b"history");
    let tag = name.split('.').next().unwrap().to_string();
    rig.host.ptys.lock().unwrap().insert(format!("{tag}.orphan"), Pty { out: b"old".to_vec(), cols: 90, rows: 20, ..Pty::default() });
    rig.host.ptys.lock().unwrap().insert("ffffffff.other".into(), Pty::default());
    // When the terminals of w1 are listed, and the orphan is attached
    let listed = rig.ask(api::LIST, json!({"workspace": "w1"})).await;
    let (_, reader) = rig.ask_on(api::ATTACH, json!({"workspace": "w1", "terminal": "orphan"})).await;
    // Then both of w1 are known, the other workspace's is not, and the orphan's output is replayed from the executor's tail
    assert_eq!(listed["result"]["terminals"].as_array().unwrap().iter().map(|t| t["terminal"].clone()).collect::<Vec<_>>(), [json!("keep"), json!("orphan")]);
    assert_eq!(listed["result"]["terminals"][1]["cols"], json!(90));
    reader.until("the history", |e| e.iter().any(|e| e.kind == "terminal.output")).await;
    assert_eq!(reader.text(), "old");
    assert_eq!(code(&rig.ask(api::ATTACH, json!({"workspace": "w1", "terminal": "other"})).await), json!(api::code::UNKNOWN_TERMINAL));
}

#[tokio::test]
async fn given_bad_requests_when_asked_then_each_is_refused_with_its_code_before_or_without_a_pty_and_an_open_that_repeats_returns_the_running_terminal() {
    let rig = rig();
    // Given params that are wrong, a workspace nobody attached, and a terminal that does not exist
    assert_eq!(code(&rig.ask(api::OPEN, json!({"workspace": "w1", "terminal": "../x"})).await), json!("INVALID_PARAMS"));
    assert_eq!(code(&rig.ask(api::WRITE, json!({"workspace": "w1", "terminal": "t"})).await), json!("INVALID_PARAMS"));
    assert_eq!(code(&rig.ask(api::OPEN, json!({"workspace": "nowhere", "terminal": "t"})).await), json!("NO_EXECUTOR"));
    assert_eq!(code(&rig.ask(api::RESIZE, json!({"workspace": "w1", "terminal": "ghost", "cols": 80, "rows": 24})).await), json!(api::code::UNKNOWN_TERMINAL));
    assert!(rig.host.asked.lock().unwrap().iter().all(|(t, _)| t == "pty_list"), "{:?}", rig.host.asked.lock().unwrap());
    // When a terminal is opened twice with a cwd that climbs out of a directory workspace and then with a good one
    rig.executors.add_workspace_at("e1", "proj", rca_exec_channel::Place { prefix: Some("apps/web".into()), worktree: None }).unwrap();
    assert_eq!(code(&rig.ask(api::OPEN, json!({"workspace": "proj", "terminal": "t", "cwd": "../.."})).await), json!("INVALID_PARAMS"));
    let first = rig.ask(api::OPEN, json!({"workspace": "proj", "terminal": "t", "cwd": "src"})).await;
    let again = rig.ask(api::OPEN, json!({"workspace": "proj", "terminal": "t"})).await;
    // Then the cwd was placed under the directory, and the repeat is the same terminal, not opened a second time
    assert_eq!(first["result"]["cwd"], json!("apps/web/src"));
    assert_eq!((again["result"]["opened"].clone(), again["result"]["pid"].clone()), (json!(false), json!(4242)));
    assert_eq!(rig.host.asked.lock().unwrap().iter().filter(|(t, _)| t == "pty_open").count(), 1);
}

#[test]
fn given_the_rows_of_the_group_when_compared_with_the_contract_then_each_method_has_one_in_the_contracts_order_and_every_action_is_a_pty_action() {
    // Given the rows
    // Then there is one for each method of the contract, in its order, and each names pty actions only
    assert_eq!(crate::SERVED.iter().map(|s| s.method).collect::<Vec<_>>(), api::METHODS);
    let pty: Vec<&str> = exec_pty::ACTIONS.iter().map(|a| a.name).collect();
    assert!(crate::SERVED.iter().flat_map(|s| s.actions).all(|a| pty.contains(a)));
}

fn asked(host: &Host, tool: &str) -> Vec<Value> {
    host.asked.lock().unwrap().iter().filter(|(t, _)| t == tool).map(|(_, a)| a.clone()).collect()
}

#[tokio::test]
async fn given_two_shells_when_the_workspace_sleeps_then_both_are_closed_at_the_executor_and_kept_with_their_title_and_size_and_a_second_sleep_keeps_the_new_one_too() {
    // Given two running shells, one renamed
    let rig = rig();
    rig.open("t1").await;
    rig.open("t2").await;
    rig.ask(api::RENAME, json!({"workspace": "w1", "terminal": "t2", "title": "server"})).await;
    // When the workspace sleeps
    let slept = rig.ask(api::SLEEP, json!({"workspace": "w1"})).await;
    // Then both shells are closed at the executor and kept by name with title, cwd and size, and the call says it was the first sleep
    assert_eq!(slept["result"], json!({"workspace": "w1", "already": false, "slept": [
        {"terminal": "t1", "title": "t1", "cwd": "/repo", "cols": 100, "rows": 30},
        {"terminal": "t2", "title": "server", "cwd": "/repo", "cols": 100, "rows": 30}]}));
    assert!(rig.host.ptys.lock().unwrap().is_empty());
    assert_eq!(rig.ask(api::LIST, json!({"workspace": "w1"})).await["result"]["terminals"], json!([]));
    let sleeping = rig.ask(api::SLEEPING, json!({})).await;
    assert_eq!((sleeping["result"]["sleeping"][0]["workspace"].clone(), sleeping["result"]["sleeping"][0]["slept_at_ms"].is_u64(), sleeping["result"]["sleeping"][0]["terminals"].as_array().map(Vec::len)), (json!("w1"), true, Some(2)));
    // And a shell opened in the sleeping workspace is closed by the next sleep and kept with the others
    rig.open("t3").await;
    let again = rig.ask(api::SLEEP, json!({"workspace": "w1"})).await;
    assert_eq!((again["result"]["already"].clone(), again["result"]["slept"][0]["terminal"].clone()), (json!(true), json!("t3")));
    assert_eq!(rig.ask(api::SLEEPING, json!({"workspace": "w1"})).await["result"]["sleeping"][0]["terminals"].as_array().map(Vec::len), Some(3));
}

#[tokio::test]
async fn given_a_sleeping_workspace_when_it_wakes_then_each_shell_starts_again_with_its_name_title_and_size_and_nothing_stays_kept() {
    // Given a renamed shell that slept
    let rig = rig();
    rig.open("t1").await;
    rig.ask(api::RENAME, json!({"workspace": "w1", "terminal": "t1", "title": "build"})).await;
    rig.ask(api::SLEEP, json!({"workspace": "w1"})).await;
    // When the workspace wakes
    let woke = rig.ask(api::WAKE, json!({"workspace": "w1"})).await;
    // Then the executor was asked to open the shell again at its size, the terminal is running with its title, and the sleep is gone
    let woken = &woke["result"]["woken"][0];
    assert_eq!((woken["terminal"].clone(), woken["title"].clone(), woken["cols"].clone(), woken["rows"].clone(), woken["status"].clone()), (json!("t1"), json!("build"), json!(100), json!(30), json!("running")));
    assert_eq!(woke["result"]["failed"], json!([]));
    let reopened = asked(&rig.host, "pty_open");
    assert_eq!((reopened.len(), reopened[1]["cols"].clone(), reopened[1]["rows"].clone(), reopened[1]["cwd"].clone()), (2, json!(100), json!(30), json!("/repo")));
    assert_eq!(rig.ask(api::LIST, json!({"workspace": "w1"})).await["result"]["terminals"][0]["title"], json!("build"));
    assert_eq!(rig.ask(api::SLEEPING, json!({})).await["result"], json!({"sleeping": []}));
    // And a second wake finds nothing to start
    assert_eq!(rig.ask(api::WAKE, json!({"workspace": "w1"})).await["result"], json!({"workspace": "w1", "woken": [], "failed": []}));
}

#[tokio::test]
async fn given_a_sleep_kept_in_a_store_when_the_service_restarts_then_the_new_service_lists_it_and_wakes_the_shell() {
    // Given a shell that slept in a service, and a store that outlives it
    let store: Arc<dyn SleepStore> = Arc::new(MemorySleep::default());
    let host = Arc::new(Host::default());
    let before = rig_keeping(store.clone(), host.clone());
    before.open("t1").await;
    before.ask(api::SLEEP, json!({"workspace": "w1"})).await;
    drop(before);
    // When a new service starts over the same store and executor
    let after = rig_keeping(store, host);
    // Then it lists the sleep and wakes the shell
    assert_eq!(after.ask(api::SLEEPING, json!({})).await["result"]["sleeping"][0]["terminals"][0]["terminal"], json!("t1"));
    let woke = after.ask(api::WAKE, json!({"workspace": "w1"})).await;
    assert_eq!(woke["result"]["woken"][0]["status"], json!("running"));
    assert_eq!(after.ask(api::SLEEPING, json!({})).await["result"], json!({"sleeping": []}));
}

#[tokio::test]
async fn given_a_shell_whose_pty_is_already_gone_when_the_workspace_sleeps_then_it_is_kept_all_the_same_and_a_wake_with_no_executor_leaves_the_sleep_kept() {
    // Given a terminal whose pty the executor no longer has
    let rig = rig();
    rig.open("t1").await;
    rig.host.ptys.lock().unwrap().clear();
    // When the workspace sleeps, Then the shell is kept for the wake
    let slept = rig.ask(api::SLEEP, json!({"workspace": "w1"})).await;
    assert_eq!(slept["result"]["slept"][0]["terminal"], json!("t1"));
    // And when the executor is gone, a wake is refused and the sleep stays kept
    rig.channel.close(4001);
    let refused = rig.ask(api::WAKE, json!({"workspace": "w1"})).await;
    assert_eq!(code(&refused), json!("NO_EXECUTOR"));
    assert_eq!(rig.ask(api::SLEEPING, json!({})).await["result"]["sleeping"][0]["terminals"][0]["terminal"], json!("t1"));
}

#[tokio::test]
async fn given_bad_params_when_a_workspace_sleeps_or_wakes_then_each_is_refused_by_name_and_a_workspace_without_an_executor_cannot_sleep() {
    let rig = rig();
    assert_eq!(rig.ask(api::SLEEP, json!({})).await["error"]["message"], json!("workspace is required"));
    assert_eq!(rig.ask(api::WAKE, json!({})).await["error"]["message"], json!("workspace is required"));
    assert_eq!(rig.ask(api::SLEEPING, json!({"workspace": 3})).await["error"]["message"], json!("workspace must be a non-empty string"));
    assert_eq!(code(&rig.ask(api::SLEEP, json!({"workspace": "nowhere"})).await), json!("NO_EXECUTOR"));
    assert_eq!(rig.ask(api::SLEEPING, json!({})).await["result"], json!({"sleeping": []}));
}

#[tokio::test]
async fn given_a_workspace_with_no_shells_and_no_record_when_it_sleeps_then_nothing_is_kept_and_the_workspace_is_not_listed_as_sleeping() {
    // Given a workspace with no terminals and a store with no record
    let store = Arc::new(MemorySleep::default());
    let rig = rig_keeping(store.clone(), Arc::new(Host::default()));
    // When it sleeps
    let slept = rig.ask(api::SLEEP, json!({"workspace": "w1"})).await;
    // Then it closed nothing, says it was the first sleep, and no record was written
    assert_eq!(slept["result"], json!({"workspace": "w1", "already": false, "slept": []}));
    assert_eq!(store.load().unwrap(), Vec::new());
    assert_eq!(rig.ask(api::SLEEPING, json!({})).await["result"], json!({"sleeping": []}));
}

#[tokio::test]
async fn given_sleeping_records_whose_cwd_lies_outside_the_workspace_or_climbs_out_of_it_when_it_wakes_then_each_shell_starts_in_the_workspace_and_one_inside_keeps_its_cwd() {
    // Given a workspace placed under /base/proj and a store holding three shells that slept with an outside cwd, a cwd with a dotdot and an inside one
    let store = Arc::new(MemorySleep::default());
    let shell = |terminal: &str, cwd: &str| api::SleptTerminal { terminal: terminal.into(), title: terminal.into(), cwd: cwd.into(), cols: 100, rows: 30 };
    store.save(&[api::SleepingWorkspace { workspace: "proj".into(), slept_at_ms: 1, terminals: vec![shell("t1", "/etc"), shell("t2", "/base/proj/../secret"), shell("t3", "/base/proj/src")] }]).unwrap();
    let rig = rig_keeping(store.clone(), Arc::new(Host::default()));
    rig.executors.add_workspace_at("e1", "proj", rca_exec_channel::Place { prefix: Some("/base/proj".into()), worktree: None }).unwrap();
    // When the workspace wakes
    let woke = rig.ask(api::WAKE, json!({"workspace": "proj"})).await;
    // Then all three started, the outside and climbing cwds fell back to the workspace's place, and the inside one stayed
    assert_eq!((woke["result"]["woken"].as_array().map(Vec::len), woke["result"]["failed"].clone()), (Some(3), json!([])));
    let cwds: Vec<Value> = asked(&rig.host, "pty_open").iter().map(|a| a["cwd"].clone()).collect();
    assert_eq!(cwds, [json!("/base/proj"), json!("/base/proj"), json!("/base/proj/src")]);
    assert_eq!(store.load().unwrap(), Vec::new());
}
