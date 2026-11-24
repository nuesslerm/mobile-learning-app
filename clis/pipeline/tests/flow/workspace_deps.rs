//! `wf check --unused-workspace-deps` and `wf check --unused-deps`: the rule that no crate declares a workspace crate its sources never name,
//! and the declared-but-unused dependency list, on a throwaway repo.
use crate::support::{run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::Path;

fn put(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn krate(root: &Path, name: &str, deps: &[&str]) {
    put(root, "clis/Cargo.toml", "[workspace]\nmembers = [\"*\"]\n");
    let table: String = deps.iter().map(|d| format!("{d} = {{ path = \"../{d}\" }}\n")).collect();
    put(root, &format!("clis/{name}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\n[dependencies]\n{table}"));
    put(root, &format!("clis/{name}/src/lib.rs"), &deps.iter().map(|d| format!("use {};\n", d.replace('-', "_"))).collect::<String>());
}

fn run(root: &Path, verb: &str) -> RunResult {
    run_wf(&["check", verb], &RunOpts::default().cwd(root).clean_env())
}

/// A crate of its own workspace at `<dir>/<name>`, with path dependencies given as (name, path from the crate) and named in its source.
fn in_workspace(root: &Path, dir: &str, name: &str, deps: &[(&str, &str)]) {
    put(root, &format!("{dir}/Cargo.toml"), "[workspace]\nmembers = [\"*\"]\n");
    let table: String = deps.iter().map(|(d, path)| format!("{d} = {{ path = \"{path}\" }}\n")).collect();
    put(root, &format!("{dir}/{name}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\n[dependencies]\n{table}"));
    put(root, &format!("{dir}/{name}/src/lib.rs"), &deps.iter().map(|(d, _)| format!("use {};\n", d.replace('-', "_"))).collect::<String>());
}

#[test]
fn the_unused_dependency_list_names_a_declared_dependency_no_source_names() {
    // Given a crate that declares two path dependencies and names only one in its source
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    krate(root, "kit-core", &[]);
    krate(root, "kit-extra", &[]);
    krate(root, "user", &["kit-core", "kit-extra"]);
    put(root, "clis/user/src/lib.rs", "use kit_core::x;\n");
    // When the list is asked for
    let r = run(root, "--unused-deps");
    // Then only the dependency the source never names is listed
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(report["unused"], json!(["user: kit-extra"]));
}

#[test]
fn a_crate_that_declares_a_workspace_crate_its_sources_never_name_fails_the_check() {
    // Given a workspace whose crate `user` declares the path dependency `kit` but names only `m1` in its source
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    krate(root, "kit", &[]);
    krate(root, "m1", &[]);
    krate(root, "user", &["m1"]);
    put(root, "clis/user/Cargo.toml", "[package]\nname = \"user\"\n[dependencies]\nm1 = { path = \"../m1\" }\nkit = { path = \"../kit\" }\n");

    // When the check runs
    let r = run(root, "--unused-workspace-deps");

    // Then it fails naming the crate and the edge, and removing the edge makes it pass
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("user declares the workspace crate kit"), "{}", r.stderr);
    put(root, "clis/user/Cargo.toml", "[package]\nname = \"user\"\n[dependencies]\nm1 = { path = \"../m1\" }\n");
    let r = run(root, "--unused-workspace-deps");
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
}

#[test]
fn the_unused_dependency_list_covers_crates_of_every_workspace() {
    // Given a crate of a second workspace that declares a path dependency its source never names
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    in_workspace(root, "libraries", "base", &[]);
    in_workspace(root, "backend", "app", &[("base", "../../libraries/base")]);
    put(root, "backend/app/src/lib.rs", "");
    // When the list is asked for
    let r = run(root, "--unused-deps");
    // Then it names that crate
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(report["unused"], json!(["app: base"]));
}

#[test]
fn an_unused_workspace_dependency_across_workspaces_fails_until_the_source_names_it_or_the_edge_goes() {
    // Given a crate of another workspace that declares a path dependency its source never names
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    in_workspace(root, "libraries", "base", &[]);
    in_workspace(root, "backend", "app", &[("base", "../../libraries/base")]);
    put(root, "backend/app/src/lib.rs", "");
    // When the check runs, Then it fails naming the edge
    let r = run(root, "--unused-workspace-deps");
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("app declares the workspace crate base"), "{}", r.stderr);
    // And once the source names it the check passes
    put(root, "backend/app/src/lib.rs", "use base;\n");
    assert_eq!(run(root, "--unused-workspace-deps").exit_code, 0);
}
