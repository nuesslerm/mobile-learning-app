//! The terminal rows through the `Host` dispatcher with an in-memory terminal set as rca: each reply is read the way the phone reads it.
//! Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{AgentApi, Connection, Host, HostInfo, ServiceStatus};
use orca_map_terminal::testing::Terminals;
use orca_map_terminal::TerminalArea;
use orca_map_workspace::testing::Workspaces;
use serde_json::{json, Value};
use std::sync::Arc;

struct Rig {
    terminals: Terminals,
    conn: Connection,
    n: u32,
}

impl Rig {
    fn new() -> Rig {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let files = Workspaces::default();
        files.workspace("main", "root", Some("/srv/main")).workspace("feat", "worktree", Some("/srv/wt/feat"));
        files.install(&agent);
        let terminals = Terminals::default();
        terminals.terminal("main", "t1", true).terminal("feat", "t2", true).terminal("feat", "gone", false);
        terminals.install(&agent);
        let host = Host::new("rt".into(), vec![Box::new(TerminalArea)], agent.clone(), Arc::new(RevocablePrincipals::default()), HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() }).unwrap();
        Rig { terminals, conn: host.connection("dev", "tok", vec![]), n: 0 }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.n += 1;
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        serde_json::from_str(&self.conn.on_message(&frame)[0]).unwrap()
    }

    /// Every frame a request brings back, the reply and then what the open streams queued.
    fn frames(&mut self, method: &str, params: Value) -> Vec<Value> {
        self.n += 1;
        let frame = json!({"id": format!("r{}", self.n), "method": method, "params": params}).to_string();
        self.conn.on_message(&frame).iter().map(|f| serde_json::from_str(f).unwrap()).collect()
    }

    /// The events the open streams queued after their poll interval, as the phone reads them.
    fn stream_events(&mut self) -> Vec<Value> {
        std::thread::sleep(std::time::Duration::from_millis(300));
        self.conn.poll().iter().map(|f| serde_json::from_str::<Value>(f).unwrap()["result"].clone()).collect()
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let r = self.call(method, params);
        assert_eq!(r["ok"], true, "{method}: {r}");
        r["result"].clone()
    }

    fn err(&mut self, method: &str, params: Value) -> String {
        let r = self.call(method, params);
        assert_eq!(r["ok"], false, "{method} should be refused: {r}");
        r["error"]["code"].as_str().unwrap().to_string()
    }
}

fn handles(listed: &Value) -> Vec<String> {
    listed["terminals"].as_array().unwrap().iter().map(|t| t["handle"].as_str().unwrap().to_string()).collect()
}

#[test]
fn given_terminals_in_two_workspaces_when_listed_then_each_row_names_its_worktree_and_a_selector_narrows_the_list() {
    let mut rig = Rig::new();
    // When the phone lists every terminal
    let all = rig.ok("terminal.list", json!({}));
    // Then each is a handle of its workspace, and an exited one is neither connected nor writable
    assert_eq!(handles(&all), ["feat:gone", "feat:t2", "main:t1"]);
    let gone = &all["terminals"][0];
    assert_eq!((gone["connected"].clone(), gone["writable"].clone(), gone["worktreePath"].clone()), (json!(false), json!(false), json!("/srv/wt/feat")));
    // When it lists one worktree, by its path
    let feat = rig.ok("terminal.list", json!({"worktree": "path:/srv/wt/feat"}));
    // Then only that worktree's terminals come back, and a limit says it truncated
    assert_eq!(handles(&feat), ["feat:gone", "feat:t2"]);
    let limited = rig.ok("terminal.list", json!({"limit": 1}));
    assert_eq!((limited["terminals"].as_array().unwrap().len(), limited["totalCount"].clone(), limited["truncated"].clone()), (1, json!(3), json!(true)));
    // And a worktree no workspace owns is refused as the host refuses a selector
    assert_eq!(rig.err("terminal.list", json!({"worktree": "path:/nowhere"})), "selector_not_found");
}

#[test]
fn given_a_running_terminal_when_text_enter_and_interrupt_are_sent_then_the_shell_is_written_the_bytes_the_host_would_write() {
    let mut rig = Rig::new();
    // When text with enter is sent
    let sent = rig.ok("terminal.send", json!({"terminal": "main:t1", "text": "ls", "enter": true}));
    // Then the phone reads it as accepted and the terminal got the line with a carriage return
    assert_eq!((sent["send"]["accepted"].clone(), sent["send"]["bytesWritten"].clone()), (json!(true), json!(3)));
    // When an interrupt is sent
    rig.ok("terminal.send", json!({"terminal": "main:t1", "interrupt": true}));
    // Then ctrl-c reaches the terminal
    assert_eq!(rig.terminals.written().into_iter().map(|(_, _, d)| d).collect::<Vec<_>>(), ["ls\r", "\u{3}"]);
    // And an empty send, an exited terminal and an unknown handle are each refused with the host's code
    assert_eq!(rig.err("terminal.send", json!({"terminal": "main:t1"})), "invalid_argument");
    assert_eq!(rig.err("terminal.send", json!({"terminal": "feat:gone", "text": "x"})), "terminal_not_writable");
    assert_eq!(rig.err("terminal.send", json!({"terminal": "main:nope", "text": "x"})), "terminal_handle_stale");
    assert_eq!(rig.err("terminal.send", json!({"terminal": "garbage", "text": "x"})), "terminal_handle_stale");
}

#[test]
fn given_a_terminal_when_cleared_renamed_and_closed_then_rca_did_each_and_the_phone_reads_the_host_replies() {
    let mut rig = Rig::new();
    // When the buffer is cleared
    let cleared = rig.ok("terminal.clearBuffer", json!({"terminal": "main:t1"}));
    // Then the phone reads a confirmed clear and rca cleared that terminal
    assert_eq!(cleared["clear"]["cleared"], json!(true));
    assert_eq!(rig.terminals.cleared(), ["t1"]);
    // When it is renamed, then reset with null
    let renamed = rig.ok("terminal.rename", json!({"terminal": "main:t1", "title": "build"}));
    assert_eq!((renamed["rename"]["title"].clone(), rig.terminals.title("main", "t1")), (json!("build"), Some("build".into())));
    rig.ok("terminal.rename", json!({"terminal": "main:t1", "title": null}));
    assert_eq!(rig.terminals.title("main", "t1"), Some("t1".into()));
    // When it is closed
    let closed = rig.ok("terminal.close", json!({"terminal": "main:t1"}));
    // Then it is gone from rca and closing it again is stale
    assert_eq!(closed["close"]["closed"], json!(true));
    assert!(!rig.terminals.exists("main", "t1"));
    assert_eq!(rig.err("terminal.close", json!({"terminal": "main:t1"})), "terminal_handle_stale");
}

#[test]
fn given_a_running_terminal_when_the_phone_fits_its_viewport_then_rca_resizes_the_pty_within_its_limit_and_a_dead_handle_is_no_connected_pty() {
    let mut rig = Rig::new();
    let client = json!({"id": "c1", "type": "mobile"});
    // When the phone fits the terminal to its screen
    let fitted = rig.ok("terminal.updateViewport", json!({"terminal": "main:t1", "client": client, "viewport": {"cols": 40, "rows": 20}}));
    // Then the answer says it was applied and the pty is that size
    assert_eq!(fitted, json!({"updated": true, "applied": true}));
    // And a width the host allows but rca does not is cut to rca's limit
    rig.ok("terminal.updateViewport", json!({"terminal": "main:t1", "client": client, "viewport": {"cols": 1024, "rows": 20}}));
    assert_eq!(rig.terminals.resized(), [("t1".to_string(), 40, 20), ("t1".to_string(), 1000, 20)]);
    // And an exited terminal, an unknown one and a malformed handle are each the host's no_connected_pty
    for handle in ["feat:gone", "main:nope", "garbage"] {
        let refused = rig.call("terminal.updateViewport", json!({"terminal": handle, "client": client, "viewport": {"cols": 40, "rows": 20}}));
        assert_eq!((refused["error"]["code"].clone(), refused["error"]["message"].clone()), (json!("runtime_error"), json!("no_connected_pty")), "{handle}");
    }
    // And input the schema refuses never reaches rca
    assert_eq!(rig.err("terminal.updateViewport", json!({"terminal": "main:t1", "viewport": {"cols": 40, "rows": 20}})), "invalid_argument");
    assert_eq!(rig.err("terminal.updateViewport", json!({"terminal": "main:t1", "client": client, "viewport": {"cols": 19, "rows": 20}})), "invalid_argument");
    assert_eq!(rig.err("terminal.updateViewport", json!({"terminal": "main:t1", "client": client, "viewport": {"cols": 40, "rows": 501}})), "invalid_argument");
    assert_eq!(rig.terminals.resized().len(), 2);
}

#[test]
fn given_a_terminal_when_the_display_mode_is_set_then_auto_fits_the_pty_and_desktop_only_proves_it_is_live() {
    let mut rig = Rig::new();
    // When the mode is auto with a viewport
    let auto = rig.ok("terminal.setDisplayMode", json!({"terminal": "main:t1", "mode": "auto", "viewport": {"cols": 50, "rows": 30}}));
    // Then the mode is echoed and the pty was fitted
    assert_eq!(auto, json!({"mode": "auto"}));
    assert_eq!(rig.terminals.resized(), [("t1".to_string(), 50, 30)]);
    // When the mode is desktop
    let desktop = rig.ok("terminal.setDisplayMode", json!({"terminal": "main:t1", "mode": "desktop"}));
    // Then the mode is echoed and nothing was resized
    assert_eq!(desktop, json!({"mode": "desktop"}));
    assert_eq!(rig.terminals.resized().len(), 1);
    // And a missing or unknown mode is refused, a dead handle is no_connected_pty
    assert_eq!(rig.err("terminal.setDisplayMode", json!({"terminal": "main:t1"})), "invalid_argument");
    assert_eq!(rig.err("terminal.setDisplayMode", json!({"terminal": "feat:gone", "mode": "auto"})), "runtime_error");
}

#[test]
fn given_an_unknown_stream_token_when_output_is_acknowledged_then_nothing_is_credited_and_a_bad_count_is_refused() {
    let mut rig = Rig::new();
    // When the phone acknowledges output of a stream
    let acked = rig.ok("terminal.ackOutput", json!({"streamToken": "tok", "units": 4}));
    // Then no credit is held, as no live stream holds that token
    assert_eq!(acked, json!({"credited": false}));
    // And a missing token, zero units and fractional units are each refused
    assert_eq!(rig.err("terminal.ackOutput", json!({"units": 4})), "invalid_argument");
    assert_eq!(rig.err("terminal.ackOutput", json!({"streamToken": "tok", "units": 0})), "invalid_argument");
    assert_eq!(rig.err("terminal.ackOutput", json!({"streamToken": "tok", "units": 1.5})), "invalid_argument");
}

#[test]
fn given_a_terminal_when_it_is_focused_or_an_input_session_is_opened_then_focus_names_the_worktree_without_navigating_and_the_session_is_unordered() {
    let mut rig = Rig::new();
    // When the phone focuses a terminal, even an exited one
    let focused = rig.ok("terminal.focus", json!({"terminal": "feat:gone"}));
    // Then it reads the handle, tab and worktree, and that nothing navigated
    assert_eq!((focused["focus"]["handle"].clone(), focused["focus"]["tabId"].clone(), focused["focus"]["navigated"].clone()), (json!("feat:gone"), json!("gone"), json!(false)));
    assert_eq!(focused["focus"]["worktreeId"], rig.ok("terminal.list", json!({"handles": ["feat:gone"]}))["terminals"][0]["worktreeId"]);
    // And an unknown or malformed handle is stale
    assert_eq!(rig.err("terminal.focus", json!({"terminal": "main:nope"})), "terminal_handle_stale");
    assert_eq!(rig.err("terminal.focus", json!({"terminal": "garbage"})), "terminal_handle_stale");
    // When an input session is opened by a mobile client
    let session = rig.ok("terminal.openInputSession", json!({"terminal": "main:t1", "client": {"id": "c1", "type": "mobile"}}));
    // Then the phone gets no ordering ledger and falls back to unordered input
    assert_eq!(session, json!({"inputSessionId": null}));
    assert_eq!(rig.err("terminal.openInputSession", json!({"terminal": "main:t1", "client": {"id": "c1", "type": "desktop"}})), "invalid_argument");
}

fn tabs_of(snapshot: &Value) -> Vec<(String, bool)> {
    snapshot["tabs"].as_array().unwrap().iter().map(|t| (t["id"].as_str().unwrap().to_string(), t["isActive"].as_bool().unwrap())).collect()
}

#[test]
fn given_a_worktree_with_terminals_when_its_tabs_are_listed_activated_and_closed_then_each_terminal_is_a_tab_and_the_phone_owns_which_is_in_front() {
    let mut rig = Rig::new();
    let feat = json!({"worktree": "path:/srv/wt/feat"});
    // When the phone lists the tabs of a worktree
    let listed = rig.ok("session.tabs.list", feat.clone());
    // Then every terminal is a terminal tab, the running one is in front, and an exited one has no pty
    assert_eq!(tabs_of(&listed), [("gone".to_string(), false), ("t2".to_string(), true)]);
    assert_eq!((listed["activeTabId"].clone(), listed["activeTabType"].clone(), listed["activeGroupId"].clone()), (json!("t2"), json!("terminal"), Value::Null));
    assert_eq!(listed["tabs"][1]["ptyId"], json!("feat:t2"));
    assert!(listed["tabs"][0].get("ptyId").is_none());
    // When it activates the other tab
    let activated = rig.ok("session.tabs.activate", json!({"worktree": "path:/srv/wt/feat", "tabId": "gone"}));
    // Then that tab is in front, and a later list agrees
    assert_eq!(tabs_of(&activated), [("gone".to_string(), true), ("t2".to_string(), false)]);
    assert_eq!(tabs_of(&rig.ok("session.tabs.list", feat.clone())), [("gone".to_string(), true), ("t2".to_string(), false)]);
    // And a tab that is no terminal, a missing tab id, an unknown worktree and a missing selector are each refused
    let missing = rig.call("session.tabs.activate", json!({"worktree": "path:/srv/wt/feat", "tabId": "nope"}));
    assert_eq!((missing["error"]["code"].clone(), missing["error"]["message"].clone()), (json!("runtime_error"), json!("tab_not_found")));
    assert_eq!(rig.err("session.tabs.activate", json!({"worktree": "path:/srv/wt/feat"})), "invalid_argument");
    assert_eq!(rig.err("session.tabs.list", json!({"worktree": "path:/nowhere"})), "selector_not_found");
    assert_eq!(rig.err("session.tabs.list", json!({})), "invalid_argument");
    // When the front tab is closed
    let closed = rig.ok("session.tabs.close", json!({"worktree": "path:/srv/wt/feat", "tabId": "gone"}));
    // Then rca closed the terminal, the phone reads it closed, and the other tab is in front again
    assert_eq!(closed, json!({"closed": true}));
    assert!(!rig.terminals.exists("feat", "gone"));
    assert_eq!(tabs_of(&rig.ok("session.tabs.list", feat)), [("t2".to_string(), true)]);
}

#[test]
fn given_a_worktree_when_a_terminal_tab_is_created_then_rca_opens_it_under_a_free_name_with_its_command_and_it_is_in_front() {
    let mut rig = Rig::new();
    // When the phone creates a terminal tab with a command, a title and an environment that holds a non-string
    let created = rig.ok("session.tabs.createTerminal", json!({"worktree": "path:/srv/wt/feat", "title": "build", "command": "make", "env": {"A": "1", "B": 2}}));
    // Then rca opened a terminal with only the string environment, wrote the command with a carriage return, and the new tab is in front
    assert_eq!(created["tab"]["id"], json!("terminal-1"));
    assert_eq!((created["tab"]["title"].clone(), created["tab"]["isActive"].clone()), (json!("build"), json!(true)));
    assert_eq!(rig.terminals.opened()[0]["env"], json!({"A": "1"}));
    assert_eq!(rig.terminals.written(), [("feat".to_string(), "terminal-1".to_string(), "make\r".to_string())]);
    // And a second one takes the next free name
    assert_eq!(rig.ok("session.tabs.createTerminal", json!({"worktree": "path:/srv/wt/feat"}))["tab"]["id"], json!("terminal-2"));
}

#[test]
fn given_an_open_tabs_stream_when_a_tab_appears_then_the_phone_gets_a_snapshot_then_an_update_and_unsubscribing_ends_it() {
    let mut rig = Rig::new();
    let feat = json!({"worktree": "path:/srv/wt/feat"});
    // When the phone subscribes to a worktree's tabs
    let opened = rig.frames("session.tabs.subscribe", feat.clone());
    // Then the first frame is the snapshot of its two tabs
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert_eq!((opened[0]["result"]["type"].clone(), tabs_of(&opened[0]["result"]).len()), (json!("snapshot"), 2));
    // And nothing more comes while the tabs hold still
    assert!(rig.stream_events().is_empty());
    // When a tab is created
    rig.ok("session.tabs.createTerminal", feat.clone());
    // Then the next frame is an update with three tabs
    let next = rig.stream_events();
    assert_eq!((next[0]["type"].clone(), tabs_of(&next[0]).len()), (json!("updated"), 3));
    // When the phone unsubscribes by an id: selector that matches nothing, it still answers unsubscribed and the stream goes on
    assert_eq!(rig.ok("session.tabs.unsubscribe", json!({"worktree": "id:nowhere::/x"})), json!({"unsubscribed": true}));
    rig.ok("session.tabs.createTerminal", feat.clone());
    assert_eq!(rig.stream_events()[0]["type"], json!("updated"));
    // When it unsubscribes by the worktree
    let ended = rig.frames("session.tabs.unsubscribe", feat);
    // Then it is told so and the stream ends
    assert_eq!(ended[0]["result"], json!({"unsubscribed": true}));
    let tail: Vec<Value> = ended[1..].iter().map(|f| f["result"].clone()).chain(rig.stream_events()).collect();
    assert!(tail.iter().any(|f| f["type"] == "end"), "{tail:?}");
}

#[test]
fn given_a_terminal_that_printed_colored_lines_when_it_is_read_then_the_tail_is_plain_lines_limited_and_a_cursor_returns_only_what_followed() {
    let mut rig = Rig::new();
    // Given a terminal that printed three lines with colors and a title sequence
    rig.terminals.output("main", "t1", b"\x1b]0;shell\x07\x1b[32mone\x1b[0m\r\ntwo\r\nthree\r\n");
    // When the phone reads it with no cursor
    let all = rig.ok("terminal.read", json!({"terminal": "main:t1"}))["terminal"].clone();
    // Then it reads the lines without escape sequences, the stream's source, and the offsets as decimal strings
    assert_eq!((all["tail"].clone(), all["source"].clone(), all["status"].clone()), (json!(["one", "two", "three"]), json!("stream"), json!("running")));
    assert_eq!((all["oldestCursor"].clone(), all["latestCursor"].clone(), all["nextCursor"].clone(), all["returnedLineCount"].clone()), (json!("0"), json!("36"), json!("36"), json!(3)));
    // When it reads with a limit of two
    let last = rig.ok("terminal.read", json!({"terminal": "main:t1", "limit": 2}))["terminal"].clone();
    // Then the last two lines come back and it says it truncated
    assert_eq!((last["tail"].clone(), last["truncated"].clone()), (json!(["two", "three"]), json!(true)));
    // When it reads from the end cursor after more output
    rig.terminals.output("main", "t1", b"\x1b]0;shell\x07\x1b[32mone\x1b[0m\r\ntwo\r\nthree\r\nfour\r\n");
    let more = rig.ok("terminal.read", json!({"terminal": "main:t1", "cursor": 36}))["terminal"].clone();
    // Then only the new line comes back
    assert_eq!((more["tail"].clone(), more["oldestCursor"].clone()), (json!(["four"]), json!("36")));
    // And a screen read has no screen, a cursor with it is refused, and a bad cursor, an exited terminal and an unknown handle each answer as the host does
    let screen = rig.ok("terminal.read", json!({"terminal": "main:t1", "screen": true}))["terminal"].clone();
    assert_eq!((screen["source"].clone(), screen["tail"].clone()), (json!("screen-unavailable"), json!([])));
    assert_eq!(rig.err("terminal.read", json!({"terminal": "main:t1", "screen": true, "cursor": 1})), "invalid_argument");
    assert_eq!(rig.err("terminal.read", json!({"terminal": "main:t1", "cursor": "x"})), "invalid_argument");
    assert_eq!(rig.ok("terminal.read", json!({"terminal": "feat:gone"}))["terminal"]["status"], json!("exited"));
    assert_eq!(rig.err("terminal.read", json!({"terminal": "main:nope"})), "terminal_handle_stale");
}

struct Bare {
    agent: Arc<ScriptedAgent>,
    terminals: Terminals,
}

impl Bare {
    fn new() -> Bare {
        let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
        let terminals = Terminals::default();
        terminals.terminal("w", "t", true);
        terminals.install(&agent);
        Bare { agent, terminals }
    }

    fn attach(&self, after: Option<u64>) -> (Value, Box<dyn orca_core::EventStream>) {
        let mut p = json!({"workspace": "w", "terminal": "t"});
        if let Some(a) = after {
            p["after_offset"] = json!(a);
        }
        self.agent.follow("terminal.attach", "terminal/w/t", p, "after_offset").expect("attach is answered")
    }
}

fn drain(stream: &mut dyn orca_core::EventStream) -> Vec<(u64, String, Value)> {
    let mut out = vec![];
    while let Some(e) = stream.next(std::time::Duration::ZERO).unwrap() {
        out.push((e.seq, e.kind, e.data));
    }
    out
}

fn data_of(e: &Value) -> Vec<u8> {
    agent_api_terminal::b64::decode(e["data"].as_str().unwrap()).unwrap()
}

#[test]
fn given_an_attach_at_the_end_when_bytes_are_pushed_then_each_arrives_as_an_output_event_numbered_by_the_offset_after_its_bytes() {
    let rig = Bare::new();
    // Given a terminal that printed five bytes and an attach from its end
    rig.terminals.output("w", "t", b"hello");
    let (answer, mut stream) = rig.attach(Some(5));
    assert_eq!((answer["start_offset"].clone(), answer["offset"].clone(), answer["reset"].clone(), answer["status"].clone()), (json!(5), json!(5), json!(false), json!("running")));
    // When it prints two chunks
    rig.terminals.push_output("w", "t", b"ab").push_output("w", "t", b"cde");
    // Then the attach holds two output events, each at the offset after its bytes, with the bytes
    let events = drain(stream.as_mut());
    assert_eq!(events.iter().map(|(seq, kind, _)| (*seq, kind.as_str())).collect::<Vec<_>>(), vec![(7, "terminal.output"), (10, "terminal.output")]);
    assert_eq!((data_of(&events[0].2), events[0].2["offset"].clone(), data_of(&events[1].2)), (b"ab".to_vec(), json!(5), b"cde".to_vec()));
    // And a read from the same offset reports the same end
    let read = rig.agent.request("terminal.read", json!({"workspace": "w", "terminal": "t", "after_offset": 5})).unwrap();
    assert_eq!((read["start_offset"].clone(), read["offset"].clone(), data_of(&read)), (json!(5), json!(10), b"abcde".to_vec()));
}

#[test]
fn given_a_four_byte_tail_when_attached_from_an_older_offset_then_the_answer_resets_and_one_snapshot_replays_the_kept_bytes() {
    let rig = Bare::new();
    // Given a terminal that printed ten bytes and keeps only the last four
    rig.terminals.with_retain(4).output("w", "t", b"0123456789");
    // When a phone attaches from offset 2
    let (answer, mut stream) = rig.attach(Some(2));
    // Then the answer says reset from offset 6 to 10
    assert_eq!((answer["start_offset"].clone(), answer["offset"].clone(), answer["reset"].clone()), (json!(6), json!(10), json!(true)));
    // And the first event is the snapshot of the kept bytes
    let events = drain(stream.as_mut());
    assert_eq!((events[0].0, events[0].1.as_str(), events[0].2["reset"].clone(), data_of(&events[0].2)), (10, "terminal.snapshot", json!(true), b"6789".to_vec()));
    // And a read from the lost offset resets the same way, while an offset inside the tail does not
    let old = rig.agent.request("terminal.read", json!({"workspace": "w", "terminal": "t", "after_offset": 2})).unwrap();
    assert_eq!((old["start_offset"].clone(), old["reset"].clone()), (json!(6), json!(true)));
    let (inside, _) = rig.attach(Some(8));
    assert_eq!((inside["start_offset"].clone(), inside["reset"].clone()), (json!(8), json!(false)));
    // And more output past the cap moves the tail on
    rig.terminals.push_output("w", "t", b"ab");
    let (moved, _) = rig.attach(Some(6));
    assert_eq!((moved["start_offset"].clone(), moved["reset"].clone()), (json!(8), json!(true)));
}

#[test]
fn given_an_attached_terminal_when_it_exits_and_exits_again_then_the_stream_holds_one_exited_event_and_the_status_is_exited() {
    let rig = Bare::new();
    // Given an attach and some output
    let (_, mut stream) = rig.attach(None);
    rig.terminals.push_output("w", "t", b"bye");
    // When the process exits with 3, twice
    rig.terminals.exit("w", "t", 3).exit("w", "t", 9);
    // Then the stream ends with one terminal.exited at the end offset carrying the first code
    let events = drain(stream.as_mut());
    assert_eq!(events.iter().map(|(seq, kind, _)| (*seq, kind.as_str())).collect::<Vec<_>>(), vec![(3, "terminal.output"), (3, "terminal.exited")]);
    assert_eq!(events[1].2["exit_code"], json!(3));
    // And a new attach answers exited with the code, and a write is refused
    let (answer, _) = rig.attach(Some(3));
    assert_eq!((answer["status"].clone(), answer["exit_code"].clone()), (json!("exited"), json!(3)));
    assert!(matches!(rig.agent.request("terminal.write", json!({"workspace": "w", "terminal": "t", "data": "x"})), Err(orca_core::AgentError::Refused { code, .. }) if code == "TERMINAL_EXITED"));
}

#[test]
fn given_an_attached_terminal_when_it_is_cleared_then_a_cleared_event_follows_and_older_offsets_reset() {
    let rig = Bare::new();
    // Given an attach over four printed bytes
    rig.terminals.output("w", "t", b"abcd");
    let (_, mut stream) = rig.attach(Some(4));
    // When the buffer is cleared and more is printed
    rig.terminals.clear("w", "t").push_output("w", "t", b"e");
    // Then a cleared event at offset 4 comes before the output at 5
    let events = drain(stream.as_mut());
    assert_eq!(events.iter().map(|(seq, kind, _)| (*seq, kind.as_str())).collect::<Vec<_>>(), vec![(4, "terminal.cleared"), (5, "terminal.output")]);
    // And an attach from before the clear resets to the bytes after it
    let (answer, _) = rig.attach(Some(0));
    assert_eq!((answer["start_offset"].clone(), answer["reset"].clone()), (json!(4), json!(true)));
}

#[test]
fn given_the_double_when_an_offset_is_past_the_end_or_a_terminal_is_missing_then_attach_and_read_refuse_as_the_contract_does() {
    let rig = Bare::new();
    rig.terminals.output("w", "t", b"abc");
    let code = |r: Result<(), orca_core::AgentError>| match r {
        Err(orca_core::AgentError::Refused { code, .. }) => code,
        Ok(_) => "ok".to_string(),
        Err(e) => e.to_string(),
    };
    // When attaching or reading past the end, Then OFFSET_AHEAD
    let past = json!({"workspace": "w", "terminal": "t", "after_offset": 4});
    assert_eq!(code(rig.agent.follow("terminal.attach", "t", past.clone(), "after_offset").map(|_| ())), "OFFSET_AHEAD");
    assert_eq!(code(rig.agent.request("terminal.read", past).map(|_| ())), "OFFSET_AHEAD");
    // When the terminal is missing, Then UNKNOWN_TERMINAL
    let gone = json!({"workspace": "w", "terminal": "nope"});
    assert_eq!(code(rig.agent.follow("terminal.attach", "t", gone.clone(), "after_offset").map(|_| ())), "UNKNOWN_TERMINAL");
    assert_eq!(code(rig.agent.request("terminal.read", gone).map(|_| ())), "UNKNOWN_TERMINAL");
}
