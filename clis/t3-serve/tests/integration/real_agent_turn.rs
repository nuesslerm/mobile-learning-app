//! A turn of the agent behind the phone, asserted only where it is deterministic: the methods the agent API answered in the call's window, the run's
//! final state and the shape of the thread (a user message, then an assistant one); never the reply's words or ids. The same assertions run against the
//! scripted CLI by default and the real ones when the run was started through `wf mimic exec --clis real[:names]` (it hands the rig the binaries' directory and credentials as environment). Only the scripted run is verified:
//! no real-CLI run has passed them yet (a real turn may not produce the executor's `run_command` event), and nothing compares the two runs.
use crate::chain::{hit, last_window, said};
use crate::rig::*;
use crate::support::*;
use exec_testkit::script::{Script, Turn};

#[test]
fn a_trivial_prompt_runs_a_turn_whose_methods_and_final_state_hold_for_the_cli_in_use_scripted_unless_real_is_asked() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain (scripted claude by default, the real CLIs when asked) and a paired phone with a thread on Claude's instance
        let chain = Chain::start();
        if chain.real.as_ref().is_none_or(|r| !r.names.iter().any(|n| n == "claude")) {
            chain.cli.script("claude", &Script::of(vec![Turn::replaying(&[said("OK")])]).printing_version("0.0.0-flow"));
        }
        let mut phone = Phone::connect(case, &chain);
        phone.seed(case, &chain);
        // When the phone sends the trivial prompt and the run ends
        let projection = phone.message(case, &chain, "t1", "c-ok", "reply with OK");
        // Then the agent API answered the turn's methods in the window, the CLI ran through the executor, and the run completed
        let (agent, executor) = last_window(case);
        let methods: Vec<&str> = agent.iter().map(|(m, _)| m.as_str()).collect();
        assert!(agent.iter().all(|(_, ok)| *ok) && methods.contains(&"turn.start"), "{agent:?}\n{}", chain.evidence());
        assert!(hit(&executor, "tool", "session_send").is_some_and(|h| h["ok"] == true), "{executor}");
        assert_eq!(projection["runs"].as_array().and_then(|r| r.last()).map(|r| r["status"].clone()), Some(json!("completed")), "{projection}");
        // And the thread's shape: the user's message first, then an assistant message that has some text (which text is not compared)
        let roles: Vec<(&str, bool)> = projection["messages"].as_array().unwrap().iter().map(|m| (m["role"].as_str().unwrap(), m["text"].as_str().is_some_and(|t| !t.is_empty()))).collect();
        assert_eq!(roles.first(), Some(&("user", true)), "{roles:?}");
        assert!(roles.iter().any(|(role, filled)| *role == "assistant" && *filled), "{roles:?}");
    });
}
