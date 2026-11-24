//! The `files:` globs of the ast-grep rules come from the package types (`wf check --layout`, region `ast-grep-files`), and the rules
//! they feed catch planted violations in the packages the types name. The ast-grep tests of the rules themselves are `backend/arch-tests`.
use crate::support::{run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

const MANIFEST: &str = include_str!("../../data/package-types.json");
const REPO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
const REGION: &str = "  # BEGIN generated:ast-grep-files\n  # END generated:ast-grep-files\n";

fn put(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

fn krate(root: &Path, dir: &str, name: &str, arch: &str, files: &[(&str, &str)]) {
    put(root, &format!("{dir}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\n[package.metadata.arch]\n{arch}\n"));
    for (file, body) in files {
        put(root, &format!("{dir}/src/{file}"), body);
    }
}

/// The manifest of the repository with only its ast-grep regions kept, so a throwaway repo needs only the rule files.
fn repo_with(entries: &[Value]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let mut manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    manifest["generated"] = Value::Array(entries.to_vec());
    put(dir.path(), "clis/pipeline/data/package-types.json", &manifest.to_string());
    dir
}

fn entry(file: &str, selector: Value) -> Value {
    let mut e = json!({ "file": file, "region": "ast-grep-files" });
    e.as_object_mut().unwrap().extend(selector.as_object().unwrap().clone());
    e
}

fn layout(root: &Path, extra: &[&str]) -> RunResult {
    let mut args = vec!["check", "--layout"];
    args.extend(extra);
    run_wf(&args, &RunOpts::default().cwd(root).clean_env())
}

fn drift(r: &RunResult) -> Vec<String> {
    let found: Value = serde_json::from_str(r.stdout.lines().next().unwrap_or("{}")).unwrap();
    found["findings"].as_array().unwrap().iter().filter(|f| f["pass"] == "drift").map(|f| format!("{}: {}", f["package"].as_str().unwrap(), f["message"].as_str().unwrap())).collect()
}

fn globs(root: &Path, rel: &str) -> Vec<String> {
    read(root, rel).lines().filter_map(|l| l.trim().strip_prefix("- \"").map(|g| g.trim_end_matches('"').to_string())).collect()
}

/// A service with a core and an rs crate, a contract crate, and a flat crate that declares itself pure.
fn workspace(root: &Path) {
    krate(root, "backend/svc/core", "svc-core", "type = \"rust-core\"", &[("lib.rs", ""), ("domain/mod.rs", ""), ("application/mod.rs", "")]);
    krate(root, "backend/svc/rs", "svc-rs", "type = \"rust-rs\"", &[("lib.rs", ""), ("main.rs", ""), ("api/mod.rs", ""), ("infrastructure/mod.rs", "")]);
    krate(root, "libraries/agent-api/wire", "wire", "type = \"rust-contract\"", &[("lib.rs", "")]);
    krate(root, "backend/svc/group", "svc-group", "type = \"rust-flat-adapter\"\npure = true", &[("lib.rs", "")]);
    krate(root, "backend/svc/adapter", "svc-adapter", "type = \"rust-flat-adapter\"", &[("lib.rs", "")]);
    krate(root, "clis/tool/core", "tool-core", "type = \"rust-flat-core\"", &[("lib.rs", ""), ("domain/mod.rs", "")]);
}

#[test]
fn the_globs_of_a_rule_are_the_ring_directories_of_the_packages_of_its_types() {
    // Given rule files with an empty generated region, one per selector: a ring, two rings, a ring limited to two types, a type and a pure override
    let dir = repo_with(&[
        entry("backend/arch-rules/domain.yml", json!({ "rings": ["domain"] })),
        entry("backend/arch-rules/inner.yml", json!({ "rings": ["domain", "application"] })),
        entry("backend/arch-rules/trait.yml", json!({ "rings": ["domain", "api"], "types": ["rust-core", "rust-rs"] })),
        entry("backend/arch-rules/contract.yml", json!({ "types": ["rust-contract"] })),
        entry("backend/arch-rules/core.yml", json!({ "types": ["rust-core"], "pureOverride": true })),
        entry("clis/arch-rules/flat.yml", json!({ "types": ["rust-flat-core"] })),
    ]);
    let root = dir.path();
    workspace(root);
    for rule in ["domain", "inner", "trait", "contract", "core"] {
        put(root, &format!("backend/arch-rules/{rule}.yml"), &format!("id: {rule}\nfiles:\n{REGION}"));
    }
    put(root, "clis/arch-rules/flat.yml", &format!("id: flat\nfiles:\n{REGION}"));
    // When the regions are written
    layout(root, &["--write"]);
    // Then each file lists what its selector names, spelled from the directory of its own sgconfig.yml
    assert_eq!(globs(root, "backend/arch-rules/domain.yml"), ["**/clis/tool/core/src/domain/**/*.rs", "**/svc/core/src/domain/**/*.rs"], "a flat core's optional domain ring is a domain too");
    assert_eq!(globs(root, "backend/arch-rules/inner.yml"), ["**/clis/tool/core/src/domain/**/*.rs", "**/svc/core/src/application/**/*.rs", "**/svc/core/src/domain/**/*.rs"]);
    assert_eq!(globs(root, "backend/arch-rules/trait.yml"), ["**/svc/core/src/domain/**/*.rs", "**/svc/rs/src/api/**/*.rs"], "the flat core's domain ring may hold a port");
    assert_eq!(globs(root, "backend/arch-rules/contract.yml"), ["**/libraries/agent-api/wire/src/**/*.rs"]);
    assert_eq!(globs(root, "backend/arch-rules/core.yml"), ["**/svc/core/src/**/*.rs", "**/svc/group/src/**/*.rs"], "a flat adapter that declares pure = true is a core; one that does not is not");
    assert_eq!(globs(root, "clis/arch-rules/flat.yml"), ["**/tool/core/src/**/*.rs"]);
    assert!(drift(&layout(root, &[])).is_empty());
}

#[test]
fn a_package_typed_after_the_globs_were_written_is_drift_until_they_are_regenerated() {
    // Given written globs of the core rule and a second core typed afterwards
    let dir = repo_with(&[entry("backend/arch-rules/core.yml", json!({ "types": ["rust-core"] }))]);
    let root = dir.path();
    workspace(root);
    put(root, "backend/arch-rules/core.yml", &format!("id: core\nfiles:\n{REGION}"));
    layout(root, &["--write"]);
    krate(root, "backend/other/core", "other-core", "type = \"rust-core\"", &[("lib.rs", ""), ("domain/mod.rs", "")]);
    // When the check runs, and then writes
    let stale = drift(&layout(root, &["--strict"]));
    layout(root, &["--write"]);
    // Then the stale file is named, and the regenerated one covers the new crate the day it is typed
    assert!(stale.iter().any(|m| m.starts_with("backend/arch-rules/core.yml: region ast-grep-files is stale")), "{stale:?}");
    assert_eq!(globs(root, "backend/arch-rules/core.yml"), ["**/other/core/src/**/*.rs", "**/svc/core/src/**/*.rs"]);
    assert!(drift(&layout(root, &["--strict"])).is_empty());
}

#[test]
fn a_hand_listed_glob_inside_the_region_is_overwritten() {
    // Given a rule whose region holds a hand-written glob for an untyped crate
    let dir = repo_with(&[entry("backend/arch-rules/contract.yml", json!({ "types": ["rust-contract"] }))]);
    let root = dir.path();
    workspace(root);
    put(root, "backend/arch-rules/contract.yml", "id: contract\nfiles:\n  # BEGIN generated:ast-grep-files\n  - \"**/libraries/hand/listed/src/**/*.rs\"\n  # END generated:ast-grep-files\nignores:\n  - \"**/src/testing.rs\"\n");
    // When the region is written
    layout(root, &["--write"]);
    // Then only the typed package is listed, and the lines outside the region are kept
    assert_eq!(globs(root, "backend/arch-rules/contract.yml").len(), 2, "{}", read(root, "backend/arch-rules/contract.yml"));
    let text = read(root, "backend/arch-rules/contract.yml");
    assert!(text.contains("  - \"**/libraries/agent-api/wire/src/**/*.rs\"\n  # END") && text.contains("ignores:\n  - \"**/src/testing.rs\""), "{text}");
}

fn ast_grep() -> Option<std::path::PathBuf> {
    let bin = Path::new(REPO).join("node_modules/.bin/ast-grep");
    bin.exists().then_some(bin)
}

fn scan(root: &Path, bin: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for config in ["backend/sgconfig.yml", "clis/sgconfig.yml"] {
        let out = Command::new(bin).args(["scan", "--config", config, "--json=stream"]).current_dir(root).output().unwrap();
        found.extend(String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).map(|v| (v["ruleId"].as_str().unwrap().to_string(), v["file"].as_str().unwrap().to_string())));
    }
    found
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(e.file_name());
        if e.path().is_dir() { copy_dir(&e.path(), &dest) } else { std::fs::copy(e.path(), dest).unwrap(); }
    }
}

#[test]
fn the_real_rules_catch_planted_violations_in_the_packages_the_types_name_and_pass_clean_ones() {
    let Some(bin) = ast_grep() else { return eprintln!("skipped: {REPO}/node_modules/.bin/ast-grep is not installed") };
    // Given the repository's own ast-grep rules and manifest regions in a throwaway repo, and typed packages holding one planted violation each:
    // a trait in a domain and in an api, a glob import in an application, a layer break in a domain, I/O in a contract, a pure group and a flat core
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    let entries: Vec<Value> = manifest["generated"].as_array().unwrap().iter().filter(|e| e["region"] == "ast-grep-files").cloned().collect();
    let dir = repo_with(&entries);
    let root = dir.path();
    for sub in ["backend/arch-rules", "backend/arch-utils", "clis/arch-rules", "clis/arch-utils"] {
        copy_dir(&Path::new(REPO).join(sub), &root.join(sub));
    }
    for config in ["backend/sgconfig.yml", "clis/sgconfig.yml"] {
        std::fs::copy(Path::new(REPO).join(config), root.join(config)).unwrap();
    }
    for entry in &entries {
        let file = entry["file"].as_str().unwrap();
        let text = read(root, file);
        let (head, tail) = (text.split("# BEGIN generated:ast-grep-files").next().unwrap(), text.split("# END generated:ast-grep-files").nth(1).unwrap());
        put(root, file, &format!("{head}# BEGIN generated:ast-grep-files\n  # END generated:ast-grep-files{tail}"));
    }
    krate(root, "backend/svc/core", "svc-core", "type = \"rust-core\"", &[
        ("lib.rs", ""),
        ("domain/mod.rs", "pub trait Clock { fn now(&self) -> u64; }\n"),
        ("domain/layers.rs", "use crate::application::ports::Port;\n"),
        ("application/mod.rs", "use crate::domain::*;\n"),
    ]);
    krate(root, "backend/svc/rs", "svc-rs", "type = \"rust-rs\"", &[("lib.rs", ""), ("main.rs", ""), ("api/mod.rs", "pub trait Ext { fn go(&self); }\n"), ("infrastructure/mod.rs", "")]);
    krate(root, "libraries/agent-api/wire", "wire", "type = \"rust-contract\"", &[("lib.rs", "use std::fs;\n")]);
    krate(root, "backend/svc/group", "svc-group", "type = \"rust-flat-adapter\"\npure = true", &[("lib.rs", "use std::env;\n")]);
    krate(root, "clis/tool/core", "tool-core", "type = \"rust-flat-core\"", &[("lib.rs", "use std::process::Command;\n")]);
    krate(root, "backend/svc/adapter", "svc-adapter", "type = \"rust-flat-adapter\"", &[("lib.rs", "use std::fs;\nuse std::env;\npub trait Port {}\n")]);
    layout(root, &["--write"]);
    // When ast-grep scans the throwaway repo
    let found = scan(root, &bin);
    // Then each planted violation is reported by the rule of its layer, in its own file
    let has = |rule: &str, file: &str| found.iter().any(|(r, f)| r == rule && f.ends_with(file));
    assert!(has("no-trait-in-layer", "svc/core/src/domain/mod.rs") && has("no-trait-in-layer", "svc/rs/src/api/mod.rs"), "{found:?}");
    assert!(has("layer-imports-are-explicit", "svc/core/src/application/mod.rs"), "{found:?}");
    assert!(has("domain-depends-on-nothing-outer", "svc/core/src/domain/layers.rs"), "{found:?}");
    assert!(has("contract-crate-does-no-io", "libraries/agent-api/wire/src/lib.rs"), "{found:?}");
    assert!(has("core-crate-does-no-io", "svc/group/src/lib.rs"), "{found:?}");
    assert!(has("core-does-no-io", "tool/core/src/lib.rs"), "{found:?}");
    // And an impure flat adapter that declares no pure = true is not held to the purity rules
    assert!(!found.iter().any(|(_, f)| f.ends_with("svc/adapter/src/lib.rs")), "{found:?}");
}
