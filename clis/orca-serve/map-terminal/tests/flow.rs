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
