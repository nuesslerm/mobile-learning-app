//! `wf check --blast-radius` and `wf check --unused-deps`: the blast radius of each crate of every Cargo workspace against its checked-in budget, the rule
//! that a low-level crate gains no edge to a heavy one, and the declared-but-unused dependency list, on a throwaway workspace.
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

fn budget(root: &Path, radius: Value, allowed: Value) {
    put(root, "clis/pipeline/data/blast-radius.json", &json!({ "radius": radius, "allowed_edges": allowed }).to_string());
}

fn run(root: &Path, verb: &str) -> RunResult {
    run_wf(&["check", verb], &RunOpts::default().cwd(root).clean_env())
}

/// kit under three dependants, and a heavy `top` over two crates.
fn workspace(root: &Path) {
    for name in ["a", "b", "c"] {
        krate(root, name, &["kit"]);
    }
    krate(root, "kit", &[]);
    krate(root, "m1", &[]);
    krate(root, "m2", &[]);
    krate(root, "top", &["m1", "m2"]);
}

fn radii(r: &RunResult) -> Value {
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    report["crates"].as_array().unwrap().iter().map(|c| (c["name"].as_str().unwrap().to_string(), c["radius"].clone())).collect()
}

#[test]
fn an_unchanged_tree_passes_and_reports_each_radius_with_the_stages_it_reaches() {
    // Given a workspace whose budget is its measured radii, and a flow test under one dependant
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    workspace(root);
    put(root, "clis/a/tests/flow.rs", "");
    budget(root, json!({ "kit": 3, "m1": 1, "m2": 1 }), json!({}));
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then it passes, kit's radius is its three dependants and the flow stage is counted among what a change to kit reaches
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(radii(&r)["kit"], 3);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    let kit = report["crates"].as_array().unwrap().iter().find(|c| c["name"] == "kit").unwrap();
    assert_eq!(kit["stages"]["flow"], 1, "{}", r.stdout);
}

#[test]
fn a_crate_that_gains_a_dependant_fails_with_the_path_that_pulls_it_in() {
    // Given the budgeted workspace and a new crate that depends on a dependant of kit
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    workspace(root);
    budget(root, json!({ "kit": 3, "m1": 1, "m2": 1 }), json!({}));
    krate(root, "d", &["c"]);
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then it fails, and names the new dependant's whole path down to kit
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("blast radius of kit is 4, over its budget 3") && r.stderr.contains("d -> c -> kit"), "{}", r.stderr);
}

#[test]
fn a_radius_under_its_budget_passes_and_says_to_lower_the_budget() {
    // Given a budget above what kit's dependants make today
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    workspace(root);
    budget(root, json!({ "kit": 5, "m1": 1, "m2": 1 }), json!({}));
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then it passes and the shrink is reported
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let report: Value = serde_json::from_str(&r.stdout).unwrap();
    assert!(report["shrunk"][0].as_str().unwrap().contains("kit is 3, under its budget 5"), "{}", r.stdout);
}

#[test]
fn a_low_level_crate_that_gains_an_edge_to_a_heavy_one_fails_unless_the_edge_is_allowed_with_a_reason() {
    // Given room in every radius budget, so only the edge rule can speak, and kit (three dependants) newly depending on top (two crates below it)
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    workspace(root);
    budget(root, json!({ "kit": 9, "m1": 9, "m2": 9, "top": 9 }), json!({}));
    krate(root, "kit", &["top"]);
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then it fails, naming the edge, both sides' sizes and what the heavy crate brings in
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("low-level crate kit (radius 3) depends on the heavy crate top (it pulls in 2 crates") && r.stderr.contains("kit -> top"), "{}", r.stderr);
    // And with the edge allowed under a reason the same tree passes
    budget(root, json!({ "kit": 9, "m1": 9, "m2": 9, "top": 9 }), json!({ "kit -> top": "kit owns the top-level wiring until it moves" }));
    assert_eq!(run(root, "--blast-radius").exit_code, 0);
    // And an empty reason does not allow it
    budget(root, json!({ "kit": 9, "m1": 9, "m2": 9, "top": 9 }), json!({ "kit -> top": " " }));
    assert_eq!(run(root, "--blast-radius").exit_code, 1);
}

#[test]
fn a_crate_with_no_budget_entry_may_have_no_dependants() {
    // Given a workspace with an empty budget, Then kit is over (its three dependants against nothing) and a crate nobody uses is not
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    workspace(root);
    let r = run(root, "--blast-radius");
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("blast radius of kit is 3") && r.stderr.contains("no budget entry") && !r.stderr.contains("blast radius of a "), "{}", r.stderr);
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
    budget(root, json!({ "kit": 1, "m1": 1, "user": 0 }), json!({}));

    // When the check runs
    let r = run(root, "--blast-radius");

    // Then it fails naming the crate and the edge, and removing the edge makes it pass
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("user declares the workspace crate kit"), "{}", r.stderr);
    put(root, "clis/user/Cargo.toml", "[package]\nname = \"user\"\n[dependencies]\nm1 = { path = \"../m1\" }\n");
    budget(root, json!({ "kit": 0, "m1": 1, "user": 0 }), json!({}));
    let r = run(root, "--blast-radius");
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
}

/// A crate of its own workspace at `<dir>/<name>`, with path dependencies given as (name, path from the crate) and named in its source.
fn in_workspace(root: &Path, dir: &str, name: &str, deps: &[(&str, &str)]) {
    put(root, &format!("{dir}/Cargo.toml"), "[workspace]\nmembers = [\"*\"]\n");
    let table: String = deps.iter().map(|(d, path)| format!("{d} = {{ path = \"{path}\" }}\n")).collect();
    put(root, &format!("{dir}/{name}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\n[dependencies]\n{table}"));
    put(root, &format!("{dir}/{name}/src/lib.rs"), &deps.iter().map(|(d, _)| format!("use {};\n", d.replace('-', "_"))).collect::<String>());
}

#[test]
fn a_path_dependency_across_workspaces_widens_the_radius_of_the_crate_it_names() {
    // Given a library in one workspace, an app in another that names it by path, and a budget that gives the library no dependant
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    in_workspace(root, "libraries", "base", &[]);
    in_workspace(root, "backend", "app", &[("base", "../../libraries/base")]);
    budget(root, json!({}), json!({}));
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then it fails naming the app across the boundary, and a budget of one passes
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("blast radius of base is 1, over its budget 0") && r.stderr.contains("app -> base"), "{}", r.stderr);
    budget(root, json!({ "base": 1 }), json!({}));
    let r = run(root, "--blast-radius");
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(radii(&r)["base"], 1);
}

#[test]
fn a_dependency_inherited_from_the_workspace_root_by_path_is_an_edge() {
    // Given a workspace root that maps `base` to a path in another workspace and a member that inherits it
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    in_workspace(root, "libraries", "base", &[]);
    put(root, "backend/Cargo.toml", "[workspace]\nmembers = [\"app\"]\n[workspace.dependencies]\nbase = { path = \"../libraries/base\" }\n");
    put(root, "backend/app/Cargo.toml", "[package]\nname = \"app\"\n[dependencies]\nbase = { workspace = true }\n");
    put(root, "backend/app/src/lib.rs", "use base;\n");
    budget(root, json!({ "base": 1 }), json!({}));
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then app is a dependant of base
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(radii(&r)["base"], 1);
}

#[test]
fn a_version_dependency_on_a_crate_of_another_workspace_is_no_edge() {
    // Given an app that takes `base` from a registry while a crate of that name lives in another workspace
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    in_workspace(root, "libraries", "base", &[]);
    in_workspace(root, "backend", "app", &[]);
    put(root, "backend/app/Cargo.toml", "[package]\nname = \"app\"\n[dependencies]\nbase = \"1.0\"\n");
    budget(root, json!({}), json!({}));
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then a release is not a source edit: base has no dependant
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(radii(&r)["base"], 0);
}

#[test]
fn two_workspaces_that_share_a_crate_name_are_measured_apart() {
    // Given a `core` in two workspaces, and an app that depends on only one of them
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    in_workspace(root, "libraries", "core", &[]);
    in_workspace(root, "backend", "core", &[]);
    in_workspace(root, "services", "app", &[("core", "../../backend/core")]);
    budget(root, json!({ "core (backend/core)": 1 }), json!({}));
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then only the one it names has a dependant
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let radii = radii(&r);
    assert_eq!((radii["core (backend/core)"].clone(), radii["core (libraries/core)"].clone()), (json!(1), json!(0)));
}

#[test]
fn a_low_level_crate_gains_no_edge_to_a_heavy_crate_of_another_workspace() {
    // Given a base crate with two dependants in its own workspace, and a heavy crate in another workspace with two crates below it
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    in_workspace(root, "libraries", "m1", &[]);
    in_workspace(root, "libraries", "m2", &[]);
    in_workspace(root, "libraries", "base", &[]);
    in_workspace(root, "libraries", "u1", &[("base", "../base")]);
    in_workspace(root, "libraries", "u2", &[("base", "../base")]);
    in_workspace(root, "backend", "top", &[("m1", "../../libraries/m1"), ("m2", "../../libraries/m2")]);
    in_workspace(root, "libraries", "base", &[("top", "../../backend/top")]);
    budget(root, json!({ "base": 9, "m1": 9, "m2": 9, "top": 9 }), json!({}));
    // When the check runs
    let r = run(root, "--blast-radius");
    // Then it fails on the edge from the low-level crate to the heavy one, and an edge allowed with a reason passes
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("low-level crate base") && r.stderr.contains("heavy crate top") && r.stderr.contains("base -> top"), "{}", r.stderr);
    budget(root, json!({ "base": 9, "m1": 9, "m2": 9, "top": 9 }), json!({ "base -> top": "base owns the wiring until it moves" }));
    assert_eq!(run(root, "--blast-radius").exit_code, 0);
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
fn an_unused_workspace_dependency_passes_only_while_listed_with_a_reason_and_a_stale_entry_fails() {
    // Given a crate of another workspace that declares a path dependency its source never names, and a budget that gives base its one dependant
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    in_workspace(root, "libraries", "base", &[]);
    in_workspace(root, "backend", "app", &[("base", "../../libraries/base")]);
    put(root, "backend/app/src/lib.rs", "");
    budget(root, json!({ "base": 1 }), json!({}));
    // When the check runs, Then it fails naming the edge and the file to list it in
    let r = run(root, "--blast-radius");
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("app declares the workspace crate base") && r.stderr.contains("app -> base"), "{}", r.stderr);
    // And listed with a reason it passes, an empty reason does not count
    put(root, "clis/pipeline/data/blast-radius.json", &json!({ "radius": { "base": 1 }, "allowed_unused": { "app -> base": "removed with the next app change" } }).to_string());
    assert_eq!(run(root, "--blast-radius").exit_code, 0);
    put(root, "clis/pipeline/data/blast-radius.json", &json!({ "radius": { "base": 1 }, "allowed_unused": { "app -> base": " " } }).to_string());
    assert_eq!(run(root, "--blast-radius").exit_code, 1);
    // And once the source names it the entry is stale and fails
    put(root, "backend/app/src/lib.rs", "use base;\n");
    put(root, "clis/pipeline/data/blast-radius.json", &json!({ "radius": { "base": 1 }, "allowed_unused": { "app -> base": "removed with the next app change" } }).to_string());
    let r = run(root, "--blast-radius");
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("allowed_unused `app -> base` is no longer an unused"), "{}", r.stderr);
}
