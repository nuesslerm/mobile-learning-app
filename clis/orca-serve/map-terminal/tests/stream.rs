//! `terminal.subscribe` through the `Host` dispatcher with the terminal double: refusals, the lease and exited answers, and the JSON stream.
//! Given/When/Then in each test.
use orca_core::area::Out;
use orca_map_terminal::binary::{self, op};
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Connection, Host, HostInfo, ServiceStatus};
use orca_map_terminal::testing::Terminals;
use orca_map_terminal::TerminalArea;
use orca_map_workspace::testing::Workspaces;
use serde_json::{json, Value};
use std::sync::Arc;

struct Rig {
    agent: Arc<ScriptedAgent>,
    terminals: Terminals,
    conn: Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let files = Workspaces::default();
        files.workspace("main", "root", Some("/srv/main"));
        files.install(&agent);
        let terminals = Terminals::default();
        terminals.terminal("main", "t1", true).terminal("main", "gone", false);
        terminals.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(TerminalArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { agent, terminals, conn: host.connection("dev", "tok", vec![]), n: 0 }
    }

    fn frames(&mut self, method: &str, params: Value) -> Vec<Value> {
        self.n += 1;
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        self.conn.on_message(&frame).iter().map(|f| serde_json::from_str(f).unwrap()).collect()
    }

    /// The events the open streams queued since the last call, as the phone reads them.
    fn events(&mut self) -> Vec<Value> {
        self.conn.poll().iter().map(|f| serde_json::from_str::<Value>(f).unwrap()["result"].clone()).collect()
    }

    fn refused(&mut self, params: Value) -> (String, String) {
        let frames = self.frames("terminal.subscribe", params);
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert_eq!(frames[0]["ok"], false, "{frames:?}");
        (frames[0]["error"]["code"].as_str().unwrap().to_string(), frames[0]["error"]["message"].as_str().unwrap().to_string())
    }

    fn subscribe(&mut self, params: Value) -> Vec<Value> {
        self.frames("terminal.subscribe", params).iter().map(|f| f["result"].clone()).collect()
    }
}

fn desktop() -> Value {
    json!({"terminal": "main:t1", "client": {"id": "d1"}})
}

#[test]
fn given_a_handle_rca_does_not_hold_when_subscribed_then_it_is_stale() {
    let mut rig = Rig::new();
    // When the phone subscribes to a handle no terminal answers to, or to one that is not a handle
    // Then each is a stale handle, as the recorded host answers
    for handle in ["main:nope", "a"] {
        assert_eq!(rig.refused(json!({"terminal": handle})).0, "terminal_handle_stale", "{handle}");
    }
}

#[test]
fn given_params_the_schema_rejects_when_subscribed_then_each_is_refused_in_the_words_of_the_host() {
    let mut rig = Rig::new();
    let cases = [
        (json!({}), "Missing terminal handle"),
        (json!({"terminal": "main:t1", "client": {}}), "Missing client ID"),
        (json!({"terminal": "main:t1", "client": {"id": "d", "type": "tablet"}}), "Invalid option: expected one of \"mobile\"|\"desktop\""),
        (json!({"terminal": "main:t1", "capabilities": {"outputAck": 2}}), "Invalid input: expected 1"),
        (json!({"terminal": "main:t1", "resume": {"units": 3}}), "Missing stream token"),
        (json!({"terminal": "main:t1", "resume": {"streamToken": "t", "units": -1}}), "Too small: expected number to be >=0"),
        (json!({"terminal": "main:t1", "snapshotByteBudget": 0}), "Too small: expected number to be >0"),
        (json!({"terminal": "main:t1", "viewport": {"cols": 0, "rows": 5}}), "Too small: expected number to be >=1"),
    ];
    for (params, message) in cases {
        // When subscribed, Then it is refused with the schema's words and nothing was asked of rca
        assert_eq!(rig.refused(params.clone()), ("invalid_argument".to_string(), message.to_string()), "{params}");
    }
    assert!(rig.agent.follows().is_empty());
}

#[test]
fn given_a_mobile_client_without_the_binary_stream_when_subscribed_then_it_is_refused_before_anything_is_held() {
    let mut rig = Rig::new();
    // When a mobile client does not offer the binary stream
    let refused = rig.refused(json!({"terminal": "main:t1", "client": {"id": "p", "type": "mobile"}}));
    // Then the host's refusal comes back and rca was never attached
    assert_eq!(refused, ("runtime_error".to_string(), "binary_terminal_stream_required".to_string()));
    assert!(rig.agent.follows().is_empty());
}

#[test]
fn given_a_mobile_client_that_only_wants_the_input_lease_when_subscribed_then_a_null_stream_is_answered_and_it_stays_silent() {
    let mut rig = Rig::new();
    rig.terminals.output("main", "t1", b"hello\n");
    let params = json!({"terminal": "main:t1", "client": {"id": "p", "type": "mobile"}, "capabilities": {"terminalBinaryStream": 1, "mobileInputLeaseOnly": 1}});
    // When it subscribes
    let opened = rig.subscribe(params.clone());
    // Then it gets the empty null-stream answer and rca's output is not followed
    assert_eq!(opened, [json!({"type": "subscribed", "streamId": null, "lines": [], "truncated": false})]);
    assert!(rig.agent.follows().is_empty());
    // When output is printed and time passes, Then the stream stays silent
    rig.terminals.push_output("main", "t1", b"more\n");
    assert_eq!(rig.events(), Vec::<Value>::new());
}

#[test]
fn given_an_exited_terminal_when_subscribed_then_one_answer_carries_the_tail_and_the_stream_is_over() {
    let mut rig = Rig::new();
    rig.terminals.output("main", "gone", b"\x1b[31mbye\x1b[0m\nlast\n");
    // When the phone subscribes to the exited terminal
    let frames = rig.subscribe(json!({"terminal": "main:gone", "client": {"id": "d1"}}));
    // Then it gets the tail as plain lines, and the stream ends without ever attaching
    assert_eq!(frames, [json!({"type": "subscribed", "streamId": null, "lines": ["bye", "last"], "truncated": false}), json!({"type": "end"})]);
    assert!(rig.agent.follows().is_empty());
    assert_eq!(rig.events(), Vec::<Value>::new());
}

#[test]
fn given_a_running_terminal_when_a_desktop_client_subscribes_then_scrollback_replays_the_tail_and_data_follows_byte_exact_until_the_exit_ends_it() {
    let mut rig = Rig::new();
    rig.terminals.output("main", "t1", b"one\r\n\x1b[6ntwo\r\n");
    // When it subscribes
    let opened = rig.subscribe(desktop());
    // Then the scrollback has the lines and the replay with the query stripped, and the double saw one attach from the read's offset
    assert_eq!(opened, [json!({"type": "scrollback", "lines": ["one", "two"], "truncated": false, "serialized": "one\r\ntwo\r\n", "cols": 80, "rows": 24, "displayMode": "auto"})]);
    let follows = rig.agent.follows();
    assert_eq!(follows, [("terminal.attach".to_string(), json!({"workspace": "main", "terminal": "t1", "after_offset": 14}))]);
    // When an emoji arrives split across two pushes
    let bytes = "a😀b".as_bytes();
    rig.terminals.push_output("main", "t1", &bytes[..3]);
    rig.terminals.push_output("main", "t1", &bytes[3..]);
    // Then it appears once, whole, and nothing before the read is repeated
    let chunks: Vec<Value> = rig.events();
    assert_eq!(chunks, [json!({"type": "data", "chunk": "a"}), json!({"type": "data", "chunk": "😀b"})]);
    // When the shell exits, Then the stream ends once
    rig.terminals.exit("main", "t1", 0);
    assert_eq!(rig.events(), [json!({"type": "end"})]);
    assert_eq!(rig.events(), Vec::<Value>::new());
}

#[test]
fn given_a_tail_over_the_snapshot_budget_when_subscribed_then_the_replay_is_cut_after_a_newline_and_marked_truncated() {
    let mut rig = Rig::new();
    rig.terminals.output("main", "t1", b"aaaa\nbbbb\ncccc\n");
    // When the phone asks for a budget smaller than the tail
    let opened = rig.subscribe(json!({"terminal": "main:t1", "snapshotByteBudget": 8}));
    // Then only the newest whole lines are replayed
    assert_eq!(opened[0]["serialized"], "cccc\n");
    assert_eq!(opened[0]["truncated"], true);
}

#[test]
fn given_a_stream_when_the_same_client_subscribes_again_then_the_first_ends_and_only_the_second_gets_output() {
    let mut rig = Rig::new();
    // Given a client streaming a terminal
    rig.subscribe(desktop());
    // When it subscribes again with the same client id
    let second = rig.frames("terminal.subscribe", desktop());
    // Then the older stream ends and the new one opens with its scrollback
    let results: Vec<Value> = second.iter().map(|f| f["result"].clone()).collect();
    assert_eq!(results[0]["type"], "end");
    assert_eq!(second[0]["id"], "r1");
    assert_eq!(results[1]["type"], "scrollback");
    assert_eq!(second[1]["id"], "r2");
    // When output is printed, Then it arrives on the new request only
    rig.terminals.push_output("main", "t1", b"x");
    let frames: Vec<Value> = rig.conn.poll().iter().map(|f| serde_json::from_str(f).unwrap()).collect();
    assert_eq!(frames.len(), 1);
    assert_eq!((frames[0]["id"].clone(), frames[0]["result"].clone()), (json!("r2"), json!({"type": "data", "chunk": "x"})));
}

#[test]
fn given_two_clients_when_each_subscribes_then_neither_replaces_the_other() {
    let mut rig = Rig::new();
    // When two clients stream the same terminal
    rig.subscribe(desktop());
    let second = rig.subscribe(json!({"terminal": "main:t1", "client": {"id": "d2"}}));
    // Then the second just opens
    assert_eq!(second.len(), 1);
    rig.terminals.push_output("main", "t1", b"y");
    assert_eq!(rig.events(), [json!({"type": "data", "chunk": "y"}), json!({"type": "data", "chunk": "y"})]);
}

/// One outbound frame as the phone reads it: an event's JSON, or a binary frame decoded with the codec.
#[derive(Debug, PartialEq)]
enum Got {
    Json(Value),
    Bin(u8, u32, u64, Vec<u8>),
}

fn read(frames: Vec<Out>) -> Vec<Got> {
    frames
        .into_iter()
        .map(|f| match f {
            Out::Text(t) => Got::Json(serde_json::from_str::<Value>(&t).unwrap()["result"].clone()),
            Out::Binary(b) => {
                let frame = binary::decode(&b).expect("a terminal frame");
                Got::Bin(frame.opcode, frame.stream, frame.seq, frame.payload.to_vec())
            }
        })
        .collect()
}

fn phone() -> Value {
    json!({"terminal": "main:t1", "client": {"id": "p", "type": "mobile"}, "capabilities": {"terminalBinaryStream": 1, "outputAck": 1, "writeUnavailable": 1}})
}

impl Rig {
    fn open_binary(&mut self, params: Value) -> Vec<Got> {
        self.n += 1;
        let frame = json!({"id": format!("r{}", self.n), "method": "terminal.subscribe", "params": params}).to_string();
        read(self.conn.on_message_out(&frame))
    }

    fn polled(&mut self) -> Vec<Got> {
        read(self.conn.poll_out())
    }

    fn sent(&mut self, opcode: u8, stream: u32, payload: &[u8]) -> Vec<Got> {
        read(self.conn.on_binary(&binary::encode(opcode, stream, 0, payload)))
    }
}

fn number(opened: &[Got]) -> u32 {
    match &opened[0] {
        Got::Json(v) => v["streamId"].as_u64().unwrap() as u32,
        other => panic!("{other:?}"),
    }
}

fn text(payload: &[u8]) -> &str {
    std::str::from_utf8(payload).unwrap()
}

#[test]
fn given_a_running_terminal_when_the_phone_subscribes_then_subscribed_then_the_snapshot_frames_arrive_in_order_without_queries() {
    let mut rig = Rig::new();
    rig.terminals.output("main", "t1", b"one\r\n\x1b[6ntwo\r\n");
    // When the phone subscribes with the binary stream and output credit
    let opened = rig.open_binary(phone());
    // Then the JSON answer names the stream and its token, and the replay follows as start, chunk, end with the byte offset as seq
    let Got::Json(first) = &opened[0] else { panic!("{opened:?}") };
    let id = number(&opened);
    assert_eq!((first["type"].clone(), first["truncated"].clone(), first["outputAck"].clone(), first["displayMode"].clone()), (json!("subscribed"), json!(false), json!(true), json!("auto")));
    assert_eq!((first["cols"].clone(), first["rows"].clone()), (json!(80), json!(24)));
    assert!(first["streamToken"].as_str().is_some_and(|t| !t.is_empty()));
    assert_eq!(opened.len(), 4);
    let Got::Bin(opcode, stream, seq, meta) = &opened[1] else { panic!("{opened:?}") };
    assert_eq!((*opcode, *stream, *seq), (op::SNAPSHOT_START, id, 14));
    let meta: Value = serde_json::from_slice(meta).unwrap();
    assert_eq!(meta, json!({"kind": "scrollback", "cols": 80, "rows": 24, "displayMode": "auto", "seq": 14, "truncated": false, "truncatedByByteBudget": false, "source": "headless"}));
    assert_eq!(opened[2], Got::Bin(op::SNAPSHOT_CHUNK, id, 14, b"one\r\ntwo\r\n".to_vec()));
    assert_eq!(opened[3], Got::Bin(op::SNAPSHOT_END, id, 14, Vec::new()));
}

#[test]
fn given_a_snapshot_of_20_kib_when_subscribed_then_it_splits_into_two_chunks_that_join_back() {
    let mut rig = Rig::new();
    let tail = "x".repeat(20 * 1024);
    rig.terminals.output("main", "t1", tail.as_bytes());
    // When the phone subscribes
    let opened = rig.open_binary(phone());
    // Then the replay is two chunks of at most 16 KiB holding all of it
    let chunks: Vec<&Vec<u8>> = opened.iter().filter_map(|g| match g { Got::Bin(op::SNAPSHOT_CHUNK, _, _, p) => Some(p), _ => None }).collect();
    assert_eq!(chunks.iter().map(|c| c.len()).collect::<Vec<_>>(), [16 * 1024, 4 * 1024]);
}

#[test]
fn given_a_snapshot_stream_when_output_is_pushed_then_output_frames_carry_the_offset_after_them() {
    let mut rig = Rig::new();
    rig.terminals.output("main", "t1", b"abc");
    let opened = rig.open_binary(phone());
    let id = number(&opened);
    // When the terminal prints, including an emoji split across two pushes
    rig.terminals.push_output("main", "t1", b"de");
    let bytes = "a😀".as_bytes();
    rig.terminals.push_output("main", "t1", &bytes[..3]);
    rig.terminals.push_output("main", "t1", &bytes[3..]);
    // Then each Output frame is the text with seq = the byte offset after it, and the split character appears once
    assert_eq!(rig.polled(), [Got::Bin(op::OUTPUT, id, 5, b"de".to_vec()), Got::Bin(op::OUTPUT, id, 6, b"a".to_vec()), Got::Bin(op::OUTPUT, id, 10, "😀".as_bytes().to_vec())]);
}

#[test]
fn given_a_snapshot_budget_smaller_than_the_tail_when_subscribed_then_the_start_says_truncated_by_byte_budget() {
    let mut rig = Rig::new();
    rig.terminals.output("main", "t1", b"aaaa\nbbbb\ncccc\n");
    // When the phone asks for a budget of 8 bytes
    let mut params = phone();
    params["snapshotByteBudget"] = json!(8);
    let opened = rig.open_binary(params);
    // Then only the newest whole line is sent and the meta says the budget cut it
    let Got::Bin(_, _, _, meta) = &opened[1] else { panic!("{opened:?}") };
    let meta: Value = serde_json::from_slice(meta).unwrap();
    assert_eq!((meta["truncatedByByteBudget"].clone(), meta["truncated"].clone()), (json!(true), json!(false)));
    let Got::Bin(_, _, _, chunk) = &opened[2] else { panic!("{opened:?}") };
    assert_eq!(text(chunk), "cccc\n");
}

#[test]
fn given_a_tail_that_lost_its_start_when_subscribed_then_the_snapshot_is_truncated() {
    let mut rig = Rig::new();
    rig.terminals.with_retain(4).output("main", "t1", b"abcdefgh");
    // When the phone subscribes, Then the meta says the earlier output is gone
    let opened = rig.open_binary(phone());
    let Got::Bin(_, _, _, meta) = &opened[1] else { panic!("{opened:?}") };
    assert_eq!(serde_json::from_slice::<Value>(meta).unwrap()["truncated"], true);
}

#[test]
fn given_a_binary_stream_when_an_input_frame_arrives_then_the_pty_gets_the_bytes() {
    let mut rig = Rig::new();
    let id = number(&rig.open_binary(phone()));
    // When the phone sends keystrokes, with a control byte and multibyte text
    let replies = rig.sent(op::INPUT, id, "ls é\r\x03".as_bytes());
    // Then they are written to the terminal byte for byte and nothing is answered
    assert_eq!(rig.terminals.written(), [("main".to_string(), "t1".to_string(), "ls é\r\x03".to_string())]);
    assert_eq!(replies, []);
}

#[test]
fn given_an_exited_terminal_when_input_arrives_then_write_unavailable_is_answered_only_if_negotiated() {
    let mut rig = Rig::new();
    let id = number(&rig.open_binary(phone()));
    rig.terminals.exit("main", "t1", 0);
    // When the phone types after the exit, Then it is told writes are unavailable, and the stream's end rides along
    assert_eq!(rig.sent(op::INPUT, id, b"x"), [Got::Bin(op::WRITE_UNAVAILABLE, id, 0, Vec::new()), Got::Json(json!({"type": "end"}))]);
    // Given a second phone that did not negotiate it
    let mut other = Rig::new();
    let mut params = phone();
    params["capabilities"] = json!({"terminalBinaryStream": 1});
    let id = number(&other.open_binary(params));
    other.terminals.exit("main", "t1", 0);
    // Then the same input is dropped without a WriteUnavailable frame
    assert_eq!(other.sent(op::INPUT, id, b"x"), [Got::Json(json!({"type": "end"}))]);
}

#[test]
fn given_a_binary_stream_when_a_resize_frame_arrives_then_the_pty_is_resized_and_resized_is_answered() {
    let mut rig = Rig::new();
    let id = number(&rig.open_binary(phone()));
    // When the phone fits the terminal to its screen
    let replies = rig.sent(op::RESIZE, id, br#"{"cols":100,"rows":30}"#);
    // Then rca resizes and a Resized frame echoes the size
    assert_eq!(rig.terminals.resized(), [("t1".to_string(), 100, 30)]);
    assert_eq!(replies.len(), 1);
    let Got::Bin(opcode, stream, _, body) = &replies[0] else { panic!("{replies:?}") };
    assert_eq!((*opcode, *stream), (op::RESIZED, id));
    let body: Value = serde_json::from_slice(body).unwrap();
    assert_eq!((body["cols"].clone(), body["rows"].clone(), body["displayMode"].clone()), (json!(100), json!(30), json!("auto")));
}

#[test]
fn given_a_binary_stream_when_the_terminal_exits_then_the_last_output_comes_first_and_then_end() {
    let mut rig = Rig::new();
    let id = number(&rig.open_binary(phone()));
    // When the shell prints and exits before the stream is read
    rig.terminals.push_output("main", "t1", b"bye\n");
    rig.terminals.exit("main", "t1", 0);
    // Then the output is flushed, then end, once
    assert_eq!(rig.polled(), [Got::Bin(op::OUTPUT, id, 4, b"bye\n".to_vec()), Got::Json(json!({"type": "end"}))]);
    assert_eq!(rig.polled(), []);
}

#[test]
fn given_a_binary_stream_when_the_buffer_is_cleared_then_an_empty_snapshot_follows_and_when_unsubscribed_it_ends() {
    let mut rig = Rig::new();
    rig.terminals.output("main", "t1", b"old\n");
    let id = number(&rig.open_binary(phone()));
    // When the terminal's buffer is cleared
    rig.terminals.clear("main", "t1");
    // Then a fresh snapshot with no chunks arrives at the offset it was cleared at
    let got = rig.polled();
    assert_eq!(got.len(), 2);
    assert_eq!(got[1], Got::Bin(op::SNAPSHOT_END, id, 4, Vec::new()));
    // When the phone unsubscribes, Then the stream ends and later output is not sent
    assert_eq!(rig.sent(op::UNSUBSCRIBE, id, b""), [Got::Json(json!({"type": "end"}))]);
    rig.terminals.push_output("main", "t1", b"more");
    assert_eq!(rig.polled(), []);
}
