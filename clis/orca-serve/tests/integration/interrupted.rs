//! The restart half of Continue, through the chain: a turn is running when the real rca-service is killed and started again over its own state, the
//! phone's history shows the turn cut with the stop row, and `agentSession.continueInterrupted` makes rca start one more turn in the same chat. A row
//! counts only when the agent method that does its work answered in the call's window.
use crate::agent_sessions::{answered, asked_nothing_of, create_params};
use crate::rig::*;
use exec_testkit::script::{Script, Turn};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const SESSION: &str = "claude_cut1";

fn said(text: &str) -> String {
    json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}).to_string()
}

fn history(phone: &mut Phone, case: &mut exec_testkit::Case, chain: &Chain) -> Value {
    let r = phone.call(case, chain, &["rpc:agentSession.history"], "agentSession.history", json!({"sessionId": SESSION, "direction": "tail", "limit": 200}));
    r["result"]["page"].clone()
}

fn stop_rows(page: &Value) -> Vec<Value> {
    page["items"].as_array().unwrap().iter().filter(|i| i["body"]["presentation"] == "orca-stop").cloned().collect()
}

#[test]
fn a_turn_cut_by_a_service_restart_shows_the_stop_row_and_continue_starts_one_more_turn_once() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain over a state directory, a CLI whose first turn says one thing and stays up and whose second turn answers, and a chat with a running turn
        let state = tempfile::tempdir().unwrap();
        let mut chain = Chain::start_restartable(state.path());
        chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("working on it")]).then_sleeping(60_000), Turn::replaying(&[said("carried on")])]).printing_version("0.0.0-flow"));
        let mut phone = Phone::connect(case, &chain);
        let made = phone.call(case, &chain, &["rpc:agentSession.create"], "agentSession.create", create_params(SESSION, "c1", None));
        assert_eq!(made["result"]["ok"], true, "{made}");
        let sent = phone.call(case, &chain, &["rpc:agentSession.send"], "agentSession.send", json!({"envelope": {"sessionId": SESSION, "clientOperationId": "s1", "expectedRuntimeFence": null, "payloadFingerprint": "fp-s1"}, "body": {"kind": "message", "role": "user", "blocks": [{"type": "text", "text": "build it"}]}}));
        assert_eq!(sent["result"]["ok"], true, "{sent}");
        chain.direct().wait_events(SESSION, "message.delta", 2);
        // When the service is killed and started again over the same state, and the phone reads the chat once rca has settled the turn
        chain.restart_service();
        let mut page = history(&mut phone, case, &chain);
        let deadline = Instant::now() + Duration::from_secs(60);
        while page["latestTurn"]["turn"]["state"] != "interrupted" {
            assert!(Instant::now() < deadline, "the turn never showed interrupted: {page}\n{}", chain.evidence());
            std::thread::sleep(Duration::from_millis(200));
            page = history(&mut phone, case, &chain);
        }
        // Then the turn is interrupted, the finish rca logged names the restart, and one stop row of the turn names the cause the phone offers Continue for
        let finishes = chain.direct().events(SESSION, "turn.finished");
        assert_eq!((finishes.len(), finishes[0]["reason"].clone(), finishes[0]["cause"].clone()), (1, json!("interrupted"), json!("restart")), "{finishes:?}");
        let rows = stop_rows(&page);
        assert_eq!(rows.len(), 1, "{page}");
        assert_eq!(rows[0]["body"]["orcaStop"], json!({"cause": "crash"}));
        let turn_item = page["items"].as_array().unwrap().iter().find(|i| i["body"]["kind"] == "turn").unwrap()["itemId"].clone();
        assert_eq!(rows[0]["turnScope"]["turnItemId"], turn_item);
        // And the executor has reconnected to the restarted service
        let mut rca = chain.direct();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !rca.call("workspace", "repo.list", json!({})).is_ok_and(|r| r["repos"].as_array().is_some_and(|l| l.iter().any(|w| w["workspace"] == "w1" && w["attached"] == true))) {
            assert!(Instant::now() < deadline, "the executor never reattached\n{}", chain.evidence());
            std::thread::sleep(Duration::from_millis(200));
        }
        // When the phone continues the cut turn
        let done = phone.call(case, &chain, &["rpc:agentSession.continueInterrupted"], "agentSession.continueInterrupted", json!({"sessionId": SESSION, "turnItemId": turn_item}));
        // Then rca started a turn, the answer is the accepted one, and the CLI was started a second time with the host's continue wording
        assert_eq!(done["result"], json!({"sessionId": SESSION, "outcome": "pending"}), "{done}\n{}", chain.evidence());
        answered(case, "turn.start");
        chain.direct().wait_events(SESSION, "turn.finished", 2);
        let starts = chain.cli.invocations("claude");
        assert_eq!(starts.len(), 2, "{starts:?}");
        assert!(starts[1].argv.iter().any(|a| a.contains("Orca restarted, so your previous reply was cut off")), "{:?}", starts[1].argv);
        // When the same Continue arrives again
        let again = phone.call(case, &chain, &["rpc:agentSession.continueInterrupted"], "agentSession.continueInterrupted", json!({"sessionId": SESSION, "turnItemId": turn_item}));
        // Then it is superseded and rca was asked for no turn
        assert_eq!(again["result"], json!({"sessionId": SESSION, "outcome": "superseded"}));
        asked_nothing_of(case, "turn.start");
        assert_eq!(chain.cli.invocations("claude").len(), 2);
    });
}
