//! `wf check --ts-packages`: a TypeScript package outside `frontend/` fails unless it is on the checked-in list, and the list only shrinks.
use crate::support::{run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::Path;

const LIST: &str = "clis/pipeline/data/ts-packages.json";

fn put(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn package(root: &Path, dir: &str) {
    put(root, &format!("{dir}/package.json"), "{\"name\":\"p\"}");
}

fn allow(root: &Path, entries: &[&str]) {
    put(root, LIST, &json!({ "allowed": entries }).to_string());
}

fn run(root: &Path) -> RunResult {
    run_wf(&["check", "--ts-packages"], &RunOpts::default().cwd(root).clean_env())
}

#[test]
fn packages_under_frontend_and_listed_ones_pass() {
    // Given a package under frontend/, a listed one outside it, and the root manifest
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    package(root, "frontend/libraries/logging");
    package(root, "libraries/hook-runtime");
    package(root, ".");
    allow(root, &["libraries/hook-runtime"]);
    // When the check runs
    let r = run(root);
    // Then it passes and reports only the listed package
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let found: Value = serde_json::from_str(r.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(found["packages"], json!(["libraries/hook-runtime"]));
}

#[test]
fn a_new_package_outside_frontend_fails_naming_it() {
    // Given a listed package and a new one next to it
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    package(root, "libraries/hook-runtime");
    package(root, "libraries/new-thing");
    allow(root, &["libraries/hook-runtime"]);
    // When the check runs
    let r = run(root);
    // Then it fails on the new package only
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("libraries/new-thing") && !r.stderr.contains("libraries/hook-runtime:"), "{}", r.stderr);
}

#[test]
fn node_modules_and_fixture_trees_are_not_packages_and_a_stale_entry_fails() {
    // Given a package in node_modules, one in a test fixture, and a list entry whose package moved away
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    package(root, "libraries/node_modules/dep");
    package(root, "clis/parity/tests/fixtures/t3/pkg");
    allow(root, &["libraries/moved"]);
    // When the check runs
    let r = run(root);
    // Then only the stale entry fails
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("`libraries/moved` has no package.json"), "{}", r.stderr);
    assert!(!r.stderr.contains("node_modules") && !r.stderr.contains("fixtures"), "{}", r.stderr);
}
