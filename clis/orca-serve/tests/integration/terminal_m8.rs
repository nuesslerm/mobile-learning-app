//! The phone's terminal rows through the chain: a shell the real rca-exec runs on a pty, reached through the real rca-service. A row counts only
//! when the agent method it maps to was answered in its call's window and the executor ran the pty action, and what the shell printed is read back
//! from the service's own stream: the typed command is computed, so its output is not what was typed.
use crate::rig::*;
use agent_api_terminal::{b64, ATTACH};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const WS: &str = "w1";
const WAIT: Duration = Duration::from_secs(25);

/// What the shell has printed, read from a fresh attach to the service's stream for two seconds.
fn screen(direct: &mut Direct) -> String {
    let client = direct.client();
    client.call(ATTACH, json!({"workspace": WS, "terminal": "t1"})).expect("the service attaches");
    let mut text = Vec::new();
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        if let Some(event) = client.next_event(Duration::from_millis(200)).expect("the stream reads") {
            if let Some(bytes) = event.data.get("data").and_then(Value::as_str).and_then(b64::decode) {
                text.extend(bytes);
            }
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// What the shell has printed once `needle` shows in it.
fn screen_until(direct: &mut Direct, needle: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let text = screen(direct);
        if text.contains(needle) {
            return text;
        }
        assert!(Instant::now() < deadline, "the shell never printed {needle}; it printed {text:?}");
    }
}

#[test]
fn a_terminal_is_listed_typed_into_cleared_renamed_and_closed_and_each_call_ran_its_pty_action_in_the_executors_shell() {
    exec_testkit::case!(SUITE, |case| {
        // Given a shell the service opened on the executor and a paired phone
        let chain = Chain::start(None);
        let mut direct = chain.direct();
        direct.call("terminal", "terminal.open", json!({"workspace": WS, "terminal": "t1"})).expect("the service opens the shell");
        let mut phone = Phone::connect(case, &chain);
        let handle = format!("{WS}:t1");
        // When the phone lists the workspace's terminals
        let listed = phone.call(case, &chain, &["rpc:terminal.list"], "terminal.list", json!({"worktree": format!("id:{WS}::")}));
        answered(case, "terminal.list", true);
        assert!(ran(case, "pty_list"), "{}", chain.evidence());
        // Then the shell is a running terminal under its handle
        let row = &listed["result"]["terminals"][0];
        assert_eq!((row["handle"].clone(), row["connected"].clone(), row["writable"].clone()), (json!(handle), json!(true), json!(true)));
        // When it sends a command whose output differs from what is typed
        let sent = phone.call(case, &chain, &["rpc:terminal.send"], "terminal.send", json!({"terminal": handle, "text": "echo computed-$((40+2))", "enter": true}));
        answered(case, "terminal.write", true);
        assert!(ran(case, "pty_write"), "{}", chain.evidence());
        assert_eq!(sent["result"]["send"]["accepted"], json!(true));
        // Then the shell printed the computed answer
        screen_until(&mut direct, "computed-42");
        // When it renames the terminal
        let renamed = phone.call(case, &chain, &["rpc:terminal.rename"], "terminal.rename", json!({"terminal": handle, "title": "build"}));
        answered(case, "terminal.rename", true);
        assert_eq!(renamed["result"]["rename"]["title"], json!("build"));
        // Then the service lists it under the new title
        let after = direct.call("terminal", "terminal.list", json!({"workspace": WS})).unwrap();
        assert_eq!(after["terminals"][0]["title"], json!("build"));
        // When it clears the buffer
        let cleared = phone.call(case, &chain, &["rpc:terminal.clearBuffer"], "terminal.clearBuffer", json!({"terminal": handle}));
        answered(case, "terminal.clear", true);
        assert!(ran(case, "pty_write"), "{}", chain.evidence());
        assert_eq!(cleared["result"]["clear"]["cleared"], json!(true));
        // Then the output printed before is gone from what the service streams to a new attach
        let after_clear = screen(&mut direct);
        assert!(!after_clear.contains("computed-42"), "the cleared output is still streamed: {after_clear:?}");
        // When it closes the terminal
        let closed = phone.call(case, &chain, &["rpc:terminal.close"], "terminal.close", json!({"terminal": handle}));
        answered(case, "terminal.close", true);
        assert!(ran(case, "pty_close"), "{}", chain.evidence());
        assert_eq!(closed["result"]["close"]["closed"], json!(true));
        // Then the service has none, and a send to the old handle is refused after the request reached rca
        assert_eq!(direct.call("terminal", "terminal.list", json!({"workspace": WS})).unwrap()["terminals"], json!([]));
        let stale = phone.refused(case, &chain, &["rpc:terminal.send"], "terminal.send", json!({"terminal": handle, "text": "x"}));
        answered(case, "terminal.write", false);
        assert_eq!(stale["error"]["code"], json!("terminal_handle_stale"));
    });
}
