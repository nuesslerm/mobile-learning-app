//! The phone's terminal rows through the chain: a shell the real rca-exec runs on a pty, reached through the real rca-service. A row counts only
//! when the agent method it maps to was answered in its call's window and the executor ran the pty action, and what the shell printed is read back
//! from the service's own stream: the typed command is computed, so its output is not what was typed.
use crate::rig::*;
use agent_api_terminal::{b64, ATTACH};
use orca_map_terminal::binary::op;
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
        // When the phone reads the terminal's tail
        let read = phone.call(case, &chain, &["rpc:terminal.read"], "terminal.read", json!({"terminal": handle}));
        // Then rca answered the read and the lines include the computed output, without escape sequences
        answered(case, "terminal.read", true);
        let tail = read["result"]["terminal"]["tail"].as_array().unwrap();
        assert!(tail.iter().any(|l| l.as_str().is_some_and(|l| l.trim() == "computed-42")), "{read}");
        assert!(tail.iter().all(|l| !l.as_str().unwrap_or_default().contains('\u{1b}')), "{read}");
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

#[test]
fn a_phone_fits_a_terminal_to_its_screen_and_the_executors_pty_is_resized_and_a_dead_handle_reaches_rca_before_it_is_refused() {
    exec_testkit::case!(SUITE, |case| {
        // Given a shell on the executor and a paired phone
        let chain = Chain::start(None);
        let mut direct = chain.direct();
        direct.call("terminal", "terminal.open", json!({"workspace": WS, "terminal": "t1"})).expect("the service opens the shell");
        let mut phone = Phone::connect(case, &chain);
        let handle = format!("{WS}:t1");
        let client = json!({"id": "c1", "type": "mobile"});
        // When the phone fits the terminal to its viewport
        let fitted = phone.call(case, &chain, &["rpc:terminal.updateViewport"], "terminal.updateViewport", json!({"terminal": handle, "client": client, "viewport": {"cols": 52, "rows": 18}}));
        // Then rca resized the pty and the phone reads the fit as applied
        answered(case, "terminal.resize", true);
        assert!(ran(case, "pty_resize"), "{}", chain.evidence());
        assert_eq!(fitted["result"], json!({"updated": true, "applied": true}));
        // And the service lists the terminal at that size
        let listed = direct.call("terminal", "terminal.list", json!({"workspace": WS})).unwrap();
        assert_eq!((listed["terminals"][0]["cols"].clone(), listed["terminals"][0]["rows"].clone()), (json!(52), json!(18)));
        // When it sets the display mode to auto with another viewport
        let mode = phone.call(case, &chain, &["rpc:terminal.setDisplayMode"], "terminal.setDisplayMode", json!({"terminal": handle, "mode": "auto", "viewport": {"cols": 60, "rows": 24}}));
        // Then the pty was resized again
        answered(case, "terminal.resize", true);
        assert_eq!(mode["result"], json!({"mode": "auto"}));
        // When it focuses the terminal
        let focused = phone.call(case, &chain, &["rpc:terminal.focus"], "terminal.focus", json!({"terminal": handle}));
        // Then rca listed the workspace's terminals and the phone reads that nothing navigated
        answered(case, "terminal.list", true);
        assert_eq!(focused["result"]["focus"]["navigated"], json!(false));
        // When it fits a handle the service holds no terminal for
        let dead = phone.refused(case, &chain, &["rpc:terminal.updateViewport"], "terminal.updateViewport", json!({"terminal": format!("{WS}:nope"), "client": client, "viewport": {"cols": 52, "rows": 18}}));
        // Then rca was asked first, and the refusal is the host's no_connected_pty
        answered(case, "terminal.list", true);
        assert_eq!(dead["error"]["message"], json!("no_connected_pty"));
    });
}

#[test]
fn a_phone_creates_lists_activates_and_closes_terminal_tabs_and_each_call_reached_rca_and_the_phones_front_tab_is_kept_by_the_service() {
    exec_testkit::case!(SUITE, |case| {
        // Given a shell on the executor and a paired phone
        let chain = Chain::start(None);
        let mut direct = chain.direct();
        direct.call("terminal", "terminal.open", json!({"workspace": WS, "terminal": "t1"})).expect("the service opens the shell");
        let mut phone = Phone::connect(case, &chain);
        let selector = json!({"worktree": format!("id:{WS}::")});
        // When the phone lists the tabs
        let listed = phone.call(case, &chain, &["rpc:session.tabs.list"], "session.tabs.list", selector.clone());
        // Then rca listed the terminals and the shell is the one tab, in front, with its pty
        answered(case, "terminal.list", true);
        assert_eq!((listed["result"]["tabs"][0]["id"].clone(), listed["result"]["activeTabId"].clone(), listed["result"]["tabs"][0]["ptyId"].clone()), (json!("t1"), json!("t1"), json!(format!("{WS}:t1"))));
        // When it creates a terminal tab
        let created = phone.call(case, &chain, &["rpc:session.tabs.createTerminal"], "session.tabs.createTerminal", json!({"worktree": format!("id:{WS}::"), "title": "build"}));
        // Then the executor opened a pty, the front tab is kept in the service's state, and the tab reads as in front
        answered(case, "terminal.open", true);
        assert!(ran(case, "pty_open"), "{}", chain.evidence());
        answered(case, "state.set", true);
        assert_eq!((created["result"]["tab"]["id"].clone(), created["result"]["tab"]["isActive"].clone()), (json!("terminal-1"), json!(true)));
        // When it activates the first tab again
        let activated = phone.call(case, &chain, &["rpc:session.tabs.activate"], "session.tabs.activate", json!({"worktree": format!("id:{WS}::"), "tabId": "t1"}));
        // Then the service kept the choice and a list reads it
        answered(case, "state.set", true);
        assert_eq!(activated["result"]["activeTabId"], json!("t1"));
        let again = phone.call(case, &chain, &["rpc:session.tabs.list"], "session.tabs.list", selector.clone());
        assert_eq!(again["result"]["activeTabId"], json!("t1"));
        // When it opens the tabs stream
        let (stream, first) = phone.subscribe(case, &chain, &["rpc:session.tabs.subscribe"], "session.tabs.subscribe", selector.clone());
        // Then rca listed the terminals and the first frame is the snapshot of both tabs
        answered(case, "terminal.list", true);
        assert_eq!(first["result"]["tabs"].as_array().unwrap().len(), 2, "{first}");
        // When it unsubscribes by the worktree
        let ended = phone.call(case, &chain, &["rpc:session.tabs.unsubscribe"], "session.tabs.unsubscribe", selector.clone());
        // Then it is acknowledged and the stream carrying `stream` ends
        assert_eq!(ended["result"], json!({"unsubscribed": true}), "{stream}");
        // When it closes the second tab
        let closed = phone.call(case, &chain, &["rpc:session.tabs.close"], "session.tabs.close", json!({"worktree": format!("id:{WS}::"), "tabId": "terminal-1"}));
        // Then the executor closed that pty and only the shell is left
        answered(case, "terminal.close", true);
        assert!(ran(case, "pty_close"), "{}", chain.evidence());
        assert_eq!(closed["result"]["closed"], json!(true));
        assert_eq!(direct.call("terminal", "terminal.list", json!({"workspace": WS})).unwrap()["terminals"].as_array().unwrap().len(), 1);
    });
}

#[test]
fn a_phone_streams_a_real_shell_over_the_binary_stream_types_into_it_and_resumes_with_only_what_it_missed_and_the_stream_ends_when_it_asks() {
    exec_testkit::case!(SUITE, |case| {
        // Given a shell on the executor that has printed a computed answer, and a paired phone
        let chain = Chain::start(None);
        let mut direct = chain.direct();
        direct.call("terminal", "terminal.open", json!({"workspace": WS, "terminal": "t1"})).expect("the service opens the shell");
        let mut phone = Phone::connect(case, &chain);
        let handle = format!("{WS}:t1");
        let client = json!({"id": "chain-phone", "type": "mobile"});
        phone.call(case, &chain, &["rpc:terminal.send"], "terminal.send", json!({"terminal": handle, "text": "echo before-$((40+2))", "enter": true}));
        screen_until(&mut direct, "before-42");
        // When the phone opens an input session, fits the terminal to its viewport and subscribes to the stream
        let session = phone.call(case, &chain, &["rpc:terminal.openInputSession"], "terminal.openInputSession", json!({"terminal": handle, "client": client}));
        assert_eq!(session["result"]["inputSessionId"], Value::Null);
        phone.call(case, &chain, &["rpc:terminal.updateViewport"], "terminal.updateViewport", json!({"terminal": handle, "client": client, "viewport": {"cols": 64, "rows": 20}}));
        answered(case, "terminal.resize", true);
        let first = phone.subscribe_terminal(case, &chain, &handle, None);
        // Then rca was read and attached, and the snapshot replays what the shell printed
        answered(case, "terminal.read", true);
        answered(case, "terminal.attach", true);
        assert_eq!((first.opened["result"]["outputAck"].clone(), first.opened["result"]["cols"].clone(), first.opened["result"]["rows"].clone()), (json!(true), json!(64), json!(20)));
        let snapshot = phone.snapshot(&first);
        assert!(snapshot.contains("before-42"), "{snapshot:?}");
        // When it types a command as an Input frame
        phone.send_terminal_frame(&first, op::INPUT, b"echo live-$((50+5))\r");
        // Then the shell's own bytes of its computed output arrive as Output frames
        let live = phone.output_until(&first, "live-55");
        assert!(live.units > 0);
        // When it acknowledges what it heard
        let credited = phone.call(case, &chain, &["rpc:terminal.ackOutput"], "terminal.ackOutput", json!({"streamToken": first.token, "units": live.units}));
        // Then the credit is taken for the live token
        assert_eq!(credited["result"]["credited"], json!(true));
        // When it drops the stream with an Unsubscribe frame
        phone.send_terminal_frame(&first, op::UNSUBSCRIBE, b"");
        // Then the stream ends, and its token no longer takes credit
        assert_eq!(phone.next_frame(&first.request)["result"], json!({"type": "end"}));
        let spent = phone.call(case, &chain, &["rpc:terminal.ackOutput"], "terminal.ackOutput", json!({"streamToken": first.token, "units": 1}));
        assert_eq!(spent["result"]["credited"], json!(false));
        // When the shell prints more while nobody watches, and the phone subscribes again with its token and the units it heard
        phone.call(case, &chain, &["rpc:terminal.send"], "terminal.send", json!({"terminal": handle, "text": "echo away-$((60+6))", "enter": true}));
        screen_until(&mut direct, "away-66");
        let second = phone.subscribe_terminal(case, &chain, &handle, Some((&first.token, live.units)));
        // Then the stream is a resumed one that carries the missed output and not what the phone already had
        assert_eq!(second.opened["result"]["resumed"], json!(true), "{}", second.opened);
        assert_ne!(second.token, first.token);
        let missed = phone.output_until(&second, "away-66");
        assert!(!missed.text.contains("before-42") && !missed.text.contains("live-55"), "{:?}", missed.text);
        // When it resizes the stream with a Resize frame
        phone.send_terminal_frame(&second, op::RESIZE, br#"{"cols": 72, "rows": 22}"#);
        // Then the stream announces the new size and the executor's pty has it
        let announced = phone.resized(&second);
        assert_eq!((announced["cols"].clone(), announced["rows"].clone()), (json!(72), json!(22)));
        let listed = direct.call("terminal", "terminal.list", json!({"workspace": WS})).unwrap();
        let size = (listed["terminals"][0]["cols"].clone(), listed["terminals"][0]["rows"].clone());
        assert_eq!(size, (json!(72), json!(22)));
        // When it names the stream to terminal.unsubscribe
        let ended = phone.call(case, &chain, &["rpc:terminal.unsubscribe"], "terminal.unsubscribe", json!({"subscriptionId": handle, "requestId": second.request}));
        // Then it is acknowledged and the stream ends
        assert_eq!(ended["result"], json!({"unsubscribed": true}));
        assert_eq!(phone.next_frame(&second.request)["result"], json!({"type": "end"}));
        // When it closes the terminal and subscribes again
        phone.call(case, &chain, &["rpc:terminal.close"], "terminal.close", json!({"terminal": handle}));
        let stale = phone.refused(case, &chain, &["rpc:terminal.subscribe"], "terminal.subscribe", phone_client_params(&handle, None));
        // Then the handle is refused as stale
        assert_eq!(stale["error"]["code"], json!("terminal_handle_stale"));
    });
}
