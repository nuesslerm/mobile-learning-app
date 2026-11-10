//! `wf check --workspace-crossings`: a path dependency that leaves its Cargo workspace fails unless it is on the checked-in list, and the list only
//! shrinks, on a throwaway repo of several workspaces and a standalone package.
use crate::support::{run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::Path;

const LIST: &str = "clis/pipeline/data/workspace-crossings.json";

fn put(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn package(root: &Path, dir: &str, name: &str, deps: &str) {
    put(root, &format!("{dir}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\n[dependencies]\n{deps}"));
}

fn workspace(root: &Path, dir: &str, members: &[&str]) {
    let list: Vec<String> = members.iter().map(|m| format!("\"{m}\"")).collect();
    put(root, &format!("{dir}/Cargo.toml"), &format!("[workspace]\nmembers = [{}]\n", list.join(", ")));
}

/// Workspace `app` (members `a`, `b`) and a standalone `lib`; `a` reaches `b` inside its workspace and `lib` across it.
fn repo(root: &Path) {
    workspace(root, "app", &["a", "b"]);
    package(root, "app/b", "b", "");
    package(root, "app/a", "a", "b = { path = \"../b\" }\nlib = { path = \"../../lib\" }\n");
    package(root, "lib", "lib", "");
}

fn allow(root: &Path, entries: &[&str]) {
    put(root, LIST, &json!({ "allowed": entries }).to_string());
}

fn run(root: &Path) -> RunResult {
    run_wf(&["check", "--workspace-crossings"], &RunOpts::default().cwd(root).clean_env())
}

fn report(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.lines().next().unwrap()).unwrap()
}

#[test]
fn a_crossing_on_the_list_passes_and_a_dependency_inside_the_workspace_is_not_one() {
    // Given a path dependency across workspaces that is listed, and one inside a workspace
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    repo(root);
    allow(root, &["app/a -> lib"]);
    // When the check runs
    let r = run(root);
    // Then it passes and reports only the crossing
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let found = report(&r);
    assert_eq!(found["crossings"].as_array().unwrap().len(), 1);
    assert_eq!(found["crossings"][0]["dependency"], "lib");
}

#[test]
fn a_new_crossing_fails_naming_it() {
    // Given a listed crossing and a second package that takes the same standalone package by path
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    repo(root);
    allow(root, &["app/a -> lib"]);
    package(root, "app/c", "c", "lib = { path = \"../../lib\" }\n");
    workspace(root, "app", &["a", "b", "c"]);
    // When the check runs
    let r = run(root);
    // Then it fails on the new crossing only
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("app/c -> lib") && !r.stderr.contains("app/a -> lib"), "{}", r.stderr);
}

#[test]
fn a_standalone_package_is_its_own_workspace() {
    // Given two standalone packages, one depending on the other by path
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    package(root, "one", "one", "two = { path = \"../two\" }\n");
    package(root, "two", "two", "");
    allow(root, &[]);
    // When the check runs
    let r = run(root);
    // Then the edge is a crossing
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("one -> two"), "{}", r.stderr);
}

#[test]
fn removing_a_crossing_is_accepted_and_a_stale_entry_fails() {
    // Given a listed crossing that is then replaced by a registry version
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    repo(root);
    allow(root, &["app/a -> lib"]);
    package(root, "app/a", "a", "b = { path = \"../b\" }\nlib = \"1\"\n");
    // When the check runs with the entry still listed
    let stale = run(root);
    // Then the stale entry fails
    assert_eq!(stale.exit_code, 1);
    assert!(stale.stderr.contains("`app/a -> lib` no longer crosses"), "{}", stale.stderr);
    // And once the entry is removed it passes
    allow(root, &[]);
    assert_eq!(run(root).exit_code, 0);
}

#[test]
fn an_inherited_workspace_dependency_counts_its_consumers_and_dev_dependencies_cross_too() {
    // Given a workspace whose root declares a path dependency that two members inherit, and a dev-dependency crossing
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    put(root, "app/Cargo.toml", "[workspace]\nmembers = [\"a\", \"b\"]\n[workspace.dependencies]\nlib = { path = \"../lib\" }\n");
    package(root, "app/a", "a", "lib = { workspace = true }\n");
    put(root, "app/b/Cargo.toml", "[package]\nname = \"b\"\n[dev-dependencies]\nlib = { workspace = true }\n");
    package(root, "lib", "lib", "");
    allow(root, &[]);
    // When the check runs
    let r = run(root);
    // Then the crossing is named for the workspace root with both consumers counted
    assert_eq!(r.exit_code, 1);
    let found = report(&r);
    assert_eq!(found["crossings"][0]["manifest"], "app");
    assert_eq!(found["crossings"][0]["consumers"], 2);
}
