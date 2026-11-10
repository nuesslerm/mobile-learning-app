//! What the cargo plugin keys each Rust task by (`tooling/nx/cargo-crates.js`, plan rca-parity §3 "Fixes"), on a fixture under the real
//! `nx.json`, `tooling/nx` and Nx, with `cargo` and the scripts doubled: each case plants one change and checks what `wf test run --stage` ran.
use crate::nx_cache::{put_file, write_script};
use crate::support::{node_path, repo_root, run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

type Ran = BTreeSet<String>;
/// What changed, the file, and what must run.
type Case<'a> = (&'a str, &'a str, Ran);
/// A `Cargo.lock` entry: the crate, its registry checksum (none for a workspace member) and what it depends on.
type Locked<'a> = (&'a str, Option<&'a str>, &'a [&'a str]);

const SVC_FLOW: &str = "@fx/svc test:flow";
const SVC_INTEGRATION: &str = "@fx/svc test:integration";
const CHAIN_FLOW: &str = "test chain flow";
const CHAIN_INTEGRATION: &str = "test chain integration";
const KIT_FLOW: &str = "test kit flow";
const HELPER_FLOW: &str = "test helper flow";
const ALPHA: &str = "test tool flow alpha::";
const BETA: &str = "test tool flow beta::";
const CLIPPED: [&str; 9] = ["kit", "assets-gen", "tool", "tool-alpha", "chain", "helper", "helper-plugin", "svc", "svc-core"];

fn ran(items: &[&str]) -> Ran {
    items.iter().map(|s| s.to_string()).collect()
}

fn clippy(crates: &[&str]) -> Vec<String> {
    crates.iter().map(|c| format!("clippy {c}")).collect()
}

fn checks(units: &[&str], linted: &[&str]) -> Ran {
    units.iter().map(|u| format!("test {u} unit")).chain(clippy(linted)).collect()
}

fn manifest(name: &str, deps: &str, metadata: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}{metadata}")
}

fn lock(entries: &[Locked]) -> String {
    let mut text = String::from("version = 4\n");
    for (name, checksum, deps) in entries {
        text.push_str(&format!("\n[[package]]\nname = \"{name}\"\nversion = \"{}\"\n", if checksum.is_some() { "1.0.0" } else { "0.1.0" }));
        if let Some(sum) = checksum {
            text.push_str(&format!("source = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{sum}\"\n"));
        }
        if !deps.is_empty() {
            text.push_str(&format!("dependencies = [{}]\n", deps.iter().map(|d| format!("\"{d}\"")).collect::<Vec<_>>().join(", ")));
        }
    }
    text
}

/// Three cargo workspaces: `clis` (kit, tool with flow groups, and tool-alpha and assets-gen nested in what tool reads, and chain, whose
/// integration tests run the service of another workspace and the helper of a third), `libraries` (helper, which tool and svc use, and
/// helper-plugin nested in it) and `backend/svc`, a package opted in as the remote chat agent is.
struct Fixture {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    aside: PathBuf,
    /// What the `platform` input prints (the real one-liner answers for the machine running the test).
    platform: Cell<&'static str>,
}

impl Fixture {
    fn new() -> Fixture {
        let dir = tempfile::Builder::new().prefix("wf-flow-crates-").tempdir().unwrap();
        let (ws, aside) = (dir.path().join("ws"), dir.path().join("aside"));
        std::fs::create_dir_all(aside.join("bin")).unwrap();
        let repo = repo_root();
        put_file(&ws, "nx.json", &std::fs::read_to_string(repo.join("nx.json")).unwrap());
        for file in std::fs::read_dir(repo.join("tooling/nx")).unwrap().flatten() {
            put_file(&ws, &format!("tooling/nx/{}", file.file_name().to_string_lossy()), &std::fs::read_to_string(file.path()).unwrap());
        }
        std::os::unix::fs::symlink(repo.join("node_modules"), ws.join("node_modules")).unwrap();
        put_file(&ws, "package.json", &json!({ "name": "fx-root", "private": true }).to_string());
        put_file(&ws, "pnpm-workspace.yaml", "packages:\n  - backend/*\n");
        put_file(&ws, "pnpm-lock.yaml", "lockfileVersion: '9.0'\n\nsettings:\n  autoInstallPeers: true\n  excludeLinksFromLockfile: false\n\nimporters:\n\n  .: {}\n\n  backend/svc: {}\n\npackages: {}\n\nsnapshots: {}\n");
        // node, but its platform one-liner (the `platform` input) prints what the case says the machine is
        let node = format!("if [ \"$1\" = -p ] && [ \"${{2#*process.platform}}\" != \"$2\" ]; then echo \"$FX_PLATFORM\"; else exec {} \"$@\"; fi\n", node_path().expect("node on PATH").display());
        write_script(&aside.join("bin/node"), &node);
        // a cargo that logs `<subcommand> <crate>[ <test binary or unit>][ <filter>]`, and refuses `metadata`: CI's plan job has no toolchain
        write_script(
            &aside.join("bin/cargo"),
            r#"[ "$1" = metadata ] && { echo 'the Nx graph must not need cargo' >&2; exit 1; }
sub="$1"; shift; p=""; t=""; f=""
while [ $# -gt 0 ]; do case "$1" in -p) p="$2"; shift;; --test) t=" $2"; shift;; --lib|--bins) t=" unit";; --manifest-path) shift;; --) break;; -*) ;; *) f=" $1";; esac; shift; done
echo "$sub $p$t$f" >> "$RAN_LOG"
"#,
        );

        put_file(&ws, "clis/Cargo.toml", "[workspace]\nmembers = [\"kit\", \"assets/gen\", \"tool\", \"tool/alpha\", \"chain\"]\nresolver = \"2\"\n");
        put_file(&ws, "clis/Cargo.lock", &lock(&[("assets-gen", None, &[]), ("chain", None, &[]), ("kit", None, &["lite-serde"]), ("lite-rand", Some("aaa"), &[]), ("lite-serde", Some("bbb"), &[]), ("tool", None, &["kit", "tool-alpha"]), ("tool-alpha", None, &["lite-rand"])]));
        put_file(&ws, "clis/kit/Cargo.toml", &manifest("kit", "lite-serde = \"1\"\n", ""));
        put_file(&ws, "clis/kit/src/lib.rs", "pub fn one() -> u8 {\n    1\n}\n\n#[cfg(test)]\nmod tests {}\n");
        put_file(&ws, "clis/kit/tests/flow.rs", "#[test]\nfn one() {}\n");
        put_file(&ws, "clis/assets/logo.txt", "logo\n");
        put_file(&ws, "clis/assets/gen/Cargo.toml", &manifest("assets-gen", "", ""));
        put_file(&ws, "clis/assets/gen/src/lib.rs", "pub fn gen() {}\n");
        put_file(
            &ws,
            "clis/tool/Cargo.toml",
            &manifest("tool", "kit = { path = \"../kit\" }\ntool-alpha = { path = \"alpha\" }\n", "\n[package.metadata.flow]\ninputs = [\"../assets/**\"]\nuses = [\"helper\"]\nshared = [\"tests/flow/support/**\"]\ngroups = { alpha = [\"tool-alpha\"], beta = [\"tests/fixtures/beta/**\"] }\n"),
        );
        put_file(&ws, "clis/tool/src/main.rs", "fn main() {}\n");
        put_file(&ws, "clis/tool/tests/flow.rs", "mod alpha;\nmod beta;\nmod support;\n");
        for module in ["alpha", "beta"] {
            put_file(&ws, &format!("clis/tool/tests/flow/{module}.rs"), "#[test]\nfn runs() {}\n");
        }
        put_file(&ws, "clis/tool/tests/flow/support/mod.rs", "pub fn tool() {}\n");
        put_file(&ws, "clis/tool/tests/fixtures/beta/case.json", "{}\n");
        put_file(&ws, "clis/tool/alpha/Cargo.toml", &manifest("tool-alpha", "lite-rand = \"1\"\n", ""));
        put_file(&ws, "clis/tool/alpha/src/lib.rs", "pub fn alpha() {}\n");
        // a chain: its integration tests run the service of another workspace and the helper of a third (as t3-serve's run rca-service and the scripted
        // CLI that exec-testkit builds from its own workspace) and name their rows in a table, its flow tests read none of them
        put_file(&ws, "clis/chain/Cargo.toml", &manifest("chain", "", "\n[package.metadata.integration]\ninputs = [\"../rows/**\"]\nuses = [\"svc\", \"helper\"]\n"));
        put_file(&ws, "clis/chain/src/main.rs", "fn main() {}\n");
        put_file(&ws, "clis/chain/tests/flow.rs", "#[test]\nfn chains() {}\n");
        put_file(&ws, "clis/chain/tests/integration.rs", "#[test]\nfn runs_the_chain() {}\n");
        put_file(&ws, "clis/rows/table.json", "{}\n");

        put_file(&ws, "libraries/Cargo.toml", "[workspace]\nmembers = [\"helper\", \"helper/plugin\"]\nresolver = \"2\"\n");
        put_file(&ws, "libraries/.cargo/config.toml", "[build]\ntarget-dir = \"../target\"\n");
        put_file(&ws, "libraries/Cargo.lock", &lock(&[("helper", None, &["lite-clock"]), ("helper-plugin", None, &["lite-json"]), ("lite-clock", Some("ccc"), &[]), ("lite-json", Some("ddd"), &[])]));
        put_file(&ws, "libraries/helper/Cargo.toml", &manifest("helper", "lite-clock = \"1\"\n", ""));
        put_file(&ws, "libraries/helper/src/main.rs", "fn main() {}\n");
        put_file(&ws, "libraries/helper/tests/flow.rs", "#[test]\nfn helps() {}\n");
        put_file(&ws, "libraries/helper/plugin/Cargo.toml", &manifest("helper-plugin", "lite-json = \"1\"\n", ""));
        put_file(&ws, "libraries/helper/plugin/src/lib.rs", "pub fn plug() {}\n");

        // the service package takes the remote chat agent's own `nx` settings: the opt-in and the suites' inputs it gives
        let rca: Value = serde_json::from_str(&std::fs::read_to_string(repo.join("backend/remote-chat-agent/package.json")).unwrap()).unwrap();
        let script = |name: &str, original: &str| format!("echo '@fx/svc {name}' >> \"$RAN_LOG\" # {original}");
        let scripts = json!({
            "test:flow": script("test:flow", "cargo test --locked -p svc --test flow --manifest-path Cargo.toml"),
            "test:integration": script("test:integration", "cargo test --locked -p svc --test integration --manifest-path Cargo.toml"),
        });
        put_file(&ws, "backend/svc/package.json", &serde_json::to_string_pretty(&json!({ "name": "@fx/svc", "private": true, "scripts": scripts, "nx": rca["nx"] })).unwrap());
        put_file(&ws, "backend/svc/Cargo.toml", "[workspace]\nmembers = [\"core\", \"rs\"]\nresolver = \"2\"\n");
        put_file(&ws, "backend/svc/.cargo/config.toml", "[build]\ntarget-dir = \"../target\"\n");
        put_file(&ws, "backend/svc/Cargo.lock", &lock(&[("lite-time", Some("eee"), &[]), ("svc", None, &["svc-core"]), ("svc-core", None, &["lite-time"])]));
        put_file(&ws, "backend/svc/core/Cargo.toml", &manifest("svc-core", "lite-time = \"1\"\n", ""));
        put_file(&ws, "backend/svc/core/src/lib.rs", "pub fn core() {}\n\n#[cfg(test)]\nmod tests {}\n");
        put_file(&ws, "backend/svc/rs/Cargo.toml", &manifest("svc", "svc-core = { path = \"../core\" }\n", "\n[package.metadata.flow]\ninputs = [\"../tests/fixtures/**\"]\nuses = [\"helper\"]\n"));
        put_file(&ws, "backend/svc/rs/src/main.rs", "fn main() {}\n");
        put_file(&ws, "backend/svc/rs/tests/flow.rs", "#[test]\nfn serves() {}\n");
        put_file(&ws, "backend/svc/rs/tests/integration.rs", "#[test]\nfn answers() {}\n");
        put_file(&ws, "backend/svc/tests/fixtures/session.json", "{}\n");
        put_file(&ws, "backend/svc/tests/e2e/app.test.ts", "export {};\n");
        put_file(&ws, "backend/svc/Dockerfile", "FROM scratch\n");

        // five crates that differ only in how they depend on the CPU, a sixth that depends on the first, and a seventh that does not
        put_file(&ws, "tiers/Cargo.toml", "[workspace]\nmembers = [\"plain\", \"flagged\", \"detected\", \"quoted\", \"waived\", \"leaning\"]\nresolver = \"2\"\n");
        put_file(&ws, "tiers/Cargo.lock", &lock(&[("detected", None, &[]), ("flagged", None, &[]), ("leaning", None, &["flagged"]), ("plain", None, &[]), ("quoted", None, &[]), ("waived", None, &[])]));
        let sources = [
            ("plain", "pub fn id() {}\n\n#[cfg(test)]\nmod tests {}\n", ""),
            ("flagged", "pub fn id() {}\n", "\n[package.metadata.flow]\nplatform = true\n"),
            ("detected", "pub fn wide() -> bool {\n    cfg!(target_pointer_width = \"64\")\n}\n", ""),
            ("quoted", "// #[cfg(target_arch = \"x86_64\")]\n/* cfg!(target_endian = \"little\") /* nested */ */\npub const A: &str = \"cfg!(target_arch = \\\"arm\\\")\";\npub const B: &str = r#\"#[cfg(target_pointer_width = \"32\")]\"#;\npub const C: &str = \"#![cfg(unix)]\";\n#[cfg(unix)]\npub fn os() {}\n#[cfg(target_os = \"linux\")]\npub fn linux() {}\n", ""),
            ("waived", "#[cfg(target_arch = \"wasm32\")]\npub fn id() {}\n", "\n[package.metadata.flow]\n# a hit that is a pointer to code the CPU does not change\nplatform = false\n"),
            ("leaning", "pub fn id() {}\n", ""),
        ];
        for (name, src, metadata) in sources {
            let deps = if name == "leaning" { "flagged = { path = \"../flagged\" }\n" } else { "" };
            put_file(&ws, &format!("tiers/{name}/Cargo.toml"), &manifest(name, deps, metadata));
            put_file(&ws, &format!("tiers/{name}/src/lib.rs"), src);
            put_file(&ws, &format!("tiers/{name}/tests/flow.rs"), "#[test]\nfn runs() {}\n");
        }

        let f = Fixture { _dir: dir, ws, aside, platform: Cell::new("linux-x64") };
        let flow = |c: &str| json!({ "kind": "flow", "crate": c });
        let chain = json!({ "kind": "integration", "crate": "chain" });
        let group = |g: &str| json!({ "kind": "flow", "crate": "tool", "group": g, "target": format!("cargo-flow-{g}") });
        f.plan("flow-tests", json!([{ "kind": "flow", "package": "@fx/svc" }, flow("kit"), flow("chain"), flow("helper"), group("alpha"), group("beta")]));
        f.plan("integration-tests", json!([{ "kind": "integration", "package": "@fx/svc" }, chain]));
        let units = ["kit", "svc-core"].map(|c| json!({ "kind": "unit", "crate": c }));
        f.plan("typecheck-lint-unit-tests", Value::Array(units.into_iter().chain(CLIPPED.map(|c| json!({ "kind": "clippy", "crate": c }))).collect()));
        f
    }

    fn plan(&self, stage: &str, tests: Value) {
        std::fs::write(self.aside.join(format!("{stage}.json")), json!({ "stages": { stage: tests } }).to_string()).unwrap();
    }

    /// Runs a stage of the plan (the work doubled) and returns what ran, from the log.
    fn stage(&self, stage: &str) -> (RunResult, Ran) {
        let log = self.aside.join("ran.log");
        let _ = std::fs::remove_file(&log);
        let plan = self.aside.join(format!("{stage}.json"));
        let opts = RunOpts::default()
            .bin_dir(&self.aside.join("bin"))
            .cwd(&self.ws)
            .timeout(Duration::from_secs(300))
            .env("WF_TEST_REPO", &self.ws.display().to_string())
            .env("RAN_LOG", &log.display().to_string())
            .env("NX_DAEMON", "false")
            .env("FX_PLATFORM", self.platform.get())
            // the fixture shares the installed node_modules, so pnpm's check before `run` would reinstall it
            .env("pnpm_config_verify_deps_before_run", "false");
        let r = run_wf(&["test", "run", "--stage", stage, "--plan", plan.to_str().unwrap()], &opts);
        let did = std::fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_string).collect();
        (r, did)
    }

    /// What a stage runs; it must pass.
    fn runs(&self, stage: &str) -> Ran {
        let (r, did) = self.stage(stage);
        assert_eq!(r.exit_code, 0, "{stage}: {}\n{}", r.stdout, r.stderr);
        did
    }

    /// Appends a comment (or a line) to one file, runs `check`, and puts the file back.
    fn with_edit(&self, rel: &str, check: impl FnOnce()) {
        let file = self.ws.join(rel);
        let before = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{rel}: {e}"));
        let comment = match rel.rsplit('.').next() {
            Some("rs" | "ts") => "// nx trigger case",
            Some("toml" | "lock") => "# nx trigger case",
            _ => "nx trigger case",
        };
        std::fs::write(&file, format!("{before}\n{comment}\n")).unwrap();
        check();
        std::fs::write(&file, before).unwrap();
    }

    /// Replaces one text in a file, runs `check`, and puts the file back.
    fn with_replace(&self, rel: &str, from: &str, to: &str, check: impl FnOnce()) {
        let file = self.ws.join(rel);
        let before = std::fs::read_to_string(&file).unwrap();
        assert!(before.contains(from), "{rel} has no {from}");
        std::fs::write(&file, before.replacen(from, to, 1)).unwrap();
        check();
        std::fs::write(&file, before).unwrap();
    }

    /// Seeds a stage: the first run runs everything, a second runs nothing.
    fn seed(&self, stage: &str, everything: Ran) {
        assert_eq!(self.runs(stage), everything, "{stage}: the first run runs every task");
        assert_eq!(self.runs(stage), ran(&[]), "{stage}: nothing changed, nothing runs");
    }

    fn check_cases(&self, stage: &str, cases: Vec<Case>) {
        for (what, file, expected) in cases {
            self.with_edit(file, || assert_eq!(self.runs(stage), expected, "{stage}, {what}: {file}"));
        }
        assert_eq!(self.runs(stage), ran(&[]), "{stage}: once each file is back, everything replays");
    }
}

fn every_flow() -> Ran {
    ran(&[SVC_FLOW, KIT_FLOW, CHAIN_FLOW, HELPER_FLOW, ALPHA, BETA])
}

fn every_integration() -> Ran {
    ran(&[SVC_INTEGRATION, CHAIN_INTEGRATION])
}

fn every_check() -> Ran {
    checks(&["kit", "svc-core"], &CLIPPED)
}

#[test]
fn an_opted_in_package_s_crates_are_projects_and_its_suites_key_on_the_crates_and_files_they_build() {
    // Given a package that opts in to crate projects, and every task of the flow, integration and check stages passed once
    let f = Fixture::new();
    f.seed("flow-tests", every_flow());
    f.seed("integration-tests", every_integration());
    f.seed("typecheck-lint-unit-tests", every_check());

    // When one file of the package changes, Then its flow suite runs exactly when the file is one the suite builds or reads
    f.check_cases(
        "flow-tests",
        vec![
            ("the service crate's source", "backend/svc/rs/src/main.rs", ran(&[SVC_FLOW])),
            ("a crate of the package the service builds", "backend/svc/core/src/lib.rs", ran(&[SVC_FLOW])),
            ("the service's flow test", "backend/svc/rs/tests/flow.rs", ran(&[SVC_FLOW])),
            ("the fixtures its flow tests declare", "backend/svc/tests/fixtures/session.json", ran(&[SVC_FLOW])),
            ("the workspace's cargo config", "backend/svc/.cargo/config.toml", ran(&[SVC_FLOW])),
            ("the workspace manifest", "backend/svc/Cargo.toml", ran(&[SVC_FLOW])),
            ("a TypeScript e2e test of the package, which no flow test reads", "backend/svc/tests/e2e/app.test.ts", ran(&[])),
            ("the package's Dockerfile", "backend/svc/Dockerfile", ran(&[])),
        ],
    );
    // And its integration suite runs for the crates it builds, not for the flow tests' fixtures
    f.check_cases(
        "integration-tests",
        vec![
            ("the service crate's source, which the chain's integration tests run too", "backend/svc/rs/src/main.rs", every_integration()),
            ("the service's integration test", "backend/svc/rs/tests/integration.rs", ran(&[SVC_INTEGRATION])),
            ("the fixtures the flow tests declare", "backend/svc/tests/fixtures/session.json", ran(&[])),
            ("a TypeScript e2e test of the package", "backend/svc/tests/e2e/app.test.ts", ran(&[])),
        ],
    );
    // And each crate of the package is a project with its own clippy and unit tasks, keyed by its sources and its dependencies'
    f.check_cases(
        "typecheck-lint-unit-tests",
        vec![
            ("a crate the service builds", "backend/svc/core/src/lib.rs", checks(&["svc-core"], &["svc-core", "svc"])),
            ("the service's flow test (clippy lints every target)", "backend/svc/rs/tests/flow.rs", checks(&[], &["svc"])),
            ("the fixtures the flow tests declare", "backend/svc/tests/fixtures/session.json", ran(&[])),
            ("the workspace's cargo config", "backend/svc/.cargo/config.toml", checks(&["svc-core"], &["svc-core", "svc"])),
        ],
    );
}

#[test]
fn a_declared_glob_leaves_out_the_crates_nested_under_it() {
    // Given tool's flow tests read `clis/assets/**`, where the crate assets-gen sits, and run helper, in whose dir helper-plugin sits
    let f = Fixture::new();
    f.seed("flow-tests", every_flow());

    // When a file under the declared dir or a nested crate changes, Then only a file the glob means reruns the groups
    f.check_cases(
        "flow-tests",
        vec![
            ("a data file the flow tests declare", "clis/assets/logo.txt", ran(&[ALPHA, BETA])),
            ("a crate nested in that dir, which tool does not build", "clis/assets/gen/src/lib.rs", ran(&[])),
            ("a crate nested in a used crate's dir, which that crate does not build", "libraries/helper/plugin/src/lib.rs", ran(&[])),
        ],
    );
    // And the nested crate is a project of its own, checked on its own changes
    f.seed("typecheck-lint-unit-tests", every_check());
    f.check_cases("typecheck-lint-unit-tests", vec![("the nested crate", "clis/assets/gen/src/lib.rs", checks(&[], &["assets-gen"]))]);
}

#[test]
fn a_flow_group_runs_again_only_for_its_module_the_shared_files_and_the_crates_it_builds() {
    // Given tool's flow tests are two groups (alpha names the crate tool-alpha, beta a fixture dir) sharing `tests/flow/support`
    let f = Fixture::new();
    f.seed("flow-tests", every_flow());

    // When one file changes, Then exactly the groups that read it run, each filtered to its own module
    f.check_cases(
        "flow-tests",
        vec![
            ("one group's module", "clis/tool/tests/flow/alpha.rs", ran(&[ALPHA])),
            ("the other group's module", "clis/tool/tests/flow/beta.rs", ran(&[BETA])),
            ("a fixture only one group names", "clis/tool/tests/fixtures/beta/case.json", ran(&[BETA])),
            ("the crate one group names", "clis/tool/alpha/src/lib.rs", ran(&[ALPHA])),
            ("the support every group shares", "clis/tool/tests/flow/support/mod.rs", ran(&[ALPHA, BETA])),
            ("the suite's module list", "clis/tool/tests/flow.rs", ran(&[ALPHA, BETA])),
            ("the crate's own source", "clis/tool/src/main.rs", ran(&[ALPHA, BETA])),
            ("the crate's manifest", "clis/tool/Cargo.toml", ran(&[ALPHA, BETA])),
            ("a dependency no group names", "clis/kit/src/lib.rs", ran(&[KIT_FLOW, ALPHA, BETA])),
            ("another crate's flow test", "clis/kit/tests/flow.rs", ran(&[KIT_FLOW])),
        ],
    );
}

#[test]
fn a_rust_task_keys_on_the_lockfile_entries_its_crate_reaches_and_on_the_whole_file_when_it_cannot_read_it() {
    // Given every flow and check task passed once
    let f = Fixture::new();
    f.seed("flow-tests", every_flow());
    f.seed("typecheck-lint-unit-tests", every_check());
    let lockfile = |file: &str, from: &str, to: &str, stage: &str, expected: Ran| f.with_replace(file, from, to, || assert_eq!(f.runs(stage), expected, "{stage}: {file} {from} -> {to}"));

    // When a registry crate only tool-alpha uses moves, Then the tasks of tool-alpha and of tool, which builds it, run; kit's do not
    lockfile("clis/Cargo.lock", "checksum = \"aaa\"", "checksum = \"aab\"", "flow-tests", ran(&[ALPHA, BETA]));
    lockfile("clis/Cargo.lock", "checksum = \"aaa\"", "checksum = \"aab\"", "typecheck-lint-unit-tests", checks(&[], &["tool-alpha", "tool"]));
    // When kit's registry crate moves, Then kit's tasks run, and tool's, which builds kit
    lockfile("clis/Cargo.lock", "checksum = \"bbb\"", "checksum = \"bbc\"", "flow-tests", ran(&[KIT_FLOW, ALPHA, BETA]));
    lockfile("clis/Cargo.lock", "checksum = \"bbb\"", "checksum = \"bbc\"", "typecheck-lint-unit-tests", checks(&["kit"], &["kit", "tool"]));
    // When the lockfile no longer parses, Then every task of its workspace runs: a slice it cannot read is the whole file
    lockfile("clis/Cargo.lock", "[[package]]", "[[package", "flow-tests", ran(&[KIT_FLOW, CHAIN_FLOW, ALPHA, BETA]));
    lockfile("clis/Cargo.lock", "[[package]]", "[[package", "typecheck-lint-unit-tests", checks(&["kit"], &["kit", "assets-gen", "chain", "tool", "tool-alpha"]));
    // When only a comment of the lockfile changes, Then nothing runs
    f.check_cases("flow-tests", vec![("a comment in the lockfile", "clis/Cargo.lock", ran(&[]))]);
    // When the service's registry crate moves, Then the package's flow suite and its crates' checks run
    lockfile("backend/svc/Cargo.lock", "checksum = \"eee\"", "checksum = \"eef\"", "flow-tests", ran(&[SVC_FLOW]));
    lockfile("backend/svc/Cargo.lock", "checksum = \"eee\"", "checksum = \"eef\"", "typecheck-lint-unit-tests", checks(&["svc-core"], &["svc-core", "svc"]));
}

#[test]
fn a_flow_test_keys_on_the_crates_it_uses_and_a_misspelled_one_stops_the_graph() {
    // Given tool's flow groups and the service's flow and integration suites run the helper binary of another workspace
    let f = Fixture::new();
    f.seed("flow-tests", every_flow());
    f.seed("integration-tests", every_integration());
    f.seed("typecheck-lint-unit-tests", every_check());
    let users = ran(&[HELPER_FLOW, ALPHA, BETA, SVC_FLOW]);

    // When a file helper builds from changes, Then every flow suite that uses it runs
    f.check_cases(
        "flow-tests",
        vec![
            ("the used crate's source", "libraries/helper/src/main.rs", users.clone()),
            ("the used crate's manifest", "libraries/helper/Cargo.toml", users.clone()),
            ("its workspace manifest", "libraries/Cargo.toml", users.clone()),
            ("its workspace's cargo config", "libraries/.cargo/config.toml", users.clone()),
            ("the used crate's own flow test", "libraries/helper/tests/flow.rs", ran(&[HELPER_FLOW])),
        ],
    );
    let lockfile = |from: &str, to: &str, stage: &str, expected: Ran| f.with_replace("libraries/Cargo.lock", from, to, || assert_eq!(f.runs(stage), expected, "{stage}: {from} -> {to}"));
    lockfile("checksum = \"ccc\"", "checksum = \"ccd\"", "flow-tests", users.clone());
    lockfile("checksum = \"ddd\"", "checksum = \"dde\"", "flow-tests", ran(&[]));
    // And the integration suites that run it too (the service's, and the chain's, which names it for itself), while the users' clippy and unit tasks do not: they build no helper
    f.check_cases("integration-tests", vec![("the used crate's source", "libraries/helper/src/main.rs", every_integration())]);
    f.check_cases("typecheck-lint-unit-tests", vec![("the used crate's source", "libraries/helper/src/main.rs", checks(&[], &["helper"]))]);

    // When a crate's flow metadata names a crate no workspace has, Then the graph stops, naming the crate, the key and the name (Nx
    // runs the plugins in process for wf, so the plugin's own line is never lost with a worker)
    for (from, to, named) in [("uses = [\"helper\"]", "uses = [\"helpr\"]", "tool: [package.metadata.flow] uses names \"helpr\""), ("alpha = [\"tool-alpha\"]", "alpha = [\"tool-alfa\"]", "tool: [package.metadata.flow] groups.alpha names \"tool-alfa\"")] {
        f.with_replace("clis/tool/Cargo.toml", from, to, || {
            let (r, did) = f.stage("flow-tests");
            assert_eq!((r.exit_code, did), (1, ran(&[])), "{}\n{}", r.stdout, r.stderr);
            assert!(r.stderr.contains(named), "{named} not in: {}", r.stderr);
        });
    }
}

#[test]
fn an_integration_chain_keys_on_the_binaries_it_runs_in_other_workspaces_and_the_table_it_names_and_its_flow_tests_on_none_of_them() {
    // Given a chain whose integration tests run the service of another workspace and the helper of a third, and name their rows in a table,
    // and every task passed once
    let f = Fixture::new();
    f.seed("flow-tests", every_flow());
    f.seed("integration-tests", every_integration());
    f.seed("typecheck-lint-unit-tests", every_check());

    // When a file the chain runs or reads changes, Then its integration task runs, with the service's own when the file is the service's
    f.check_cases(
        "integration-tests",
        vec![
            ("the chain's source", "clis/chain/src/main.rs", ran(&[CHAIN_INTEGRATION])),
            ("the chain's integration test", "clis/chain/tests/integration.rs", ran(&[CHAIN_INTEGRATION])),
            ("the table it names its rows from", "clis/rows/table.json", ran(&[CHAIN_INTEGRATION])),
            ("the service binary's source, in another workspace", "backend/svc/rs/src/main.rs", every_integration()),
            ("a crate the service builds", "backend/svc/core/src/lib.rs", every_integration()),
            ("the service workspace's manifest", "backend/svc/Cargo.toml", every_integration()),
            ("the service workspace's cargo config", "backend/svc/.cargo/config.toml", every_integration()),
            ("a file only the service's flow tests read", "backend/svc/tests/fixtures/session.json", ran(&[])),
            ("the service's own integration test, which the chain does not build", "backend/svc/rs/tests/integration.rs", ran(&[SVC_INTEGRATION])),
        ],
    );
    // And a registry crate the service reaches moving in its workspace's lockfile reruns the chain too
    f.with_replace("backend/svc/Cargo.lock", "checksum = \"eee\"", "checksum = \"eef\"", || assert_eq!(f.runs("integration-tests"), every_integration(), "the service's lockfile slice"));
    assert_eq!(f.runs("integration-tests"), ran(&[]), "once the lockfile is back, everything replays");
    // And what the chain runs in the third workspace is an input of its task and of the service's alike: the helper's source, its workspace's
    // manifest, cargo config and the lockfile slice it reaches, but not a registry crate only the nested plugin reaches
    f.check_cases(
        "integration-tests",
        vec![
            ("the helper's source, in a third workspace", "libraries/helper/src/main.rs", every_integration()),
            ("that workspace's manifest", "libraries/Cargo.toml", every_integration()),
            ("that workspace's cargo config", "libraries/.cargo/config.toml", every_integration()),
        ],
    );
    let third = |from: &str, to: &str, expected: Ran| f.with_replace("libraries/Cargo.lock", from, to, || assert_eq!(f.runs("integration-tests"), expected, "the third workspace's lockfile: {from}"));
    third("checksum = \"ccc\"", "checksum = \"ccd\"", every_integration());
    third("checksum = \"ddd\"", "checksum = \"dde\"", ran(&[]));
    // And the chain's flow tests read none of that: not the service, not the table
    f.check_cases(
        "flow-tests",
        vec![
            ("the service binary's source", "backend/svc/rs/src/main.rs", ran(&[SVC_FLOW])),
            ("the service workspace's manifest", "backend/svc/Cargo.toml", ran(&[SVC_FLOW])),
            ("the table the integration tests name", "clis/rows/table.json", ran(&[])),
            ("the chain's own source", "clis/chain/src/main.rs", ran(&[CHAIN_FLOW])),
            ("the chain's flow test", "clis/chain/tests/flow.rs", ran(&[CHAIN_FLOW])),
            ("the helper the chain's integration tests run", "libraries/helper/src/main.rs", ran(&[HELPER_FLOW, ALPHA, BETA, SVC_FLOW])),
        ],
    );

    // When the chain's integration metadata names a crate no workspace has, Then the graph stops, naming the crate, the key and the name
    f.with_replace("clis/chain/Cargo.toml", "uses = [\"svc\", \"helper\"]", "uses = [\"svcs\", \"helper\"]", || {
        let (r, did) = f.stage("integration-tests");
        assert_eq!((r.exit_code, did), (1, ran(&[])), "{}\n{}", r.stdout, r.stderr);
        assert!(r.stderr.contains("chain: [package.metadata.integration] uses names \"svcs\""), "{}", r.stderr);
    });
}

const TIERED: [&str; 6] = ["plain", "flagged", "detected", "quoted", "waived", "leaning"];

/// The tier fixture's tasks: each crate's flow test and its clippy run.
fn tier_tasks(crates: &[&str]) -> Ran {
    crates.iter().flat_map(|c| [format!("test {c} flow"), format!("clippy {c}")]).collect()
}

fn tier_plan(f: &Fixture) {
    let flows = TIERED.map(|c| json!({ "kind": "flow", "crate": c }));
    f.plan("flow-tests", Value::Array(flows.into_iter().collect()));
    f.plan("typecheck-lint-unit-tests", Value::Array(TIERED.map(|c| json!({ "kind": "clippy", "crate": c })).into_iter().collect()));
}

/// Both stages of the tier fixture on the platform the fixture is set to.
fn tier_runs(f: &Fixture) -> Ran {
    f.runs("flow-tests").into_iter().chain(f.runs("typecheck-lint-unit-tests")).collect()
}

#[test]
fn only_a_crate_in_the_platform_tier_runs_again_on_another_platform_and_its_dependents_do_not() {
    // Given six crates, each task passed once on x86_64: one flagged `platform = true`, one that reads the pointer width in code, one whose only
    // hits sit in comments and strings or name the OS, one with a hit its manifest waives, a dependent of the flagged one, and a plain one
    let f = Fixture::new();
    tier_plan(&f);
    assert_eq!(tier_runs(&f), tier_tasks(&TIERED), "the first run runs every task");
    assert_eq!(tier_runs(&f), ran(&[]), "nothing changed, nothing runs");

    // When the same commit is checked out on arm64, Then exactly the flagged and the detected crate run again, each in all its cargo tasks
    f.platform.set("linux-arm64");
    assert_eq!(tier_runs(&f), tier_tasks(&["flagged", "detected"]), "the platform tier");
    // And what x86_64 passed still replays there
    f.platform.set("linux-x64");
    assert_eq!(tier_runs(&f), ran(&[]), "back on x86_64 every pass replays");

    // When a plain crate gains a CPU-dependent cfg in code, Then it is in the platform tier from that edit on: x86_64 runs it for the edit, arm64 again
    f.with_replace("tiers/plain/src/lib.rs", "pub fn id() {}", "pub fn id() -> bool {\n    cfg!(target_arch = \"aarch64\")\n}", || {
        assert_eq!(tier_runs(&f), tier_tasks(&["plain"]), "the edit on x86_64");
        f.platform.set("linux-arm64");
        assert_eq!(tier_runs(&f), tier_tasks(&["plain"]), "the gained cfg on arm64");
        f.platform.set("linux-x64");
    });
    // And once the edit is back it is plain again: its old pass replays on either platform
    assert_eq!(tier_runs(&f), ran(&[]), "the plain crate on x86_64");
    f.platform.set("linux-arm64");
    assert_eq!(tier_runs(&f), ran(&[]), "the plain crate on arm64");
}
