//! What no recording shows of a terminal: the bytes a real shell prints on the executor's pty reach the phone over the binary stream, what the
//! phone types runs in it, a resumed stream carries only what was missed and a resize reaches the pty. The headless orcad answers "no open app
//! window" to every terminal row, so the recorded replay serves the rows and this behaviour test (listed in `orca-decisions.json`) keeps the pty.
use crate::rig::*;
use agent_api_terminal::{b64, ATTACH};
use orca_map_terminal::binary::op;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const WS: &str = "w1";
const WAIT: Duration = Duration::from_secs(25);

/// What the shell has printed, read from a fresh attach to the service's stream for two seconds.
fn screen(direct: &mut Direct, terminal: &str) -> String {
    let client = direct.client();
    client.call(ATTACH, json!({"workspace": WS, "terminal": terminal})).expect("the service attaches");
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
fn screen_until(direct: &mut Direct, terminal: &str, needle: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let text = screen(direct, terminal);
        if text.contains(needle) {
            return text;
        }
        assert!(Instant::now() < deadline, "the shell never printed {needle}; it printed {text:?}");
    }
}

#[test]
fn a_phone_streams_a_real_shell_over_the_binary_stream_types_into_it_and_resumes_with_only_what_it_missed_and_the_stream_ends_when_it_asks() {
    exec_testkit::case!(SUITE, |case| {
        // Given a shell on the executor that has printed a computed answer, and a paired phone
        let chain = Chain::start(None);
        let mut direct = chain.direct();
        let mut phone = Phone::connect(case, &chain);
        let created = phone.call(case, &chain, &["rpc:session.tabs.createTerminal"], "session.tabs.createTerminal", json!({"worktree": "id:w1::"}));
        let tab = &created["result"]["tab"];
        assert_eq!(tab["status"], "ready", "{created}");
        let terminal = tab["id"].as_str().unwrap();
        let handle = tab["terminal"].as_str().expect("the new tab exposes its live handle");
        let client = json!({"id": "chain-phone", "type": "mobile"});
        phone.call(case, &chain, &["rpc:terminal.send"], "terminal.send", json!({"terminal": handle, "text": "echo before-$((40+2))", "enter": true}));
        screen_until(&mut direct, terminal, "before-42");
        // When the phone opens an input session, fits the terminal to its viewport and subscribes to the stream
        let session = phone.call(case, &chain, &["rpc:terminal.openInputSession"], "terminal.openInputSession", json!({"terminal": handle, "client": client}));
        assert_eq!(session["result"]["inputSessionId"], Value::Null);
        phone.call(case, &chain, &["rpc:terminal.updateViewport"], "terminal.updateViewport", json!({"terminal": handle, "client": client, "viewport": {"cols": 64, "rows": 20}}));
        answered(case, "terminal.resize", true);
        let first = phone.subscribe_terminal(case, &chain, handle, None);
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
        assert_eq!(phone.end_of(&first)["result"], json!({"type": "end"}));
        let spent = phone.call(case, &chain, &["rpc:terminal.ackOutput"], "terminal.ackOutput", json!({"streamToken": first.token, "units": 1}));
        assert_eq!(spent["result"]["credited"], json!(false));
        // When the shell prints more while nobody watches, and the phone subscribes again with its token and the units it heard
        phone.call(case, &chain, &["rpc:terminal.send"], "terminal.send", json!({"terminal": handle, "text": "echo away-$((60+6))", "enter": true}));
        screen_until(&mut direct, terminal, "away-66");
        let second = phone.subscribe_terminal(case, &chain, handle, Some((&first.token, live.units)));
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
        assert_eq!(phone.end_of(&second)["result"], json!({"type": "end"}));
        // When it closes the terminal and subscribes again
        phone.call(case, &chain, &["rpc:terminal.close"], "terminal.close", json!({"terminal": handle}));
        let stale = phone.refused(case, &chain, &["rpc:terminal.subscribe"], "terminal.subscribe", phone_client_params(handle, None));
        // Then the handle is refused as stale
        assert_eq!(stale["error"]["code"], json!("terminal_handle_stale"));
    });
}
