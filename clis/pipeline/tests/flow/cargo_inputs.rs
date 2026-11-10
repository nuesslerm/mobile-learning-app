//! `wf check --cargo-inputs`: a package whose cached scripts only run cargo must pin `externalDependencies: []`, or Nx hashes the CPU-specific npm layout
//! into the task and CI never replays what the push gate seeded (run 1208, `@backend/vm-ops-api:test:integration`). On a throwaway repo, and on this one.
use crate::support::{repo_root, run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::Path;

fn package(root: &Path, dir: &str, package: Value) {
    let path = root.join(dir).join("package.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, package.to_string()).unwrap();
}

fn pinned(target: &str) -> Value {
    json!({ "targets": { target: { "inputs": ["default", { "externalDependencies": [] }] } } })
}

fn run(root: &Path) -> RunResult {
    run_wf(&["check", "--cargo-inputs"], &RunOpts::default().cwd(root).clean_env())
}

#[test]
fn a_cargo_only_integration_script_without_the_pin_is_named_and_fails() {
    // Given a package whose integration script builds then tests with cargo and lists no Nx inputs
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    package(root, "backend/svc", json!({ "scripts": { "test:integration": "cargo build --locked -p svc && SVC_CMD=\"$PWD/svc\" cargo test --locked" } }));

    // When the check runs
    let r = run(root);

    // Then it exits 1 and names the package and the target
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("backend/svc test:integration runs only cargo"), "{}", r.stderr);
}

#[test]
fn a_pinned_cargo_package_and_a_package_that_runs_node_pass() {
    // Given one cargo-only package that pins the inputs, one flow script that runs vitest, one that mixes node into a cargo line, and a copy under node_modules
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    package(root, "backend/svc", json!({ "scripts": { "test:flow": "cargo test --locked" }, "nx": pinned("test:flow") }));
    package(root, "app/web", json!({ "scripts": { "test:flow": "vitest run" } }));
    package(root, "app/mixed", json!({ "scripts": { "test:flow": "node gen.js && cargo test" } }));
    package(root, "node_modules/dep", json!({ "scripts": { "test:flow": "cargo test" } }));

    // When the check runs
    let r = run(root);

    // Then it passes and counted only the one cargo-only script
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let report: Value = serde_json::from_str(r.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(report["checked"], 1, "{}", r.stdout);
}

#[test]
fn a_pin_on_the_other_target_does_not_cover_the_flow_script() {
    // Given a package that pins test:integration only
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    package(root, "backend/svc", json!({ "scripts": { "test:flow": "cargo test", "test:integration": "cargo test" }, "nx": pinned("test:integration") }));

    // When the check runs
    let r = run(root);

    // Then only test:flow is named
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("test:flow runs only cargo") && !r.stderr.contains("test:integration runs only cargo"), "{}", r.stderr);
}

#[test]
fn every_cargo_only_package_of_this_repository_pins_its_inputs() {
    // Given the repository's own packages
    // When the check runs on them
    let r = run(&repo_root());

    // Then none is named
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
}
