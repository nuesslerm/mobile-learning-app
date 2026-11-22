//! The core-tests step runs two stages side by side. A hung stage must end by our own hand, with a log that names it, before the step's own limit cancels the job without one.
use crate::workflow_steps::{read, running_as_root, step_script, Scratch};
use std::time::{Duration, Instant};

const STEP: &str = "Flow, typecheck, lint and unit tests";

struct Run {
    code: Option<i32>,
    out: String,
    took: Duration,
}

/// Runs the step's real script with stub stages. `flow` is the body of the flow stage; the unit stage prints two lines and passes.
fn step(s: &Scratch, flow: &str, as_user: bool) -> Run {
    s.stub("wf", &format!("case \"$*\" in\n  *--stage\\ flow-tests*) {flow};;\n  *--stage\\ typecheck-lint-unit-tests*) echo unit-line-1; echo unit-line-2;;\nesac"));
    let script = step_script(&read(".github/workflows/pipeline.yml"), STEP);
    std::fs::write(s.path("temp/plan.json"), "{}").unwrap();
    let env = [("RUN_FLOW", "true"), ("RUN_UNIT", "true"), ("SHARD", "1"), ("COUNT", "1"), ("FRESH", "false"), ("STAGE_LIMIT_S", "4"), ("STAGE_KILL_S", "1")];
    let started = Instant::now();
    let out = s.run(&script, &env, as_user, false);
    Run { code: out.status.code(), out: format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)), took: started.elapsed() }
}

fn running(pattern: &str) -> bool {
    std::process::Command::new("pgrep").args(["-f", pattern]).output().map(|o| !o.stdout.is_empty()).unwrap_or(false)
}

fn a_hung_stage_is_killed_by_the_step_with_a_diagnostic_block(as_user: bool) {
    // Given a flow stage that prints, starts a test binary that holds the stage's output, and hangs; the other stage passes
    let s = Scratch::new();
    let name = format!("hung-test-{}", s.root.file_name().unwrap().to_string_lossy().replace('.', ""));
    std::fs::copy("/bin/sleep", s.bin.join(&name)).unwrap();
    let flow = format!("for i in $(seq 1 60); do echo \"flow line $i\"; done; {name} 600 & exec sleep 600");

    // When the step runs with a 4 second stage limit
    let run = step(&s, &flow, as_user);

    // Then it ended by its own hand, well inside the bounds, and failed (not the 124 or 137 of an outer kill)
    assert!(run.took < Duration::from_secs(30), "took {:?}\n{}", run.took, run.out);
    assert!(matches!(run.code, Some(c) if c != 0 && c != 124 && c != 137), "{:?}\n{}", run.code, run.out);

    // And the log names the stage, shows its process tree with the test binary, the test binaries, and the last 40 lines of its output (the output was also streamed live, so only the block is judged)
    assert!(run.out.contains("::error title=stage timed out::flow-tests"), "{}", run.out);
    let tree = run.out.split("process tree").nth(1).unwrap_or_else(|| panic!("no process tree in the log:\n{}", run.out));
    assert!(tree.contains(&name), "the tree names the hung binary:\n{}", run.out);
    let alive = run.out.split("what was running in it").nth(1).and_then(|t| t.split("the last 40 lines").next()).unwrap_or_else(|| panic!("no running block:\n{}", run.out));
    assert!(alive.contains(&name), "the processes without a child name the hung binary:\n{}", run.out);
    let tail = run.out.split("the last 40 lines of its output").nth(1).and_then(|t| t.split("left in its session").next()).unwrap_or_else(|| panic!("no output block in the log:\n{}", run.out));
    assert!(tail.contains("flow line 60\n") && tail.contains("flow line 21\n") && !tail.contains("flow line 20\n"), "{tail}");

    // And the other stage's output and its pass are in the log, with no timeout named for it
    assert!(run.out.contains("unit-line-1") && run.out.contains("unit-line-2"), "{}", run.out);
    assert!(!run.out.contains("stage timed out::typecheck-lint-unit-tests"), "{}", run.out);

    // And nothing of the hung stage is left running
    assert!(!running(&name), "the hung test binary survived the step");
}

#[test]
fn a_hung_stage_is_killed_by_the_step_within_its_bound_and_the_log_names_it_as_root() {
    a_hung_stage_is_killed_by_the_step_with_a_diagnostic_block(false);
}

#[test]
fn a_hung_stage_is_killed_by_the_step_within_its_bound_and_the_log_names_it_as_an_unprivileged_user() {
    if !running_as_root() {
        // the run above is already the unprivileged one
        return;
    }
    a_hung_stage_is_killed_by_the_step_with_a_diagnostic_block(true);
}

fn two_healthy_stages_pass(as_user: bool) {
    // Given a flow stage that prints and passes, and a unit stage that does
    let s = Scratch::new();

    // When the step runs
    let run = step(&s, "echo flow-line-1", as_user);

    // Then it succeeds at once and both stages' output is in the log
    assert_eq!(run.code, Some(0), "{}", run.out);
    assert!(run.took < Duration::from_secs(15), "{:?}", run.took);
    assert!(run.out.contains("flow-line-1") && run.out.contains("unit-line-2"), "{}", run.out);
    assert!(!run.out.contains("::error"), "{}", run.out);
}

#[test]
fn two_healthy_stages_pass_the_step_as_root() {
    two_healthy_stages_pass(false);
}

#[test]
fn two_healthy_stages_pass_the_step_as_an_unprivileged_user() {
    if running_as_root() {
        two_healthy_stages_pass(true);
    }
}

#[test]
fn a_stage_that_fails_fails_the_step_and_is_named_without_the_timeout_block() {
    // Given a flow stage that exits 1 at once
    let s = Scratch::new();

    // When the step runs
    let run = step(&s, "echo broken; exit 1", false);

    // Then the step fails, the stage is named, and no timeout block is printed
    assert_eq!(run.code, Some(1), "{}", run.out);
    assert!(run.out.contains("::error title=flow-tests::the flow-tests stage failed on this machine"), "{}", run.out);
    assert!(!run.out.contains("stage timed out"), "{}", run.out);
}

#[test]
fn the_stage_limit_is_one_constant_under_the_step_limit_and_the_kill_grace_fits_before_it() {
    // Given the core-tests step
    let text = read(".github/workflows/pipeline.yml");
    let head = &text[text.find(&format!("- name: {STEP}")).unwrap()..];
    let head = head.split("run: |").next().unwrap();

    // When its limits are read
    let num = |key: &str| -> u32 { head.split(&format!("{key}: ")).nth(1).unwrap_or_else(|| panic!("no {key}")).split_whitespace().next().unwrap().trim_matches('\'').parse().unwrap() };
    let (step_min, stage_s, grace_s) = (num("timeout-minutes"), num("STAGE_LIMIT_S"), num("STAGE_KILL_S"));

    // Then a stage, its kill grace and the diagnostics fit with a minute to spare inside the step's own limit
    assert!(stage_s + grace_s + 60 <= step_min * 60, "stage {stage_s} s + grace {grace_s} s vs step {step_min} min");
    assert!(stage_s >= 15 * 60, "an honest cold run takes up to 15 minutes: {stage_s} s");
}
