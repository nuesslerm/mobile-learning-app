//! The Nx cache's contract (owner, 18 Oct): a task runs again exactly when one of its inputs changed, and is replayed exactly
//! when none did. The workspace is a copy of this repository's files with its real `nx.json`, the plugins in `tooling/nx` and
//! the real Nx; only the work is doubled: the `wf` an e2e target calls, the package scripts and `cargo test` write the task
//! they ran to a log (a replay prints the same output, so the log, not the output, says what ran). Each case edits one file,
//! runs the stage through `wf test run --stage`, compares what ran with what must, and puts the file back.
use crate::support::{node_path, repo_root, run_wf, RunOpts, RunResult};
use blob_cache::adapters::DirStore;
use blob_cache::domain::{Budget, Degraded, Generation, Key};
use blob_cache::ports::{BlobStore, PutOutcome, StoreError};
use blob_cache::{Cache, NxServer, Served, Writes};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const BROWSER: [&str; 4] = ["app-routes", "editor-typing", "tamil-writing", "voice-edit"];
const ANDROID: [&str; 4] = ["editor-typing", "stubbed-run-local", "tamil-writing", "voice-edit"];
const HARNESS: [&str; 5] = ["@cli/harness-contracts", "@cli/harness-data", "@cli/harness-use-cases", "@cli/harness-di", "@cli/harness-ui"];
const SERVICES: [&str; 5] = ["@app/leetcode-auth-cookie-capture", "@app/leetcode-backend", "@app/remote-chat-agent", "@backend/reporting-service", "@infra/watchdog"];
const CRATES: [&str; 17] = ["agent-config-sync", "cdp", "cli-http", "harness-flow", "knowledge", "rca-core", "rca-ladder", "wf", "offload", "wf-bench", "wf-bucket", "wf-harness", "wf-login", "pipeline", "secrets", "deploy", "wf-vmops"];

struct Workspace {
    dir: tempfile::TempDir,
    /// The doubles, plans, logs and run dirs: outside the workspace, so they are no project's files.
    aside: tempfile::TempDir,
    bin: PathBuf,
    log: PathBuf,
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub(crate) fn write_script(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

impl Workspace {
    /// The repository's tracked and new files (not the ignored ones), its installed node_modules, and the doubles.
    fn copy() -> Workspace {
        let repo = repo_root();
        let dir = tempfile::Builder::new().prefix("wf-flow-nx-").tempdir().unwrap();
        let listed = Command::new("git").args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"]).current_dir(&repo).output().unwrap();
        for file in String::from_utf8_lossy(&listed.stdout).split('\0').filter(|f| !f.is_empty() && !f.starts_with("references/")) {
            let (from, to) = (repo.join(file), dir.path().join(file));
            if from.is_file() {
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::copy(&from, &to).unwrap();
            }
        }
        std::os::unix::fs::symlink(repo.join("node_modules"), dir.path().join("node_modules")).unwrap();
        // each package's own links as pnpm wrote them (relative): Nx resolves a bare import (`@lib/...`) through them to the
        // copy's own library, and an npm package through the root node_modules to the store
        for manifest in String::from_utf8_lossy(&listed.stdout).split('\0').filter(|f| f.ends_with("/package.json") && !f.starts_with("references/")) {
            let modules = Path::new(manifest).parent().unwrap().join("node_modules");
            for entry in std::fs::read_dir(repo.join(&modules)).into_iter().flatten().flatten() {
                let scoped = entry.file_name().to_string_lossy().starts_with('@') && entry.path().is_dir() && !entry.path().is_symlink();
                let links: Vec<PathBuf> = if scoped { std::fs::read_dir(entry.path()).unwrap().flatten().map(|e| e.path()).collect() } else { vec![entry.path()] };
                for link in links.into_iter().filter(|l| l.is_symlink()) {
                    let to = dir.path().join(link.strip_prefix(&repo).unwrap());
                    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                    std::os::unix::fs::symlink(std::fs::read_link(&link).unwrap(), to).unwrap();
                }
            }
        }
        let aside = tempfile::Builder::new().prefix("wf-flow-nx-aside-").tempdir().unwrap();
        let (bin, log) = (aside.path().join("bin"), aside.path().join("ran.log"));
        std::fs::create_dir_all(&bin).unwrap();
        // the e2e target's `wf test run <test> --platform <p> --sha <s> --out <dir>`: a pass, or a failure for $FAIL_TEST
        write_script(
            &bin.join("wf"),
            r#"test="$3"; while [ $# -gt 0 ]; do case "$1" in --platform) p="$2";; --out) out="$2";; esac; shift; done
echo "e2e $p $test" >> "$RAN_LOG"; mkdir -p "$out"
if [ "$FAIL_TEST" = "$test" ]; then echo '{"ok":false,"units":[]}' > "$out/summary.json"; exit 1; fi
echo "{\"ok\":true,\"sha\":\"seeded\",\"units\":[{\"test\":\"$test\",\"verdict\":\"pass\"}]}" > "$out/summary.json"
"#,
        );
        // the graph is built in CI jobs that have no Rust toolchain, so it must not need cargo (18 Oct: every e2e job failed)
        write_script(&bin.join("cargo"), "if [ \"$1\" = metadata ]; then echo 'the Nx graph must not need cargo' >&2; exit 1; fi\nwhile [ $# -gt 0 ]; do [ \"$1\" = -p ] && echo \"cargo $2\" >> \"$RAN_LOG\"; shift; done\n");
        let w = Workspace { dir, aside, bin, log };
        // each package script the stages run only says it ran
        for (name, root) in w.roots(&[&HARNESS[..], &SERVICES[..]].concat()) {
            let manifest = w.dir.path().join(&root).join("package.json");
            let mut m: Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
            for script in ["typecheck", "lint", "test:unit", "test:flow"] {
                // the original stays as a comment: a `cargo -p <crate>` in it is what links the package to that crate
                let original = m["scripts"][script].as_str().unwrap_or_default().to_string();
                m["scripts"][script] = json!(format!("echo '{name} {script}' >> \"$RAN_LOG\" # {original}"));
            }
            std::fs::write(&manifest, serde_json::to_string_pretty(&m).unwrap()).unwrap();
        }
        w
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// Project name to root, from the real graph.
    fn roots(&self, names: &[&str]) -> Vec<(String, String)> {
        names
            .iter()
            .map(|n| {
                let out = Command::new(self.path("node_modules/.bin/nx")).args(["show", "project", n, "--json"]).current_dir(self.dir.path()).env("NX_DAEMON", "false").output().unwrap();
                let v: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| panic!("no project {n}: {}", String::from_utf8_lossy(&out.stderr)));
                (n.to_string(), v["root"].as_str().unwrap().to_string())
            })
            .collect()
    }

    fn wf(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut opts = RunOpts::default().bin_dir(&self.bin).cwd(self.dir.path()).timeout(Duration::from_secs(300)).env("WF_TEST_REPO", &self.dir.path().display().to_string()).env("RAN_LOG", &self.log.display().to_string()).env("NX_DAEMON", "false").env("WF_NX_WHY", "0")
            // the copy shares the installed node_modules, so pnpm's check before `run` would reinstall it
            .env("pnpm_config_verify_deps_before_run", "false")
            .env("WF_E2E_BIN", &self.bin.join("wf").display().to_string());
        for (k, v) in env {
            opts = opts.env(k, v);
        }
        run_wf(args, &opts)
    }

    fn plan(&self, stage: &str, tests: Vec<Value>) -> PathBuf {
        let file = self.aside.path().join(format!("{stage}.json"));
        std::fs::write(&file, json!({ "stages": { stage: tests } }).to_string()).unwrap();
        file
    }

    fn e2e_plan(&self, platform: &str) -> PathBuf {
        let names: &[&str] = if platform == "browser" { &BROWSER } else { &ANDROID };
        self.plan(&format!("{platform}-e2e-tests"), names.iter().map(|n| json!({ "kind": "e2e", "name": n })).collect())
    }

    /// The e2e tests the stage would run now (`--probe`: Nx's verdict, nothing runs).
    fn would_run(&self, platform: &str) -> BTreeSet<String> {
        let plan = self.e2e_plan(platform);
        let r = self.wf(&["test", "run", "--stage", &format!("{platform}-e2e-tests"), "--plan", plan.to_str().unwrap(), "--probe"], &[]);
        assert_eq!(r.exit_code, 0, "probe {platform}: {}\n{}", r.stdout, r.stderr);
        let v: Value = serde_json::from_str(r.stdout.trim().lines().last().unwrap()).unwrap();
        v["ran"].as_array().unwrap().iter().map(|t| t.as_str().unwrap().to_string()).collect()
    }

    /// Runs a stage for real (the work doubled) and returns what ran, from the log.
    fn run_stage(&self, stage: &str, plan: &Path, env: &[(&str, &str)]) -> (RunResult, BTreeSet<String>) {
        self.run_stage_at(stage, plan, "seeded", env)
    }

    fn run_stage_at(&self, stage: &str, plan: &Path, sha: &str, env: &[(&str, &str)]) -> (RunResult, BTreeSet<String>) {
        let _ = std::fs::remove_file(&self.log);
        let out = self.aside.path().join(format!("out-{stage}"));
        let r = self.wf(&["test", "run", "--stage", stage, "--plan", plan.to_str().unwrap(), "--sha", sha, "--out", out.to_str().unwrap()], env);
        let ran = std::fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_string).collect();
        (r, ran)
    }

    /// Commits nothing with `message` at the copy's head (the first call makes the copy a git repository) and returns the sha.
    fn commit(&self, message: &str) -> String {
        let git = |args: &[&str]| Command::new("git").args(args).current_dir(self.dir.path()).output().unwrap();
        if !self.path(".git").exists() {
            git(&["init", "-q"]);
        }
        let done = git(&["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", message]);
        assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
        String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout).trim().to_string()
    }

    /// Replaces one text in a file, runs `check`, and puts the file back.
    fn with_replace(&self, rel: &str, from: &str, to: &str, check: impl FnOnce()) {
        let file = self.path(rel);
        let before = std::fs::read_to_string(&file).unwrap();
        assert!(before.contains(from), "{rel} has no {from}");
        std::fs::write(&file, before.replacen(from, to, 1)).unwrap();
        check();
        std::fs::write(&file, before).unwrap();
    }

    /// Edits one file (appends, or creates it), runs `check`, and puts the file back.
    fn with_edit(&self, rel: &str, check: impl FnOnce()) {
        let file = self.path(rel);
        let before = std::fs::read(&file).ok();
        let mut after = before.clone().unwrap_or_default();
        after.extend_from_slice(if rel.ends_with(".lua") { b"\n-- nx trigger case\n" } else if rel.ends_with(".md") || rel.ends_with(".yaml") { b"\n" } else { b"\n// nx trigger case\n" });
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, after).unwrap();
        check();
        match before {
            Some(b) => std::fs::write(&file, b).unwrap(),
            None => std::fs::remove_file(&file).unwrap(),
        }
    }
}

fn package_plan(w: &Workspace) -> PathBuf {
    let mut tests = Vec::new();
    for kind in ["typecheck", "lint", "unit"] {
        tests.extend(HARNESS.iter().map(|p| json!({ "kind": kind, "package": p })));
    }
    w.plan("typecheck-lint-unit-tests", tests)
}

fn flow_plan(w: &Workspace) -> PathBuf {
    let mut tests: Vec<Value> = SERVICES.iter().map(|p| json!({ "kind": "flow", "package": p })).collect();
    tests.extend(CRATES.iter().map(|c| json!({ "kind": "flow", "crate": c })));
    w.plan("flow-tests", tests)
}

fn ran(lines: &[String]) -> BTreeSet<String> {
    lines.iter().cloned().collect()
}

#[test]
fn nx_runs_an_e2e_test_exactly_when_its_feature_code_test_case_or_the_app_shell_changed_and_replays_it_otherwise() {
    let w = Workspace::copy();

    // Given every focused test passed once on each harness
    for platform in ["browser", "android"] {
        let plan = w.e2e_plan(platform);
        let (r, did) = w.run_stage(&format!("{platform}-e2e-tests"), &plan, &[]);
        assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
        let all: &[&str] = if platform == "browser" { &BROWSER } else { &ANDROID };
        assert_eq!(did, all.iter().map(|t| format!("e2e {platform} {t}")).collect(), "the first run runs every test");
    }
    // Then nothing changed means nothing runs
    assert_eq!((w.would_run("browser"), w.would_run("android")), (set(&[]), set(&[])));

    let (all_b, all_a) = (set(&BROWSER), set(&ANDROID));
    let cases: Vec<(&str, &str, BTreeSet<String>, BTreeSet<String>)> = vec![
        ("an in-app feature's code", "frontend/mla-tamagui/features/tamil-writing/data/tamilCourse.ts", set(&["app-routes", "tamil-writing"]), set(&["tamil-writing"])),
        ("the leetcode feature's code", "frontend/mla-tamagui/features/leetcode/auth/LeetCodeBridge.tsx", set(&["app-routes", "editor-typing", "voice-edit"]), set(&["editor-typing", "stubbed-run-local", "voice-edit"])),
        ("a new file in a feature", "frontend/mla-tamagui/features/tamil-writing/data/added.ts", set(&["app-routes", "tamil-writing"]), set(&["tamil-writing"])),
        ("a feature package's code", "frontend/mla-tamagui/features/coding-agent/src/contracts/dispatchRuns.ts", set(&["app-routes", "voice-edit"]), set(&["voice-edit"])),
        // the whole-app test is app-routes in the browser; no focused Android test is, so an app change none of them names publishes an update
        // no Android run loads (progress ledger, 22 Oct S3 stage 1: open items)
        ("a feature package no test names", "frontend/mla-tamagui/features/settings/src/index.ts", set(&["app-routes"]), set(&[])),
        ("one test case's script", "e2e/editor-typing.lua", set(&["editor-typing"]), set(&["editor-typing"])),
        ("the Android warm-up every Android test starts with", "e2e/warm-app.lua", set(&[]), all_a.clone()),
        ("an Android golden", "docs/runbooks/goldens/android/editor-two-sum-result.json", set(&[]), all_a.clone()),
        ("a browser golden", "docs/runbooks/goldens/browser/web-home.json", all_b.clone(), set(&[])),
        ("the app shell", "frontend/mla-tamagui/app-shell/AppDrawerContent.tsx", all_b.clone(), all_a.clone()),
        ("a library every feature imports", "frontend/libraries/tamagui-design-system/composites/collapsing-header.tsx", all_b.clone(), all_a.clone()),
        ("a library only the app shell imports: the whole-app test", "frontend/libraries/rn-agentation/src/index.ts", set(&["app-routes"]), set(&[])),
        // a service has its own deployment verification tests (owner, 18 Oct), and the lockfile counts only through the packages the code imports
        ("the backend a test calls", "backend/remote-chat-agent/rs/src/api/app.rs", set(&[]), set(&[])),
        ("a crate that backend builds", "libraries/rca-core/src/file_store.rs", set(&[]), set(&[])),
        ("the lockfile, no package changed", "pnpm-lock.yaml", set(&[]), set(&[])),
        ("a patch file alone (pnpm records its hash in the lockfile)", "patches/sonner-native@0.26.4.patch", set(&[]), set(&[])),
        ("the rest of wf (owner, 18 Oct)", "clis/wf/src/main.rs", set(&[]), set(&[])),
        // the device harness reruns the tests it drives (A5, 20 Oct): each platform's driver its own, the shared Lua API both
        ("the Android driver", "clis/harness-android/src/driver.rs", set(&[]), all_a.clone()),
        ("the browser driver", "clis/harness-browser/src/browser_launch.rs", all_b.clone(), set(&[])),
        ("the Lua API both drivers serve", "clis/harness-core/src/device/lua.rs", all_b.clone(), all_a.clone()),
        ("the command dispatcher both drivers serve", "clis/harness-core/src/device/commands.rs", all_b.clone(), all_a.clone()),
        ("the browser's page finder", "clis/harness-browser/src/browser_find.js", all_b.clone(), set(&[])),
        ("the server of the web export the browser tests load", "clis/pipeline/src/export_site.rs", all_b.clone(), set(&[])),
        ("the HTTP listener that server answers through", "clis/harness-core/src/staticd.rs", all_b.clone(), set(&[])),
        // the stage runner only starts units and hands each its base; what a unit does is in the harness files above
        ("the runner that picks each unit's target", "clis/pipeline/src/e2e_run.rs", set(&[]), set(&[])),
        ("the resolver of the update an Android unit loads", "clis/pipeline/src/device_run.rs", set(&[]), set(&[])),
        ("the CDP crate the browser driver runs on", "clis/cdp/src/attached.rs", all_b.clone(), set(&[])),
        ("a feature's unit test", "frontend/mla-tamagui/features/leetcode/auth/__tests__/webBridgeClient.test.ts", set(&[]), set(&[])),
        ("a doc", "docs/plans/nx-cache/plan.md", set(&[]), set(&[])),
        ("a service no test calls", "backend/leetcode-backend/rs/src/main.rs", set(&[]), set(&[])),
    ];
    for (what, file, browser, android) in cases {
        // When one file changes, Then exactly the tests that read it run, on each harness
        w.with_edit(file, || assert_eq!((w.would_run("browser"), w.would_run("android")), (browser, android), "{what}: {file}"));
    }
    // When a package the leetcode feature imports is patched anew (its patch hash in the lockfile), Then the tests that drive
    // leetcode run, and Tamil's, which never loads it, stays replayed
    let sonner = std::fs::read_to_string(w.path("pnpm-lock.yaml")).unwrap().lines().find(|l| l.starts_with("  sonner-native@")).unwrap().to_string();
    let repatched = format!("{}0", &sonner[..sonner.len() - 1]);
    w.with_replace("pnpm-lock.yaml", &sonner, &repatched, || {
        assert_eq!((w.would_run("browser"), w.would_run("android")), (set(&["app-routes", "editor-typing", "voice-edit"]), set(&["editor-typing", "stubbed-run-local", "voice-edit"])), "sonner-native patched");
    });
    // And once each file is back, every test is replayed again
    assert_eq!((w.would_run("browser"), w.would_run("android")), (set(&[]), set(&[])), "every edit put back");

    // Given a test fails after its feature changed, When it is run again unchanged, Then it runs again: a failure is never replayed
    w.with_edit("frontend/mla-tamagui/features/tamil-writing/data/tamilCourse.ts", || {
        let plan = w.e2e_plan("android");
        let (r, did) = w.run_stage("android-e2e-tests", &plan, &[("FAIL_TEST", "tamil-writing")]);
        assert_eq!((r.exit_code, did), (1, set(&["e2e android tamil-writing"])), "{}", r.stdout);
        assert_eq!(w.would_run("android"), set(&["tamil-writing"]), "the failure is not cached");
        let (r, did) = w.run_stage("android-e2e-tests", &plan, &[]);
        assert_eq!((r.exit_code, did), (0, set(&["e2e android tamil-writing"])));
        let summary: Value = serde_json::from_str(r.stdout.trim().lines().last().unwrap()).unwrap();
        assert_eq!((summary["ran"].clone(), summary["cached"].as_array().unwrap().len()), (json!(["tamil-writing"]), 3), "the other three are replayed with their earlier runs");
        assert_eq!(w.would_run("android"), set(&[]), "the pass is cached");
    });

    // Given run flags (an --update-goldens run), Then every test runs, cache or not
    let plan = w.e2e_plan("browser");
    let r = w.wf(&["test", "run", "--stage", "browser-e2e-tests", "--plan", plan.to_str().unwrap(), "--sha", "seeded", "--out", w.aside.path().join("flags").to_str().unwrap(), "--", "--update-goldens"], &[]);
    let summary: Value = serde_json::from_str(r.stdout.trim().lines().last().unwrap()).unwrap();
    assert_eq!(summary["ran"].as_array().unwrap().len(), 4, "{}", r.stdout);
}

#[test]
fn an_e2e_fresh_trailer_runs_the_stage_fresh() {
    let w = Workspace::copy();
    // Given a copy that is a git repository from the start (Nx honours .gitignore only inside one, so the hashes move with it)
    w.commit("The copy");
    // And every focused test passed once on each harness, so nothing would run
    for platform in ["browser", "android"] {
        let (r, _) = w.run_stage(&format!("{platform}-e2e-tests"), &w.e2e_plan(platform), &[]);
        assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
    }
    assert_eq!((w.would_run("browser"), w.would_run("android")), (set(&[]), set(&[])));
    // And when the browser job builds the web export into the app's dist before its tests (git ignores it), nothing would run: a
    // built export is a product of the inputs the tests are keyed on, not one of them
    w.with_edit("frontend/mla-tamagui/dist/index.html", || assert_eq!((w.would_run("browser"), w.would_run("android")), (set(&[]), set(&[])), "a built export is no input"));
    w.with_edit("frontend/mla-tamagui/dist/wf-export.json", || assert_eq!(w.would_run("browser"), set(&[]), "its stamp is no input"));

    // When the head commit carries `E2E-Fresh: android` and changes nothing
    let sha = w.commit("Measure the Android units again\n\nE2E-Fresh: android");
    // Then the Android probe names every test, and the browser stage stays replayed: the trailer names one platform
    assert_eq!((w.would_run("browser"), w.would_run("android")), (set(&[]), set(&ANDROID)));
    // When the Android stage runs at that commit, Then every test ran though its inputs are unchanged, and the summary says why
    let (r, did) = w.run_stage_at("android-e2e-tests", &w.e2e_plan("android"), &sha, &[]);
    assert_eq!((r.exit_code, did), (0, ANDROID.iter().map(|t| format!("e2e android {t}")).collect()), "{}", r.stdout);
    assert_eq!(last_line(&r)["fresh"], json!(true), "{}", r.stdout);

    // When the trailer says `all`, Then both stages run fresh
    w.commit("Measure everything\n\nE2E-Fresh: all");
    assert_eq!((w.would_run("browser"), w.would_run("android")), (set(&BROWSER), set(&ANDROID)));
    // And a later commit without it is replayed again
    w.commit("An ordinary commit");
    assert_eq!((w.would_run("browser"), w.would_run("android")), (set(&[]), set(&[])));
}

#[test]
fn integration_scripts_are_planned_in_the_integration_stage_run_through_nx_and_replayed_when_nothing_changed() {
    let w = Workspace::copy();
    // Given a service package whose test:integration only logs its run; the `cargo -p` it names is what links it to a crate
    let (name, root) = w.roots(&["@backend/vm-ops-api"]).remove(0);
    let manifest = w.path(&format!("{root}/package.json"));
    let mut m: Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    m["scripts"]["test:integration"] = json!(format!("echo '{name} test:integration' >> \"$RAN_LOG\" # cargo test --locked -p rca-core"));
    std::fs::write(&manifest, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    // And a copy that is a git repository, so a change is a commit to plan from
    let base = w.commit("The copy");
    let source = format!("{root}/rs/src/main.rs");
    let only = set(&[format!("{name} test:integration").as_str()]);

    w.with_edit(&source, || {
        // When a file of that package changes in a commit
        assert!(Command::new("git").args(["add", &source]).current_dir(w.dir.path()).status().unwrap().success());
        let sha = w.commit("Change the service");
        let r = w.wf(&["test", "plan", "--changed", "--base", &base, "--sha", &sha, "--json"], &[]);
        assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
        let plan = last_line(&r);
        // Then the plan lists one integration entry, in integration-tests (after the deploy), for that package's script
        let integration: Vec<(String, Value)> = plan["stages"]
            .as_object()
            .unwrap()
            .iter()
            .flat_map(|(stage, tests)| tests.as_array().unwrap().iter().filter(|t| t["kind"] == "integration").map(move |t| (stage.clone(), json!({ "package": t["package"], "script": t["script"], "why": t["why"] }))))
            .collect();
        assert_eq!(integration, vec![("integration-tests".to_string(), json!({ "package": name, "script": "test:integration", "why": ["changed"] }))], "{plan}");
        // And the stage runs it
        let planned = w.aside.path().join("planned.json");
        let integration_only: Vec<&Value> = plan["stages"]["integration-tests"].as_array().unwrap().iter().filter(|t| t["kind"] == "integration").collect();
        std::fs::write(&planned, json!({ "stages": { "integration-tests": integration_only } }).to_string()).unwrap();
        let (r, did) = w.run_stage_at("integration-tests", &planned, &sha, &[]);
        assert_eq!((r.exit_code, did), (0, only.clone()), "{}\n{}", r.stdout, r.stderr);
        assert_eq!(last_line(&r)["kinds"].as_array().unwrap().iter().map(|k| (k["kind"].clone(), k["target"].clone())).collect::<Vec<_>>(), vec![(json!("integration"), json!("test:integration"))], "{}", r.stdout);

        // When the stage runs again with nothing changed, Then Nx replays it: the log shows no second run
        let (r, did) = w.run_stage_at("integration-tests", &planned, &sha, &[]);
        assert_eq!((r.exit_code, did), (0, ran(&[])), "{}\n{}", r.stdout, r.stderr);
        // When the crate its script names changes, Then it runs again: the plugin links every script's crates
        w.with_edit("libraries/rca-core/src/file_store.rs", || assert_eq!(w.run_stage_at("integration-tests", &planned, &sha, &[]).1, only.clone()));
        // And once the crate is back, it is replayed again
        assert_eq!(w.run_stage_at("integration-tests", &planned, &sha, &[]).1, ran(&[]));
    });
}

#[test]
fn a_crate_with_an_e2e_binary_is_planned_in_the_cli_e2e_stage_and_its_flow_binary_stays_in_flow() {
    let w = Workspace::copy();
    // Given the copy's files tracked (crates are found through the manifests git lists) and a commit that changes a crate holding
    // both tests/flow.rs and tests/e2e.rs
    w.commit("The copy");
    assert!(Command::new("git").args(["add", "-A"]).current_dir(w.dir.path()).status().unwrap().success());
    let base = w.commit("The copy, tracked");
    let source = "clis/wf-bench/src/bench.rs";
    w.with_edit(source, || {
        assert!(Command::new("git").args(["add", source]).current_dir(w.dir.path()).status().unwrap().success());
        let sha = w.commit("Change the bench");
        // When the changed tests are planned
        let r = w.wf(&["test", "plan", "--changed", "--base", &base, "--sha", &sha, "--json"], &[]);
        assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
        let plan = last_line(&r);
        // Then its e2e test is a cli-e2e-tests entry and its flow test a flow-tests entry, both by crate
        let of = |stage: &str, kind: &str| plan["stages"][stage].as_array().unwrap().iter().filter(|t| t["kind"] == kind && t["crate"] == "wf-bench").count();
        assert_eq!((of("cli-e2e-tests", "e2e"), of("flow-tests", "flow"), of("flow-tests", "e2e")), (1, 1, 0), "{plan}");
    });
}

fn last_line(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.trim().lines().last().unwrap()).unwrap_or_else(|_| panic!("{}", r.stdout))
}

#[test]
fn nx_reruns_a_package_or_crate_task_exactly_when_its_sources_or_a_dependency_s_sources_changed() {
    let w = Workspace::copy();
    let (packages, flows) = (package_plan(&w), flow_plan(&w));
    let pkg = |names: &[&str], scripts: &[&str]| -> BTreeSet<String> { names.iter().flat_map(|n| scripts.iter().map(move |s| format!("{n} {s}"))).collect() };
    let cargo = |names: &[&str]| -> BTreeSet<String> { names.iter().map(|n| format!("cargo {n}")).collect() };
    let flow = |services: &[&str], crates: &[&str]| -> BTreeSet<String> { pkg(services, &["test:flow"]).union(&cargo(crates)).cloned().collect() };

    // Given every task passed once
    let (r, did) = w.run_stage("typecheck-lint-unit-tests", &packages, &[]);
    assert_eq!((r.exit_code, did), (0, pkg(&HARNESS, &["typecheck", "lint", "test:unit"])), "{}", r.stderr);
    let (r, did) = w.run_stage("flow-tests", &flows, &[]);
    assert_eq!((r.exit_code, did), (0, flow(&SERVICES, &CRATES)), "{}", r.stderr);
    // Then nothing changed means nothing runs
    assert_eq!(w.run_stage("typecheck-lint-unit-tests", &packages, &[]).1, ran(&[]));
    assert_eq!(w.run_stage("flow-tests", &flows, &[]).1, ran(&[]));

    let typecheck_cases: Vec<(&str, &str, BTreeSet<String>)> = vec![
        ("a library everything depends on", "tooling/browser-harness/contracts/src/index.ts", pkg(&HARNESS, &["typecheck", "test:unit"]).union(&pkg(&["@cli/harness-contracts"], &["lint"])).cloned().collect()),
        ("a package nothing depends on", "tooling/browser-harness/ui/src/App.tsx", pkg(&["@cli/harness-ui"], &["typecheck", "lint", "test:unit"])),
        ("a library's own test", "tooling/browser-harness/contracts/src/added.test.ts", pkg(&["@cli/harness-contracts"], &["typecheck", "lint", "test:unit"])),
        ("an unrelated library", "frontend/libraries/gesture-geometry/src/index.ts", ran(&[])),
        ("a doc", "docs/plans/nx-cache/plan.md", ran(&[])),
    ];
    for (what, file, expected) in typecheck_cases {
        w.with_edit(file, || assert_eq!(w.run_stage("typecheck-lint-unit-tests", &packages, &[]).1, expected, "{what}: {file}"));
    }
    let flow_cases: Vec<(&str, &str, BTreeSet<String>)> = vec![
        ("the crate every service takes from the registry: no service flow test reruns", "backend/service-kit/src/heartbeat.rs", flow(&[], &[])),
        ("a crate the CLIs build, and the agent service reaches through its own dependencies", "clis/http/src/lib.rs", flow(&["@app/remote-chat-agent"], &["cli-http", "cdp", "harness-flow", "knowledge", "wf", "offload", "wf-bench", "wf-bucket", "wf-harness", "wf-login", "pipeline", "secrets", "deploy", "wf-vmops"])),
        ("a library a CLI and a service take from the registry", "libraries/rca-core/src/file_store.rs", flow(&[], &["rca-core", "rca-ladder"])),
        ("one crate's flow test", "clis/cdp/tests/flow.rs", flow(&[], &["cdp"])),
        ("a wf crate with a flow test of its own", "clis/pipeline/src/ci_comment.rs", flow(&[], &["pipeline", "deploy", "wf"])),
        ("the bucket crate the publisher and the planner build on","clis/wf-bucket/src/cache_runs.rs", flow(&["@app/remote-chat-agent"], &["harness-flow", "wf-bucket", "wf-bench", "wf-harness", "pipeline", "deploy", "wf"])),
        ("the VM crate every caller of vm-ops-api builds", "clis/wf-vmops/src/vm_exec.rs", flow(&["@app/remote-chat-agent"], &["harness-flow", "wf-vmops", "wf-bucket", "offload", "wf-bench", "wf-harness", "wf-login", "pipeline", "deploy", "wf"])),
        ("the planner crate", "clis/pipeline/src/legs.rs", flow(&[], &["pipeline", "deploy", "wf"])),
        ("the crate of `wf ship` and `wf check --deployed`", "clis/deploy/src/ship_eas.rs", flow(&[], &["deploy", "wf"])),
        ("the crate of the `wf vm` verbs", "clis/deploy/src/vm_ops.rs", flow(&[], &["deploy", "wf"])),
        ("the crate of `wf secret login` and the desktop's listener", "clis/wf-login/src/vnc/engine.rs", flow(&[], &["wf-login", "wf"])),
        ("the crate of `wf offload ask` and the worker", "clis/offload/src/dispatch.rs", flow(&[], &["offload", "wf-bench", "wf"])),
        ("the device harness crate", "clis/harness-core/src/device/lua.rs", flow(&[], &["harness-flow", "wf-harness", "wf-bench", "pipeline", "deploy", "wf"])),
        ("the crate of `wf bench`", "clis/wf-bench/src/bench.rs", flow(&[], &["wf-bench", "wf"])),
        ("the secrets crate every wf crate that reads a value builds", "clis/secrets/src/secrets/util.rs", flow(&["@app/remote-chat-agent"], &["harness-flow", "secrets", "offload", "wf-bench", "wf-bucket", "wf-harness", "wf-login", "pipeline", "deploy", "wf-vmops", "wf"])),
        ("the kit every wf crate builds, and rca-exec, whose binary the agent service's flow tests run", "clis/wf-kit/src/args.rs", flow(&["@app/remote-chat-agent"], &["harness-flow", "knowledge", "agent-config-sync", "secrets", "offload", "wf-bench", "wf-bucket", "wf-harness", "wf-login", "pipeline", "deploy", "wf-vmops", "wf"])),
        ("a double the flow tests share (in src, so its users rerun)", "clis/wf-testkit/src/stubs.rs", flow(&["@app/remote-chat-agent"], &["harness-flow", "knowledge", "secrets", "offload", "wf-bench", "wf-bucket", "wf-harness", "wf-login", "pipeline", "deploy", "wf-vmops", "wf"])),
        ("the flow test of a crate inside a service", "backend/leetcode-backend/rs/tests/flow.rs", flow(&["@app/leetcode-backend"], &[])),
        ("the flow test of the crate a package runs", "backend/watchdog/rs/tests/flow.rs", flow(&["@infra/watchdog"], &[])),
        ("the CLIs' lockfile, which an edit that does not parse makes the whole file (the agent service's flow tests run rca-exec from it)", "clis/Cargo.lock", flow(&["@app/remote-chat-agent"], &CRATES.iter().copied().filter(|c| *c != "rca-core" && *c != "rca-ladder").collect::<Vec<_>>())),
        ("the libraries' lockfile", "libraries/Cargo.lock", flow(&[], &["rca-core", "rca-ladder"])),
        ("the lockfile of the service-kit workspace: no service builds from it", "backend/Cargo.lock", flow(&[], &[])),
        ("a TypeScript library", "tooling/browser-harness/contracts/src/index.ts", ran(&[])),
        ("an Nx plugin (pipeline's flow tests hold this contract, its dependants' hashes do not)", "tooling/nx/e2e-tests.js", flow(&[], &["pipeline"])),
        ("a doc", "docs/plans/nx-cache/plan.md", ran(&[])),
    ];
    let mut wrong = Vec::new();
    for (what, file, expected) in flow_cases {
        w.with_edit(file, || {
            let got = w.run_stage("flow-tests", &flows, &[]).1;
            if got != expected {
                wrong.push(format!("{what}: {file}\n  got      {got:?}\n  expected {expected:?}"));
            }
        });
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // When only a comment of the CLIs' lockfile changes, Then no crate reruns: each hashes its own slice, none the whole file
    let header = std::fs::read_to_string(w.path("clis/Cargo.lock")).unwrap().lines().next().unwrap().to_string();
    w.with_replace("clis/Cargo.lock", &header, &format!("{header} (a comment)"), || assert_eq!(w.run_stage("flow-tests", &flows, &[]).1, flow(&[], &[]), "a comment in clis/Cargo.lock"));
    // And once each file is back, everything is replayed again
    assert_eq!(w.run_stage("typecheck-lint-unit-tests", &packages, &[]).1, ran(&[]));
    assert_eq!(w.run_stage("flow-tests", &flows, &[]).1, ran(&[]));
}

#[test]
fn a_change_to_only_a_service_s_container_files_or_docs_reruns_none_of_its_typecheck_lint_or_unit_tests() {
    let w = Workspace::copy();
    let tasks = ["typecheck", "lint", "unit"];
    let plan = w.plan("typecheck-lint-unit-tests", ["@app/remote-chat-agent", "@app/leetcode-backend"].iter().flat_map(|p| tasks.iter().map(move |k| json!({ "kind": k, "package": p }))).collect());
    let all = |name: &str, scripts: &[&str]| -> BTreeSet<String> { scripts.iter().map(|s| format!("{name} {s}")).collect() };
    let agent = all("@app/remote-chat-agent", &["typecheck", "lint", "test:unit"]);

    // Given every task passed once
    let (r, did) = w.run_stage("typecheck-lint-unit-tests", &plan, &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(did, agent.union(&all("@app/leetcode-backend", &["typecheck", "lint", "test:unit"])).cloned().collect::<BTreeSet<_>>());
    // When only a Dockerfile, a compose file or a doc of the package changes, Then none of them runs again
    let replayed = [
        ("the agent's Dockerfile", "backend/remote-chat-agent/Dockerfile"),
        ("the opencode image's Dockerfile", "backend/remote-chat-agent/opencode-server/Dockerfile"),
        ("the agent's compose file", "backend/remote-chat-agent/docker-compose.prod.yml"),
        ("a doc beside the sources", "backend/remote-chat-agent/core/NOTES.md"),
        ("the backend's Dockerfile", "backend/leetcode-backend/Dockerfile"),
        ("the backend's compose file", "backend/leetcode-backend/docker-compose.yml"),
    ];
    for (what, file) in replayed {
        w.with_edit(file, || assert_eq!(w.run_stage("typecheck-lint-unit-tests", &plan, &[]).1, ran(&[]), "{what}: {file}"));
    }
    // And when a source or a test of the package changes, Then its tasks run again
    w.with_edit("backend/remote-chat-agent/tests/e2e/added.ts", || assert_eq!(w.run_stage("typecheck-lint-unit-tests", &plan, &[]).1, agent, "a TypeScript test"));
    w.with_edit("backend/remote-chat-agent/core/src/lib.rs", || assert_eq!(w.run_stage("typecheck-lint-unit-tests", &plan, &[]).1, set(&["@app/remote-chat-agent typecheck", "@app/remote-chat-agent test:unit"]), "a source of a crate the package runs: its dependency, not the package's own file"));
}

/// The crates with flow tests that build `on`, directly or through other workspace crates, read from `cargo metadata` rather than listed by hand.
fn workspace_packages() -> Vec<Value> {
    let out = Command::new("cargo").args(["metadata", "--format-version", "1", "--no-deps", "--offline", "--manifest-path"]).arg(repo_root().join("clis/Cargo.toml")).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let meta: Value = serde_json::from_slice(&out.stdout).unwrap();
    meta["packages"].as_array().unwrap().clone()
}

fn build_closure(packages: &[Value], on: &str) -> BTreeSet<String> {
    let mut built: BTreeSet<String> = BTreeSet::from([on.to_string()]);
    while let Some(next) = packages.iter().find(|p| !built.contains(p["name"].as_str().unwrap()) && p["dependencies"].as_array().unwrap().iter().any(|d| built.contains(d["name"].as_str().unwrap()))) {
        built.insert(next["name"].as_str().unwrap().to_string());
    }
    built
}

fn crates_that_build(on: &str) -> Vec<String> {
    let packages = workspace_packages();
    let built = build_closure(&packages, on);
    packages.iter().filter(|p| p["name"] != on && built.contains(p["name"].as_str().unwrap()) && Path::new(p["manifest_path"].as_str().unwrap()).with_file_name("tests").join("flow.rs").exists()).map(|p| p["name"].as_str().unwrap().to_string()).collect()
}

/// The crates whose flow tests run a binary built from `on` (`[package.metadata.flow] uses`) without depending on it, as the plan words them.
fn flow_runners_of(on: &str) -> BTreeSet<String> {
    let packages = workspace_packages();
    let built = build_closure(&packages, on);
    packages
        .iter()
        .filter(|p| !built.contains(p["name"].as_str().unwrap()))
        .filter_map(|p| {
            let uses = p["metadata"]["flow"]["uses"].as_array()?;
            let used = uses.iter().filter_map(Value::as_str).find(|u| built.contains(*u))?;
            Some(format!("{} its flow tests run {used}", p["name"].as_str()?))
        })
        .collect()
}

#[test]
fn a_change_in_a_wf_crate_plans_the_flow_tests_of_exactly_the_crates_that_build_it() {
    // Given this repository's files as a git checkout
    let w = Workspace::copy();
    let git = |args: &[&str]| Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(w.dir.path()).output().unwrap();
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    assert!(git(&["commit", "-q", "-m", "The copy"]).status.success());
    // And the crate flow tests a commit that changes `file` would plan, by crate and reason
    let plan_of = |file: &str| -> Value {
        let head = || String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout).trim().to_string();
        let (base, mut planned) = (head(), Value::Null);
        w.with_edit(file, || {
            assert!(git(&["add", file]).status.success());
            let sha = w.commit("Change one file");
            let r = w.wf(&["test", "plan", "--changed", "--base", &base, "--sha", &sha, "--json"], &[]);
            assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
            planned = last_line(&r);
        });
        planned
    };
    let planned = |file: &str| -> BTreeSet<String> { plan_of(file)["stages"]["flow-tests"].as_array().unwrap().iter().filter_map(|t| Some(format!("{} {}", t["crate"].as_str()?, t["why"][0].as_str()?))).collect() };
    let dependants = |on: &str, names: &[&str]| -> BTreeSet<String> { names.iter().map(|n| format!("{n} depends on {on} (cargo)")).collect() };
    let with = |mut a: BTreeSet<String>, b: &[&str]| -> BTreeSet<String> {
        a.extend(b.iter().map(|s| s.to_string()));
        a
    };

    // When a file of the kit, the secrets crate or the shared doubles changes, Then every crate with tests is planned, which is every
    // one that builds it, the planner and the bucket through the crates they depend on
    let (builds_kit, builds_testkit) = (crates_that_build("wf-kit"), crates_that_build("wf-testkit"));
    assert_eq!(planned("clis/wf-kit/src/args.rs"), with(dependants("wf-kit", &builds_kit.iter().map(String::as_str).collect::<Vec<_>>()), &flow_runners_of("wf-kit").iter().map(String::as_str).collect::<Vec<_>>()));
    // The crates that build the shared doubles, and the ones whose flow tests run a binary built from them: which dependency the plan names as the reason depends on the graph's order, so the crates are compared
    let crate_of = |plan: BTreeSet<String>| -> BTreeSet<String> { plan.into_iter().filter_map(|entry| entry.split(' ').next().map(str::to_string)).collect() };
    let expected: BTreeSet<String> = builds_testkit.iter().cloned().chain(flow_runners_of("wf-testkit").into_iter().filter_map(|entry| entry.split(' ').next().map(str::to_string))).collect();
    assert_eq!(crate_of(planned("clis/wf-testkit/src/stubs.rs")), expected);
    // The crates the plan names for the secrets, VM and bucket crates: compared by crate, as which dependency is named as the reason depends on the graph's order
    let by_crate = |file: &str, names: &[&str]| -> Option<String> {
        let (got, want) = (crate_of(planned(file)), names.iter().map(|n| n.to_string()).collect::<BTreeSet<_>>());
        (got != want).then(|| format!("{file}\n  got      {got:?}\n  expected {want:?}"))
    };
    let wrong: Vec<String> = [
        by_crate("clis/secrets/src/secrets/util.rs", &["secrets", "offload", "wf-bucket", "wf-lib", "wf-login", "deploy", "wf-vmops", "wf", "pipeline", "wf-harness", "harness-flow", "wf-bench", "wf-mimic", "orca-map-mobile"]),
        by_crate("clis/wf-vmops/src/vm_exec.rs", &["wf-vmops", "offload", "wf-bench", "wf-harness", "wf-login", "pipeline", "deploy", "wf", "harness-flow", "wf-bucket", "wf-mimic", "orca-map-mobile"]),
        by_crate("clis/wf-bucket/src/cache_runs.rs", &["wf-bucket", "pipeline", "deploy", "wf", "wf-harness", "harness-flow", "wf-bench", "wf-mimic", "orca-map-mobile"]),
    ].into_iter().flatten().collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    // When a file of the VM crate changes, Then its tests are planned, and the planner's, the `wf vm` crate's and wf's, which build it
    // When a file of the bucket crate changes, Then the crates that build it are planned with it
    // When a file of the ci crate, the planner, the `wf vm` crate or the ask crate changes, Then its tests are planned, and wf's, which builds it
    assert_eq!(planned("clis/pipeline/src/ci_comment.rs"), with(dependants("pipeline", &["deploy", "wf"]), &["pipeline changed"]));
    assert_eq!(planned("clis/pipeline/src/legs.rs"), with(dependants("pipeline", &["deploy", "wf"]), &["pipeline changed"]));
    assert_eq!(planned("clis/deploy/src/ship_eas.rs"), with(dependants("deploy", &["wf"]), &["deploy changed"]));
    assert_eq!(planned("clis/deploy/src/vm_ops.rs"), with(dependants("deploy", &["wf"]), &["deploy changed"]));
    assert_eq!(planned("clis/wf-login/src/vnc/engine.rs"), with(dependants("wf-login", &["wf"]), &["wf-login changed"]));
    assert_eq!(planned("clis/offload/src/dispatch.rs"), with(dependants("offload", &["wf-bench", "wf"]), &["offload changed"]));
    assert_eq!(planned("clis/harness-core/src/device/lua.rs"), with(dependants("wf-harness", &["wf-bench"]), &["wf-harness depends on harness-core (cargo)", "harness-flow depends on harness-core (cargo)", "wf depends on harness-flow (cargo)", "pipeline depends on harness-core (cargo)", "deploy depends on pipeline (cargo)"]));
    assert_eq!(planned("clis/wf-bench/src/bench.rs"), with(dependants("wf-bench", &["wf"]), &["wf-bench changed"]));
    // When a file of wf itself changes, Then only wf's tests are planned
    assert_eq!(planned("clis/wf/src/main.rs"), set(&["wf changed"]));
    // When a file of a crate wf runs as a separate binary changes, Then only that crate's tests are planned: wf does not build it
    assert_eq!(planned("clis/t3-serve/src/verbs.rs"), set(&["t3-serve changed"]));
    assert_eq!(planned("clis/rca-exec/src/verbs.rs"), set(&["rca-exec changed"]));
    // When a file that shapes the Nx graph changes, Then the planner's tests, which hold the cache's contract, are planned, and the ship
    // crate's and wf's through their dependency: the Nx cache replays them, as the contract inputs are the planner's own
    assert_eq!(planned("tooling/nx/e2e-tests.js"), set(&["pipeline the Nx graph or its inputs changed (the cache's contract suite)", "deploy depends on pipeline (cargo)", "wf depends on pipeline (cargo)"]));
    // When a file of a crate the remote chat agent's flow tests build or run changes, outside the service's own package, Then the service's flow
    // suite is planned: the executor binary it runs, the library its group crate depends on, and the scripted CLI it builds in another workspace
    // (a file no case above edited: an edit the checkout already holds is no change)
    for file in ["clis/rca-exec/src/domain.rs", "libraries/agent-api/agent-api-agent/src/events.rs", "libraries/agent-api/exec-testkit/src/cli.rs"] {
        let packages: Vec<String> = plan_of(file)["stages"]["flow-tests"].as_array().unwrap().iter().filter_map(|t| t["package"].as_str().map(str::to_string)).collect();
        assert_eq!(packages, ["@app/remote-chat-agent"], "{file}");
    }
}

const OFFLOAD_GROUPS: [&str; 4] = ["ask", "parity", "parity_features", "parity_capabilities"];

#[test]
fn a_crate_with_e2e_groups_plans_one_entry_per_group_for_a_change_of_its_own_and_none_for_one_of_a_dependency() {
    // Given this repository's files as a git checkout
    let w = Workspace::copy();
    let git = |args: &[&str]| Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(w.dir.path()).output().unwrap();
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    assert!(git(&["commit", "-q", "-m", "The copy"]).status.success());
    // And the e2e entries of the offload crate a commit that changes `file` would plan, by group and Nx target
    let planned = |file: &str| -> BTreeSet<String> {
        let mut entries = BTreeSet::new();
        let base = String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout).trim().to_string();
        w.with_edit(file, || {
            assert!(git(&["add", file]).status.success());
            let sha = w.commit("Change one file");
            let r = w.wf(&["test", "plan", "--changed", "--base", &base, "--sha", &sha, "--json"], &[]);
            assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
            entries = last_line(&r)["stages"]["cli-e2e-tests"].as_array().unwrap().iter().filter(|t| t["crate"] == "offload").map(|t| format!("{} {}", t["group"].as_str().unwrap_or("-"), t["target"].as_str().unwrap_or("-"))).collect();
        });
        entries
    };

    // When a file of the crate changes, Then every group is an entry of its own, run as its own Nx task
    let every: BTreeSet<String> = OFFLOAD_GROUPS.iter().map(|g| format!("{g} cargo-e2e-{g}")).collect();
    assert_eq!(planned("clis/offload/src/worker_run.rs"), every);
    // When only a crate it builds on changes, Then no live-model group is planned
    assert_eq!(planned("clis/wf-kit/src/args.rs"), set(&[]));
}

#[test]
fn nx_reruns_an_e2e_group_exactly_when_its_test_file_or_what_every_group_reads_changed() {
    let w = Workspace::copy();
    // the double logs the crate and the test filter each task was asked for
    write_script(&w.bin.join("cargo"), "if [ \"$1\" = metadata ]; then echo 'the Nx graph must not need cargo' >&2; exit 1; fi\nwhile [ $# -gt 0 ]; do [ \"$1\" = -p ] && p=\"$2\"; last=\"$1\"; shift; done\necho \"cargo $p $last\" >> \"$RAN_LOG\"\n");
    let entries: Vec<Value> = OFFLOAD_GROUPS.iter().map(|g| json!({ "kind": "e2e", "crate": "offload", "group": g, "target": format!("cargo-e2e-{g}") })).collect();
    let plan = w.plan("cli-e2e-tests", entries);
    let groups = |names: &[&str]| -> BTreeSet<String> { names.iter().map(|g| format!("cargo offload {g}::")).collect() };
    let every = groups(&OFFLOAD_GROUPS);

    // Given every group passed once
    let (r, did) = w.run_stage("cli-e2e-tests", &plan, &[]);
    assert_eq!((r.exit_code, did), (0, every.clone()), "{}\n{}", r.stdout, r.stderr);
    // Then nothing changed means nothing runs
    assert_eq!(w.run_stage("cli-e2e-tests", &plan, &[]).1, ran(&[]));

    // When a file changes, Then exactly the groups that read it run again
    let cases: Vec<(&str, &str, BTreeSet<String>)> = vec![
        ("the test file of one group", "clis/offload/tests/e2e/parity_features.rs", groups(&["parity_features"])),
        ("the test file of another group", "clis/offload/tests/e2e/ask.rs", groups(&["ask"])),
        ("the helpers every group imports", "clis/offload/tests/e2e/parity.rs", every.clone()),
        ("the module list of the suite", "clis/offload/tests/e2e.rs", every.clone()),
        ("the support the suite shares with the flow tests", "clis/offload/tests/flow/support/mod.rs", every.clone()),
        ("the source the asks go through", "clis/offload/src/dispatch_run.rs", every.clone()),
        ("the rca client every ask calls", "clis/offload/src/rca_client.rs", every.clone()),
        ("the worker, which no ask runs", "clis/offload/src/worker_run.rs", ran(&[])),
        ("the batch mode, which no group drives", "clis/offload/src/dispatch_batch.rs", ran(&[])),
        ("the pricing sync", "clis/offload/src/pricing_sync.rs", ran(&[])),
        ("a flow test of the crate", "clis/offload/tests/flow/ask.rs", ran(&[])),
        ("the lockfile", "clis/Cargo.lock", every.clone()),
        ("a crate it builds on", "clis/wf-kit/src/args.rs", every.clone()),
        ("the read-only agent every group's checkout holds", ".opencode/agents/read-only.md", every.clone()),
        ("the opencode version the service pins", "backend/remote-chat-agent/opencode-server/Dockerfile", every.clone()),
        ("a doc", "docs/plans/nx-cache/plan.md", ran(&[])),
    ];
    for (what, file, expected) in cases {
        w.with_edit(file, || assert_eq!(w.run_stage("cli-e2e-tests", &plan, &[]).1, expected, "{what}: {file}"));
    }
    // And once each file is back, everything is replayed again
    assert_eq!(w.run_stage("cli-e2e-tests", &plan, &[]).1, ran(&[]));
}

#[test]
fn fresh_package_stage_runs_again_every_task_the_cache_would_replay() {
    let w = Workspace::copy();
    let packages = package_plan(&w);
    let expected: BTreeSet<String> = HARNESS.iter().flat_map(|n| ["typecheck", "lint", "test:unit"].map(|s| format!("{n} {s}"))).collect();
    let stage = |extra: &[&str]| {
        let _ = std::fs::remove_file(&w.log);
        let mut args = vec!["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", packages.to_str().unwrap()];
        args.extend_from_slice(extra);
        let r = w.wf(&args, &[]);
        let did: BTreeSet<String> = std::fs::read_to_string(&w.log).unwrap_or_default().lines().map(str::to_string).collect();
        (r, did)
    };

    // Given every task of the stage passed once, and a second run with nothing changed replays them all
    let (r, did) = stage(&[]);
    assert_eq!((r.exit_code, did), (0, expected.clone()), "{}", r.stderr);
    assert_eq!(stage(&[]).1, ran(&[]));

    // When the stage runs with --fresh, Then the log shows every task ran again and the summary says so
    let (r, did) = stage(&["--fresh"]);
    assert_eq!((r.exit_code, did), (0, expected), "{}\n{}", r.stdout, r.stderr);
    let summary = last_line(&r);
    assert_eq!((summary["ok"].clone(), summary["fresh"].clone()), (json!(true), json!(true)), "{}", r.stdout);
    assert!(summary["kinds"].as_array().unwrap().iter().all(|k| k["verdict"] == "pass"));
}

/// One place tasks are hashed: its own checkout, HOME, PATH, browsers directory and platform, and the tools a task's runtime
/// inputs call (`FX_NODE`, `FX_PNPM`, `FX_RUSTC`, `FX_PLATFORM` say what they print). The workspace is a small fixture with the
/// repository's own `nx.json` and plugins and the real Nx: one pure TypeScript package (every cached script), a package whose
/// script builds Rust, a crate with a flow test, and one browser and one Android e2e test.
struct Site {
    _dir: tempfile::TempDir,
    ws: PathBuf,
    bin: PathBuf,
    browsers: PathBuf,
    path_extra: &'static str,
}

pub(crate) fn put_file(ws: &Path, rel: &str, text: &str) {
    std::fs::create_dir_all(ws.join(rel).parent().unwrap()).unwrap();
    std::fs::write(ws.join(rel), text).unwrap();
}

impl Site {
    fn new(name: &str, path_extra: &'static str) -> Site {
        let dir = tempfile::Builder::new().prefix(&format!("wf-flow-hash-{name}-")).tempdir().unwrap();
        let (ws, bin, browsers) = (dir.path().join("checkout"), dir.path().join("bin"), dir.path().join("browsers"));
        for d in [&ws, &bin, &browsers.join("chromium-1200"), &dir.path().join("home")] {
            std::fs::create_dir_all(d).unwrap();
        }
        let repo = repo_root();
        std::fs::copy(repo.join("nx.json"), ws.join("nx.json")).unwrap();
        // the plugins and every script their inputs run
        for file in std::fs::read_dir(repo.join("tooling/nx")).unwrap().flatten() {
            put_file(&ws, &format!("tooling/nx/{}", file.file_name().to_string_lossy()), &std::fs::read_to_string(file.path()).unwrap());
        }
        std::os::unix::fs::symlink(repo.join("node_modules"), ws.join("node_modules")).unwrap();
        let scripts = |names: &[&str]| -> Value { names.iter().map(|n| (n.to_string(), json!("true"))).collect::<serde_json::Map<_, _>>().into() };
        put_file(&ws, "package.json", &json!({ "name": "fx-root", "private": true }).to_string());
        put_file(&ws, "pnpm-workspace.yaml", "packages:\n  - libraries/*\n  - clis/*\n  - frontend/*\n");
        put_file(&ws, "pnpm-lock.yaml", "lockfileVersion: '9.0'\n\nsettings:\n  autoInstallPeers: true\n  excludeLinksFromLockfile: false\n\nimporters:\n\n  .: {}\n\n  clis/native: {}\n\n  frontend/mla-tamagui: {}\n\n  libraries/pure: {}\n\npackages: {}\n\nsnapshots: {}\n");
        put_file(&ws, "mise.toml", "[tools]\nnode = \"24.13.1\"\npnpm = \"11.5.2\"\n");
        put_file(&ws, "libraries/pure/package.json", &json!({ "name": "@fx/pure", "scripts": scripts(&["lint", "typecheck", "test:unit", "test:flow"]) }).to_string());
        put_file(&ws, "libraries/pure/src/index.ts", "export const one = 1;\n");
        put_file(&ws, "clis/native/package.json", &json!({ "name": "@fx/native", "scripts": { "test:flow": "cargo test --locked -p fx-crate --test flow --manifest-path ../Cargo.toml" } }).to_string());
        put_file(&ws, "clis/Cargo.toml", "[workspace]\nmembers = [\"fx-crate\"]\nresolver = \"2\"\n");
        put_file(&ws, "clis/fx-crate/Cargo.toml", "[package]\nname = \"fx-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        put_file(&ws, "clis/fx-crate/src/lib.rs", "pub fn one() -> u8 {\n    1\n}\n");
        put_file(&ws, "clis/fx-crate/tests/flow.rs", "#[test]\nfn one() {}\n");
        put_file(&ws, "frontend/mla-tamagui/package.json", &json!({ "name": "mla-tamagui", "scripts": { "export:web": "true" } }).to_string());
        put_file(&ws, "frontend/mla-tamagui/app/index.tsx", "export default 1;\n");
        put_file(&ws, "e2e/web.lua", "--[[---\nplatform: browser\nfeatures: *\n---]]\nprint(1)\n");
        put_file(&ws, "e2e/phone.lua", "--[[---\nplatform: android\nfeatures: *\n---]]\nprint(1)\n");
        put_file(&ws, "docs/note.md", "a note\n");
        // the tools a runtime input runs: node answers --version and the platform one-liner (the `platform` input) and otherwise is the real one
        write_script(&bin.join("pnpm"), "echo \"$FX_PNPM\"\n");
        write_script(&bin.join("rustc"), "echo \"rustc $FX_RUSTC (2d8144b78 2026-07-07)\"\n");
        write_script(&bin.join("node"), &format!("if [ \"$1\" = --version ]; then echo \"v$FX_NODE\"; elif [ \"$1\" = -p ] && [ \"${{2#*process.platform}}\" != \"$2\" ]; then echo \"$FX_PLATFORM\"; else exec {} \"$@\"; fi\n", node_path().expect("node on PATH").display()));
        Site { _dir: dir, ws, bin, browsers, path_extra }
    }

    /// `wf test drift <args>` here, with the tools at the given versions; the exit code and the JSON it printed.
    fn drift(&self, args: &[&str], tools: &[(&str, &str)]) -> (i32, Value) {
        let home = self.ws.parent().unwrap().join("home");
        let path = format!("{}:{}:{}", self.bin.display(), self.path_extra, std::env::var("PATH").unwrap_or_default());
        let mut opts = RunOpts::default().cwd(&self.ws).timeout(Duration::from_secs(180)).env("WF_TEST_REPO", &self.ws.display().to_string()).env("WF_NX_WHY", "0").env("HOME", &home.display().to_string()).env("PATH", &path).env("PLAYWRIGHT_BROWSERS_PATH", &self.browsers.display().to_string());
        for (k, v) in [("FX_NODE", "24.13.1"), ("FX_PNPM", "11.5.2"), ("FX_RUSTC", "1.97.0"), ("FX_PLATFORM", "linux-x64")] {
            opts = opts.env(k, v);
        }
        for (k, v) in tools {
            opts = opts.env(k, v);
        }
        let mut full = vec!["test", "drift"];
        full.extend_from_slice(args);
        let r = run_wf(&full, &opts);
        (r.exit_code, serde_json::from_str(r.stdout.trim().lines().last().unwrap_or_default()).unwrap_or_else(|_| panic!("{}\n{}", r.stdout, r.stderr)))
    }
}

/// The tasks a report names as drifted, each with the inputs that differ.
fn drifted(report: &Value) -> BTreeMap<String, BTreeSet<String>> {
    report["drifted"].as_array().unwrap().iter().map(|d| (d["task"].as_str().unwrap().to_string(), d["inputs"].as_array().unwrap().iter().map(|i| i.as_str().unwrap().to_string()).collect())).collect()
}

fn caused_by(tasks: &[&str], inputs: &[&str]) -> BTreeMap<String, BTreeSet<String>> {
    tasks.iter().map(|t| (t.to_string(), inputs.iter().map(|i| i.to_string()).collect())).collect()
}

#[test]
fn the_sandbox_and_the_runner_hash_alike() {
    const PURE: [&str; 4] = ["@fx/pure:lint", "@fx/pure:test:flow", "@fx/pure:test:unit", "@fx/pure:typecheck"];
    const NATIVE: [&str; 3] = ["@fx/native:test:flow", "crate-fx-crate:cargo-clippy", "crate-fx-crate:cargo-flow"];
    // Given the same commit checked out in the sandbox and on the CI runner, each with its own directory, HOME, PATH and browsers
    let (sandbox, runner) = (Site::new("sandbox", "/opt/sandbox/bin"), Site::new("runner", "/usr/local/runner/bin:/home/runner/.cargo/bin"));
    // And the sandbox hashed every cached task of the fixture
    let table = sandbox.ws.parent().unwrap().join("hashes.json");
    let (code, saved) = sandbox.drift(&["--save", table.to_str().unwrap()], &[]);
    assert_eq!((code, saved["tasks"].clone()), (0, json!(10)), "{saved}");
    // Then no input of a task names a path of the checkout: the hash would not mean the same elsewhere
    let text = std::fs::read_to_string(&table).unwrap();
    assert!(!text.contains(sandbox.ws.parent().unwrap().to_str().unwrap()), "an absolute path is hashed");
    let against = |site: &Site, tools: &[(&str, &str)]| site.drift(&["--against", table.to_str().unwrap()], tools);

    // When the runner differs in directory, HOME, PATH and the browsers' directory only, Then every task hashes alike
    let (code, report) = against(&runner, &[]);
    assert_eq!((code, report["ok"].clone(), report["same"].clone()), (0, json!(true), json!(10)), "{report}");
    // And a doc no project reads changes nothing
    put_file(&runner.ws, "docs/note.md", "another note\n");
    let (code, report) = against(&runner, &[]);
    assert_eq!((code, report["ok"].clone()), (0, json!(true)), "{report}");

    // When the runner is another platform, Then no task differs: the fixture has no crate in the platform tier, so the pure ones, the Rust ones and the browser test all replay across platforms (the tier's own key is covered in nx_crates)
    let (code, report) = against(&runner, &[("FX_PLATFORM", "linux-arm64")]);
    assert_eq!((code, report["ok"].clone(), report["same"].clone()), (0, json!(true), json!(10)), "{report}");

    // When Node and pnpm are other versions, Then the tasks run by pnpm differ, naming each, and the crate's does not
    let (code, report) = against(&runner, &[("FX_NODE", "24.14.0"), ("FX_PNPM", "11.6.0")]);
    let expected = caused_by(&[&PURE[..], &NATIVE[..1]].concat(), &["runtime:node --version", "runtime:pnpm --version"]);
    assert_eq!((code, drifted(&report)), (1, expected), "{report}");
    assert_eq!(report["causes"]["runtime:node --version"], json!({ "kind": "runtime", "mine": "v24.14.0", "theirs": "v24.13.1", "tasks": 5 }), "{report}");

    // When the Rust toolchain differs, Then the native tasks differ and the TypeScript ones do not
    let (code, report) = against(&runner, &[("FX_RUSTC", "1.98.0")]);
    assert_eq!((code, drifted(&report)), (1, caused_by(&NATIVE, &["runtime:rustc --version"])), "{report}");

    // When the runner's flow run left golden screenshots in the app, Then no task differs: they are outputs, not inputs
    put_file(&runner.ws, "frontend/mla-tamagui/.flow-goldens/light/snapshots/a/final.png", "pixels\n");
    let (code, report) = against(&runner, &[]);
    assert_eq!((code, report["ok"].clone(), report["same"].clone()), (0, json!(true), json!(10)), "{report}");
    std::fs::remove_dir_all(runner.ws.join("frontend/mla-tamagui/.flow-goldens")).unwrap();

    // When the export bakes another Better Stack token, Then only the export differs, on that variable
    let (code, report) = against(&runner, &[("EXPO_PUBLIC_BETTERSTACK_TOKEN", "another")]);
    assert_eq!((code, drifted(&report)), (1, caused_by(&["mla-tamagui:export:web"], &["env:EXPO_PUBLIC_BETTERSTACK_TOKEN"])), "{report}");

    // When the runner hashes fewer targets than the table holds, Then the report is red but --blocking is not: nothing both sides hash differs
    let (code, report) = runner.drift(&["--against", table.to_str().unwrap(), "--projects", "@fx/pure", "--targets", "test:unit"], &[]);
    assert_eq!((code, report["onlyThere"].as_array().map_or(0, Vec::len) > 0, report["blocking"].clone()), (1, true, json!([])), "{report}");
    let (code, _) = runner.drift(&["--against", table.to_str().unwrap(), "--projects", "@fx/pure", "--targets", "test:unit", "--blocking"], &[]);
    assert_eq!(code, 0);
    // And a Node of another version blocks, naming the input
    let (code, report) = runner.drift(&["--against", table.to_str().unwrap(), "--blocking"], &[("FX_NODE", "24.14.0")]);
    assert_eq!(code, 1, "{report}");
    assert!(report["blocking"].as_array().unwrap().iter().any(|i| i == "runtime:node --version"), "{report}");

    // When the runner has a newer Chromium, Then only the browser test differs
    std::fs::create_dir(runner.browsers.join("chromium-1201")).unwrap();
    let (code, report) = against(&runner, &[]);
    assert_eq!((code, drifted(&report)), (1, caused_by(&["e2e-web:e2e-browser"], &["runtime:node tooling/nx/chromium-build.js"])), "{report}");
    assert_eq!(report["causes"]["runtime:node tooling/nx/chromium-build.js"]["mine"], json!("chromium-1201"), "{report}");
    std::fs::remove_dir(runner.browsers.join("chromium-1201")).unwrap();

    // When a source file of the pure package differs, Then exactly that package's tasks differ, on its files
    put_file(&runner.ws, "libraries/pure/src/index.ts", "export const one = 2;\n");
    let (code, report) = against(&runner, &[]);
    assert_eq!((code, drifted(&report).keys().cloned().collect::<Vec<_>>()), (1, PURE.iter().map(|t| t.to_string()).collect::<Vec<_>>()), "{report}");
    assert!(report["causes"].as_object().unwrap().values().all(|c| c["kind"] == "files"), "{report}");

    // And the node pin check says which tool is not the one mise.toml pins
    let (code, report) = sandbox.drift(&["--pins"], &[("FX_NODE", "22.22.0")]);
    assert_eq!((code, report["pins"][0].clone()), (1, json!({ "tool": "node", "pinned": "24.13.1", "actual": "22.22.0", "ok": false })), "{report}");
    assert_eq!(sandbox.drift(&["--pins"], &[]).0, 0);
}

/// The first `name` on the test process's own PATH.
fn tool_path(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|dir| dir.join(name)).find(|p| p.is_file()).unwrap_or_else(|| panic!("{name} on PATH"))
}

/// A directory store that counts what reaches it.
struct Counting {
    inner: DirStore,
    puts: AtomicU64,
}

impl BlobStore for Counting {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        self.inner.get(key)
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        self.inner.put(key, body, if_absent)
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        self.inner.exists(key)
    }
}

impl Site {
    /// The fixture's pure package logs each task it runs to `$RAN_LOG`, and `pnpm` really runs its scripts (the hashes still
    /// see the versions `FX_PNPM` and `FX_NODE` give): what a replay must not do again.
    fn logging(name: &str, path_extra: &'static str) -> Site {
        let site = Site::new(name, path_extra);
        let scripts: serde_json::Map<String, Value> = ["lint", "typecheck", "test:unit", "test:flow"].iter().map(|s| (s.to_string(), json!(format!("echo '@fx/pure {s}' >> \"$RAN_LOG\"")))).collect();
        put_file(&site.ws, "libraries/pure/package.json", &json!({ "name": "@fx/pure", "scripts": scripts }).to_string());
        write_script(&site.bin.join("pnpm"), &format!("if [ \"$1\" = --version ]; then echo \"$FX_PNPM\"; else exec {} \"$@\"; fi\n", tool_path("pnpm").display()));
        site
    }

    /// `wf <args>` in this site, with `env` added to what the site runs in.
    fn wf(&self, args: &[&str], env: &[(&str, String)]) -> RunResult {
        let aside = self.ws.parent().unwrap();
        let path = format!("{}:{}:{}", self.bin.display(), self.path_extra, std::env::var("PATH").unwrap_or_default());
        let mut opts = RunOpts::default()
            .cwd(&self.ws)
            .timeout(Duration::from_secs(300))
            .env("WF_TEST_REPO", &self.ws.display().to_string())
            .env("HOME", &aside.join("home").display().to_string())
            .env("PATH", &path)
            .env("PLAYWRIGHT_BROWSERS_PATH", &self.browsers.display().to_string())
            .env("RAN_LOG", &aside.join("ran.log").display().to_string())
            .env("NX_DAEMON", "false").env("WF_NX_WHY", "0")
            .env("pnpm_config_verify_deps_before_run", "false");
        for (k, v) in [("FX_NODE", "22.22.0"), ("FX_PNPM", "11.5.2"), ("FX_RUSTC", "1.97.0"), ("FX_PLATFORM", "linux-x64")] {
            opts = opts.env(k, v);
        }
        for (k, v) in env {
            opts = opts.env(k, v);
        }
        run_wf(args, &opts)
    }

    /// `tests` of `stage` through `wf test run --stage`; what ran, from the log (a replay prints the same output, so the log
    /// says what ran).
    fn run_plan(&self, stage: &str, tests: Vec<Value>, env: &[(&str, String)]) -> (RunResult, BTreeSet<String>) {
        let aside = self.ws.parent().unwrap();
        let (log, plan, out) = (aside.join("ran.log"), aside.join("plan.json"), aside.join("out"));
        std::fs::write(&plan, json!({ "stages": { stage: tests } }).to_string()).unwrap();
        let _ = std::fs::remove_file(&log);
        let r = self.wf(&["test", "run", "--stage", stage, "--plan", plan.to_str().unwrap(), "--sha", "seeded", "--out", out.to_str().unwrap()], env);
        let ran = std::fs::read_to_string(&log).unwrap_or_default().lines().map(str::to_string).collect();
        (r, ran)
    }

    /// The typecheck, lint and unit tasks of the pure package.
    fn stage(&self, env: &[(&str, String)]) -> (RunResult, BTreeSet<String>) {
        let tests: Vec<Value> = ["typecheck", "lint", "unit"].iter().map(|kind| json!({ "kind": kind, "package": "@fx/pure" })).collect();
        self.run_plan("typecheck-lint-unit-tests", tests, env)
    }
}

/// The entries the shim left in a directory store.
fn nx_entries(store: &Path) -> usize {
    std::fs::read_dir(store.join("nx/g0")).into_iter().flatten().flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).count()
}

fn shim<S: BlobStore + 'static>(store: S, writes: Writes) -> (NxServer, Arc<Cache<S>>) {
    let cache = Arc::new(Cache::with_generation(store, Budget::committed().clone(), writes, Generation(0)));
    (NxServer::start(cache.clone()).expect("the shim listens"), cache)
}

/// A proxy address that nothing listens on: a call that is sent through it fails at once.
fn dead_proxy() -> String {
    format!("http://127.0.0.1:{}", std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port())
}

#[test]
fn a_sandbox_pass_replays_on_ci() {
    let aside = tempfile::Builder::new().prefix("wf-flow-blob-").tempdir().unwrap();
    let store = aside.path().join("store");
    let tasks: BTreeSet<String> = ["typecheck", "lint", "test:unit"].iter().map(|s| format!("@fx/pure {s}")).collect();
    let (sandbox, runner) = (Site::logging("sandbox", "/opt/sandbox/bin"), Site::logging("runner", "/usr/local/runner/bin:/home/runner/.cargo/bin"));
    let expect_run = |what: &str, (r, ran): (RunResult, BTreeSet<String>), ran_expected: &BTreeSet<String>| {
        assert_eq!((r.exit_code, &ran), (0, ran_expected), "{what}\n{}\n{}", r.stdout, r.stderr);
    };

    // Given the same commit in the sandbox and on the runner, each with its own checkout, HOME, PATH and no .nx/cache, and one store
    assert!(!sandbox.ws.join(".nx").exists() && !runner.ws.join(".nx").exists());
    // When the sandbox runs the stage with writes allowed, Then every task ran and the store holds one entry for each
    let (server, cache) = shim(DirStore::new(&store), Writes::Allowed);
    expect_run("sandbox", sandbox.stage(&server.env(None)), &tasks);
    assert_eq!((nx_entries(&store), server.served(), cache.stats().puts), (3, Served { gets: 3, puts: 3 }, 3));
    server.stop();

    // When the runner runs it at the same commit, Then all three replay from the store: nothing ran, nothing was written
    let (server, cache) = shim(DirStore::new(&store), Writes::Allowed);
    expect_run("runner", runner.stage(&server.env(None)), &BTreeSet::new());
    assert_eq!((nx_entries(&store), server.served(), cache.stats().hits), (3, Served { gets: 3, puts: 0 }, 3));
    server.stop();

    // When a third site replays with a proxy set that nothing listens on and the loopback in NO_PROXY, Then it still replays
    let proxied = Site::logging("proxied", "/usr/local/runner/bin");
    let proxy = dead_proxy();
    let (server, _) = shim(DirStore::new(&store), Writes::Allowed);
    let mut env = server.env(Some("example.invalid"));
    env.extend([("HTTP_PROXY", proxy.clone()), ("http_proxy", proxy.clone())]);
    expect_run("proxied", proxied.stage(&env), &BTreeSet::new());
    assert_eq!(server.served(), Served { gets: 3, puts: 0 });
    server.stop();

    // And without the loopback in NO_PROXY the same call never reaches the shim: Nx 23.2.1 then skips the tasks it could not ask
    // about and fails the run, so the replay above is what NO_PROXY bought and the shim must be up for as long as Nx runs
    let unprotected = Site::logging("unprotected", "/usr/local/runner/bin");
    let (server, _) = shim(DirStore::new(&store), Writes::Allowed);
    let mut env: Vec<(&str, String)> = server.env(None).into_iter().filter(|(k, _)| !k.eq_ignore_ascii_case("no_proxy")).collect();
    env.extend([("HTTP_PROXY", proxy.clone()), ("http_proxy", proxy), ("NO_PROXY", "example.invalid".into()), ("no_proxy", "example.invalid".into())]);
    let (r, ran) = unprotected.stage(&env);
    assert_eq!((r.exit_code, ran.len(), server.served()), (1, 0, Served::default()), "{}\n{}", r.stdout, r.stderr);
    server.stop();

    // When the store cannot be reached, Then the run ends green and cold: every task ran, and Nx was only ever told 404 and 200
    let broken = Site::logging("broken", "/usr/local/runner/bin");
    std::fs::write(aside.path().join("plain-file"), b"x").unwrap();
    let (server, cache) = shim(DirStore::new(aside.path().join("plain-file")), Writes::Allowed);
    expect_run("broken store", broken.stage(&server.env(None)), &tasks);
    assert_eq!((server.served(), cache.degraded()), (Served { gets: 3, puts: 3 }, Some(Degraded::Unreachable)));
    server.stop();

    // When the guard allows no writes, Then the tasks run, Nx sends all three entries and none reaches the store
    let read_only = Site::logging("read-only", "/usr/local/runner/bin");
    let counting = Arc::new(Counting { inner: DirStore::new(aside.path().join("empty")), puts: AtomicU64::new(0) });
    let (server, _) = shim(counting.clone(), Writes::ReadOnly("dirty: the work tree has changes".into()));
    expect_run("read-only", read_only.stage(&server.env(None)), &tasks);
    assert_eq!((server.served(), counting.puts.load(Ordering::SeqCst), nx_entries(&aside.path().join("empty"))), (Served { gets: 3, puts: 3 }, 0, 0));
    server.stop();
}

/// A crate of the fixture with a flow test and, if given, a path dependency.
fn put_crate(ws: &Path, name: &str, dep: Option<&str>) {
    let dep = dep.map(|d| format!("\n[dependencies]\n{d} = {{ path = \"../{d}\" }}\n")).unwrap_or_default();
    put_file(ws, &format!("clis/{name}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{dep}"));
    put_file(ws, &format!("clis/{name}/src/lib.rs"), "pub fn one() -> u8 {\n    1\n}\n");
    put_file(ws, &format!("clis/{name}/tests/flow.rs"), "#[test]\nfn one() {}\n");
}

#[test]
fn a_change_two_crates_down_reruns_the_dependant_and_leaves_an_unrelated_crate_replayed() {
    // Given crates A -> B -> C (A names only B) and an unrelated D, each with a flow test, in a git checkout
    let site = Site::logging("chain", "/opt/chain/bin");
    let ws = &site.ws;
    put_file(ws, "clis/Cargo.toml", "[workspace]\nmembers = [\"fx-a\", \"fx-b\", \"fx-c\", \"fx-d\"]\nresolver = \"2\"\n");
    for (name, dep) in [("fx-a", Some("fx-b")), ("fx-b", Some("fx-c")), ("fx-c", None), ("fx-d", None)] {
        put_crate(ws, name, dep);
    }
    put_file(ws, ".gitignore", "node_modules\n.nx\n");
    write_script(&site.bin.join("cargo"), "while [ $# -gt 0 ]; do [ \"$1\" = -p ] && echo \"cargo $2\" >> \"$RAN_LOG\"; shift; done\n");
    let git = |args: &[&str]| Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).current_dir(ws).output().unwrap();
    git(&["init", "-q"]);
    let commit = |message: &str| {
        git(&["add", "-A"]);
        let done = git(&["commit", "-q", "-m", message]);
        assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
        String::from_utf8_lossy(&git(&["rev-parse", "HEAD"]).stdout).trim().to_string()
    };
    let base = commit("Four crates");
    let all: Vec<Value> = ["fx-a", "fx-b", "fx-c", "fx-d"].iter().map(|c| json!({ "kind": "flow", "crate": c })).collect();
    let flows = |names: &[&str]| -> BTreeSet<String> { names.iter().map(|n| format!("cargo {n}")).collect() };
    // And every crate's flow test passed once, so a second run replays them all
    let (r, did) = site.run_plan("flow-tests", all.clone(), &[]);
    assert_eq!((r.exit_code, did), (0, flows(&["fx-a", "fx-b", "fx-c", "fx-d"])), "{}\n{}", r.stdout, r.stderr);
    assert_eq!(site.run_plan("flow-tests", all.clone(), &[]).1, ran(&[]));

    // When a source file of C changes
    put_file(ws, "clis/fx-c/src/lib.rs", "pub fn one() -> u8 {\n    2\n}\n");
    let sha = commit("Change C");
    // Then the plan lists A, B and C and not D
    let r = site.wf(&["test", "plan", "--changed", "--base", &base, "--sha", &sha, "--json"], &[]);
    assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
    let planned: BTreeSet<String> = last_line(&r)["stages"]["flow-tests"].as_array().unwrap().iter().map(|t| format!("{} {}", t["crate"].as_str().unwrap(), t["why"][0].as_str().unwrap())).collect();
    assert_eq!(planned, set(&["fx-a depends on fx-b (cargo)", "fx-b depends on fx-c (cargo)", "fx-c changed"]));
    // And the stage reruns A, B and C through the Nx cache and replays D
    let (r, did) = site.run_plan("flow-tests", all.clone(), &[]);
    assert_eq!((r.exit_code, did), (0, flows(&["fx-a", "fx-b", "fx-c"])), "{}\n{}", r.stdout, r.stderr);
    assert_eq!(site.run_plan("flow-tests", all, &[]).1, ran(&[]));
}

#[test]
fn the_chromium_a_browser_test_hashes_on_follows_the_pinned_playwright_not_the_installed_build() {
    // Given a checkout whose lockfile pins Playwright, and two machines that installed different Chromium builds
    let dir = tempfile::tempdir().unwrap();
    let lock = |version: &str| format!("lockfileVersion: '9.0'\n\npackages:\n\n  playwright-core@{version}:\n    resolution: {{integrity: sha512-x}}\n");
    let build = |lockfile: Option<&str>, installed: &str| {
        let (ws, browsers) = (dir.path().join(format!("ws-{installed}")), dir.path().join(format!("browsers-{installed}")));
        std::fs::create_dir_all(browsers.join(installed)).unwrap();
        std::fs::create_dir_all(&ws).unwrap();
        match lockfile {
            Some(text) => std::fs::write(ws.join("pnpm-lock.yaml"), text).unwrap(),
            None => std::fs::remove_file(ws.join("pnpm-lock.yaml")).unwrap_or(()),
        }
        let out = Command::new("node").arg(repo_root().join("tooling/nx/chromium-build.js")).current_dir(&ws).env("PLAYWRIGHT_BROWSERS_PATH", &browsers).env_remove("WF_CHROMIUM").output().unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };

    // When each machine names its Chromium, Then the two agree although the installed builds differ
    assert_eq!(build(Some(&lock("1.62.1")), "chromium-1234"), build(Some(&lock("1.62.1")), "chromium-1243"));

    // And a different pinned Playwright names a different Chromium
    assert_ne!(build(Some(&lock("1.62.1")), "chromium-1234"), build(Some(&lock("1.63.0")), "chromium-1234"));

    // And without a pinned Playwright the installed build is what counts
    assert_eq!(build(None, "chromium-1243"), "chromium-1243");
}

#[test]
fn a_task_reruns_when_a_file_its_test_reads_outside_the_project_changes_and_not_when_a_flow_run_leaves_goldens() {
    // Given the repository's real graph, and the hashes of the app's unit tests and export, the logging integration test and the browser tests
    let w = Workspace::copy();
    let (projects, targets) = ("mla-tamagui,@lib/logging,e2e-tamil-writing", "test:unit,test:integration,export:web,e2e-browser");
    let table = w.aside.path().join("hashes.json");
    let saved = w.wf(&["test", "drift", "--projects", projects, "--targets", targets, "--save", table.to_str().unwrap()], &[]);
    assert_eq!(saved.exit_code, 0, "{}{}", saved.stdout, saved.stderr);
    let drifted_by = |w: &Workspace| -> BTreeSet<String> {
        let r = w.wf(&["test", "drift", "--projects", projects, "--targets", targets, "--against", table.to_str().unwrap()], &[]);
        let report: Value = serde_json::from_str(r.stdout.trim().lines().last().unwrap()).unwrap_or_else(|_| panic!("{}{}", r.stdout, r.stderr));
        report["drifted"].as_array().unwrap().iter().map(|d| d["task"].as_str().unwrap().to_string()).collect()
    };
    assert_eq!(drifted_by(&w), set(&[]), "the same tree hashes alike");

    // When the doc the app's unit test reads changes, Then that test reruns and the export does not
    w.with_edit("docs/solutions/workflow/eas-publishing.md", || assert_eq!(drifted_by(&w), set(&["mla-tamagui:test:unit"])));
    // When the contract the logging integration test reads changes, Then that test reruns
    w.with_edit("clis/telemetry/contract/callsite.json", || assert_eq!(drifted_by(&w), set(&["@lib/logging:test:integration"])));
    // When a flow run left golden screenshots in the app, Then no task reruns
    w.with_edit("frontend/mla-tamagui/.flow-goldens/light/snapshots/a/final.png", || assert_eq!(drifted_by(&w), set(&[])));
    // When the app's own source changes, Then the export and the tests that load it rerun
    w.with_edit("frontend/mla-tamagui/app-shell/AppDrawerContent.tsx", || {
        let changed = drifted_by(&w);
        assert!(changed.contains("mla-tamagui:export:web") && changed.contains("mla-tamagui:test:unit"), "{changed:?}");
    });
}
