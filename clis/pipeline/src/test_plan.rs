//! `wf test plan`: every test a push needs, by CI stage, decided in one place. Package tests (flow, integration, typecheck,
//! lint, unit, deployment verification) follow pnpm's dependency graph from the changed files, and the crate graph for a package whose crates are
//! projects of their own; the `e2e/*.lua` tests are the focused ones. Each
//! entry says why it was picked. CI writes the plan once and every stage job runs its part with `wf test run --stage
//! <stage> --plan <file>`, through the Nx cache when the workspace has one (`nx.rs`).
use crate::e2e_tests::{self as tq, E2eTest};
use crate::verify::Package;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The CI stages that run tests, in pipeline order.
pub const STAGES: [&str; 7] = ["flow-tests", "typecheck-lint-unit-tests", "integration-tests", "cli-e2e-tests", "deployment-verification-tests", "browser-e2e-tests", "android-e2e-tests"];

/// Package scripts by stage and kind, in the order a stage runs them.
const SCRIPTS: [(&str, &str, &str); 9] = [
    ("flow-tests", "flow", "test:flow"),
    // no package defines this script: the stage runs the crates' `cargo-contract`, the Nx cache's contract suite
    ("flow-tests", "contract", "test:contract"),
    ("typecheck-lint-unit-tests", "typecheck", "typecheck"),
    ("typecheck-lint-unit-tests", "lint", "lint"),
    ("typecheck-lint-unit-tests", "unit", "test:unit"),
    ("integration-tests", "integration", "test:integration"),
    // no package defines this script: the stage runs the crates' `cargo-e2e`. A package's own `test:e2e` runs against local services (a
    // backend binary, an opencode server) that this job does not have; its deployed form is `test:verify:deployed`.
    ("cli-e2e-tests", "e2e", "test:cli-e2e"),
    // the crates' `cargo-e2e-smoke`: the same suite without the parity tests, for a crate reached only through a dependency
    ("cli-e2e-tests", "e2e-smoke", "test:cli-e2e-smoke"),
    ("deployment-verification-tests", "deployment-verification", "test:verify:deployed"),
];

/// The workspace root is a package too: in scope when a file outside every package changed, for its typecheck only (its
/// lint leaves the packages to their own lint, and it has no unit script).
pub(crate) const ROOT: &str = "expo-hello-world-app-2-workspace";

/// What the plan covers: every test, the ones a change reaches, or named e2e tests.
pub enum Scope {
    All,
    Changed(Vec<String>),
    Named(Vec<String>),
}

fn script_text(root: &Path, dir: &str, script: &str) -> Option<String> {
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(root.join(dir).join("package.json")).ok()?).ok()?;
    manifest["scripts"][script].as_str().map(str::to_string)
}

fn scripts_of(root: &Path, dir: &str) -> HashSet<String> {
    let manifest = std::fs::read_to_string(root.join(dir).join("package.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    manifest["scripts"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default()
}

/// Why each package is in scope: it owns a changed file, or its crates are reached (`through_crates`), or it depends (through pnpm) on one
/// that is in scope.
fn package_reasons(files: &[String], packages: &[Package], through_crates: Vec<(String, String)>) -> Vec<(String, String)> {
    let dirs: Vec<&str> = packages.iter().map(|p| p.dir.as_str()).collect();
    let owned: HashSet<&str> = files.iter().map(|f| crate::verify::owning_dir(f, &dirs)).collect();
    let mut why: Vec<(String, String)> = packages.iter().filter(|p| owned.contains(p.dir.as_str())).map(|p| (p.name.clone(), "changed".to_string())).collect();
    let known: HashSet<String> = why.iter().map(|(n, _)| n.clone()).collect();
    why.extend(through_crates.into_iter().filter(|(n, _)| !known.contains(n)));
    loop {
        let known: HashSet<String> = why.iter().map(|(n, _)| n.clone()).collect();
        let grown: Vec<(String, String)> = packages
            .iter()
            .filter(|p| !known.contains(&p.name))
            .filter_map(|p| p.deps.iter().find(|d| known.contains(*d)).map(|d| (p.name.clone(), format!("depends on {d} (pnpm)"))))
            .collect();
        if grown.is_empty() {
            return why;
        }
        why.extend(grown);
    }
}

/// Whether a package opts in to a project per crate (`"nx": {"cargoCrates": true}`): Nx then keys its suites on the crates it builds and runs.
fn opts_in_to_crate_projects(root: &Path, dir: &str) -> bool {
    let manifest = std::fs::read_to_string(root.join(dir).join("package.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    manifest.is_some_and(|m| m["nx"]["cargoCrates"] == json!(true))
}

/// The packages that opt in to crate projects and are reached by what their crates build or run, not by a file of their own, as Nx keys their
/// suites: a crate of the package depends (cargo) on a reached crate, runs one (`uses`, flow or integration), or reads a changed file that its
/// flow or integration `inputs` name. Without this, the plan never asks Nx to run the suite of a service for a change in a library it builds.
fn packages_reached_through_crates(root: &Path, scope: &Scope, packages: &[Package], every: &[Crate]) -> Vec<(String, String)> {
    let Scope::Changed(files) = scope else { return Vec::new() };
    let reached = reached_crates(every, scope);
    let mut out = Vec::new();
    for package in packages.iter().filter(|p| opts_in_to_crate_projects(root, &p.dir)) {
        let inside = |c: &&Crate| c.dir == package.dir || c.dir.starts_with(&format!("{}/", package.dir));
        let why = every.iter().filter(inside).find_map(|c| {
            let built = c.deps.iter().find(|d| reached.contains(*d)).map(|d| format!("its crate {} depends on {d} (cargo)", c.name));
            let run = || c.flow_uses.iter().chain(&c.integration_uses).find(|u| reached.contains(*u)).map(|u| format!("its crate {} runs {u}", c.name));
            let read = || files.iter().find(|f| c.flow_files.iter().chain(&c.integration_files).any(|g| glob_matches(g, f))).map(|f| format!("an input of its crate {} changed: {f}", c.name));
            built.or_else(run).or_else(read)
        });
        out.extend(why.map(|why| (package.name.clone(), why)));
    }
    out
}

fn package_tests(root: &Path, scope: &Scope, packages: &[Package]) -> Result<Vec<(String, Value)>, String> {
    if matches!(scope, Scope::Named(_)) {
        return Ok(Vec::new());
    }
    let every = rust_crates(root)?;
    let mut in_scope: Vec<(String, String, String)> = match scope {
        Scope::Named(_) => return Ok(Vec::new()),
        Scope::All => packages.iter().map(|p| (p.name.clone(), p.dir.clone(), "all".to_string())).collect(),
        Scope::Changed(files) => {
            let dir_of = |n: &str| packages.iter().find(|p| p.name == n).map(|p| p.dir.clone()).unwrap_or_default();
            let through_crates = packages_reached_through_crates(root, scope, packages, &every);
            package_reasons(files, packages, through_crates).into_iter().map(|(n, why)| (n.clone(), dir_of(&n), why)).collect()
        }
    };
    let dirs: Vec<&str> = packages.iter().map(|p| p.dir.as_str()).collect();
    let root_touched = match scope {
        Scope::All => true,
        Scope::Changed(files) => files.iter().any(|f| crate::verify::owning_dir(f, &dirs).is_empty()),
        Scope::Named(_) => false,
    };
    if root_touched {
        in_scope.push((ROOT.to_string(), ".".to_string(), if matches!(scope, Scope::All) { "all" } else { "a file outside every package changed" }.to_string()));
    }
    in_scope.sort();
    let mut out = Vec::new();
    for (stage, kind, script) in SCRIPTS {
        for (name, dir, why) in &in_scope {
            if name == ROOT && kind != "typecheck" {
                continue;
            }
            if scripts_of(root, dir).contains(script) {
                out.push((stage.to_string(), json!({ "kind": kind, "package": name, "dir": dir, "script": script, "command": format!("pnpm --filter {name} run {script}"), "why": [why] })));
            }
        }
    }
    out.extend(crate_tests(root, scope, packages, every)?);
    Ok(out)
}

/// A Rust crate of a cargo workspace that no pnpm package wraps (the CLIs): its `tests/flow.rs` is its flow test.
struct Crate {
    name: String,
    dir: String,
    manifest: String,
    deps: Vec<String>,
    /// `[package.metadata.e2e] groups`: the e2e modules that are an Nx task each (`cargo-e2e-<group>`).
    e2e_groups: Vec<String>,
    /// `[package.metadata.flow] groups`: the flow modules that are an Nx task each (`cargo-flow-<group>`).
    flow_groups: Vec<String>,
    /// The repo-relative globs of the files the flow tests read beyond the crate: `inputs`, `shared` and the groups' file globs.
    flow_files: Vec<String>,
    /// The crates whose binaries the flow tests run: `uses` and the crates the groups name.
    flow_uses: Vec<String>,
    /// `[package.metadata.integration] inputs`: the repo-relative globs of the files the integration tests read beyond the crate.
    integration_files: Vec<String>,
    /// `[package.metadata.integration] uses`: the crates whose binaries the integration tests run, which the flow tests do not.
    integration_uses: Vec<String>,
}

/// Every crate of the repo's cargo workspaces, with its path dependencies, read from the manifests: the plan job has no
/// Rust toolchain (18 Oct: `cargo metadata` failed there and every crate's flow test went unplanned).
fn rust_crates(root: &Path) -> Result<Vec<Crate>, String> {
    let listed = std::process::Command::new("git").args(["ls-files", "*Cargo.toml"]).current_dir(root).output().map_err(|e| format!("git ls-files: {e}"))?;
    let read = |file: &str| -> Result<toml::Table, String> {
        let text = std::fs::read_to_string(root.join(file)).map_err(|e| format!("{file}: {e}"))?;
        text.parse::<toml::Table>().map_err(|e| format!("{file}: {e}"))
    };
    let mut out = Vec::new();
    for manifest in String::from_utf8_lossy(&listed.stdout).lines().filter(|m| !m.starts_with("references/")) {
        let ws = read(manifest)?;
        let Some(workspace) = ws.get("workspace").and_then(toml::Value::as_table) else { continue };
        let ws_dir = Path::new(manifest).parent().unwrap_or(Path::new(""));
        let members = workspace.get("members").and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str).map(|m| ws_dir.join(m));
        let dirs: Vec<std::path::PathBuf> = ws.contains_key("package").then(|| ws_dir.to_path_buf()).into_iter().chain(members).collect();
        for dir in dirs {
            let m = read(&dir.join("Cargo.toml").to_string_lossy())?;
            let name = m.get("package").and_then(|p| p.get("name")).and_then(toml::Value::as_str).ok_or_else(|| format!("{}/Cargo.toml has no package name", dir.display()))?;
            let targets = m.get("target").and_then(toml::Value::as_table).into_iter().flat_map(|t| t.values());
            let sections = ["dependencies", "dev-dependencies", "build-dependencies"].iter().filter_map(|k| m.get(*k)).chain(targets.flat_map(|t| ["dependencies", "dev-dependencies"].into_iter().filter_map(move |k| t.get(k))));
            let mut deps = Vec::new();
            for (key, spec) in sections.filter_map(toml::Value::as_table).flatten() {
                let inherited = spec.get("workspace").and_then(toml::Value::as_bool) == Some(true);
                let path = if inherited { workspace.get("dependencies").and_then(|d| d.get(key)).and_then(|d| d.get("path")).and_then(toml::Value::as_str).map(|p| ws_dir.join(p)) } else { spec.get("path").and_then(toml::Value::as_str).map(|p| dir.join(p)) };
                // a path dependency is named by its own manifest, whatever key the dependent gives it
                if let Some(path) = path {
                    let dep = read(&normalize(&path.join("Cargo.toml")).to_string_lossy())?;
                    deps.extend(dep.get("package").and_then(|p| p.get("name")).and_then(toml::Value::as_str).map(str::to_string));
                }
            }
            let metadata = |section: &str| m.get("package").and_then(|p| p.get("metadata")).and_then(|d| d.get(section)).and_then(|e| e.get("groups")).cloned();
            let e2e_groups = metadata("e2e").as_ref().and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str).map(str::to_string).collect();
            let flow_groups = metadata("flow").as_ref().and_then(toml::Value::as_table).into_iter().flat_map(|t| t.keys().cloned()).collect();
            let flow = m.get("package").and_then(|p| p.get("metadata")).and_then(|d| d.get("flow"));
            let list = |key: &str| -> Vec<String> { flow.and_then(|f| f.get(key)).and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str).map(str::to_string).collect() };
            let grouped: Vec<String> = flow.and_then(|f| f.get("groups")).and_then(toml::Value::as_table).into_iter().flat_map(|t| t.values()).filter_map(toml::Value::as_array).flatten().filter_map(toml::Value::as_str).map(str::to_string).collect();
            // as the Nx plugin tells them apart: an entry with no `/`, `.` or `*` names a crate
            let names_crate = |e: &String| !e.contains(['/', '.', '*']);
            let flow_files = list("inputs").into_iter().chain(list("shared")).chain(grouped.iter().filter(|e| !names_crate(e)).cloned()).map(|g| normalize(&dir.join(g)).to_string_lossy().to_string()).collect();
            let flow_uses = list("uses").into_iter().chain(grouped.iter().filter(|e| names_crate(e)).cloned()).collect();
            let integration = m.get("package").and_then(|p| p.get("metadata")).and_then(|d| d.get("integration"));
            let integration_list = |key: &str| -> Vec<String> { integration.and_then(|f| f.get(key)).and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str).map(str::to_string).collect() };
            let integration_files = integration_list("inputs").into_iter().map(|g| normalize(&dir.join(g)).to_string_lossy().to_string()).collect();
            let integration_uses = integration_list("uses");
            out.push(Crate { name: name.to_string(), dir: normalize(&dir).to_string_lossy().to_string(), manifest: manifest.to_string(), deps, e2e_groups, flow_groups, flow_files, flow_uses, integration_files, integration_uses });
        }
    }
    Ok(out)
}

/// `a/b/../c` as `a/c`: path dependencies climb out of their crate.
fn normalize(path: &Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Whether a repo-relative path matches a glob of `*` (within a segment) and `**` (any number of segments).
fn glob_matches(glob: &str, path: &str) -> bool {
    fn segments(g: &[&str], p: &[&str]) -> bool {
        match (g.first(), p.first()) {
            (None, None) => true,
            (Some(&"**"), _) => segments(&g[1..], p) || (!p.is_empty() && segments(g, &p[1..])),
            (Some(seg), Some(part)) => segment(seg.as_bytes(), part.as_bytes()) && segments(&g[1..], &p[1..]),
            _ => false,
        }
    }
    fn segment(g: &[u8], p: &[u8]) -> bool {
        match (g.first(), p.first()) {
            (None, None) => true,
            (Some(b'*'), _) => segment(&g[1..], p) || (!p.is_empty() && segment(g, &p[1..])),
            (Some(a), Some(b)) => a == b && segment(&g[1..], &p[1..]),
            _ => false,
        }
    }
    segments(&glob.split('/').collect::<Vec<_>>(), &path.split('/').collect::<Vec<_>>())
}

/// A file that decides which projects exist, how they depend on each other, or what a task hashes.
pub(crate) fn shapes_the_nx_graph(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    matches!(file, "nx.json" | ".nxignore" | ".gitignore") || file.starts_with("tooling/nx/") || (file.starts_with("e2e/") && file.ends_with(".lua")) || matches!(name, "package.json" | "project.json" | "Cargo.toml")
}

/// Files of a crate that cannot move the Nx cache's contract. Only the modules that drive Nx or plan its tasks (`nx.rs`, `test_plan.rs`,
/// `e2e_tests.rs`, `verify.rs`, the router and binaries), the contract's own tests and their support can; every other module, the help texts and the
/// crate's other flow tests cannot (a change to `legs.rs` ran the 11 minute suite on 31 Oct), and the weekly cold run, which plans every test, is the backstop.
fn outside_the_contract(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    if file.contains("/src/help/") {
        return true;
    }
    if file.contains("/src/") {
        return !(matches!(name, "nx.rs" | "e2e_tests.rs" | "test_plan.rs" | "verify.rs" | "lib.rs" | "main.rs") || file.contains("/src/bin/"));
    }
    if file.contains("/tests/flow/") {
        return !(matches!(name, "nx_cache.rs" | "nx_crates.rs" | "cache_contract.rs") || file.contains("/tests/flow/support/"));
    }
    false
}

/// Whether the Nx cache's contract suite (`cargo-contract`) runs for a crate that has one: always when the plan is not a change, when the
/// crate is reached through a dependency or through the Nx graph, and when one of its own changed files can move the contract.
fn contract_selected(scope: &Scope, reason: &str, dir: &str) -> bool {
    match scope {
        Scope::Changed(files) if reason == "changed" => files.iter().any(|f| f.starts_with(&format!("{dir}/")) && !outside_the_contract(f)),
        _ => true,
    }
}

/// Whether a crate's parity tests (`cargo-e2e`, live models) run: for a full plan and when a file of the crate itself changed; a crate reached
/// only through a dependency or the Nx graph runs `cargo-e2e-smoke`, the rest of its e2e suite, or nothing when the suite is all live models.
fn parity_selected(scope: &Scope, reason: &str, dir: &str) -> bool {
    match scope {
        Scope::Changed(files) => reason == "changed" && files.iter().any(|f| f.starts_with(&format!("{dir}/"))),
        _ => true,
    }
}

/// Whether a crate has inline unit tests: a `#[cfg(test)]` module under its src.
fn has_unit_tests(dir: &Path) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| {
        let path = e.path();
        if path.is_dir() {
            has_unit_tests(&path)
        } else {
            path.extension().is_some_and(|x| x == "rs") && std::fs::read_to_string(&path).is_ok_and(|t| t.contains("#[cfg(test)]"))
        }
    })
}

/// A crate's flow-tests entries: one, or one per flow group, each the Nx task `cargo-flow-<group>` replayed on its own inputs.
fn flow_entries(root: &Path, c: &Crate, reason: &str) -> Vec<(String, Value)> {
    if !root.join(&c.dir).join("tests/flow.rs").is_file() {
        return vec![];
    }
    let command = format!("cargo test --locked -p {} --test flow --manifest-path {}", c.name, c.manifest);
    if c.flow_groups.is_empty() {
        return vec![("flow-tests".to_string(), json!({ "kind": "flow", "crate": c.name, "dir": c.dir, "manifest": c.manifest, "command": command, "why": [reason] }))];
    }
    c.flow_groups.iter().map(|g| ("flow-tests".to_string(), json!({ "kind": "flow", "crate": c.name, "dir": c.dir, "manifest": c.manifest, "command": format!("{command} {g}::"), "group": g, "target": format!("cargo-flow-{g}"), "why": [reason] }))).collect()
}

fn integration_entry(root: &Path, c: &Crate, reason: &str) -> Option<(String, Value)> {
    root.join(&c.dir).join("tests/integration.rs").is_file().then(|| ("integration-tests".to_string(), json!({ "kind": "integration", "crate": c.name, "dir": c.dir, "manifest": c.manifest, "command": format!("cargo test --locked -p {} --test integration --manifest-path {} -- --test-threads=1", c.name, c.manifest), "why": [reason] })))
}

/// The deployed service asked over its real interface (`tests/deployed_verification.rs`, the deployment-verification-tests stage): a crate that
/// builds a service image has one, and it runs when the crate is reached, as a package's `test:verify:deployed` does.
fn deployed_verification_entry(root: &Path, c: &Crate, reason: &str) -> Option<(String, Value)> {
    root.join(&c.dir).join("tests/deployed_verification.rs").is_file().then(|| ("deployment-verification-tests".to_string(), json!({ "kind": "deployment-verification", "crate": c.name, "dir": c.dir, "manifest": c.manifest, "command": format!("cargo test --locked -p {} --test deployed_verification --manifest-path {} -- --ignored --test-threads=1", c.name, c.manifest), "why": [reason] })))
}

/// The Nx projects of a stage's tests: a package by its name, a crate as `crate-<name>`.
fn verified_projects(tests: &[Value]) -> Vec<String> {
    tests.iter().filter_map(|t| t["package"].as_str().map(str::to_string).or_else(|| t["crate"].as_str().map(|c| format!("crate-{c}")))).collect()
}

/// The names of every crate a change reaches, the crates inside a pnpm package too: those with a changed file, and those that depend on one.
fn reached_crates(every: &[Crate], scope: &Scope) -> HashSet<String> {
    let mut reached: HashSet<String> = match scope {
        Scope::Changed(files) => every.iter().filter(|c| files.iter().any(|f| f.starts_with(&format!("{}/", c.dir)))).map(|c| c.name.clone()).collect(),
        Scope::All => every.iter().map(|c| c.name.clone()).collect(),
        Scope::Named(_) => HashSet::new(),
    };
    loop {
        let grown: Vec<String> = every.iter().filter(|c| !reached.contains(&c.name) && c.deps.iter().any(|d| reached.contains(d))).map(|c| c.name.clone()).collect();
        if grown.is_empty() {
            return reached;
        }
        reached.extend(grown);
    }
}

/// The checks of the crates a change reaches, through their own files or a path dependency (cargo), and the flow (and, for a used
/// crate, integration) tests alone of a crate whose `[package.metadata.flow]` names a changed file or a reached crate: the flow test (`tests/flow.rs`,
/// a flow-tests stage entry), the integration test (`tests/integration.rs`, an integration-tests stage entry: the built binary as a black box, every dependency a fake, run once the services are deployed), the e2e test (`tests/e2e.rs`, a cli-e2e-tests stage entry: a real Chromium or ffmpeg), the inline unit tests and `cargo clippy -D warnings` (typecheck-lint-unit-tests stage entries). A crate inside a pnpm package
/// is that package's to test.
fn crate_tests(root: &Path, scope: &Scope, packages: &[Package], every: Vec<Crate>) -> Result<Vec<(String, Value)>, String> {
    if matches!(scope, Scope::Named(_)) {
        return Ok(Vec::new());
    }
    let wrapped: Vec<String> = packages.iter().filter_map(|p| script_text(root, &p.dir, "test:flow")).collect();
    let reached = reached_crates(&every, scope);
    let crates: Vec<Crate> = every
        .into_iter()
        .filter(|c| !packages.iter().any(|p| c.dir == p.dir || c.dir.starts_with(&format!("{}/", p.dir))))
        .filter(|c| !wrapped.iter().any(|w| w.contains(&format!("-p {} ", c.name))))
        .collect();
    let mut why: Vec<(String, String)> = match scope {
        Scope::Changed(files) => crates.iter().filter(|c| files.iter().any(|f| f.starts_with(&format!("{}/", c.dir)))).map(|c| (c.name.clone(), "changed".to_string())).collect(),
        _ => crates.iter().map(|c| (c.name.clone(), "all".to_string())).collect(),
    };
    // the pipeline crate's `nx_contract` tests hold the Nx cache's contract (tests/flow/nx_cache.rs): what decides a task's inputs reaches them
    if let Scope::Changed(files) = scope {
        if crates.iter().any(|c| c.name == "pipeline") && !why.iter().any(|(n, _)| n == "pipeline") && files.iter().any(|f| shapes_the_nx_graph(f)) {
            why.push(("pipeline".to_string(), "the Nx graph or its inputs changed (the cache's contract suite)".to_string()));
        }
    }
    loop {
        let known: HashSet<String> = why.iter().map(|(n, _)| n.clone()).collect();
        let grown: Vec<(String, String)> = crates.iter().filter(|c| !known.contains(&c.name)).filter_map(|c| c.deps.iter().find(|d| known.contains(*d)).map(|d| (c.name.clone(), format!("depends on {d} (cargo)")))).collect();
        if grown.is_empty() {
            break;
        }
        why.extend(grown);
    }
    // what only a crate's flow tests read: a file of its flow metadata, or a crate whose binary they run
    let known: HashSet<String> = why.iter().map(|(n, _)| n.clone()).collect();
    let mut flow_only: Vec<(String, String)> = crates
        .iter()
        .filter(|c| !known.contains(&c.name))
        .filter_map(|c| {
            let input = match scope {
                Scope::Changed(files) => files.iter().find(|f| c.flow_files.iter().any(|g| glob_matches(g, f))).map(|f| format!("a flow input changed: {f}")),
                _ => None,
            };
            input.or_else(|| c.flow_uses.iter().find(|u| known.contains(*u)).map(|u| format!("its flow tests run {u}"))).map(|reason| (c.name.clone(), reason))
        })
        .collect();
    flow_only.sort();
    why.sort();
    let mut out = Vec::new();
    let mut integrated: HashSet<String> = HashSet::new();
    for (name, reason) in flow_only {
        let Some(c) = crates.iter().find(|c| c.name == name) else { continue };
        out.extend(flow_entries(root, c, &reason));
        // the Nx plugin keys a crate's integration tests on the crates it uses too, not on its flow inputs
        if c.flow_uses.iter().any(|u| known.contains(u)) && integrated.insert(c.name.clone()) {
            out.extend(integration_entry(root, c, &reason));
        }
    }
    // a crate whose integration tests alone read a changed file or run a reached crate (the crates of a pnpm package count: a service's binary)
    let mut integration_only: Vec<(String, String)> = crates
        .iter()
        .filter(|c| !known.contains(&c.name) && !integrated.contains(&c.name))
        .filter_map(|c| {
            let input = match scope {
                Scope::Changed(files) => files.iter().find(|f| c.integration_files.iter().any(|g| glob_matches(g, f))).map(|f| format!("an integration input changed: {f}")),
                _ => None,
            };
            input.or_else(|| c.integration_uses.iter().find(|u| reached.contains(*u)).map(|u| format!("its integration tests run {u}"))).map(|reason| (c.name.clone(), reason))
        })
        .collect();
    integration_only.sort();
    for (name, reason) in integration_only {
        let Some(c) = crates.iter().find(|c| c.name == name) else { continue };
        out.extend(integration_entry(root, c, &reason));
    }
    for (name, reason) in why {
        let Some(c) = crates.iter().find(|c| c.name == name) else { continue };
        out.push(("typecheck-lint-unit-tests".to_string(), json!({ "kind": "clippy", "crate": c.name, "dir": c.dir, "manifest": c.manifest, "command": format!("cargo clippy --locked -p {} --all-targets --manifest-path {} -- -D warnings", c.name, c.manifest), "why": [reason.clone()] })));
        out.extend(flow_entries(root, c, &reason));
        if root.join(&c.dir).join("tests/nx_contract.rs").is_file() && contract_selected(scope, &reason, &c.dir) {
            out.push(("flow-tests".to_string(), json!({ "kind": "contract", "crate": c.name, "dir": c.dir, "manifest": c.manifest, "command": format!("cargo test --locked -p {} --test nx_contract --manifest-path {}", c.name, c.manifest), "why": [reason.clone()] })));
        }
        out.extend(integration_entry(root, c, &reason));
        out.extend(deployed_verification_entry(root, c, &reason));
        if root.join(&c.dir).join("tests/e2e.rs").is_file() {
            // a crate whose e2e suite is the live-model parity one (`tests/e2e/parity.rs`) runs none of it for a change that only reaches it
            let live = root.join(&c.dir).join("tests/e2e/parity.rs").is_file();
            let selected = parity_selected(scope, &reason, &c.dir);
            if selected || !live {
                let (kind, filter) = if selected { ("e2e", "") } else { ("e2e-smoke", " --skip parity") };
                // a full-suite crate with groups is one entry per group: each is the Nx task `cargo-e2e-<group>`, replayed on its own inputs
                let groups: Vec<Option<&String>> = if selected && !c.e2e_groups.is_empty() { c.e2e_groups.iter().map(Some).collect() } else { vec![None] };
                for group in groups {
                    let (target, filter) = match group {
                        Some(g) => (format!("cargo-e2e-{g}"), format!(" {g}::")),
                        None => (format!("cargo-{kind}"), filter.to_string()),
                    };
                    let mut entry = json!({ "kind": kind, "crate": c.name, "dir": c.dir, "manifest": c.manifest, "command": format!("cargo test --locked -p {} --test e2e --manifest-path {} -- --test-threads=1{filter}", c.name, c.manifest), "target": target, "why": [reason.clone()] });
                    if let Some(g) = group {
                        entry["group"] = json!(g);
                    }
                    out.push(("cli-e2e-tests".to_string(), entry));
                }
            }
        }
        if has_unit_tests(&root.join(&c.dir).join("src")) {
            let target = if root.join(&c.dir).join("src/lib.rs").is_file() { "--lib" } else { "--bins" };
            out.push(("typecheck-lint-unit-tests".to_string(), json!({ "kind": "unit", "crate": c.name, "dir": c.dir, "manifest": c.manifest, "command": format!("cargo test --locked -p {} {target} --manifest-path {}", c.name, c.manifest), "why": [reason] })));
        }
    }
    Ok(out)
}

/// Why an e2e test is picked: its focus alone on a change (owner, 18 Oct), or its name, or --all.
fn e2e_reasons(t: &E2eTest, scope: &Scope) -> Option<Vec<String>> {
    if !t.focus || t.script.is_none() {
        return None;
    }
    let mut why = vec!["focused".to_string()];
    match scope {
        Scope::All => why.push("all".into()),
        Scope::Named(names) => {
            if !names.contains(&t.name) {
                return None;
            }
            why.push("named".into());
        }
        Scope::Changed(_) => {}
    }
    Some(why)
}

/// The plan: `{sha, base, stages: {<stage>: [test, ...]}}`, every stage present, empty when it has nothing to run.
pub fn plan(root: &Path, sha: &str, base: Option<&str>, scope: &Scope, books: &[E2eTest], packages: &[Package]) -> Result<Value, String> {
    let mut stages: Map<String, Value> = STAGES.iter().map(|s| (s.to_string(), json!([]))).collect();
    for (stage, test) in package_tests(root, scope, packages)? {
        stages[&stage].as_array_mut().expect("stage list").push(test);
    }
    for t in books {
        let Some(why) = e2e_reasons(t, scope) else { continue };
        let stage = format!("{}-e2e-tests", t.platform);
        let Some(list) = stages.get_mut(&stage).and_then(Value::as_array_mut) else { continue };
        let units: Vec<String> = t.units.iter().map(|u| tq::unit_key(u)).collect();
        list.push(json!({ "kind": "e2e", "platform": t.platform, "name": t.name, "script": t.script, "against": t.against, "units": units, "why": why }));
    }
    Ok(json!({ "sha": sha, "base": base, "stages": stages }))
}

/// `--force all|a,b`: tests the change did not pick, added to the plan whatever changed. `all` adds every test; names add the tests of those packages,
/// crates and e2e tests (a package's flow, unit and typecheck tests alike), each marked `why: ["forced"]`. A test the plan holds already is not doubled.
pub fn force(planned: &mut Value, everything: &Value, names: &[String]) {
    let all = names.iter().any(|n| n == "all");
    let key = |t: &Value| {
        let mut bare = t.clone();
        if let Some(o) = bare.as_object_mut() {
            o.remove("why");
        }
        bare.to_string()
    };
    let named = |t: &Value| ["package", "crate", "name"].iter().filter_map(|k| t[*k].as_str()).any(|v| names.iter().any(|n| n == v || format!("crate-{n}") == v));
    for stage in STAGES {
        let Some(candidates) = everything["stages"][stage].as_array() else { continue };
        let Some(have) = planned["stages"][stage].as_array_mut() else { continue };
        let mut seen: std::collections::HashSet<String> = have.iter().map(key).collect();
        for t in candidates {
            if (all || named(t)) && seen.insert(key(t)) {
                let mut forced = t.clone();
                forced["why"] = json!(["forced"]);
                have.push(forced);
            }
        }
    }
}

/// `--github-output`: the plan as one line, and each stage's test count, for the CI jobs' `if:`.
pub fn github_output(plan: &Value) -> String {
    let mut out = format!("plan={plan}\n");
    for s in STAGES {
        out.push_str(&format!("{}={}\n", s.replace('-', "_"), plan["stages"][s].as_array().map_or(0, Vec::len)));
    }
    out
}

/// The write scope of a stage run: `wf check --pushed` sets `WF_PUSH_TO_DEV` for a push to `dev`, whose HEAD is about to be on
/// origin/dev; every other run is a clean-tree run.
fn stage_scope() -> wf_bucket::cache::Scope {
    if std::env::var_os("WF_PUSH_TO_DEV").is_some_and(|v| !v.is_empty()) { wf_bucket::cache::Scope::Pushed } else { wf_bucket::cache::Scope::Tree }
}

/// `wf test run --stage <stage> --plan <file>`: the stage's tests through the Nx cache (owner, 18 Oct): a package stage runs
/// each kind's script over its packages and the crates' flow tests, an e2e stage its focused tests; a task whose inputs hash
/// the same as an earlier pass is replayed, the rest run.
pub fn run_stage(stage: &str, plan: &Value, rest: &[String]) -> i32 {
    let tests = plan["stages"][stage].as_array().cloned().unwrap_or_default();
    let root = tq::repo();
    let nx = match crate::nx::bin(&root) {
        Ok(nx) => nx,
        Err(e) => {
            println!("{}", json!({ "ok": false, "stage": stage, "error": e, "next": "pnpm install --frozen-lockfile" }));
            return 1;
        }
    };
    if matches!(stage, "browser-e2e-tests" | "android-e2e-tests") {
        let names: Vec<String> = tests.iter().filter_map(|t| t["name"].as_str().map(str::to_string)).collect();
        return run_e2e_stage(&nx, &root, stage, &names, rest);
    }
    let parallel = match rest.iter().position(|a| a == "--parallel") {
        Some(i) => match rest.get(i + 1).and_then(|n| n.parse::<usize>().ok()).filter(|n| *n > 0) {
            Some(n) => n,
            None => {
                println!("{}", json!({ "ok": false, "stage": stage, "error": "--parallel wants a positive number", "next": "--parallel 2" }));
                return 2;
            }
        },
        None => 4,
    };
    let tests = match rest.iter().position(|a| a == "--shard") {
        Some(i) => match rest.get(i + 1).and_then(|v| parse_shard(v)) {
            Some((index, count)) => shard_of(&tests, index, count),
            None => {
                println!("{}", json!({ "ok": false, "stage": stage, "error": "--shard wants <index>/<count> with 1 <= index <= count", "next": "--shard 2/4" }));
                return 2;
            }
        },
        None => tests,
    };
    run_package_stage(&nx, &root, stage, &tests, plan["sha"].as_str().unwrap_or_default(), parallel, rest.iter().any(|a| a == "--fresh"))
}

/// `--shard 2/4`: the second of four machines' parts of a package stage.
fn parse_shard(text: &str) -> Option<(usize, usize)> {
    let (index, count) = text.split_once('/')?;
    let (index, count) = (index.parse::<usize>().ok()?, count.parse::<usize>().ok()?);
    (1 <= index && index <= count).then_some((index, count))
}

/// One shard's tests: a package (or crate) with all its kinds goes to one shard, so its tasks share the cache and the build; the packages, sorted by
/// name, are dealt out round-robin, so every machine of a stage computes the same split from the same plan.
pub fn shard_of(tests: &[Value], index: usize, count: usize) -> Vec<Value> {
    let key = |t: &Value| t["package"].as_str().map(str::to_string).or_else(|| t["crate"].as_str().map(|c| format!("crate-{c}"))).unwrap_or_default();
    let mut keys: Vec<String> = tests.iter().map(key).collect();
    keys.sort();
    keys.dedup();
    tests.iter().filter(|t| keys.iter().position(|k| *k == key(t)).is_some_and(|p| p % count == index - 1)).cloned().collect()
}

/// `fresh` runs every task again whatever Nx has cached: the weekly audit of the cache compares it with a cached run. The deployment verification
/// stage tells each test the commit its service should run (`WF_EXPECTED_REVISIONS`, by service name): the pushed `sha` unless the push
/// left the service's graph alone, when it still runs the commit its image was built from.
fn run_package_stage(nx: &Path, root: &Path, stage: &str, tests: &[Value], sha: &str, parallel: usize, fresh: bool) -> i32 {
    let shim = if fresh { None } else { wf_bucket::cache::Shim::start(root, stage_scope()) };
    let mut env: Vec<(&str, String)> = if fresh { vec![("NX_SKIP_NX_CACHE", "true".to_string())] } else { Vec::new() };
    env.extend(wf_bucket::cache::env_of(&shim));
    let expected = if stage == "deployment-verification-tests" { {
        let verified = verified_projects(tests);
        crate::legs::expected_revisions(&verified.iter().map(String::as_str).collect::<Vec<_>>(), sha, root)
    } } else { Vec::new() };
    if !expected.is_empty() {
        let by_app: Map<String, Value> = expected.iter().filter_map(|e| Some((e["app"].as_str()?.to_string(), e["commit"].clone()))).collect();
        env.push(("WF_EXPECTED_REVISIONS", Value::Object(by_app).to_string()));
        for e in &expected {
            wf_kit::wf_err!("wf: {stage}: {} should run {} ({})", e["app"].as_str().unwrap_or_default(), e["commit"].as_str().unwrap_or_default(), e["why"].as_str().unwrap_or_default());
        }
    }
    let mut kinds: Vec<Value> = Vec::new();
    let mut run = |kind: &str, target: &str, projects: Vec<String>, parallel: usize| {
        if projects.is_empty() {
            return;
        }
        wf_kit::wf_err!("wf: {stage}: {kind} in {}", projects.join(", "));
        let started = Instant::now();
        let ok = crate::nx::run_many(nx, root, stage, target, &projects, parallel, &env);
        kinds.push(json!({ "kind": kind, "target": target, "projects": projects, "verdict": if ok { "pass" } else { "fail" }, "durationS": (started.elapsed().as_secs_f64() * 10.0).round() / 10.0 }));
    };
    for (_, kind, script) in SCRIPTS.iter().filter(|(s, _, _)| *s == stage) {
        let mut projects: Vec<String> = tests.iter().filter(|t| t["kind"] == *kind && t["package"].is_string()).filter_map(|t| t["package"].as_str()).map(str::to_string).collect();
        let mut crates: Vec<String> = tests.iter().filter(|t| t["kind"] == *kind).filter_map(|t| t["crate"].as_str()).map(|c| format!("crate-{c}")).collect();
        crates.dedup();
        // a crate's e2e groups are targets of their own, run beside the crate's other cargo task
        let groups: Vec<&str> = tests.iter().filter(|t| t["kind"] == *kind).filter_map(|t| t["group"].as_str().and(t["target"].as_str())).collect();
        // the crates' tests run beside the packages' in one call: cargo serialises builds that share a target directory,
        // and the crates' (clis/) is not the packages' (18 Oct: one after the other overran the job)
        let target = if !crates.is_empty() && matches!(*kind, "flow" | "unit" | "integration" | "e2e" | "e2e-smoke" | "contract" | "deployment-verification") {
            projects.extend(crates);
            let cargo = if *kind == "deployment-verification" { "cargo-verify-deployed".to_string() } else { format!("cargo-{kind}") };
            [script.to_string(), cargo].into_iter().chain(groups.iter().map(|g| g.to_string())).collect::<Vec<_>>().join(",")
        } else {
            script.to_string()
        };
        // each e2e binary launches real Chromiums, and two side by side starve each other's page loads; an integration suite runs
        // the built binary on a port of its own, one at a time too
        run(kind, &target, projects, if matches!(*kind, "integration" | "e2e" | "e2e-smoke") { 1 } else { parallel });
    }
    let clippy: Vec<String> = tests.iter().filter(|t| t["kind"] == "clippy").filter_map(|t| t["crate"].as_str()).map(|c| format!("crate-{c}")).collect();
    run("clippy", "cargo-clippy", clippy, parallel);
    let ok = kinds.iter().all(|k| k["verdict"] == "pass");
    let mut summary = json!({ "ok": ok, "stage": stage, "fresh": fresh, "kinds": kinds });
    if !expected.is_empty() {
        summary["expectedRevisions"] = json!(expected);
    }
    println!("{summary}");
    i32::from(!ok)
}

/// An e2e stage: a focused test whose script, test data, features (with what they import) and app shell hash the
/// same as an earlier pass is replayed with that pass's runs (bundles included); the others run one after another.
/// `--probe` only says which would run, so the job boots the device only for them. Run flags (`-- --update-goldens`) and an
/// `E2E-Fresh: <platform>` (or `all`) trailer on the commit always run fresh: the trailer buys repeated CI measurements.
fn run_e2e_stage(nx: &Path, root: &Path, stage: &str, names: &[String], rest: &[String]) -> i32 {
    let platform = stage.trim_end_matches("-e2e-tests");
    let split = rest.iter().position(|a| a == "--").unwrap_or(rest.len());
    let (own, flags) = (&rest[..split], rest.get(split + 1..).unwrap_or_default());
    let value = |key: &str| own.iter().position(|a| a == key).and_then(|i| own.get(i + 1)).cloned();
    if names.is_empty() {
        println!("{}", json!({ "ok": true, "stage": stage, "units": [], "ran": [], "cached": [], "note": "the plan has no tests for this stage" }));
        return 0;
    }
    let sha = value("--sha").unwrap_or_else(tq::head_sha);
    let trailer = tq::git(&["log", "-1", "--format=%(trailers:key=E2E-Fresh,valueonly)", &sha]).unwrap_or_default();
    let fresh = !flags.is_empty() || own.iter().any(|a| a == "--fresh") || trailer.lines().map(str::trim).any(|v| v == platform || v == "all");
    let shim = if fresh { None } else { wf_bucket::cache::Shim::start(root, stage_scope()) };
    let shim_env = wf_bucket::cache::env_of(&shim);
    let misses = if !fresh {
        match crate::nx::e2e_misses(nx, root, platform, names, &shim_env) {
            Ok(m) => m,
            Err(e) => {
                println!("{}", json!({ "ok": false, "stage": stage, "error": e }));
                return 1;
            }
        }
    } else {
        names.to_vec()
    };
    let hits: Vec<&String> = names.iter().filter(|n| !misses.contains(n)).collect();
    if own.iter().any(|a| a == "--probe") {
        let ids: Vec<String> = misses.iter().map(|t| format!("e2e-{t}:e2e-{platform}")).collect();
        let why = if fresh || ids.is_empty() { Value::Null } else { wf_bucket::nx_why::probe(root, &ids) };
        println!("{}", json!({ "ok": true, "stage": stage, "fresh": fresh, "ran": misses, "cached": hits, "why": why }));
        return 0;
    }
    let out = value("--out").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".agents/tmp/device-runs").join(&sha[..sha.len().min(9)]));
    let out = if out.is_absolute() { out } else { root.join(out) };
    let _ = std::fs::create_dir_all(&out);
    // each test's own run is this wf; WF_E2E_BIN already set (a test's double) is kept
    let me = std::env::var("WF_E2E_BIN").ok().or_else(|| std::env::current_exe().ok().map(|p| p.display().to_string())).unwrap_or_default();
    let mut env = vec![("WF_E2E_BIN", me), ("WF_E2E_SHA", sha.clone()), ("WF_E2E_OUT", out.display().to_string()), ("WF_E2E_STATE", out.join("warm.json").display().to_string()), ("WF_E2E_FLAGS", flags.join(" "))];
    if fresh {
        env.push(("NX_SKIP_NX_CACHE", "true".into()));
    }
    env.extend(shim_env);
    let projects: Vec<String> = names.iter().map(|t| format!("e2e-{t}")).collect();
    crate::nx::run_many(nx, root, stage, &format!("e2e-{platform}"), &projects, 1, &env);
    let mut units: Vec<Value> = Vec::new();
    let mut warms: Vec<Value> = Vec::new();
    let mut ok = true;
    for name in names {
        let ran = misses.contains(name);
        let s = crate::nx::e2e_summary(root, &out, platform, name, ran);
        ok &= s["ok"] == true;
        let mine: Vec<Value> = s["units"].as_array().cloned().unwrap_or_default().into_iter().map(|mut u| {
            u["cached"] = json!(!ran);
            u
        }).collect();
        println!("{}", json!({ "test": name, "platform": platform, "cached": !ran, "ok": s["ok"], "units": mine.len(), "fromSha": s["sha"], "note": s["note"] }));
        units.extend(mine);
        warms.extend(s["warms"].as_array().cloned().unwrap_or_default());
    }
    let summary = json!({ "ok": ok, "stage": stage, "sha": sha, "fresh": fresh, "out": out.display().to_string(), "cached": hits, "ran": misses, "units": units, "warms": warms });
    let _ = std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&summary).unwrap_or_default());
    println!("{summary}");
    i32::from(!ok)
}

#[cfg(test)]
mod tests {
    use super::{contract_selected, deployed_verification_entry, parity_selected, shapes_the_nx_graph, verified_projects, Crate, Scope};
    use serde_json::json;

    fn crate_at(dir: &str) -> Crate {
        Crate { name: "svc".into(), dir: dir.into(), manifest: format!("{dir}/Cargo.toml"), deps: vec![], e2e_groups: vec![], flow_groups: vec![], flow_files: vec![], flow_uses: vec![], integration_files: vec![], integration_uses: vec![] }
    }

    #[test]
    fn given_a_crate_with_a_deployed_verification_test_when_planned_then_it_is_a_deployment_verification_entry_the_stage_runs_by_its_project_name() {
        // Given a crate directory with and one without tests/deployed_verification.rs
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("with/tests")).unwrap();
        std::fs::write(root.path().join("with/tests/deployed_verification.rs"), "").unwrap();
        std::fs::create_dir_all(root.path().join("without/tests")).unwrap();
        // When each is planned
        let with = deployed_verification_entry(root.path(), &crate_at("with"), "changed");
        let without = deployed_verification_entry(root.path(), &crate_at("without"), "changed");
        // Then only the first has an entry, in the verification stage, naming the ignored test binary, and the stage asks for the crate's project
        let (stage, test) = with.expect("an entry");
        assert!(without.is_none());
        assert_eq!((stage.as_str(), test["kind"].as_str()), ("deployment-verification-tests", Some("deployment-verification")));
        assert!(test["command"].as_str().unwrap().contains("--test deployed_verification") && test["command"].as_str().unwrap().contains("--ignored"), "{test}");
        assert_eq!(verified_projects(&[test, json!({"kind": "deployment-verification", "package": "@app/x"})]), ["crate-svc", "@app/x"]);
    }

    #[test]
    fn the_files_that_shape_the_nx_graph_are_its_config_plugins_test_headers_and_manifests() {
        for f in ["nx.json", ".nxignore", "tooling/nx/e2e-tests.js", "e2e/voice-edit.lua", "frontend/mla-tamagui/features/settings/package.json", "frontend/mla-tamagui/features/leetcode/project.json", "backend/Cargo.toml"] {
            assert!(shapes_the_nx_graph(f), "{f}");
        }
        for f in ["docs/plan.md", "clis/wf/src/main.rs", "e2e/README.md", "frontend/mla-tamagui/app/_layout.tsx"] {
            assert!(!shapes_the_nx_graph(f), "{f}");
        }
    }

    fn changed(files: &[&str]) -> Scope {
        Scope::Changed(files.iter().map(|f| f.to_string()).collect())
    }

    #[test]
    fn given_a_change_to_modules_that_do_not_touch_nx_when_planned_then_the_nx_contract_suite_is_left_out_and_a_change_to_the_ones_that_do_runs_it() {
        let dir = "clis/pipeline";
        assert!(!contract_selected(&changed(&["clis/pipeline/src/ci_comment.rs", "clis/pipeline/src/help/wf__ci.txt", "clis/pipeline/tests/flow/ci_comment.rs"]), "changed", dir));
        assert!(contract_selected(&changed(&["clis/pipeline/src/ci_comment.rs", "clis/pipeline/src/nx.rs"]), "changed", dir));
        assert!(contract_selected(&changed(&["clis/pipeline/src/test_plan.rs"]), "changed", dir));
        assert!(contract_selected(&changed(&["clis/pipeline/tests/flow/nx_cache.rs"]), "changed", dir));
        assert!(contract_selected(&changed(&["clis/pipeline/Cargo.toml"]), "changed", dir), "a manifest moves what the suite builds");
        for other in ["clis/pipeline/src/legs.rs", "clis/pipeline/src/image_hash.rs", "clis/pipeline/src/brand_new_module.rs", "clis/pipeline/tests/flow/pipeline.rs"] {
            assert!(!contract_selected(&changed(&[other]), "changed", dir), "{other} does not touch Nx");
        }
    }

    #[test]
    fn given_a_crate_reached_by_a_dependency_the_graph_or_a_full_plan_when_planned_then_the_nx_contract_suite_runs() {
        let report_only = changed(&["clis/pipeline/src/ci_comment.rs"]);
        assert!(contract_selected(&report_only, "depends on wf-bucket (cargo)", "clis/pipeline"));
        assert!(contract_selected(&report_only, "the Nx graph or its inputs changed (the cache's contract suite)", "clis/pipeline"));
        assert!(contract_selected(&Scope::All, "all", "clis/pipeline"));
    }

    #[test]
    fn given_a_crate_changed_itself_or_a_full_plan_when_planned_then_its_parity_tests_run_and_through_a_dependency_or_the_graph_they_do_not() {
        let own = changed(&["clis/offload/src/lib.rs"]);
        assert!(parity_selected(&own, "changed", "clis/offload"));
        assert!(parity_selected(&Scope::All, "all", "clis/offload"));
        assert!(!parity_selected(&changed(&["clis/wf-vmops/src/host_summary.rs"]), "changed", "clis/offload"), "another crate's file is not the crate's own change");
        assert!(!parity_selected(&own, "depends on wf-vmops (cargo)", "clis/offload"));
        assert!(!parity_selected(&own, "the Nx graph or its inputs changed (the cache's contract suite)", "clis/offload"));
    }
}
