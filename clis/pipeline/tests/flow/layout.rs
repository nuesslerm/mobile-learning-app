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
    assert!(edges[0].contains("backend/svc/core: depends on svc-rs (rust-rs)") && edges[0].contains("may depend on [rust-core, rust-contract, rust-group]"), "{edges:?}");
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

fn git(root: &Path, args: &[&str]) {
    let out = std::process::Command::new("git").args(["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).current_dir(root).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn known_entries(root: &Path) -> Vec<Value> {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(root.join("clis/pipeline/data/layout-known.json")).unwrap()).unwrap();
    doc["known"].as_array().unwrap().clone()
}

#[test]
fn a_known_finding_passes_strictly_and_a_new_one_fails() {
    // Given an untyped crate recorded in the known file
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/old", "old", None, "", &["lib.rs"]);
    assert_eq!(run(root, &["--write-known"]).exit_code, 0);
    // When the check runs strictly, then plants a second untyped crate and runs again
    let before = run(root, &["--strict"]);
    crate_at(root, "libraries/fresh", "fresh", None, "", &["lib.rs"]);
    let after = run(root, &["--strict"]);
    // Then the known one is counted and passes, and the planted one fails the gate by name
    assert_eq!(before.exit_code, 0, "{}", before.stderr);
    assert_eq!((report(&before)["known"].as_u64(), report(&before)["new"].as_u64()), (Some(1), Some(0)));
    assert_eq!(after.exit_code, 1);
    assert!(messages(&report(&after), "membership").iter().any(|m| m.starts_with("libraries/fresh:")), "{}", after.stdout);
}

#[test]
fn a_known_entry_that_no_longer_occurs_must_be_removed() {
    // Given a known untyped crate that then opts into a type
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/old", "old", None, "", &["lib.rs"]);
    run(root, &["--write-known"]);
    crate_at(root, "libraries/old", "old", Some("rust-contract"), "", &["lib.rs"]);
    // When the check runs strictly
    let r = run(root, &["--strict"]);
    // Then the stale entry is a finding, and rewriting the known file empties it
    assert_eq!(r.exit_code, 1);
    assert!(messages(&report(&r), "known").iter().any(|m| m.contains("no longer occurs")), "{}", r.stdout);
    run(root, &["--write-known"]);
    assert_eq!(known_entries(root).len(), 0);
    assert_eq!(run(root, &["--strict"]).exit_code, 0);
}

#[test]
fn an_entry_added_to_the_known_file_since_the_last_commit_fails() {
    // Given a committed known file with one entry, and a second untyped crate added afterwards
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/old", "old", None, "", &["lib.rs"]);
    run(root, &["--write-known"]);
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    crate_at(root, "libraries/fresh", "fresh", None, "", &["lib.rs"]);
    // When the known file is regenerated to swallow it
    run(root, &["--write-known"]);
    let r = run(root, &["--strict"]);
    // Then the check refuses the growth
    assert_eq!(r.exit_code, 1);
    assert!(messages(&report(&r), "known").iter().any(|m| m.starts_with("libraries/fresh:") && m.contains("only shrinks")), "{}", r.stdout);
}

#[test]
fn a_known_entry_without_a_reason_fails() {
    // Given a known file whose entry has an empty reason
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/old", "old", None, "", &["lib.rs"]);
    run(root, &["--write-known"]);
    let path = root.join("clis/pipeline/data/layout-known.json");
    std::fs::write(&path, std::fs::read_to_string(&path).unwrap().replace("not yet typed: opts in with its section H item", "")).unwrap();
    // When the check runs strictly
    let r = run(root, &["--strict"]);
    // Then the entry is reported as lacking a reason
    assert_eq!(r.exit_code, 1);
    assert!(messages(&report(&r), "known").iter().any(|m| m.contains("has no reason")), "{}", r.stdout);
}

#[test]
fn a_known_missing_required_file_is_recorded_and_a_planted_one_fails() {
    // Given a rs crate missing main.rs that is recorded as known, then a core missing lib.rs planted
    let dir = repo();
    let root = dir.path();
    crate_at(root, "backend/svc/rs", "svc-rs", Some("rust-rs"), "", &["lib.rs", "api/mod.rs", "infrastructure/mod.rs"]);
    run(root, &["--write-known"]);
    let known = run(root, &["--strict"]);
    crate_at(root, "backend/svc/core", "svc-core", Some("rust-core"), "", &["domain/mod.rs"]);
    // When the check runs strictly again
    let planted = run(root, &["--strict"]);
    // Then only the planted missing lib.rs fails
    assert_eq!(known.exit_code, 0, "{}", known.stderr);
    assert_eq!(planted.exit_code, 1);
    let found = report(&planted);
    assert!(messages(&found, "placement").iter().any(|m| m.starts_with("backend/svc/core:") && m.contains("requires src/lib.rs")), "{found}");
    assert!(!messages(&found, "placement").iter().any(|m| m.starts_with("backend/svc/rs:")), "{found}");
}

const MAP_FILE: &str = "frontend/app/features/feature-map.cjs";
const MAP_REGION: &str = "// BEGIN generated:feature-map\n// END generated:feature-map\n";

fn map_repo() -> tempfile::TempDir {
    let dir = repo();
    let root = dir.path();
    let entries = serde_json::json!([
        { "file": MAP_FILE, "region": "feature-map", "scope": "frontend/app/features" },
        { "file": "frontend/kit/presets.cjs", "region": "rings" },
    ]);
    put(root, "clis/pipeline/data/package-types.json", &manifest_with(entries));
    put(root, MAP_FILE, MAP_REGION);
    put(root, "frontend/kit/presets.cjs", "// BEGIN generated:rings\n// END generated:rings\n");
    dir
}

fn in_app_feature(root: &Path, name: &str, arch: &str) {
    let base = format!("frontend/app/features/{name}");
    put(root, &format!("{base}/project.json"), &format!("{{\"name\":\"feature-{name}\",\"arch\":{arch}}}"));
    put(root, &format!("{base}/index.ts"), "");
    for ring in RINGS {
        put(root, &format!("{base}/{ring}/index.ts"), "");
    }
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap()
}

#[test]
fn the_feature_map_lists_in_app_features_and_the_packages_that_lint_themselves() {
    // Given an in-app feature with a widget and a sub-feature, one with a hyphenated name, and a feature package
    let dir = map_repo();
    let root = dir.path();
    in_app_feature(root, "alpha", "{\"type\":\"hexagonal-feature\",\"widgets\":[\"editor\"],\"subFeatures\":[\"plan\"]}");
    in_app_feature(root, "beta-two", "{\"type\":\"hexagonal-feature\"}");
    feature(root, "frontend/app/features/gamma", "@feature/gamma", "", &RINGS);
    // When the layout check writes the generated regions
    run(root, &["--write"]);
    // Then the in-app features carry their options, the package is only listed for the lint ignores
    let map = read(root, MAP_FILE);
    assert!(map.contains("  alpha: { type: 'hexagonal-feature', widgets: ['editor'], subFeatures: ['plan'] },\n"), "{map}");
    assert!(map.contains("  'beta-two': { type: 'hexagonal-feature' },\n"), "{map}");
    assert!(map.contains("const PACKAGES = ['gamma'];\n"), "{map}");
    assert!(!map.contains("gamma: {"), "{map}");
    assert_eq!(messages(&report(&run(root, &[])), "drift"), Vec::<String>::new());
}

#[test]
fn a_new_feature_directory_is_drift_until_the_map_is_regenerated() {
    // Given a feature map that was generated for one feature
    let dir = map_repo();
    let root = dir.path();
    in_app_feature(root, "alpha", "{\"type\":\"hexagonal-feature\"}");
    run(root, &["--write"]);
    // When a second in-app feature directory appears
    in_app_feature(root, "delta", "{\"type\":\"hexagonal-feature\"}");
    let stale = report(&run(root, &["--strict"]));
    // Then the check names the stale map, and regenerating adds the feature to the cruised list
    assert!(messages(&stale, "drift").iter().any(|m| m.starts_with(&format!("{MAP_FILE}: region feature-map is stale"))), "{stale}");
    run(root, &["--write"]);
    assert!(read(root, MAP_FILE).contains("  delta: { type: 'hexagonal-feature' },\n"));
    assert_eq!(run(root, &["--strict"]).exit_code, 0);
}

#[test]
fn a_hand_edited_ring_edge_is_overwritten_from_the_manifest() {
    // Given a presets file whose ring region was edited by hand to let routes import domain
    let dir = map_repo();
    let root = dir.path();
    run(root, &["--write"]);
    let edited = read(root, "frontend/kit/presets.cjs").replace("routes: ['di', 'ui']", "routes: ['di', 'ui', 'domain']");
    put(root, "frontend/kit/presets.cjs", &edited);
    // When the check runs, and then writes
    let drift = report(&run(root, &[]));
    run(root, &["--write"]);
    // Then the edit is drift and the written region has the manifest's edges and no testing ring
    assert!(messages(&drift, "drift").iter().any(|m| m.starts_with("frontend/kit/presets.cjs: region rings is stale")), "{drift}");
    let rings = read(root, "frontend/kit/presets.cjs");
    assert!(rings.contains("'hexagonal-feature': { domain: [], data: ['domain'], state: ['domain'], ui: ['domain', 'state'], di: ['domain', 'data', 'state'], routes: ['di', 'ui'] },"), "{rings}");
    assert!(rings.contains("'presentation-feature': { state: [], ui: ['state'], routes: ['ui'] },"), "{rings}");
    assert!(!rings.contains("testing"), "{rings}");
}

fn dep(path: &str, name: &str) -> String {
    format!("{name} = {{ path = \"{path}\" }}\n")
}

#[test]
fn a_group_depends_on_contracts_and_cores_never_on_another_group_an_adapter_or_the_rs_crate() {
    // Given a contract, a core, a flat adapter and an rs crate, and three groups: one on the contract and the core, one on another group, one on all the rest
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/proto", "proto", Some("rust-contract"), "", &["lib.rs"]);
    crate_at(root, "backend/rca/core", "rca-core-x", Some("rust-core"), "", &["lib.rs", "domain/mod.rs"]);
    crate_at(root, "backend/rca/adapter", "rca-adapter", Some("rust-flat-adapter"), "", &["lib.rs"]);
    crate_at(root, "backend/rca/rs", "rca-rs", Some("rust-rs"), "", &["lib.rs", "main.rs", "api/mod.rs", "infrastructure/mod.rs"]);
    crate_at(root, "backend/rca/group-ok", "group-ok", Some("rust-group"), &format!("[dependencies]\n{}{}", dep("../../../libraries/proto", "proto"), dep("../core", "rca-core-x")), &["lib.rs"]);
    crate_at(root, "backend/rca/group-twin", "group-twin", Some("rust-group"), &format!("[dependencies]\n{}", dep("../group-ok", "group-ok")), &["lib.rs"]);
    crate_at(root, "backend/rca/group-bad", "group-bad", Some("rust-group"), &format!("[dependencies]\n{}{}", dep("../adapter", "rca-adapter"), dep("../rs", "rca-rs")), &["lib.rs"]);
    // When the check runs
    let edges = messages(&report(&run(root, &[])), "edges");
    // Then the group on another group, on an adapter and on the rs crate are named, and the first group is not
    assert_eq!(edges.len(), 3, "{edges:?}");
    assert!(edges.iter().any(|e| e.starts_with("backend/rca/group-twin: depends on group-ok (rust-group)")), "{edges:?}");
    assert!(edges.iter().any(|e| e.starts_with("backend/rca/group-bad: depends on rca-adapter (rust-flat-adapter)")), "{edges:?}");
    assert!(edges.iter().any(|e| e.starts_with("backend/rca/group-bad: depends on rca-rs (rust-rs)")), "{edges:?}");
}

#[test]
fn a_core_and_an_rs_crate_may_name_a_group_and_a_flat_adapter_may_not_name_an_adapter() {
    // Given a group, a core and an rs crate that depend on it, and a flat adapter that depends on a second flat adapter
    let dir = repo();
    let root = dir.path();
    crate_at(root, "backend/rca/group-a", "group-a", Some("rust-group"), "", &["lib.rs"]);
    crate_at(root, "backend/rca/core", "rca-core-x", Some("rust-core"), &format!("[dependencies]\n{}", dep("../group-a", "group-a")), &["lib.rs", "domain/mod.rs"]);
    crate_at(root, "backend/rca/rs", "rca-rs", Some("rust-rs"), &format!("[dependencies]\n{}", dep("../group-a", "group-a")), &["lib.rs", "main.rs", "api/mod.rs", "infrastructure/mod.rs"]);
    crate_at(root, "backend/rca/host", "rca-host", Some("rust-flat-adapter"), &format!("[dependencies]\n{}", dep("../speech", "rca-speech")), &["lib.rs"]);
    crate_at(root, "backend/rca/speech", "rca-speech", Some("rust-flat-adapter"), "", &["lib.rs"]);
    // When the check runs
    let edges = messages(&report(&run(root, &[])), "edges");
    // Then only the adapter on the adapter is named
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert!(edges[0].starts_with("backend/rca/host: depends on rca-speech (rust-flat-adapter)"), "{edges:?}");
}

#[test]
fn a_contract_names_only_contracts_and_a_single_hexagon_names_no_crate_beside_contracts_and_libraries() {
    // Given a contract on a core, a contract on a contract, a single hexagon on a core and one on a contract
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/a", "a", Some("rust-contract"), "", &["lib.rs"]);
    crate_at(root, "libraries/b", "b", Some("rust-contract"), &format!("[dependencies]\n{}", dep("../a", "a")), &["lib.rs"]);
    crate_at(root, "backend/c/core", "c-core", Some("rust-core"), "", &["lib.rs", "domain/mod.rs"]);
    crate_at(root, "libraries/d", "d", Some("rust-contract"), &format!("[dependencies]\n{}", dep("../../backend/c/core", "c-core")), &["lib.rs"]);
    crate_at(root, "clis/e", "e", Some("rust-single-hexagon"), &format!("[dependencies]\n{}", dep("../../backend/c/core", "c-core")), &["lib.rs", "domain.rs", "adapters/mod.rs", "bin/mod.rs"]);
    crate_at(root, "clis/f", "f", Some("rust-single-hexagon"), &format!("[dependencies]\n{}", dep("../../libraries/a", "a")), &["lib.rs", "domain.rs", "adapters/mod.rs", "bin/mod.rs"]);
    // When the check runs
    let edges = messages(&report(&run(root, &[])), "edges");
    // Then the contract on the core and the hexagon on the core are the two findings
    assert_eq!(edges.len(), 2, "{edges:?}");
    assert!(edges.iter().any(|e| e.starts_with("libraries/d: depends on c-core (rust-core)")), "{edges:?}");
    assert!(edges.iter().any(|e| e.starts_with("clis/e: depends on c-core (rust-core)")), "{edges:?}");
}

#[test]
fn a_pure_package_naming_a_library_is_reported_and_a_cli_crate_naming_one_is_not() {
    // Given an adapter library, a pure core and a pure group that depend on it, and a cli crate and an rs crate that do
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/kit", "kit", Some("library"), "", &["lib.rs"]);
    crate_at(root, "backend/s/core", "s-core", Some("rust-core"), &format!("[dependencies]\n{}", dep("../../../libraries/kit", "kit")), &["lib.rs", "domain/mod.rs"]);
    crate_at(root, "backend/s/group-a", "group-a", Some("rust-group"), &format!("[dependencies]\n{}", dep("../../../libraries/kit", "kit")), &["lib.rs"]);
    crate_at(root, "clis/tool", "tool", Some("cli-crate"), &format!("[dependencies]\n{}", dep("../../libraries/kit", "kit")), &["main.rs"]);
    crate_at(root, "backend/s/rs", "s-rs", Some("rust-rs"), &format!("[dependencies]\n{}", dep("../../../libraries/kit", "kit")), &["lib.rs", "main.rs", "api/mod.rs", "infrastructure/mod.rs"]);
    // When the check runs
    let edges = messages(&report(&run(root, &[])), "edges");
    // Then only the core and the group are named, as pure packages that do I/O through the library
    assert_eq!(edges.len(), 2, "{edges:?}");
    assert!(edges.iter().all(|e| e.contains("a pure package depends on kit (library), which does I/O")), "{edges:?}");
}

#[test]
fn a_library_dev_depending_on_its_consumer_is_reported_and_a_contract_below_it_is_not() {
    // Given a library, a contract below it, a cli crate that consumes it, and the library's dev-dependencies on the consumer and on the contract
    let dir = repo();
    let root = dir.path();
    crate_at(root, "libraries/proto", "proto", Some("rust-contract"), "", &["lib.rs"]);
    crate_at(root, "libraries/kit", "kit", Some("library"), &format!("[dependencies]\n{}[dev-dependencies]\n{}{}", dep("../proto", "proto"), dep("../../clis/tool", "tool"), dep("../proto", "proto")), &["lib.rs"]);
    crate_at(root, "clis/tool", "tool", Some("cli-crate"), &format!("[dependencies]\n{}", dep("../../libraries/kit", "kit")), &["main.rs"]);
    // When the check runs
    let edges = messages(&report(&run(root, &[])), "edges");
    // Then the consumer is the one finding, and the contract below the library passes
    assert!(edges.iter().any(|e| e == "libraries/kit: depends on tool, which consumes it"), "{edges:?}");
    assert!(edges.iter().all(|e| !e.contains("proto")), "{edges:?}");
}

#[test]
fn a_testkit_named_as_a_normal_dependency_by_a_cli_crate_is_reported_and_as_a_dev_dependency_is_not() {
    // Given a testkit, a cli crate that dev-depends on it and another that names it outright
    let dir = repo();
    let root = dir.path();
    crate_at(root, "clis/kit", "kit", Some("rust-testkit"), "", &["lib.rs"]);
    crate_at(root, "clis/a", "a", Some("cli-crate"), &format!("[dev-dependencies]\n{}", dep("../kit", "kit")), &["main.rs"]);
    crate_at(root, "clis/b", "b", Some("cli-crate"), &format!("[dependencies]\n{}", dep("../kit", "kit")), &["main.rs"]);
    // When the check runs
    let edges = messages(&report(&run(root, &[])), "edges");
    // Then only the outright dependency is reported
    assert_eq!(edges.len(), 1, "{edges:?}");
    assert!(edges[0].starts_with("clis/b: depends on kit (rust-testkit), which is reachable only through dev-dependencies"), "{edges:?}");
}

#[test]
fn the_shape_table_lists_every_type_of_the_manifest_and_follows_an_edit_of_it() {
    // Given a doc with an empty shape-table region and a manifest that declares it
    let dir = repo();
    let root = dir.path();
    let doc = "docs/shapes.md";
    let mut manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    manifest["generated"] = serde_json::json!([{ "file": doc, "region": "shape-table" }]);
    put(root, "clis/pipeline/data/package-types.json", &manifest.to_string());
    put(root, doc, "# shapes\n<!-- BEGIN generated:shape-table -->\n<!-- END generated:shape-table -->\n");
    // When the check runs, it reports drift, and --write regenerates the table
    assert_eq!(messages(&report(&run(root, &[])), "drift").len(), 1);
    run(root, &["--write"]);
    let table = read(root, doc);
    // Then every type is a row, and a second run finds no drift
    for ty in manifest["types"].as_object().unwrap().keys() {
        assert!(table.contains(&format!("| `{ty}` |")), "{ty} missing: {table}");
    }
    assert!(table.contains("| `rust-core` | rust | yes | domain; application -> domain;"), "{table}");
    assert!(messages(&report(&run(root, &[])), "drift").is_empty());
    // And when a type's forbidden directory is added in the manifest, the doc is stale until regenerated
    manifest["types"]["cli-crate"]["forbiddenDirs"] = serde_json::json!(["planted_dir"]);
    put(root, "clis/pipeline/data/package-types.json", &manifest.to_string());
    assert_eq!(messages(&report(&run(root, &[])), "drift").len(), 1);
    run(root, &["--write"]);
    assert!(read(root, doc).contains("`planted_dir`"));
}
