//! The terminal rows through the `Host` dispatcher with an in-memory terminal set as rca: each reply is read the way the phone reads it.
//! Given/When/Then in each test.
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Connection, Host, HostInfo, ServiceStatus};
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
fn given_a_stream_token_when_output_is_acknowledged_then_nothing_is_credited_and_a_bad_count_is_refused() {
    let mut rig = Rig::new();
    // When the phone acknowledges output of a stream
    let acked = rig.ok("terminal.ackOutput", json!({"streamToken": "tok", "units": 4}));
    // Then no credit is held, as this host issues no stream token
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
