//! The agent-facing commands that need the whole router: the agent-proof benchmark and the MCP guard. claude and adb are argv-recording
//! stubs (the VM's dependency manifest is in `clis/deploy`).
use crate::support::{repo_root, run_wf, RunOpts, SlotSandbox};
use serde_json::{json, Value};

const PROOF_ADB: &str = "echo \"$*\" >> \"$0.log\"\ncase \"$*\" in\n  *'shell ls'*) printf '/sdcard/wf-record-1.mp4\\n';;\n  *devices*) echo 'List of devices attached';;\nesac\nexit 0";

#[test]
fn agent_proof_asks_an_agent_for_a_device_script_runs_it_recorded_and_reports_the_arm() {
    let s = SlotSandbox::new("wf-flow-agent-proof-");
    s.stub("adb", PROOF_ADB);
    let reply_path = s.repo.join("reply.json").display().to_string();
    s.stub("claude", &format!("echo \"$*\" >> \"$0.log\"\ncat > \"$0.prompt\"\ncat '{reply_path}'"));
    let answer = |reply: Value| std::fs::write(&reply_path, reply.to_string()).unwrap();
    let proof = |arm: &str, extra: &[&str]| {
        let out = s.repo.join("out").display().to_string();
        let mut args = vec!["bench", "agent-proof", "--arm", arm, "--base", "http://127.0.0.1:9/app", "--platform", "android", "--out", out.as_str()];
        args.extend_from_slice(extra);
        let r = s.run(&args, &[]);
        let v = serde_json::from_str(r.stdout.trim()).unwrap_or(Value::Null);
        (r, v)
    };

    // Given a claude that answers prose around two fenced blocks, with cache tokens and two models in its usage
    answer(json!({ "type": "result", "is_error": false, "result": "Here:\n```lua\nlog('ran with ' .. env.base)\n```\nand\n```\nnot this\n```", "total_cost_usd": 0.0123, "usage": { "input_tokens": 100, "output_tokens": 50, "cache_creation_input_tokens": 300, "cache_read_input_tokens": 4000 }, "modelUsage": { "claude-haiku-stub": { "outputTokens": 12 }, "claude-sonnet-stub": { "outputTokens": 50 } } }));
    // When the sonnet arm runs
    let (r, v) = proof("sonnet", &[]);
    // Then the first block ran, recorded, with the base argument, and the arm reports the busiest model, every token, the cost and timings
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!((v["arm"].clone(), v["model"].clone(), v["passed"].clone()), (json!("sonnet"), json!("claude-sonnet-stub"), json!(true)));
    assert_eq!((v["costUsd"].clone(), v["tokens"].clone(), v["script"].clone()), (json!(0.0123), json!(4450.0), json!("log('ran with ' .. env.base)\n")));
    assert!(v["recording"].as_str().is_some_and(|p| p.ends_with("sonnet-rec/run.mp4")), "{v}");
    let (agent_s, run_s, wall_s) = (v["agentS"].as_f64().unwrap(), v["runS"].as_f64().unwrap(), v["wallS"].as_f64().unwrap());
    assert!((wall_s - (agent_s + run_s)).abs() <= 0.11 && v.get("error").is_none(), "{v}");
    assert!(std::fs::read_to_string(s.repo.join("out/sonnet-run.log")).unwrap().contains("ran with http://127.0.0.1:9/app"));
    // and claude was asked as sonnet with the task and the whole device API on stdin
    let prompt = std::fs::read_to_string(s.bin_dir.join("claude.prompt")).unwrap();
    assert!(prompt.contains("ProblemEditorRunLocalButton") && prompt.contains("ProblemEditorRunOutcomeTotals") && prompt.contains("device:extended_wait_until{"), "{prompt}");
    assert!(s.calls("claude")[0].contains("--model sonnet --output-format json"), "{:?}", s.calls("claude"));
    assert!(s.calls("adb").iter().any(|c| c.contains("screenrecord")), "{:?}", s.calls("adb"));

    // Given a bare answer whose script fails, Then passed is false with the error and wf exits 1
    answer(json!({ "type": "result", "is_error": false, "result": "\n  error('boom')\n\n", "total_cost_usd": 0.01, "usage": { "input_tokens": 1, "output_tokens": 1 }, "modelUsage": { "m": {} } }));
    let (r, v) = proof("sonnet", &[]);
    assert_eq!((r.exit_code, v["passed"].clone(), v["script"].clone()), (1, json!(false), json!("error('boom')\n")), "{}", r.stderr);
    assert!(v["error"].as_str().is_some_and(|e| e.contains("boom")), "{v}");

    // Given claude reports an error, Then the arm fails with its message
    answer(json!({ "is_error": true, "result": "Not logged in" }));
    let (r, _) = proof("sonnet", &[]);
    assert!(r.exit_code != 0 && format!("{}{}", r.stdout, r.stderr).contains("Not logged in"), "{}{}", r.stdout, r.stderr);

    // Given the scripted arm, Then no agent is asked, agent time is zero, and the reference script ran
    let calls_before = s.calls("claude").len();
    std::fs::write(s.repo.join("ref.lua"), "log('floor')\n").unwrap();
    let (r, v) = proof("scripted", &["--script", &s.repo.join("ref.lua").display().to_string()]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!((v["model"].clone(), v["agentS"].clone(), v["passed"].clone()), (json!("none"), json!(0.0), json!(true)));
    assert_eq!(s.calls("claude").len(), calls_before);

    // Given an arm that does not exist, Then it is refused naming the arms
    let (r, _) = proof("gpt", &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("sonnet, opencode, scripted"), "{}", r.stderr);
}

#[test]
fn the_mcp_guard_trusts_untrusted_connectors_only_to_read() {
    // When the MCP guard sees a first-party server, a read-shaped and a write-shaped tool on untrusted connectors
    let guard = |tool: &str| run_wf(&["agent-config", "mcp-guard"], &RunOpts::default().input(&json!({ "tool_name": tool, "tool_input": {} }).to_string()));
    // Then it stays silent, allows, and denies with the reason byte for byte
    assert_eq!((guard("mcp__github__create_pull_request").exit_code, guard("mcp__github__create_pull_request").stdout), (0, String::new()));
    assert_eq!(guard("mcp__Notion__notion-fetch").stdout, "{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"allow\"}}\n");
    let deny = guard("mcp__Slack__slack_send_message");
    assert_eq!((deny.exit_code, deny.stdout), (0, std::fs::read_to_string(repo_root().join("clis/agent-config-sync/tests/flow/golden-guard-deny.json")).unwrap()));
    // and input that is not JSON fails loud, reported through wf
    let r = run_wf(&["agent-config", "mcp-guard"], &RunOpts::default().input("not json"));
    assert_eq!((r.exit_code, r.stdout.as_str()), (1, ""));
    assert!(r.stderr.lines().next().unwrap().starts_with("agent-config-sync NOT ENFORCING — failed: ") && r.stderr.contains("\nwf: agent-config-sync NOT ENFORCING — failed: "), "{}", r.stderr);
}
