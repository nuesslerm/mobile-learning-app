//! The e2e Nx targets are `cache: true`; the e2e workflow has to carry the Nx cache between runs for that to mean anything.
use crate::support::repo_root;
use std::process::Command;

fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("{path} is readable: {e}"))
}

#[test]
fn the_e2e_workflow_restores_the_nx_cache_before_it_asks_which_tests_are_replayed_and_saves_it_after_the_run() {
    // Given the e2e workflow
    let text = read(".github/workflows/e2e-tests.yml");

    // When the order of its steps is read
    let restore = text.find("mode: restore\n          stage: ${{ inputs.platform }}-e2e-tests-").expect("an Nx restore of the platform's stage");
    let pick = text.find("- name: Which e2e tests").expect("the pick step");
    let run = text.find("- name: Run the e2e tests").expect("the run step");
    let save = text.find("mode: save\n          stage: ${{ inputs.platform }}-e2e-tests-").expect("an Nx save of the platform's stage");

    // Then the probe that decides which tests replay reads a restored cache, and the save comes after the run
    assert!(restore < pick, "the restore must precede the replay probe");
    assert!(run < save, "the save must follow the run");

    // And both steps are the shared action, bounded, never failing the job, named by platform and shard so no two shards write one key
    for at in [restore, save] {
        let head = &text[text[..at].rfind("- uses: ./.github/actions/nx-cache").expect("the action is used")..at];
        assert!(head.contains("timeout-minutes: 5") && head.contains("continue-on-error: true"), "{head}");
        assert!(text[at..].lines().nth(1).unwrap().contains("e2e-tests-${{ inputs.shard || 1 }}of${{ inputs.shards || 1 }}"), "{}", &text[at..at + 200]);
    }

    // And the save runs after a failed test too (the tests that passed are kept), but never for a cancelled job
    let save_step = &text[text[..save].rfind("- uses: ./.github/actions/nx-cache").unwrap()..save];
    assert!(save_step.contains("!cancelled()"), "{save_step}");
}

/// The command of an e2e target the way the generator writes it, for a test `probe` on `platform` in a scratch workspace.
fn e2e_command(platform: &str) -> String {
    let dir = tempfile::tempdir().expect("a scratch workspace");
    std::fs::create_dir_all(dir.path().join("e2e")).unwrap();
    std::fs::write(dir.path().join("e2e/probe.lua"), format!("--[[---\nplatform: {platform}\nfeatures: *\n---]]\nprint('x')\n")).unwrap();
    let script = format!(
        "const g = require({:?}); const [, fn] = g.createNodes; const out = fn(['e2e/probe.lua'], {{}}, {{ workspaceRoot: {:?} }}); const p = Object.values(out[0][1].projects)[0]; process.stdout.write(JSON.stringify({{ target: p.targets['e2e-{platform}'] }}));",
        repo_root().join("tooling/nx/e2e-tests.js"),
        dir.path()
    );
    let out = Command::new("node").arg("-e").arg(script).output().expect("node runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["target"]["cache"], true, "{v}");
    assert!(v["target"]["outputs"][0].as_str().unwrap().contains(".e2e-results/"), "{v}");
    v["target"]["options"]["command"].as_str().unwrap().to_string()
}

fn run_target(command: &str, wf_exit: i32) -> (bool, bool) {
    let dir = tempfile::tempdir().expect("a scratch dir");
    let bin = dir.path().join("wf");
    // the run's own summary is what the target copies out as its cached output
    std::fs::write(&bin, format!("#!/bin/bash\nmkdir -p \"$WF_E2E_OUT/probe\"\necho '{{}}' > \"$WF_E2E_OUT/probe/summary.json\"\nexit {wf_exit}\n")).unwrap();
    std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let status = Command::new("/bin/bash")
        .arg("-c")
        .arg(command)
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("WF_E2E_BIN", &bin)
        .env("WF_E2E_OUT", dir.path().join("out"))
        .env("WF_E2E_SHA", "abc")
        .env("WF_E2E_FLAGS", "")
        .status()
        .expect("bash runs");
    (status.success(), dir.path().join(".e2e-results/android/probe.json").exists())
}

#[test]
fn a_failed_e2e_test_leaves_no_result_and_a_failing_exit_so_nx_never_caches_it_and_a_passed_one_does() {
    // Given the target the generator writes for an Android test
    let command = e2e_command("android");

    // When the test fails
    let (ok, wrote) = run_target(&command, 1);

    // Then the target exits non-zero (Nx stores only exit 0) and no result file exists to store
    assert!((ok, wrote) == (false, false), "ok={ok} wrote={wrote}");

    // And when the test passes, the result is written and the target succeeds
    let (ok, wrote) = run_target(&command, 0);
    assert!(ok && wrote, "ok={ok} wrote={wrote}");
}
