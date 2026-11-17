//! The terminal rows of M8 through the chain: each call goes from the phone through `t3-serve serve` to the real rca-service and on to the real
//! rca-exec, whose shell runs on a pty in the temporary repository. A call counts for its row only when it was answered and the executor ran the
//! pty action, which each test reads back from the shell's own output: the typed command is computed, so its result is not what was typed.
use crate::chain::{hit, last_window};
use crate::rig::*;
use crate::support::*;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(25);

/// Reads the watch socket, acknowledging chunks, until `ready` holds for the values of stream `id` so far.
fn watch_until(phone: &mut Phone, id: &Value, what: &str, ready: impl Fn(&[Value]) -> bool) -> Vec<Value> {
    let deadline = Instant::now() + WAIT;
    let mut values: Vec<Value> = Vec::new();
    while !ready(&values) {
        assert!(Instant::now() < deadline, "{what}; the stream said {values:?}");
        if let Some(frame) = phone.watch.try_recv(Duration::from_millis(300)) {
            if frame["_tag"] == "Chunk" {
                phone.watch.ack(&frame["requestId"]);
                if frame["requestId"] == *id {
                    values.extend(frame["values"].as_array().cloned().unwrap_or_default());
                }
            }
        }
    }
    values
}

fn screen(values: &[Value]) -> String {
    values.iter().map(|v| v["snapshot"]["history"].as_str().or_else(|| v["data"].as_str()).unwrap_or_default()).collect()
}

fn methods(window: &[(String, bool)], wanted: &str) -> Vec<bool> {
    window.iter().filter(|(m, _)| m == wanted).map(|(_, ok)| *ok).collect()
}

fn ran(executor: &Value, action: &str, ok: bool) -> bool {
    hit(executor, "tool", action).is_some_and(|h| h["ok"] == ok)
}

#[test]
fn a_terminal_is_attached_typed_into_resized_observed_cleared_listed_and_closed_and_each_call_ran_its_pty_action_in_the_executors_shell() {
    exec_testkit::case!(SUITE, |case| {
        // Given a paired phone and a project on the repository the executor serves
        let chain = Chain::start();
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        let root = chain.repo.path().display().to_string();
        let ids = json!({"threadId": "t1", "terminalId": "term-1"});
        // When the phone attaches to a terminal that does not exist yet, with the project's directory
        let (attach, first) = phone.stream(case, &chain, &["rpc:terminal.attach"], "terminal.attach", json!({"threadId": "t1", "terminalId": "term-1", "cwd": root, "cols": 100, "rows": 30}));
        let (agent, executor) = last_window(case);
        // Then rca opened the terminal for it (the executor made the pty) and the first frame is the snapshot of a running terminal
        assert_eq!(methods(&agent, "terminal.attach"), [false, true]);
        assert_eq!(methods(&agent, "terminal.open"), [true]);
        assert!(ran(&executor, "pty_open", true), "{executor}");
        assert_eq!((first[0]["type"].as_str(), first[0]["snapshot"]["status"].as_str(), first[0]["snapshot"]["cols"].clone(), first[0]["snapshot"]["rows"].clone()), (Some("snapshot"), Some("running"), json!(100), json!(30)));
        // When a command whose output differs from what is typed is written
        let write = |phone: &mut Phone, case: &mut exec_testkit::Case, data: &str| {
            let payload = json!({"threadId": "t1", "terminalId": "term-1", "data": data});
            success(&phone.call(case, &chain, &["rpc:terminal.write"], "terminal.write", payload));
        };
        write(&mut phone, case, "echo computed-$((40+2))\n");
        let (agent, executor) = last_window(case);
        assert_eq!(methods(&agent, "terminal.write"), [true]);
        assert!(ran(&executor, "pty_write", true), "{executor}");
        // Then the output arrives on the attached stream, with the offset after its bytes as its sequence
        let seen = watch_until(&mut phone, &attach, "the shell's answer", |v| screen(v).contains("computed-42"));
        let outputs: Vec<&Value> = seen.iter().filter(|v| v["type"] == "output").collect();
        assert!(!outputs.is_empty() && outputs.iter().all(|v| v["sequence"].as_u64().is_some_and(|s| s > 0)));
        // When the pty is resized and asked its size
        let resized = phone.call(case, &chain, &["rpc:terminal.resize"], "terminal.resize", json!({"threadId": "t1", "terminalId": "term-1", "cols": 101, "rows": 33}));
        let (agent, executor) = last_window(case);
        assert_eq!(success(&resized), &Value::Null);
        assert_eq!(methods(&agent, "terminal.resize"), [true]);
        assert!(ran(&executor, "pty_resize", true), "{executor}");
        write(&mut phone, case, "stty size\n");
        watch_until(&mut phone, &attach, "the size the shell sees", |v| screen(v).contains("33 101"));
        // When the phone observes the terminal, it is sent what the terminal printed so far as the history of one snapshot
        let (observe, history) = phone.stream(case, &chain, &["rpc:terminal.observe"], "terminal.observe", ids.clone());
        let (agent, _) = last_window(case);
        assert!(methods(&agent, "terminal.attach").contains(&true), "the streams of other terminals are polled in the same window");
        assert_eq!(history[0]["type"], "snapshot");
        assert!(history[0]["snapshot"]["history"].as_str().unwrap().contains("computed-42"), "{history:?}");
        // When the phone follows the terminals, the snapshot lists this one, running, in the project's directory
        let (meta, listed) = phone.stream(case, &chain, &["rpc:subscribeTerminalMetadata"], "subscribeTerminalMetadata", json!({}));
        let (agent, executor) = last_window(case);
        assert!(methods(&agent, "terminal.list").contains(&true));
        assert!(ran(&executor, "pty_list", true), "{executor}");
        let terminals = listed[0]["terminals"].as_array().unwrap();
        assert_eq!((terminals.len(), terminals[0]["threadId"].as_str(), terminals[0]["terminalId"].as_str(), terminals[0]["status"].as_str()), (1, Some("t1"), Some("term-1"), Some("running")));
        // When the terminal is cleared, then the attached stream says so, and the executor was written the clear
        success(&phone.call(case, &chain, &["rpc:terminal.clear"], "terminal.clear", ids.clone()));
        let (agent, executor) = last_window(case);
        assert_eq!(methods(&agent, "terminal.clear"), [true]);
        assert!(ran(&executor, "pty_write", true), "{executor}");
        watch_until(&mut phone, &attach, "the clear", |v| v.iter().any(|f| f["type"] == "cleared"));
        // When it is closed, the executor kills its shell, both streams say it is closed and the list drops it
        success(&phone.call(case, &chain, &["rpc:terminal.close"], "terminal.close", ids.clone()));
        let (agent, executor) = last_window(case);
        assert_eq!(methods(&agent, "terminal.close"), [true]);
        assert!(ran(&executor, "pty_close", true), "{executor}");
        // and the long attach the service held on the pty ended with it, answered by the executor
        watch_until(&mut phone, &observe, "the close on the observing stream", |v| v.iter().any(|f| f["type"] == "closed"));
        assert!(ran(&last_window(case).1, "pty_attach", true) || case.calls().iter().any(|c| hit(&c["executor"], "tool", "pty_attach").is_some_and(|h| h["ok"] == true)), "the attach never ended");
        let gone = watch_until(&mut phone, &meta, "the removal from the list", |v| v.iter().any(|f| f["type"] == "remove"));
        assert_eq!(gone.iter().find(|f| f["type"] == "remove"), Some(&json!({"type": "remove", "threadId": "t1", "terminalId": "term-1"})));
        // And writing to it now is refused as the app declares, after rca asked the executor (it looked for the pty and found none): the request arrived
        let refused = phone.calls.call("terminal.write", json!({"threadId": "t1", "terminalId": "term-1", "data": "x"}));
        assert_eq!(failure(&refused)["_tag"], "TerminalSessionLookupError");
    });
}

#[test]
fn a_terminal_cannot_start_outside_the_executors_root_and_the_executors_environment_does_not_reach_the_shell() {
    exec_testkit::case!(SUITE, |case| {
        // Given a paired phone and the executor, which holds the service's bearer in its own environment
        let chain = Chain::start();
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        let root = chain.repo.path().display().to_string();
        // When the phone opens a terminal in a directory outside the executor's root
        let outside = phone.calls.call("terminal.open", json!({"threadId": "t1", "terminalId": "term-1", "cwd": "/etc"}));
        // Then the executor refused it and the phone got the declared error: the request arrived and the pty action said no
        assert_eq!(failure(&outside)["_tag"], "TerminalCwdStatError");
        assert!(failure(&outside)["cause"].as_str().unwrap().to_lowercase().contains("outside"), "{outside}");
        // When it opens one in the root, and the shell prints its environment
        let opened = phone.call(case, &chain, &["rpc:terminal.open"], "terminal.open", json!({"threadId": "t1", "terminalId": "term-1", "cwd": root, "env": {"T3CODE_PROJECT_ROOT": root}}));
        let (agent, executor) = last_window(case);
        assert_eq!(success(&opened)["status"], json!("running"));
        assert_eq!(methods(&agent, "terminal.open"), [true]);
        assert!(ran(&executor, "pty_open", true), "{executor}");
        let (attach, _) = phone.stream(case, &chain, &["rpc:terminal.attach"], "terminal.attach", json!({"threadId": "t1", "terminalId": "term-1"}));
        success(&phone.call(case, &chain, &["rpc:terminal.write"], "terminal.write", json!({"threadId": "t1", "terminalId": "term-1", "data": "env; echo end-of-env-$((6 * 7))\n"})));
        let seen = watch_until(&mut phone, &attach, "the environment", |v| screen(v).contains("end-of-env-42\r\n"));
        // Then the bearer the executor holds is not in it
        assert!(!screen(&seen).contains(SECRET), "the shell saw the executor's secret");
        // And the phone's own project variable is
        assert!(screen(&seen).contains(&format!("T3CODE_PROJECT_ROOT={root}")), "{}", screen(&seen));
    });
}
