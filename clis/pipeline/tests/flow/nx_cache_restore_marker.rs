//! The marker the nx-cache action leaves at the start of a restore says "killed by the step's time limit" only for a restore that never reached its record step.
use crate::workflow_steps::{read, step_script, Scratch};

const ACTION: &str = ".github/actions/nx-cache/action.yml";

fn record_env<'a>() -> Vec<(&'a str, &'a str)> {
    vec![("STEP_OUTCOME", "success"), ("HIT", "false"), ("MATCHED", "nx-core-tests-1of3-Linux-X64-CI-core-1-1"), ("KEY", "nx-core-tests-1of3-Linux-X64-CI-core-2-1"), ("STAGE", "core-tests-1of3"), ("STARTED_MS", "1"), ("GITHUB_ENV", "/dev/null")]
}

fn never_finished_env<'a>() -> Vec<(&'a str, &'a str)> {
    vec![("KEY", "nx-core-tests-1of3-Linux-X64-CI-core-2-1"), ("STAGE", "core-tests-1of3")]
}

/// The shell of the save-side step, run only when its `if:` condition holds: the action's own condition is the marker file alone, so the script's guard is the test.
fn never_finished(s: &Scratch, path_only: bool) -> String {
    let script = step_script(&read(ACTION), "A restore that never finished");
    let out = s.run(&script, &never_finished_env(), false, path_only);
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn a_restore_that_finished_without_wf_leaves_no_marker_so_the_save_does_not_call_it_killed() {
    // Given the start step's marker and a restore that finished, on a runner where wf is not on PATH (it cannot write the result file)
    let s = Scratch::new();
    std::fs::write(s.path("temp/nx-cache.started"), "1").unwrap();
    let record = step_script(&read(ACTION), "Record the restore");

    // When the record step runs, and then the save-side check of a restore that never finished
    let out = s.run(&record, &record_env(), false, true);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let said = never_finished(&s, true);

    // Then the marker is gone and the save side prints no error and records no timeout
    assert!(!s.path("temp/nx-cache.started").exists());
    assert!(!said.contains("::error"), "{said}");
}

#[test]
fn a_restore_that_finished_while_wf_failed_to_record_it_leaves_no_marker_either() {
    // Given a wf that fails every call (the record is best effort), and the start step's marker
    let s = Scratch::new();
    s.stub("wf", "echo \"wf $*\" >> \"$RUNNER_TEMP/wf.calls\"; exit 1");
    std::fs::write(s.path("temp/nx-cache.started"), "1").unwrap();
    let record = step_script(&read(ACTION), "Record the restore");

    // When the record step and then the save-side check run
    let out = s.run(&record, &record_env(), false, false);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let said = never_finished(&s, false);

    // Then the save side neither prints an error nor records a timeout through wf
    assert!(!said.contains("::error"), "{said}");
    let calls = std::fs::read_to_string(s.path("temp/wf.calls")).unwrap_or_default();
    assert!(!calls.contains("--outcome timeout"), "{calls}");
}

#[test]
fn a_restore_killed_by_the_step_limit_still_leaves_its_marker_and_the_save_records_one_timeout() {
    // Given the start step's marker and no record step (the restore was killed, so the record step never ran)
    let s = Scratch::new();
    s.stub("wf", "echo \"wf $*\" >> \"$RUNNER_TEMP/wf.calls\"");
    std::fs::write(s.path("temp/nx-cache.started"), "1700000000000").unwrap();

    // When the save-side check runs
    let said = never_finished(&s, false);

    // Then it records the timeout through wf, once, with the reason, and removes the marker
    let calls = std::fs::read_to_string(s.path("temp/wf.calls")).unwrap();
    assert_eq!(calls.lines().filter(|l| l.contains("--outcome timeout")).count(), 1, "{calls}");
    assert!(calls.contains("killed by the step's time limit"), "{calls}");
    assert!(!s.path("temp/nx-cache.started").exists(), "{said}");
}

#[test]
fn a_restore_killed_on_a_runner_without_wf_is_an_error_annotation() {
    // Given the marker of a killed restore and no wf on PATH
    let s = Scratch::new();
    std::fs::write(s.path("temp/nx-cache.started"), "1").unwrap();

    // When the save-side check runs
    let said = never_finished(&s, true);

    // Then the annotation says the restore failed because it was killed
    assert!(said.contains("::error title=cache restore failed::") && said.contains("killed by the step's time limit"), "{said}");
}

#[test]
fn the_save_side_guard_waits_on_the_marker_alone() {
    // Given the action
    let text = read(ACTION);

    // When the save-side step is read
    let step = text.split("- name: A restore that never finished").nth(1).unwrap().split("- name: Decide").next().unwrap();

    // Then its only guard is the marker (the result file is written by a command the action tolerates failing)
    assert!(step.contains("[ -f \"$RUNNER_TEMP/nx-cache.started\" ]") && !step.contains("nx-cache.restore.json"), "{step}");
}
