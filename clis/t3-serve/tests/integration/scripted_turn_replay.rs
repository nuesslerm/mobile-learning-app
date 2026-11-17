//! A replay that scripts the agent CLI: the interaction's `cli.script` step writes a hung turn into the directory the chain's service starts its
//! CLIs from, so the message the interaction dispatches starts a turn on rca and the interrupt that follows stops it. The two agent methods answer
//! inside the window of the recorded call, which is what a row whose target is an agent method needs from its recording.
use crate::replay_features::{mimic, CASSETTES};
use crate::rig::*;
use crate::support::*;

const SOURCE: &str = "t3-v0.1.9-scenarios";
const INTERRUPT: &str = "t3-scn-command-run.interrupt";

/// A set of one interaction in a folder of its own: the scenario that dispatches a message and interrupts its run, on a thread of Claude's instance, with `cli.script claude hang` before it.
fn hung_turn_set(into: &std::path::Path) -> String {
    let from = format!("{CASSETTES}/{SOURCE}");
    let mut set = mimic_t3::load_set(&from);
    set.cassette.interactions.retain(|i| i.id == INTERRUPT);
    let script = set.cassette.interactions[0].input["script"].as_array_mut().unwrap();
    // the thread of the session prelude is a codex one, which rca has no driver for: the turn is on a thread of Claude's instance
    for entry in script.iter_mut() {
        if let Some(payload) = entry.get_mut("ws-send").and_then(|s| s.get_mut("payload")) {
            payload["threadId"] = json!("hung-thread");
        }
    }
    let create = json!({"await": "exit", "ws-send": {"_tag": "Request", "headers": [], "id": "c1", "tag": "orchestration.dispatchCommand", "payload": {"branch": null, "commandId": "cmd-hung-thread", "createdBy": "user", "creationSource": "mobile", "interactionMode": "default", "modelSelection": {"instanceId": "claudeAgent", "model": "sonnet"}, "projectId": "conformance-project", "runtimeMode": "full-access", "threadId": "hung-thread", "title": "Hung thread", "type": "thread.create", "worktreePath": null}}});
    script.splice(0..0, [json!({"cli.script": {"cli": "claude", "turns": [{"hang": true}]}}), create]);
    set.cassette.seal();
    let dir = into.join(SOURCE);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("cassette.json"), set.cassette.header_json()).unwrap();
    std::fs::write(dir.join("interactions.jsonl"), set.cassette.jsonl()).unwrap();
    for file in ["rules.json", "corpus.json", "allowlist.json"] {
        std::fs::copy(format!("{from}/{file}"), dir.join(file)).unwrap();
    }
    std::fs::copy(format!("{CASSETTES}/t3-prelude.json"), into.join("t3-prelude.json")).unwrap();
    let pin = std::fs::canonicalize(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tooling/t3-conformance/t3-pin.json")).unwrap();
    let toml = std::fs::read_to_string(format!("{CASSETTES}/t3-v0.1.9-recorded-3/mimic.toml")).unwrap().replace("../../../../tooling/t3-conformance/t3-pin.json", &pin.display().to_string());
    let toml: String = toml.lines().filter(|l| !l.starts_with("provision_file")).map(|l| format!("{l}\n")).collect();
    std::fs::write(dir.join("mimic.toml"), toml).unwrap();
    dir.display().to_string()
}

#[test]
fn a_replayed_call_that_scripts_a_hung_turn_has_the_start_and_the_interrupt_of_that_turn_in_its_window() {
    exec_testkit::case!(SUITE, |_case| {
        // Given a recorded interaction that scripts a hung Claude turn, dispatches a message to start it and then interrupts its run
        let work = tempfile::tempdir().unwrap();
        let dir = hung_turn_set(work.path());
        let chain = Chain::start();
        let set = mimic_t3::load_set(&dir);

        // When wf-mimic replays it against the chain with the chain's directory of scripted CLIs
        let (_run, trace) = mimic(&dir, &[INTERRUPT.to_string()], &chain, &mimic_t3::granted_scopes(&set), &work.path().join("out"));

        // Then the windows of the calls hold the agent's turn start and its interrupt, and the chain's claude was started once for the turn
        let methods: Vec<String> = trace["tests"].as_array().unwrap().iter().flat_map(|t| t["calls"].as_array().unwrap()).flat_map(|c| c["agent"].as_array().unwrap().iter().filter_map(|h| h["method"].as_str().map(String::from))).collect();
        assert!(methods.iter().any(|m| m == "turn.start") && methods.iter().any(|m| m == "turn.interrupt"), "{methods:?}\n{}", chain.evidence());
        assert_eq!(chain.cli.invocations("claude").len(), 1, "{}", chain.evidence());
    });
}
