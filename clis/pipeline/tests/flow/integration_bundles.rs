//! The integration shard's publish step as the workflow runs it: every run record an integration task leaves under its Nx output folder
//! (Orca's and T3's chains, a replay, vm-ops-api and remote-chat-agent) becomes a run bundle the shard uploads, with its trace's run named.
use crate::support::{repo_root, wf_binary};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

/// The `run:` script of the workflow step called `name` in pipeline.yml, read as text (no YAML crate is in the workspace).
fn step_script(name: &str) -> String {
    let text = std::fs::read_to_string(repo_root().join(".github/workflows/pipeline.yml")).unwrap();
    let mut lines = text.lines().skip_while(|l| l.trim() != format!("- name: {name}"));
    lines.next().unwrap_or_else(|| panic!("pipeline.yml has no step {name}"));
    let mut script: Vec<&str> = Vec::new();
    let mut in_run = false;
    for line in lines {
        if line.trim_start().starts_with("- ") && line.len() - line.trim_start().len() == 6 {
            break;
        }
        if in_run {
            script.push(line.get(10..).unwrap_or(""));
        } else if line.trim() == "run: |" {
            in_run = true;
        }
    }
    assert!(!script.is_empty(), "step {name} has a run script");
    script.join("\n")
}

fn put(root: &Path, rel: &str, body: &Value) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body.to_string()).unwrap();
}

fn report(script: &str, started: i64) -> Value {
    json!({ "schema": 1, "verdict": "pass", "ok": true, "exitCode": 0, "script": script, "platform": "integration", "sha": "abc1234", "startedMs": started, "finishedMs": started + 2000, "durationS": 2.0,
        "recordDir": format!("/work/{script}"), "cases": [{ "name": "a::b", "verdict": "pass", "ms": 5 }], "warnings": [] })
}

/// An authored trace and Nx output layout, not one a real task wrote: it proves the publish step handles these shapes, not that the tasks emit them.
fn trace(app: &str, suite: &str, passed: usize, failed: usize) -> Value {
    let tests: Vec<Value> = (0..passed + failed).map(|i| json!({ "test": format!("t{i}"), "verdict": if i < passed { "pass" } else { "fail" }, "calls": [] })).collect();
    json!({ "schema": "method-trace/1", "app": app, "suite": suite, "sha": "abc1234", "complete": true, "tests": tests })
}

#[test]
fn given_the_run_records_of_every_integration_task_when_the_shard_publishes_then_each_becomes_a_bundle_naming_the_run_its_trace_holds() {
    // Given the folders the Nx outputs of Orca's chain and replay, T3's chain, vm-ops-api and remote-chat-agent leave, each with its run record
    let work = tempfile::tempdir().unwrap();
    let out = ".agents/tmp/test-output";
    type Traced = Option<(&'static str, usize, usize)>;
    let layouts: [(&str, &str, Traced); 5] = [
        ("orca-serve/cargo-integration/orca-serve-integration", "clis/orca-serve", Some(("orca", 8, 0))),
        ("orca-serve/replay-orca-map-agent/wf-mimic-orca-recorded/map_agent/orca-v1", "clis/orca-serve/replay", Some(("orca", 5, 2))),
        ("t3-serve/cargo-integration/t3-serve-integration", "clis/t3-serve", Some(("t3", 29, 0))),
        ("vm-ops-api/test-integration/vm-ops-api-integration", "backend/vm-ops-api/integration", None),
        ("remote-chat-agent/test-integration/wf-mimic-voice-edit-dsl", "backend/remote-chat-agent", Some(("t3", 3, 0))),
    ];
    for (i, (dir, script, traced)) in layouts.iter().enumerate() {
        put(work.path(), &format!("{out}/{dir}/report.json"), &report(script, 1_792_000_000_000 + i as i64 * 10_000));
        if let Some((app, passed, failed)) = traced {
            let suite = dir.rsplit('/').find(|p| p.starts_with("wf-mimic-") || p.ends_with("-integration")).unwrap();
            put(work.path(), &format!("{out}/{dir}/trace.json"), &trace(app, suite, *passed, *failed));
        }
    }
    // and the layouts are the ones the tasks declare as Nx outputs
    let nx = std::fs::read_to_string(repo_root().join("tooling/nx/cargo-crates.js")).unwrap();
    assert!(nx.contains("const RECORDS = '.agents/tmp/test-output'") && nx.contains("/cargo-integration`"), "the crates' integration outputs");
    for (package, folder) in [("backend/vm-ops-api", "vm-ops-api/test-integration"), ("backend/remote-chat-agent", "remote-chat-agent/test-integration")] {
        let manifest = std::fs::read_to_string(repo_root().join(package).join("package.json")).unwrap();
        assert!(manifest.contains(&format!("{{workspaceRoot}}/.agents/tmp/test-output/{folder}")), "{package} declares {folder} as an Nx output");
    }

    // When the workflow's publish step runs over them, with `wf` the binary under test
    let shim = tempfile::tempdir().unwrap();
    std::fs::write(shim.path().join("wf"), format!("#!/bin/sh\nexec {} \"$@\"\n", wf_binary().display())).unwrap();
    std::fs::set_permissions(shim.path().join("wf"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let temp = work.path().join("runner-temp");
    let run = Command::new("bash")
        .args(["-ec", &step_script("Publish the integration run bundles")])
        .current_dir(work.path())
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", shim.path().display()))
        .env("HOME", work.path())
        .env("RUNNER_TEMP", &temp)
        .env("WF_TEST_OUTPUT_DIR", temp.join("test-output"))
        .output()
        .unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));

    // Then the step passes and the index lists five bundles, where the shard's upload step reads them
    assert!(run.status.success(), "{said}");
    let index: Value = serde_json::from_str(&std::fs::read_to_string(temp.join("test-runs/index.json")).unwrap()).unwrap();
    assert_eq!(index["runs"].as_array().unwrap().len(), 5, "{said}");
    let workflow = std::fs::read_to_string(repo_root().join(".github/workflows/pipeline.yml")).unwrap();
    assert!(workflow.contains("name: test-runs-integration-tests-${{ matrix.shard }}\n          path: ${{ runner.temp }}/test-runs"), "the upload reads the folder the publish writes");

    // And each bundle with a trace names its run: suite, sha, complete, tests and passed; the one without lists none
    let mut runs: Vec<(String, String, bool, u64, u64)> = Vec::new();
    for entry in index["runs"].as_array().unwrap() {
        let bundle: Value = serde_json::from_str(&std::fs::read_to_string(temp.join("test-runs").join(entry["bundle"].as_str().unwrap())).unwrap()).unwrap();
        for r in bundle["traceRuns"].as_array().unwrap() {
            assert_eq!(r["sha"], "abc1234", "{bundle}");
            runs.push((r["app"].as_str().unwrap().into(), r["suite"].as_str().unwrap().into(), r["complete"] == true, r["tests"].as_u64().unwrap(), r["passed"].as_u64().unwrap()));
        }
        assert_eq!(bundle["traceRuns"].as_array().unwrap().len(), bundle["traces"].as_array().unwrap().len());
    }
    runs.sort();
    let want = |app: &str, suite: &str, tests: u64, passed: u64| (app.to_string(), suite.to_string(), true, tests, passed);
    assert_eq!(runs, [want("orca", "orca-serve-integration", 8, 8), want("orca", "wf-mimic-orca-recorded", 7, 5), want("t3", "t3-serve-integration", 29, 29), want("t3", "wf-mimic-voice-edit-dsl", 3, 3)]);
}
