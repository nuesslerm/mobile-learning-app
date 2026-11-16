//! `wf check --layout`: a package's declared type, placement, dependency edges and generated regions, on throwaway repos with planted violations.
use crate::support::{run_wf, RunOpts, RunResult};
use serde_json::Value;
use std::path::Path;

const MANIFEST: &str = include_str!("../../data/package-types.json");

fn put(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "clis/pipeline/data/package-types.json", &manifest_with(serde_json::json!([])));
    dir
}

fn manifest_with(generated: Value) -> String {
    let mut manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    manifest["generated"] = generated;
    manifest.to_string()
}

fn crate_at(root: &Path, dir: &str, name: &str, ty: Option<&str>, deps: &str, files: &[&str]) {
    let arch = ty.map(|t| format!("[package.metadata.arch]\ntype = \"{t}\"\n")).unwrap_or_default();
    put(root, &format!("{dir}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\n{arch}{deps}"));
    for file in files {
        put(root, &format!("{dir}/src/{file}"), "");
    }
}

/// A service: a core with a domain and a use-case ring, and the rs crate over it.
fn service(root: &Path) {
    crate_at(root, "backend/svc/core", "svc-core", Some("rust-core"), "", &["lib.rs", "domain/mod.rs", "application/mod.rs"]);
    crate_at(root, "backend/svc/rs", "svc-rs", Some("rust-rs"), "[dependencies]\nsvc-core = { path = \"../core\" }\n", &["lib.rs", "main.rs", "api/mod.rs", "infrastructure/mod.rs"]);
}

fn run(root: &Path, extra: &[&str]) -> RunResult {
    let mut args = vec!["check", "--layout"];
    args.extend(extra);
    run_wf(&args, &RunOpts::default().cwd(root).clean_env())
}

fn report(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.lines().next().unwrap_or("{}")).unwrap_or_else(|_| panic!("not JSON: {}", r.stdout))
}

fn messages(report: &Value, pass: &str) -> Vec<String> {
    report["findings"].as_array().unwrap().iter().filter(|f| f["pass"] == pass).map(|f| format!("{}: {}", f["package"].as_str().unwrap(), f["message"].as_str().unwrap())).collect()
}

#[test]
fn a_well_formed_service_passes_every_pass() {
    // Given a core and an rs crate laid out as their types say
    let dir = repo();
    service(dir.path());
    // When the layout check runs strictly
    let r = run(dir.path(), &["--strict"]);
    // Then both packages pass and nothing is reported
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let found = report(&r);
    assert_eq!((found["packages"].as_u64(), found["typed"].as_u64(), found["passing"].as_u64()), (Some(2), Some(2), Some(2)));
    assert_eq!(found["findings"].as_array().unwrap().len(), 0);
}

#[test]
fn a_core_without_a_ring_is_reported_and_only_strict_fails() {
    // Given a core that has neither a domain nor an application ring
    let dir = repo();
    crate_at(dir.path(), "backend/svc/core", "svc-core", Some("rust-core"), "", &["lib.rs"]);
    // When the check runs without and with --strict
    let report_only = run(dir.path(), &[]);
    let strict = run(dir.path(), &["--strict"]);
    // Then the placement finding names the missing ring, and only the strict run exits 1
    assert_eq!(report_only.exit_code, 0, "{}", report_only.stderr);
    assert_eq!(strict.exit_code, 1);
    let found = report(&report_only);
    assert!(messages(&found, "placement").iter().any(|m| m.contains("backend/svc/core") && m.contains("one of src/domain, src/application")), "{found}");
    assert_eq!(found["passing"], 0);
}

#[test]
fn a_directory_the_type_does_not_know_and_a_forbidden_ring_are_reported() {
    // Given an rs crate with a stray ring directory and a core with an api ring
    let dir = repo();
    service(dir.path());
    put(dir.path(), "backend/svc/rs/src/misc/mod.rs", "");
    put(dir.path(), "backend/svc/core/src/api/mod.rs", "");
    // When the check runs
    let found = report(&run(dir.path(), &[]));
    // Then each is a placement finding on its own package
    let placement = messages(&found, "placement");
    assert!(placement.iter().any(|m| m.starts_with("backend/svc/rs:") && m.contains("src/misc is not part of type rust-rs")), "{placement:?}");
    assert!(placement.iter().any(|m| m.starts_with("backend/svc/core:") && m.contains("forbids src/api")), "{placement:?}");
}

#[test]
fn a_core_that_depends_on_the_rs_crate_breaks_the_edge_rule() {
    // Given a core that names the rs crate built on it
    let dir = repo();
    service(dir.path());
    crate_at(dir.path(), "backend/svc/core", "svc-core", Some("rust-core"), "[dependencies]\nsvc-rs = { path = \"../rs\" }\n", &["lib.rs", "domain/mod.rs"]);
    // When the check runs
    let found = report(&run(dir.path(), &[]));
    // Then the edge is reported against the core, with the types it may depend on
    let edges = messages(&found, "edges");
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert!(edges[0].contains("backend/svc/core: depends on svc-rs (rust-rs)") && edges[0].contains("may depend on [rust-core, rust-contract]"), "{edges:?}");
}

#[test]
fn a_testkit_is_reachable_through_dev_and_optional_dependencies_only() {
    // Given a testkit, a crate that dev-depends on it, one that holds it behind a feature, and one that depends on it outright
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/kit", "kit", Some("rust-testkit"), "", &["lib.rs"]);
    crate_at(root, "libraries/a", "a", Some("rust-contract"), "[dev-dependencies]\nkit = { path = \"../kit\" }\n", &["lib.rs"]);
    crate_at(root, "libraries/b", "b", Some("rust-contract"), "[dependencies]\nkit = { path = \"../kit\", optional = true }\n", &["lib.rs"]);
    crate_at(root, "libraries/c", "c", None, "[dependencies]\nkit = { path = \"../kit\" }\n", &["lib.rs"]);
    // When the check runs
    let found = report(&run(root, &[]));
    // Then only the outright dependency is reported, even from a package with no type
    let edges = messages(&found, "edges");
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert!(edges[0].starts_with("libraries/c: depends on kit (rust-testkit), which is reachable only through dev-dependencies"), "{edges:?}");
}

#[test]
fn a_pure_crate_naming_an_io_crate_is_reported() {
    // Given a contract crate that depends on an HTTP client
    let dir = repo();
    crate_at(dir.path(), "libraries/wire", "wire", Some("rust-contract"), "[dependencies]\nreqwest = \"0.12\"\n", &["lib.rs"]);
    // When the check runs
    let found = report(&run(dir.path(), &[]));
    // Then the dependency is reported
    assert!(messages(&found, "edges").iter().any(|m| m.contains("a pure type depends on the I/O crate reqwest")), "{found}");
}

#[test]
fn a_package_with_no_type_or_an_unknown_one_is_not_listed() {
    // Given a package that declares nothing and one that declares a type the manifest lacks
    let dir = repo();
    crate_at(dir.path(), "clis/plain", "plain", None, "", &["lib.rs"]);
    crate_at(dir.path(), "clis/odd", "odd", Some("rust-wizard"), "", &["lib.rs"]);
    put(dir.path(), "frontend/libraries/lib/package.json", "{\"name\":\"@lib/lib\"}");
    // When the check runs strictly
    let r = run(dir.path(), &["--strict"]);
    // Then each is a membership finding and none counts as typed
    assert_eq!(r.exit_code, 1);
    let found = report(&r);
    assert_eq!(found["typed"], 0);
    let membership = messages(&found, "membership");
    assert!(membership.iter().any(|m| m.starts_with("clis/plain: declares no type")), "{membership:?}");
    assert!(membership.iter().any(|m| m.starts_with("clis/odd: type rust-wizard is not in")), "{membership:?}");
    assert!(membership.iter().any(|m| m.starts_with("frontend/libraries/lib: declares no type")), "{membership:?}");
}

fn feature(root: &Path, dir: &str, name: &str, deps: &str, rings: &[&str]) {
    put(root, &format!("{dir}/package.json"), &format!("{{\"name\":\"{name}\",\"arch\":{{\"type\":\"hexagonal-feature\"}},\"dependencies\":{{{deps}}}}}"));
    put(root, &format!("{dir}/src/index.ts"), "");
    for ring in rings {
        put(root, &format!("{dir}/src/{ring}/index.ts"), "");
    }
}

const RINGS: [&str; 5] = ["domain", "state", "ui", "di", "routes"];

#[test]
fn a_feature_missing_a_ring_or_depending_on_another_feature_is_reported() {
    // Given one complete feature, and one without a ui ring that depends on the first
    let dir = repo();
    let root = dir.path();
    feature(root, "frontend/app/features/alpha", "@feature/alpha", "", &RINGS);
    feature(root, "frontend/app/features/beta", "@feature/beta", "\"@feature/alpha\":\"workspace:*\"", &["domain", "state", "di", "routes"]);
    // When the check runs
    let found = report(&run(root, &[]));
    // Then the missing ring and the feature-to-feature edge are reported against the second
    let all: Vec<String> = found["findings"].as_array().unwrap().iter().map(|f| format!("{}: {}", f["package"].as_str().unwrap(), f["message"].as_str().unwrap())).collect();
    assert!(all.iter().any(|m| m.starts_with("frontend/app/features/beta:") && m.contains("requires one of src/ui")), "{all:?}");
    assert!(all.iter().any(|m| m.starts_with("frontend/app/features/beta: depends on @feature/alpha (hexagonal-feature)")), "{all:?}");
    assert_eq!(all.iter().filter(|m| m.starts_with("frontend/app/features/alpha")).count(), 0, "{all:?}");
}

#[test]
fn an_in_app_feature_is_a_project_with_an_arch_key_and_its_widgets_are_allowed() {
    // Given an in-app feature whose project.json declares a widget directory, and one with a stray directory
    let dir = repo();
    let root = dir.path();
    for (name, extra) in [("good", ",\"widgets\":[\"editor\"]"), ("bad", "")] {
        let base = format!("frontend/app/features/{name}");
        put(root, &format!("{base}/project.json"), &format!("{{\"name\":\"feature-{name}\",\"arch\":{{\"type\":\"hexagonal-feature\"{extra}}}}}"));
        put(root, &format!("{base}/index.ts"), "");
        for ring in RINGS {
            put(root, &format!("{base}/{ring}/index.ts"), "");
        }
        put(root, &format!("{base}/editor/index.ts"), "");
    }
    // When the check runs
    let found = report(&run(root, &[]));
    // Then only the feature that did not declare the widget is reported
    let placement = messages(&found, "placement");
    assert_eq!(placement.len(), 1, "{placement:?}");
    assert!(placement[0].starts_with("frontend/app/features/bad: editor is not part of type hexagonal-feature"), "{placement:?}");
}

#[test]
fn a_generated_region_that_no_longer_matches_its_generator_is_drift() {
    // Given a manifest that declares a generated region of two files, one fresh and one stale
    let dir = repo();
    let root = dir.path();
    let mut manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    let names: String = manifest["types"].as_object().unwrap().keys().map(|k| format!("- {k}\n")).collect();
    manifest["generated"] = serde_json::json!([{ "file": "docs/fresh.md", "region": "types" }, { "file": "docs/stale.md", "region": "types" }, { "file": "docs/bare.md", "region": "types" }]);
    put(root, "clis/pipeline/data/package-types.json", &manifest.to_string());
    put(root, "docs/fresh.md", &format!("# t\n<!-- BEGIN generated:types -->\n{names}<!-- END generated:types -->\n"));
    put(root, "docs/stale.md", "<!-- BEGIN generated:types -->\n- rust-core\n<!-- END generated:types -->\n");
    put(root, "docs/bare.md", "no markers\n");
    // When the check runs
    let found = report(&run(root, &[]));
    // Then the stale and the unmarked file are drift, the fresh one is not
    let drift = messages(&found, "drift");
    assert_eq!(drift.len(), 2, "{drift:?}");
    assert!(drift.iter().any(|m| m.starts_with("docs/stale.md: region types is stale")), "{drift:?}");
    assert!(drift.iter().any(|m| m.starts_with("docs/bare.md: region types has no BEGIN/END")), "{drift:?}");
}

const LS_REGION: &str = "# BEGIN generated:ls-lint\n# END generated:ls-lint\n";

fn ls_repo() -> tempfile::TempDir {
    let dir = repo();
    put(dir.path(), "clis/pipeline/data/package-types.json", &manifest_with(serde_json::json!([{ "file": ".ls-lint.yml", "region": "ls-lint" }])));
    put(dir.path(), ".ls-lint.yml", &format!("ls:\n  {}  hand/written:\n    .rs: snake_case\n", LS_REGION.replace('\n', "\n  ").trim_end()));
    dir
}

fn ls_lint(root: &Path) -> String {
    std::fs::read_to_string(root.join(".ls-lint.yml")).unwrap()
}

#[test]
fn the_ls_lint_keys_of_every_typed_package_come_from_the_manifest() {
    // Given a service, a contract crate and a flat adapter that lists its modules, and a stale generated region
    let dir = ls_repo();
    let root = dir.path();
    service(root);
    crate_at(root, "libraries/wire", "wire", Some("rust-contract"), "", &["lib.rs"]);
    put(root, "backend/svc/group-x/Cargo.toml", "[package]\nname = \"group-x\"\n[package.metadata.arch]\ntype = \"rust-flat-adapter\"\nmodules = [\"alpha\", \"beta\"]\n");
    put(root, "backend/svc/group-x/src/lib.rs", "");
    crate_at(root, "backend/svc/group-y", "group-y", Some("rust-flat-adapter"), "", &["lib.rs"]);
    // When the check reports and then rewrites the region
    let before = report(&run(root, &[]));
    run(root, &["--write"]);
    let text = ls_lint(root);
    let after = report(&run(root, &["--strict"]));
    // Then the stale region was drift, each package got keys of its type, modules became a regex, and the hand-written key stayed
    assert_eq!(messages(&before, "drift").len(), 1, "{before}");
    assert!(text.contains("  backend/svc/core/src:\n    .dir: regex:(src|domain|application|testing)\n    .rs: regex:(lib|testing)\n"), "{text}");
    assert!(text.contains("  backend/svc/rs/src/*/*:\n    .dir: snake_case"), "{text}");
    assert!(text.contains("  libraries/wire/src:\n    .dir: regex:(src|testing)"), "{text}");
    assert!(text.contains("  backend/svc/group-x/src:\n    .dir: regex:src\n    .rs: regex:(lib|alpha|beta)\n"), "{text}");
    assert!(text.contains("  backend/svc/group-y/src:\n    .dir: regex:src\n    .rs: snake_case\n"), "{text}");
    assert!(text.contains("  hand/written:\n    .rs: snake_case\n"), "{text}");
    assert_eq!(after["findings"].as_array().unwrap().len(), 0, "{after}");
}

#[test]
fn a_package_added_without_regenerating_is_ls_lint_drift() {
    // Given a region generated for one service, then a second typed package added
    let dir = ls_repo();
    let root = dir.path();
    service(root);
    run(root, &["--write"]);
    crate_at(root, "libraries/wire", "wire", Some("rust-contract"), "", &["lib.rs"]);
    // When the check runs strictly
    let r = run(root, &["--strict"]);
    // Then the stale region fails the run and names the command that fixes it
    assert_eq!(r.exit_code, 1);
    let drift = messages(&report(&r), "drift");
    assert!(drift.iter().any(|m| m.starts_with(".ls-lint.yml: region ls-lint is stale") && m.contains("--write")), "{drift:?}");
}

#[test]
fn a_hand_edited_generated_key_is_overwritten_not_kept() {
    // Given a generated region that someone loosened by hand
    let dir = ls_repo();
    let root = dir.path();
    service(root);
    run(root, &["--write"]);
    std::fs::write(root.join(".ls-lint.yml"), ls_lint(root).replace(".rs: regex:(lib|testing)", ".rs: snake_case")).unwrap();
    // When the check runs and then rewrites
    let stale = messages(&report(&run(root, &[])), "drift");
    run(root, &["--write"]);
    // Then the loosening was drift and the rewrite restores the manifest's rule
    assert_eq!(stale.len(), 1, "{stale:?}");
    assert!(ls_lint(root).contains(".rs: regex:(lib|testing)"));
}

#[test]
fn a_key_is_generated_only_for_a_directory_the_package_has() {
    // Given two testkits, one with a src/bin directory and one without
    let dir = ls_repo();
    let root = dir.path();
    crate_at(root, "libraries/kit-a", "kit-a", Some("rust-testkit"), "", &["lib.rs", "bin/tool.rs"]);
    crate_at(root, "libraries/kit-b", "kit-b", Some("rust-testkit"), "", &["lib.rs"]);
    // When the region is generated
    run(root, &["--write"]);
    // Then only the first gets a src/bin key, so no key names a path that does not exist
    let text = ls_lint(root);
    assert!(text.contains("  libraries/kit-a/src/bin:\n    .dir: kebab-case"), "{text}");
    assert!(text.contains("  libraries/kit-b/src:\n"), "{text}");
    assert!(!text.contains("libraries/kit-b/src/bin"), "{text}");
}
