//! The CI plan as `wf ci plan tests` and `wf test run --stage` serve it: `plan` picks what a change reaches, `run --stage` runs a planned
//! stage through Nx, and `drift --pins` checks the pinned tools. `pnpm`, `nx` and `gh` are stubs (what Nx itself runs and replays is
//! `nx_cache.rs`); the repository's own `e2e` scripts, workflows and `mise.toml` are read as they are.
use crate::support::otlp::gauges;
use crate::support::{create_stub, last_json, MockServer, RunResult, SlotSandbox};
use serde_json::{json, Value};
use std::path::PathBuf;

/// A temp tree with a small pnpm workspace in a git repo of two commits, focused e2e tests and a fake harness.
pub(crate) struct Workspace {
    pub(crate) s: SlotSandbox,
    root: PathBuf,
}

/// `pnpm ls -r` lists the workspace under the cwd; `list --depth -1` names the packages `$STUB_AFFECTED` holds (what pnpm says a
/// change reaches; none by default), or prints `$STUB_LIST_FAIL_OUT` and exits 1 when STUB_LIST_FAIL is set; any other call is logged, `run lint` fails, and `run export:web` writes a
/// small export (a dynamic route and its nested page, a dynamic route with a page of its own for one id, the SPA fallback, a script, the flags it got) into the app's dist, or
/// fails when STUB_EXPORT_FAIL is set.
const PNPM: &str = r#"if [ "$1" = ls ]; then
  printf '[{"name":"ws","path":"%s"},{"name":"lib","path":"%s/libraries/lib"},{"name":"app","path":"%s/frontend/app"},{"name":"svc","path":"%s/backend/svc"}]' "$PWD" "$PWD" "$PWD" "$PWD"
  exit 0
fi
echo "$*" >> "$(dirname "$0")/pnpm.log"
case "$*" in *"list --depth -1"*) [ -n "$STUB_LIST_FAIL" ] && { printf '%s' "$STUB_LIST_FAIL_OUT"; exit 1; }; printf '%s' "${STUB_AFFECTED:-[]}"; exit 0;; esac
case "$*" in *"run lint"*) exit 1;; esac
case "$*" in *"run export:web"*)
  [ -n "$STUB_EXPORT_FAIL" ] && exit 1
  d=frontend/mla-tamagui/dist; rm -rf "$d"; mkdir -p "$d/leetcode/[id]" "$d/tamil" "$d/_expo/static/js/web"
  echo INDEX > "$d/index.html"; echo SPA > "$d/404.html"; echo LEETCODE > "$d/leetcode/[id].html"; echo DESCRIPTION > "$d/leetcode/[id]/description.html"
  echo TEMPLATE > "$d/tamil/[lessonId].html"; echo UYIR1 > "$d/tamil/uyir-1.html"
  echo 'console.log(1)' > "$d/_expo/static/js/web/entry.js"; echo "$EXPO_EXPORT_FLAGS" > "$d/flags.txt"; exit 0;;
esac
exit 0"#;

/// `nx run-many -t <target> -p <projects>`: logged; `lint` fails. An e2e target names every test as one to run when probed
/// (`WF_E2E_PROBE`), and otherwise leaves each test's summary where the stage reads it, `shaky` failed.
const NX: &str = r#"echo "$*" >> "$(dirname "$0")/nx.log"
target=$3; projects=$(echo "$5" | tr ',' ' ')
[ "$target" = typecheck ] && { mkdir -p "$HOME/.nx/ws/cache"; printf '{"tasks":[{"taskId":"app:typecheck","hash":"42","startTime":"2026-10-30T10:00:00.000Z","endTime":"2026-10-30T10:00:02.000Z","cacheStatus":"cache-miss","status":0}]}' > "$HOME/.nx/ws/cache/run.json"; }
case "$target" in lint) exit 1;; e2e-*) ;; *) exit 0;; esac
for p in $projects; do t=${p#e2e-}
  if [ -n "$WF_E2E_PROBE" ]; then echo "$t" >> "$WF_E2E_PROBE"; continue; fi
  mkdir -p "$WF_E2E_OUT/$t"; ok=true; [ "$t" = shaky ] && ok=false
  printf '{"ok":%s,"sha":"%s","units":[{"test":"%s","verdict":"pass"}]}' "$ok" "$WF_E2E_SHA" "$t" > "$WF_E2E_OUT/$t/summary.json"
done
[ -n "$WF_E2E_PROBE" ] && exit 1
case "$projects" in *e2e-shaky*) exit 1;; esac
exit 0"#;

impl Workspace {
    pub(crate) fn new(prefix: &str) -> Workspace {
        let s = SlotSandbox::new(prefix);
        s.stub("pnpm", PNPM);
        s.stub("nx", NX);
        let root = s.repo.parent().unwrap().join("ws");
        let w = Workspace { s, root };
        w.write("repo/package.json", r#"{"name":"ws","scripts":{"typecheck":"tsc"}}"#);
        w.write("repo/libraries/lib/package.json", r#"{"name":"lib","scripts":{"typecheck":"tsc","test:unit":"vitest"}}"#);
        w.write("repo/frontend/app/package.json", r#"{"name":"app","dependencies":{"lib":"workspace:*"},"scripts":{"test:flow":"vitest","typecheck":"tsc","lint":"eslint"}}"#);
        w.write("repo/backend/svc/package.json", r#"{"name":"svc","scripts":{"test:unit":"vitest","test:verify:deployed":"vitest"}}"#);
        for (name, header) in [
            ("typing", "platform: android\nagainst: update\npackages: frontend/app\ntimeout: 60\nunits: problem=two-sum; problem=valid-palindrome\nfocus: true\n"),
            ("svc", "platform: browser\nagainst: export\npackages: backend/svc\nfocus: true\n"),
            ("broken", "platform: browser\nagainst: export\npackages: backend/broken\nfocus: true\n"),
            ("slow", "platform: browser\nagainst: export\npackages: backend/slow\ntimeout: 1\nfocus: true\n"),
            ("idle", "platform: browser\npackages: frontend/app\n"),
            ("shaky", "platform: android\nagainst: update\npackages: frontend/shaky\nunits: problem=fail; problem=ok\nfocus: true\n"),
        ] {
            w.write(&format!("e2e/{name}.lua"), &format!("--[[---\n{header}---]]\nlog(1)\n"));
        }
        w.write("e2e/helper.lua", "-- a helper, no header\n");
        w.git(&["init", "-q"]);
        w.commit("one");
        w.write("repo/libraries/lib/index.ts", "export {}");
        w.commit("two");
        w
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    pub(crate) fn path(&self, sub: &str) -> String {
        self.root.join(sub).display().to_string()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = crate::support::git_command().args(args).current_dir(self.root.join("repo")).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", message]);
    }

    /// `wf <args>` on this workspace, with `extra` environment last.
    pub(crate) fn wf(&self, args: &[&str], extra: &[(&str, String)]) -> RunResult {
        let mut envs = vec![
            ("WF_TEST_E2E_DIR", self.path("e2e")),
            ("WF_TEST_REPO", self.path("repo")),
            // a commit with explicit paths runs the hook with the real repository's temporary index in the environment
            ("GIT_INDEX_FILE", self.path("repo/.git/index")),
            ("WF_NX_BIN", self.s.bin_dir.join("nx").display().to_string()),
        ];
        envs.extend(extra.iter().cloned());
        let refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.s.run(args, &refs)
    }

    /// `wf ci plan tests --changed --base <base> --json --github-output`: the plan (also saved as plan.json) and the GITHUB_OUTPUT text.
    pub(crate) fn plan(&self, base: &str) -> (Value, String) {
        let out = self.path("gh-output");
        let _ = std::fs::remove_file(&out);
        let r = self.wf(&["ci", "plan", "tests", "--changed", "--base", base, "--json", "--github-output"], &[("GITHUB_OUTPUT", out.clone())]);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", r.stdout, r.stderr));
        std::fs::write(self.path("plan.json"), v.to_string()).unwrap();
        (v, std::fs::read_to_string(out).unwrap_or_default())
    }
}

fn entries<'a>(v: &'a Value, stage: &str) -> Vec<(&'a str, &'a str)> {
    v["stages"][stage].as_array().unwrap().iter().map(|t| (t["kind"].as_str().unwrap(), t["package"].as_str().or(t["name"].as_str()).unwrap())).collect()
}

#[test]
fn ci_plans_the_stages_a_change_reaches_and_runs_each_planned_stage() {
    // Given a commit that changed only the library the app depends on
    let w = Workspace::new("wf-flow-plan-");

    // When it is planned against the commit before it
    let (v, gh) = w.plan("HEAD~1");
    // Then each stage lists the package tests pnpm's graph reaches, with the reason, and every focused e2e test, whatever changed
    // (focus alone picks them, owner 18 Oct)
    assert_eq!(entries(&v, "flow-tests"), [("flow", "app")]);
    assert_eq!(entries(&v, "typecheck-lint-unit-tests"), [("typecheck", "app"), ("typecheck", "lib"), ("lint", "app"), ("unit", "lib")]);
    let why = v["stages"]["flow-tests"][0]["why"][0].as_str().unwrap();
    assert!(why.contains("lib") && why.contains("pnpm"), "{why}");
    assert!(entries(&v, "deployment-verification-tests").is_empty(), "{v}");
    assert_eq!(entries(&v, "browser-e2e-tests"), [("e2e", "broken"), ("e2e", "slow"), ("e2e", "svc")]);
    assert_eq!(entries(&v, "android-e2e-tests"), [("e2e", "shaky"), ("e2e", "typing")]);
    assert_eq!(v["stages"]["android-e2e-tests"][1]["why"], json!(["focused"]));
    assert_eq!(v["stages"]["android-e2e-tests"][1]["units"], json!(["problem=two-sum", "problem=valid-palindrome"]));
    // and --github-output wrote the plan and each stage's count for the CI jobs
    assert!(gh.lines().any(|l| l.starts_with("plan={")), "{gh}");
    for line in ["flow_tests=1", "typecheck_lint_unit_tests=4", "deployment_verification_tests=0", "browser_e2e_tests=3", "android_e2e_tests=2"] {
        assert!(gh.lines().any(|l| l == line), "{line} missing: {gh}");
    }

    // When the typecheck, lint and unit stage runs from that plan, and lint fails
    let ledger = w.path("ledger.jsonl");
    let source = MockServer::start(None);
    let r = w.wf(&["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", &w.path("plan.json")], &[("WF_CI_METRICS_BASE", format!("http://127.0.0.1:{}", source.port)), ("WF_CI_LEDGER_FILE", ledger.clone()), ("GITHUB_RUN_ID", "777".to_string()), ("GITHUB_SHA", "abc1234".to_string()), ("GITHUB_JOB", "typecheck-lint-unit-tests".to_string())]);
    // Then each kind went to Nx once over its packages, the kinds after the red one still ran, and the stage failed
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    // and the log says which task Nx ran with which input hash, for the budget to judge
    assert!(r.stderr.contains("wf: nx ledger") && r.stderr.contains("app:typecheck") && r.stderr.contains("\"hash\":\"42\"") && r.stderr.contains("\"outcome\":\"ran\""), "{}", r.stderr);
    // and the stage's replay summary is one event and gauges: the tasks by outcome and the share a cache replayed (none here)
    let summary = r.stderr.lines().find(|l| l.contains("msg=\"nx.stage.summary\"")).unwrap_or_else(|| panic!("no summary event: {}", r.stderr));
    assert!(summary.contains("stage=typecheck-lint-unit-tests") && summary.contains("ran=1") && summary.contains("replayed=0"), "{summary}");
    let seen = gauges(&source.requests());
    let value = |metric: &str, name: &str| seen.iter().find(|g| g.metric == metric && g.scope == "typecheck-lint-unit-tests" && g.name == name).map(|g| g.value);
    assert_eq!((value("ci.nx.tasks", "ran"), value("ci.nx.tasks", "hit"), value("ci.nx.replay_ratio", "all")), (Some(1.0), Some(0.0), Some(0.0)), "{seen:?}");
    // and the same rows went to the ledger from the job itself, whatever becomes of the job's log
    let kept = std::fs::read_to_string(&ledger).unwrap_or_default();
    assert!(kept.contains("app:typecheck") && kept.contains("777") && kept.contains("typecheck-lint-unit-tests"), "{kept}");
    assert_eq!(w.s.calls("nx"), ["run-many -t typecheck -p app,lib --parallel=4 --outputStyle=stream", "run-many -t lint -p app --parallel=4 --outputStyle=stream", "run-many -t test:unit -p lib --parallel=4 --outputStyle=stream"]);
    w.s.forget("nx");
    let verdicts: Vec<(String, String)> = last_json(&r)["kinds"].as_array().unwrap().iter().map(|k| (k["kind"].as_str().unwrap().to_string(), k["verdict"].as_str().unwrap().to_string())).collect();
    assert_eq!(verdicts, [("typecheck", "pass"), ("lint", "fail"), ("unit", "pass")].map(|(a, b)| (a.to_string(), b.to_string())));

    // When the android e2e stage runs, and a stage the plan left empty
    let out = w.path("out");
    let android = w.wf(&["test", "run", "--stage", "android-e2e-tests", "--plan", &w.path("plan.json"), "--out", &out], &[]);
    let empty = w.wf(&["test", "run", "--stage", "deployment-verification-tests", "--plan", &w.path("plan.json")], &[]);
    // Then Nx was asked which android tests would run, ran them one at a time, and the stage merged their summaries (shaky
    // failed, so the stage did); the browser tests were not asked for, and the empty stage passes
    assert_eq!(android.exit_code, 1, "{}{}", android.stdout, android.stderr);
    assert_eq!(w.s.calls("nx"), ["run-many -t e2e-android -p e2e-shaky,e2e-typing --parallel=4 --outputStyle=stream", "run-many -t e2e-android -p e2e-shaky,e2e-typing --parallel=1 --outputStyle=stream"]);
    let summary = last_json(&android);
    assert_eq!((summary["ran"].clone(), summary["cached"].clone(), summary["units"].as_array().unwrap().len()), (json!(["shaky", "typing"]), json!([]), 2), "{}{}", android.stdout, android.stderr);
    assert_eq!((empty.exit_code, last_json(&empty)["kinds"].clone()), (0, json!([])), "{}{}", empty.stdout, empty.stderr);
    w.s.forget("nx");
    // When the stage is only probed (the job asks before it boots the device)
    let probe = w.wf(&["test", "run", "--stage", "android-e2e-tests", "--plan", &w.path("plan.json"), "--probe"], &[]);
    // Then it says what would run and runs nothing
    assert_eq!((probe.exit_code, last_json(&probe)["ran"].clone()), (0, json!(["shaky", "typing"])), "{}", probe.stdout);
    assert_eq!(w.s.calls("nx").len(), 1);
    // When the workspace has no nx installed, Then a stage fails at once, naming the install that brings it
    let none = w.wf(&["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", &w.path("plan.json")], &[("WF_NX_BIN", String::new())]);
    assert_eq!((none.exit_code, last_json(&none)["next"].clone()), (1, json!("pnpm install --frozen-lockfile")), "{}", none.stdout);

    // When a stage that does not exist is asked for
    let unknown = w.wf(&["test", "run", "--stage", "verify-deploy"], &[]);
    // Then it fails naming the stages it knows
    assert_eq!(unknown.exit_code, 2);
    assert!(unknown.stdout.contains("flow-tests | typecheck-lint-unit-tests"), "{}", unknown.stdout);

    // When the service and a root file change in the next commit
    w.write("repo/backend/svc/main.ts", "x");
    w.write("repo/README.md", "docs");
    w.commit("three");
    let (v, _) = w.plan("HEAD~1");
    // Then the service's deployment verification suite and unit tests come in, the root runs its typecheck, the app's flow tests are out,
    // and the e2e stages are the focused tests as before
    assert_eq!(entries(&v, "deployment-verification-tests"), [("deployment-verification", "svc")], "{v}");
    let checks = entries(&v, "typecheck-lint-unit-tests");
    assert!(checks.contains(&("unit", "svc")) && checks.iter().any(|(k, p)| *k == "typecheck" && (*p == "ws" || *p == "expo-hello-world-app-2-workspace")), "{checks:?}");
    assert!(entries(&v, "flow-tests").is_empty(), "{v}");
    assert_eq!(entries(&v, "browser-e2e-tests"), [("e2e", "broken"), ("e2e", "slow"), ("e2e", "svc")], "{v}");

    // When CI plans since the last green run: gh lists a passing run of a commit not in this history, then one of the first commit
    let first = w.git(&["rev-parse", "HEAD~2"]);
    w.s.stub("gh", &format!("echo 0123456789abcdef0123456789abcdef01234567\necho {first}"));
    let repo = [("GITHUB_REPOSITORY", "o/r".to_string())];
    let green = w.wf(&["ci", "plan", "tests", "--changed", "--base", "last-green", "--json"], &repo);
    w.s.stub("gh", "exit 1");
    let no_gh = w.wf(&["ci", "plan", "tests", "--changed", "--base", "last-green", "--json"], &repo);
    // Then the base is the newest green ancestor, and without gh it is the commit before
    let v: Value = serde_json::from_str(green.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", green.stdout, green.stderr));
    assert_eq!(v["base"], first.as_str());
    assert_eq!(entries(&v, "flow-tests"), [("flow", "app")]);
    let v: Value = serde_json::from_str(no_gh.stdout.trim()).unwrap();
    assert!(v["base"].as_str().unwrap().ends_with("~1"), "{}", v["base"]);

    // Given a cargo workspace no pnpm package wraps: cli depends on core by path, quiet on nothing, and docs has no flow binary
    w.write("repo/tools/Cargo.toml", "[workspace]\nmembers = [\"core\", \"cli\", \"quiet\", \"docs\"]\nresolver = \"2\"\n");
    for (name, deps) in [("core", ""), ("cli", "core = { path = \"../core\" }\n"), ("quiet", ""), ("docs", "core = { path = \"../core\" }\n")] {
        w.write(&format!("repo/tools/{name}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}"));
        w.write(&format!("repo/tools/{name}/src/lib.rs"), "");
        if name != "docs" {
            w.write(&format!("repo/tools/{name}/tests/flow.rs"), "");
        }
    }
    w.commit("four");
    w.write("repo/tools/core/src/lib.rs", "pub fn f() {}\n#[cfg(test)]\nmod tests {}\n");
    w.commit("five");
    // When a change to core is planned where there is no Rust toolchain (CI's plan job)
    w.s.stub("cargo", "echo 'the plan must not need cargo' >&2; exit 101");
    let (v, gh) = w.plan("HEAD~1");
    // Then the flow binaries of core and of the crate that depends on it are planned, each with its cargo command and reason
    let crates: Vec<(String, Value)> = v["stages"]["flow-tests"].as_array().unwrap().iter().map(|t| (t["crate"].as_str().unwrap_or_default().to_string(), t.clone())).collect();
    assert_eq!(crates.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["cli", "core"], "{v}");
    assert_eq!(crates[0].1["why"], json!(["depends on core (cargo)"]));
    assert_eq!((crates[1].1["kind"].as_str(), crates[1].1["dir"].as_str(), crates[1].1["why"].clone()), (Some("flow"), Some("tools/core"), json!(["changed"])));
    assert_eq!(crates[1].1["command"], "cargo test --locked -p core --test flow --manifest-path tools/Cargo.toml");
    assert!(gh.lines().any(|l| l == "flow_tests=2"), "{gh}");
    // and only core has inline unit tests, planned in the unit stage with its lib command
    let units: Vec<&Value> = v["stages"]["typecheck-lint-unit-tests"].as_array().unwrap().iter().filter(|t| t["kind"] == "unit").collect();
    assert_eq!(units.len(), 1, "{v}");
    // and clippy with -D warnings is planned for every crate the change reaches, whatever its tests
    let clippy: Vec<String> = v["stages"]["typecheck-lint-unit-tests"].as_array().unwrap().iter().filter(|t| t["kind"] == "clippy").map(|t| t["command"].as_str().unwrap_or_default().to_string()).collect();
    assert_eq!(clippy, ["cli", "core", "docs"].map(|c| format!("cargo clippy --locked -p {c} --all-targets --manifest-path tools/Cargo.toml -- -D warnings")), "{v}");
    assert_eq!((units[0]["kind"].clone(), units[0]["crate"].clone(), units[0]["command"].clone()), (json!("unit"), json!("core"), json!("cargo test --locked -p core --lib --manifest-path tools/Cargo.toml")));
    // When the flow stage runs from that plan
    w.s.forget("nx");
    let r = w.wf(&["test", "run", "--stage", "flow-tests", "--plan", &w.path("plan.json")], &[]);
    // Then the crates' flow tests went to Nx in the flow call, beside any package's, and the stage passes
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(w.s.calls("nx"), ["run-many -t test:flow,cargo-flow -p crate-cli,crate-core --parallel=4 --outputStyle=stream"]);
    assert_eq!(last_json(&r)["kinds"], json!([{ "kind": "flow", "target": "test:flow,cargo-flow", "projects": ["crate-cli", "crate-core"], "verdict": "pass", "durationS": last_json(&r)["kinds"][0]["durationS"] }]));

    // When the unit stage runs from that plan
    w.s.forget("nx");
    w.wf(&["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", &w.path("plan.json")], &[]);
    // Then the crate's unit tests went to Nx as cargo-unit, beside the packages' unit scripts
    assert!(w.s.calls("nx").iter().any(|c| c.starts_with("run-many -t test:unit,cargo-unit -p crate-core")), "{:?}", w.s.calls("nx"));
    // and the crates' clippy as cargo-clippy, in its own call
    assert!(w.s.calls("nx").iter().any(|c| c.starts_with("run-many -t cargo-clippy -p crate-cli,crate-core,crate-docs")), "{:?}", w.s.calls("nx"));

    // When it runs with the VM's two tasks at a time, and with a parallelism that is not a number
    w.s.forget("nx");
    let two = w.wf(&["test", "run", "--stage", "flow-tests", "--plan", &w.path("plan.json"), "--parallel", "2"], &[]);
    let bad = w.wf(&["test", "run", "--stage", "flow-tests", "--plan", &w.path("plan.json"), "--parallel", "many"], &[]);
    // Then Nx gets two at a time, and the bad value is refused before Nx runs
    assert_eq!(two.exit_code, 0, "{}{}", two.stdout, two.stderr);
    assert_eq!(w.s.calls("nx"), ["run-many -t test:flow,cargo-flow -p crate-cli,crate-core --parallel=2 --outputStyle=stream"]);
    assert!(bad.exit_code == 2 && bad.stdout.contains("--parallel wants a positive number"), "{}", bad.stdout);
}

#[test]
fn the_repos_android_tests_run_against_the_update_and_the_browser_ones_against_the_export() {
    // Given the repository's own e2e tests
    let s = SlotSandbox::new("wf-flow-headers-");
    let e2e = crate::support::repo_root().join("e2e").display().to_string();
    let r = s.run(&["ci", "plan", "tests", "--all", "--json"], &[("WF_TEST_E2E_DIR", e2e.as_str()), ("WF_TEST_REPO", s.repo.display().to_string().as_str())]);
    // When they are planned
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", r.stdout, r.stderr));
    let rows: Vec<(&str, &str, &str, bool)> = v["tests"].as_array().unwrap().iter().map(|t| (t["name"].as_str().unwrap(), t["platform"].as_str().unwrap(), t["against"].as_str().unwrap(), t["focus"] == true)).collect();
    // Then every focused Android test loads the update CI publishes, every browser test the export, and no test, focused or not,
    // is against the dev Metro (the debugger probe is a browser test now)
    let names = |platform: &str, against: &str, focus: bool| rows.iter().filter(|r| r.1 == platform && r.2 == against && r.3 == focus).map(|r| r.0).collect::<Vec<_>>();
    assert_eq!(names("android", "update", true), ["editor-typing", "local-run", "review-sheet-keyboard", "stubbed-run-local", "tamil-writing", "voice-edit"], "{rows:?}");
    assert_eq!(names("android", "update", false), ["stop-probe"], "{rows:?}");
    assert!(rows.iter().all(|r| r.2 != "metro"), "a test is still against the dev Metro: {rows:?}");
    assert!(rows.contains(&("debugger-probe", "browser", "export", false)) && !rows.iter().any(|r| r.0 == "debugger-probe" && r.1 == "android"), "{rows:?}");
    assert!(!rows.iter().any(|r| r.0 == "metro-smoke"), "the Metro smoke is retired: {rows:?}");
    assert!(rows.iter().filter(|r| r.1 == "browser").all(|r| r.2 == "export"), "{rows:?}");
    assert!(!rows.iter().any(|r| r.0 == "update-vs-metro"), "the benchmark is retired: {rows:?}");
}

#[test]
fn no_e2e_script_names_the_dev_metro_or_a_host_that_served_it() {
    // Given the repository's own e2e scripts
    let dir = crate::support::repo_root().join("e2e");
    // When every line of them is read for the Metro's host, its port and the tailnet address it once had
    let mut named = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "lua")) {
        for (n, line) in std::fs::read_to_string(entry.path()).unwrap().lines().enumerate() {
            if ["metro-runner", ":20000", "100.112."].iter().any(|needle| line.contains(needle)) {
                named.push(format!("{}:{}: {line}", entry.file_name().to_string_lossy(), n + 1));
            }
        }
    }
    // Then none does: a script that needs a link is handed one
    assert!(named.is_empty(), "{named:#?}");
}

#[test]
fn the_pins_check_names_each_workflow_that_sets_up_a_node_other_than_the_one_mise_toml_pins() {
    // Given the repository's own mise.toml, and node and pnpm tools that print exactly the versions it pins
    let s = SlotSandbox::new("wf-flow-pins-");
    let repo = crate::support::repo_root();
    let mise = std::fs::read_to_string(repo.join("mise.toml")).unwrap();
    let pin = |tool: &str| mise.lines().find_map(|l| l.strip_prefix(&format!("{tool} = \"")).and_then(|r| r.strip_suffix('"'))).unwrap_or_else(|| panic!("mise.toml pins no {tool}")).to_string();
    s.stub("node", &format!("echo v{}", pin("node")));
    s.stub("pnpm", &format!("echo {}", pin("pnpm")));

    // When the pins are checked in the repository, whose workflows and root package.json declare their own
    let r = s.run(&["test", "drift", "--pins"], &[("WF_TEST_REPO", repo.display().to_string().as_str())]);
    // Then the sandbox's tools and every declared version agree with mise.toml
    let report = last_json(&r);
    assert_eq!((r.exit_code, report["ok"].clone(), report["declared"]["off"].clone()), (0, json!(true), json!([])), "{report}");
    assert!(report["declared"]["checked"].as_u64().unwrap() > 10, "the workflows' setup-node steps and packageManager were not all read: {report}");

    // When a workflow of a checkout sets up the floating `22` while the tools are right
    s.file("mise.toml", format!("[tools]\nnode = \"{}\"\npnpm = \"{}\"\n", pin("node"), pin("pnpm")));
    s.file("package.json", format!("{{\n  \"packageManager\": \"pnpm@{}\"\n}}\n", pin("pnpm")));
    s.file(".github/workflows/ci.yml", format!("jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n          node-version: {}\n      - uses: actions/setup-node@v4\n        with:\n          node-version: 22 # floating\n", pin("node")));
    let r = s.run(&["test", "drift", "--pins"], &[("WF_TEST_REPO", s.repo.display().to_string().as_str())]);
    // Then the check fails naming the file and line, and what to do
    let report = last_json(&r);
    assert_eq!((r.exit_code, report["ok"].clone(), report["declared"]["checked"].clone()), (1, json!(false), json!(3)), "{report}");
    assert_eq!(report["declared"]["off"], json!([{ "file": ".github/workflows/ci.yml", "line": 9, "tool": "node", "version": "22", "pinned": pin("node") }]), "{report}");
    assert!(report["next"].as_str().unwrap().contains("mise.toml pins"), "{report}");

    // When a workflow sets up another node for a reference server and says why on the line, and another says nothing
    s.file(".github/workflows/ci.yml", format!("jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n          node-version: {}\n", pin("node")));
    s.file(".github/workflows/record.yml", "jobs:\n  a:\n    steps:\n      - uses: actions/setup-node@v4\n        with:\n          node-version: ${{ matrix.node }} # pins-exempt: the reference server's toolchain, no Nx task runs on it\n      - uses: actions/setup-node@v4\n        with:\n          node-version: 24 # pins-exempt:\n");
    let r = s.run(&["test", "drift", "--pins"], &[("WF_TEST_REPO", s.repo.display().to_string().as_str())]);
    // Then only the line without a reason is reported
    let report = last_json(&r);
    assert_eq!(report["declared"]["off"], json!([{ "file": ".github/workflows/record.yml", "line": 9, "tool": "node", "version": "24", "pinned": pin("node") }]), "{report}");
}

#[test]
fn no_workflow_or_action_sets_the_nx_cache_environment_wf_gives_nx() {
    // Given every workflow and composite action of the repository
    let github = crate::support::repo_root().join(".github");
    let mut files: Vec<PathBuf> = std::fs::read_dir(github.join("workflows")).unwrap().flatten().map(|e| e.path()).collect();
    files.extend(std::fs::read_dir(github.join("actions")).unwrap().flatten().map(|e| e.path().join("action.yml")));
    // When each is read for the variables that point Nx at a cache, skip its cache, or name the bucket's links
    let named: Vec<String> = files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok().map(|text| (f, text)))
        .flat_map(|(f, text)| {
            let name = f.strip_prefix(&github).unwrap().display().to_string();
            ["NX_SELF_HOSTED_REMOTE_CACHE", "NX_SKIP_NX_CACHE", "NX_CACHE_DIRECTORY", "BUILD_CACHE_READ_PAR_URL", "BUILD_CACHE_WRITE_PAR_URL"].into_iter().filter(move |v| text.contains(v)).map(move |v| format!("{name}: {v}"))
        })
        .collect();
    // Then none does: wf starts the shim, resolves the links from the vault and says whether a run may write
    assert!(named.is_empty(), "{named:#?}");
}

fn pipeline_job(text: &str, job: &str) -> String {
    let head = format!("\n  {job}:\n");
    let start = text.find(&head).unwrap_or_else(|| panic!("no job {job}")) + 1;
    let rest = &text[start + head.len() - 1..];
    let end = rest.lines().skip(1).scan(0, |at, l| { *at += l.len() + 1; Some((*at, l)) }).find(|(_, l)| l.starts_with("  ") && !l.starts_with("   ") && l.trim_end().ends_with(':') && !l.trim_start().starts_with('#')).map(|(at, l)| at - l.len() - 1);
    rest[..end.unwrap_or(rest.len())].to_string()
}

fn pipeline_line(block: &str, key: &str) -> String {
    block.lines().find(|l| l.starts_with(&format!("    {key}:"))).unwrap_or_else(|| panic!("no {key}")).to_string()
}

#[test]
fn a_red_test_stage_does_not_hide_the_e2e_jobs_but_still_blocks_the_deploy_but_not_the_production_update() {
    // Given the pipeline workflow
    let text = std::fs::read_to_string(crate::support::repo_root().join(".github/workflows/pipeline.yml")).unwrap();
    let tests = ["core-tests", "integration-tests", "cli-e2e-tests"];
    // When the jobs that ship are read
    for job in ["deploy", "clis"] {
        let block = pipeline_job(&text, job);
        let cond = pipeline_line(&block, "if");
        let needs = pipeline_line(&block, "needs");
        // Then they still wait on, and are skipped by, a red test, and neither parity job is a condition of shipping
        for t in tests {
            assert!(cond.contains(&format!("contains(fromJSON('[\"success\",\"skipped\"]'), needs.{t}.result)")), "{job} if: {cond}");
            assert!(needs.contains(t), "{job} needs: {needs}");
        }
        assert!(!cond.contains("parity") && !needs.contains("parity"), "{job}");
    }
    // When the jobs that report e2e evidence are read
    for job in ["eas-update", "browser-e2e-tests", "android-e2e-tests", "deployment-verification-tests"] {
        let cond = pipeline_line(&pipeline_job(&text, job), "if");
        // Then a red test stage no longer skips them
        for t in tests {
            assert!(!cond.contains(&format!("needs.{t}.result")), "{job} if: {cond}");
        }
    }
    // And the production EAS Update step, the one phones open, ships the head of dev whatever the tests say
    let eas = pipeline_job(&text, "eas-update");
    let step = &eas[eas.find("- name: Publish the EAS Update\n").expect("production step")..];
    let step_if = step.lines().find(|l| l.trim_start().starts_with("if:")).unwrap();
    assert!(step_if.contains("needs.plan.outputs.is_head == 'true'"), "{step_if}");
    for t in tests {
        assert!(!step_if.contains(&format!("needs.{t}.result")), "{step_if}");
    }
    // And the one job that dials a deployed service still needs a successful deploy unless no leg was planned
    let dvt = pipeline_line(&pipeline_job(&text, "deployment-verification-tests"), "if");
    assert!(dvt.contains("needs.deploy.result == 'success' || needs.plan.outputs.legs == '[]'"), "{dvt}");
}

#[test]
fn a_crate_s_flow_test_alone_is_planned_when_a_flow_input_outside_its_dir_or_a_crate_it_runs_changes() {
    // Given a crate whose flow tests read tables outside its workspace and run the binary of a crate it does not depend on
    let w = Workspace::new("wf-flow-plan-inputs-");
    let manifest = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[package.metadata.flow]\ninputs = [\"../../docs/tables/**\"]\nuses = [\"helper\"]\n";
    w.write("repo/tools/Cargo.toml", "[workspace]\nmembers = [\"app\", \"helper\"]\nresolver = \"2\"\n");
    w.write("repo/tools/app/Cargo.toml", manifest);
    w.write("repo/tools/app/tests/flow.rs", "\n");
    w.write("repo/tools/app/src/main.rs", "fn main() {}\n");
    w.write("repo/tools/helper/Cargo.toml", "[package]\nname = \"helper\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    w.write("repo/tools/helper/src/main.rs", "fn main() {}\n");
    w.write("repo/docs/tables/t3.json", "{}\n");
    w.write("repo/docs/other.md", "\n");
    w.commit("a crate with flow inputs");
    let input = manifest.parse::<toml::Table>().unwrap()["package"]["metadata"]["flow"]["inputs"][0].as_str().unwrap().replace("../../", "").replace("**", "t3.json");
    // the entries of crates (a stage also holds the workspace's own checks, which any file outside every package plans)
    let why_of = |v: &Value, stage: &str| -> Vec<(String, String)> { v["stages"][stage].as_array().unwrap().iter().filter_map(|t| Some((t["crate"].as_str()?.to_string(), t["why"][0].as_str().unwrap_or_default().to_string()))).collect() };

    // When a commit edits a file the flow inputs name
    w.write(&format!("repo/{input}"), "{\"rows\": []}\n");
    w.commit("edit the table");
    let (v, _) = w.plan("HEAD~1");
    // Then the crate's flow test is planned for that file, and nothing else of it
    assert_eq!(why_of(&v, "flow-tests"), [("app".to_string(), format!("a flow input changed: {input}"))], "{v}");
    assert_eq!(why_of(&v, "typecheck-lint-unit-tests"), [], "{v}");

    // When a commit edits a file beside them that they do not name, Then no flow test is planned
    w.write("repo/docs/other.md", "edited\n");
    w.commit("edit another doc");
    assert_eq!(why_of(&w.plan("HEAD~1").0, "flow-tests"), [], "a file outside the flow inputs planned a flow test");

    // When a commit edits the crate whose binary the flow tests run
    w.write("repo/tools/helper/src/main.rs", "fn main() {\n}\n");
    w.commit("edit the helper");
    let (v, _) = w.plan("HEAD~1");
    // Then the user's flow test is planned beside the helper's own checks
    assert!(why_of(&v, "flow-tests").contains(&("app".to_string(), "its flow tests run helper".to_string())), "{v}");
    assert_eq!(why_of(&v, "typecheck-lint-unit-tests").iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["helper"], "{v}");
}

#[test]
fn a_crate_s_integration_test_alone_is_planned_when_an_integration_input_or_a_crate_it_runs_changes_even_one_inside_a_package() {
    // Given a chain whose integration tests run the binary of a crate inside a pnpm package and name their rows in a table, and whose flow tests read neither
    let w = Workspace::new("wf-integration-plan-inputs-");
    let manifest = "[package]\nname = \"chain\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[package.metadata.integration]\ninputs = [\"../../docs/tables/**\"]\nuses = [\"svc-bin\"]\n";
    w.write("repo/tools/Cargo.toml", "[workspace]\nmembers = [\"chain\"]\nresolver = \"2\"\n");
    w.write("repo/tools/chain/Cargo.toml", manifest);
    for test in ["flow", "integration"] {
        w.write(&format!("repo/tools/chain/tests/{test}.rs"), "\n");
    }
    w.write("repo/tools/chain/src/main.rs", "fn main() {}\n");
    w.write("repo/backend/svc/Cargo.toml", "[workspace]\nmembers = [\"rs\"]\nresolver = \"2\"\n");
    w.write("repo/backend/svc/rs/Cargo.toml", "[package]\nname = \"svc-bin\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
    w.write("repo/backend/svc/rs/src/main.rs", "fn main() {}\n");
    w.write("repo/docs/tables/t3.json", "{}\n");
    w.write("repo/docs/other.md", "\n");
    w.commit("a chain with integration inputs");
    let why_of = |v: &Value, stage: &str| -> Vec<(String, String)> { v["stages"][stage].as_array().unwrap().iter().filter_map(|t| Some((t["crate"].as_str()?.to_string(), t["why"][0].as_str().unwrap_or_default().to_string()))).collect() };
    let input = manifest.parse::<toml::Table>().unwrap()["package"]["metadata"]["integration"]["inputs"][0].as_str().unwrap().replace("../../", "").replace("**", "t3.json");

    // When a commit edits the binary the chain runs, which lives inside a pnpm package
    w.write("repo/backend/svc/rs/src/main.rs", "fn main() {\n}\n");
    w.commit("edit the service");
    let (v, _) = w.plan("HEAD~1");
    // Then the chain's integration test is planned for it, and none of its flow, check or unit tests
    assert_eq!(why_of(&v, "integration-tests"), [("chain".to_string(), "its integration tests run svc-bin".to_string())], "{v}");
    assert_eq!(why_of(&v, "flow-tests"), [], "{v}");
    assert_eq!(why_of(&v, "typecheck-lint-unit-tests"), [], "{v}");

    // When a commit edits the table the integration tests name
    w.write(&format!("repo/{input}"), "{\"rows\": []}\n");
    w.commit("edit the table");
    let (v, _) = w.plan("HEAD~1");
    // Then the integration test is planned for that file, and the flow test still is not
    assert_eq!(why_of(&v, "integration-tests"), [("chain".to_string(), format!("an integration input changed: {input}"))], "{v}");
    assert_eq!(why_of(&v, "flow-tests"), [], "{v}");

    // When a commit edits a file beside them that they do not name, Then no integration test is planned
    w.write("repo/docs/other.md", "edited\n");
    w.commit("edit another doc");
    assert_eq!(why_of(&w.plan("HEAD~1").0, "integration-tests"), [], "a file outside the integration inputs planned an integration test");

    // When a commit edits the chain itself, Then its own checks, flow and integration tests are planned, each once
    w.write("repo/tools/chain/src/main.rs", "fn main() {\n}\n");
    w.commit("edit the chain");
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(why_of(&v, "integration-tests").len(), 1, "{v}");
    assert_eq!(why_of(&v, "flow-tests").len(), 1, "{v}");
}

#[test]
fn a_package_that_opts_in_to_crate_projects_is_planned_when_a_crate_it_builds_or_runs_or_a_file_its_crates_read_changes_outside_it() {
    // Given a service package whose crate depends on a library crate, runs the binary of another and reads a table, all outside the package,
    // and a package.json that opts in to crate projects as the remote chat agent's does
    let w = Workspace::new("wf-package-crates-plan-");
    let manifest = "[package]\nname = \"svc-bin\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nshared = { path = \"../../../tools/shared\" }\n\n[package.metadata.flow]\ninputs = [\"../../../docs/tables/**\"]\nuses = [\"helper\"]\n";
    let package = |opted_in: bool| format!("{{\"name\":\"svc\",\"scripts\":{{\"test:flow\":\"cargo test --locked -p svc-bin --test flow --manifest-path Cargo.toml\"}}{}}}", if opted_in { ",\"nx\":{\"cargoCrates\":true}" } else { "" });
    w.write("repo/backend/svc/package.json", &package(true));
    w.write("repo/backend/svc/Cargo.toml", "[workspace]\nmembers = [\"rs\"]\nresolver = \"2\"\n");
    w.write("repo/backend/svc/rs/Cargo.toml", manifest);
    w.write("repo/backend/svc/rs/src/main.rs", "fn main() {}\n");
    w.write("repo/backend/svc/rs/tests/flow.rs", "\n");
    w.write("repo/tools/Cargo.toml", "[workspace]\nmembers = [\"shared\", \"helper\"]\nresolver = \"2\"\n");
    for (name, file) in [("shared", "lib.rs"), ("helper", "main.rs")] {
        w.write(&format!("repo/tools/{name}/Cargo.toml"), &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"));
        w.write(&format!("repo/tools/{name}/src/{file}"), "\n");
    }
    w.write("repo/docs/tables/t3.json", "{}\n");
    w.write("repo/docs/other.md", "\n");
    w.commit("a service package with crates outside it");
    let input = manifest.parse::<toml::Table>().unwrap()["package"]["metadata"]["flow"]["inputs"][0].as_str().unwrap().replace("../../../", "").replace("**", "t3.json");
    // why the package's flow test is planned: the package entries only, not the crates'
    let why_of = |v: &Value| -> Vec<String> { v["stages"]["flow-tests"].as_array().unwrap().iter().filter(|t| t["package"] == "svc").map(|t| t["why"][0].as_str().unwrap_or_default().to_string()).collect() };

    // When a commit edits the library crate the package's crate depends on
    w.write("repo/tools/shared/src/lib.rs", "pub fn f() {}\n");
    w.commit("edit the library");
    // Then the package's flow test is planned, for the crate that depends on it
    assert_eq!(why_of(&w.plan("HEAD~1").0), ["its crate svc-bin depends on shared (cargo)"]);

    // When a commit edits the crate whose binary the package's flow tests run, which it does not depend on
    w.write("repo/tools/helper/src/main.rs", "fn main() {\n}\n");
    w.commit("edit the helper");
    // Then the package's flow test is planned for it
    assert_eq!(why_of(&w.plan("HEAD~1").0), ["its crate svc-bin runs helper"]);

    // When a commit edits a table the package's flow tests read
    w.write(&format!("repo/{input}"), "{\"rows\": []}\n");
    w.commit("edit the table");
    // Then the package's flow test is planned for that file
    assert_eq!(why_of(&w.plan("HEAD~1").0), [format!("an input of its crate svc-bin changed: {input}")]);

    // When a commit edits a file beside them that nothing names, Then the package's flow test is not planned
    w.write("repo/docs/other.md", "edited\n");
    w.commit("edit another doc");
    assert_eq!(why_of(&w.plan("HEAD~1").0), Vec::<String>::new());

    // When the package does not opt in and the library crate changes again, Then its crates stay folded into it and the plan leaves it out
    w.write("repo/backend/svc/package.json", &package(false));
    w.commit("the package stops opting in");
    w.write("repo/tools/shared/src/lib.rs", "pub fn g() {}\n");
    w.commit("edit the library again");
    assert_eq!(why_of(&w.plan("HEAD~1").0), Vec::<String>::new());
}

#[test]
fn a_crate_with_flow_groups_is_planned_one_flow_entry_per_group_and_the_stage_runs_each_group_s_target() {
    // Given a cargo workspace whose crate splits its flow tests into groups, and a commit that changes the crate
    let w = Workspace::new("wf-flow-plan-groups-");
    let manifest = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[package.metadata.flow]\ngroups = { threads = [], vcs = [\"tests/fixtures/vcs/**\"] }\n";
    w.write("repo/tools/Cargo.toml", "[workspace]\nmembers = [\"app\"]\nresolver = \"2\"\n");
    w.write("repo/tools/app/Cargo.toml", manifest);
    w.write("repo/tools/app/tests/flow.rs", "mod threads;\nmod vcs;\n");
    w.write("repo/tools/app/src/main.rs", "fn main() {}\n");
    w.commit("a grouped crate");
    w.write("repo/tools/app/src/main.rs", "fn main() {\n}\n");
    w.commit("change it");
    let groups: Vec<String> = manifest.parse::<toml::Table>().unwrap()["package"]["metadata"]["flow"]["groups"].as_table().unwrap().keys().cloned().collect();

    // When the commit is planned
    let (v, _) = w.plan("HEAD~1");
    // Then the crate's flow tests are one entry per group, each its own Nx target, filtered to the group's module
    let planned: Vec<(String, String, String)> = v["stages"]["flow-tests"].as_array().unwrap().iter().map(|t| (t["group"].as_str().unwrap_or("-").to_string(), t["target"].as_str().unwrap_or("-").to_string(), t["command"].as_str().unwrap_or_default().to_string())).collect();
    let expected: Vec<(String, String, String)> = groups.iter().map(|g| (g.clone(), format!("cargo-flow-{g}"), format!("cargo test --locked -p app --test flow --manifest-path tools/Cargo.toml {g}::"))).collect();
    assert_eq!(planned, expected, "{v}");

    // When the flow stage runs from that plan
    w.s.forget("nx");
    let r = w.wf(&["test", "run", "--stage", "flow-tests", "--plan", &w.path("plan.json")], &[]);
    // Then Nx runs the groups' targets for the crate, beside the whole-suite targets of any other
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let targets = groups.iter().map(|g| format!(",cargo-flow-{g}")).collect::<String>();
    assert_eq!(w.s.calls("nx"), [format!("run-many -t test:flow,cargo-flow{targets} -p crate-app --parallel=4 --outputStyle=stream")]);
}

/// A candidate crate `cand` replaying two recorded sets by feature, with the mapping crates `cand/map-a`, `cand/map-b` (which depends on `map-a`) and
/// `cand/map-big` (two named features, one with the files that can move it, one without files).
fn replay_workspace(prefix: &str) -> Workspace {
    let w = Workspace::new(prefix);
    let ws = "[workspace]\nmembers = [\"cand\", \"cand/map-a\", \"cand/map-b\", \"cand/map-big\"]\nresolver = \"2\"\n";
    let lib = |name: &str, deps: &str, meta: &str| format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{deps}\n{meta}");
    w.write("repo/tools/Cargo.toml", ws);
    w.write("repo/tools/cand/Cargo.toml", &lib("cand", "map-a = { path = \"map-a\" }\nmap-b = { path = \"map-b\" }\nmap-big = { path = \"map-big\" }\n", "[package.metadata.integration]\ninputs = [\"cassettes/**\"]\n\n[package.metadata.integration.replay]\nsets = [\"set-1\", \"set-2\"]\n"));
    w.write("repo/tools/cand/map-a/Cargo.toml", &lib("map-a", "", "[package.metadata.integration.replay]\nrows = [\"rpc:a.\"]\n"));
    w.write("repo/tools/cand/map-b/Cargo.toml", &lib("map-b", "map-a = { path = \"../map-a\" }\n", "[package.metadata.integration.replay]\nrows = [\"rpc:b.\"]\n"));
    w.write("repo/tools/cand/map-big/Cargo.toml", &lib("map-big", "", "[package.metadata.integration.replay.alpha]\nrows = [\"rpc:alpha.\"]\nfiles = [\"src/alpha.rs\"]\n\n[package.metadata.integration.replay.rest]\nrows = [\"rpc:rest.\"]\n"));
    for (c, f) in [("cand", "main.rs"), ("cand/map-a", "lib.rs"), ("cand/map-b", "lib.rs"), ("cand/map-big", "lib.rs"), ("cand/map-big", "alpha.rs"), ("cand/map-big", "beta.rs")] {
        w.write(&format!("repo/tools/{c}/src/{f}"), "\n");
    }
    w.write("repo/tools/cand/tests/integration.rs", "\n");
    w.write("repo/tools/cand/cassettes/set-1/interactions.jsonl", "{\"row\":\"rpc:a.one\"}\n{\"row\":\"rpc:alpha.x\"}\n");
    w.write("repo/tools/cand/cassettes/set-2/interactions.jsonl", "{\"row\":\"rpc:b.one\"}\n{\"row\":\"rpc:a.two\"}\n{\"row\":\"rpc:rest.y\"}\n");
    w.commit("a candidate with mapping crates");
    w
}

/// The replay entries of a plan: `(target, sets, why)`.
fn replays(v: &Value) -> Vec<(String, Vec<String>, String)> {
    let mut out: Vec<(String, Vec<String>, String)> = v["stages"]["integration-tests"].as_array().unwrap().iter().filter(|t| t["replay"].is_object()).map(|t| (t["target"].as_str().unwrap().to_string(), t["replay"]["sets"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect(), t["why"][0].as_str().unwrap().to_string())).collect();
    out.sort();
    out
}

#[test]
fn a_change_to_one_mapping_crate_plans_the_replay_of_its_rows_alone_over_the_sets_that_hold_them() {
    // Given a candidate with three mapping crates, each naming its rows, and a commit that changes one
    let w = replay_workspace("wf-replay-plan-changed-");
    w.write("repo/tools/cand/map-b/src/lib.rs", "// changed\n");
    w.commit("change map-b");
    // When it is planned
    let (v, _) = w.plan("HEAD~1");
    // Then one replay entry is planned, as the Nx task `replay-map-b`, over the one set that holds its rows, and the rest of the suite skips every replay
    let r = replays(&v);
    assert_eq!(r.iter().map(|(t, s, _)| (t.as_str(), s.clone())).collect::<Vec<_>>(), [("replay-map-b", vec!["set-2".to_string()])], "{v}");
    let all = v["stages"]["integration-tests"].as_array().unwrap();
    let entry = all.iter().find(|t| t["target"] == "replay-map-b").unwrap();
    assert_eq!(entry["command"], "cargo test --locked -p cand --test integration --manifest-path tools/Cargo.toml -- --test-threads=1 replay_features::map_b::", "{entry}");
    assert_eq!(entry["replay"]["verbs"][0], "wf mimic replay tools/cand/cassettes/set-2 --rows rpc:b. --gaps check", "{entry}");
    assert!(all.iter().any(|t| t["group"].is_null() && t["command"].as_str().unwrap().ends_with("--skip replay_features::")), "{v}");
}

#[test]
fn a_change_to_a_crate_a_mapping_crate_depends_on_plans_the_replays_of_its_dependents() {
    // Given a commit that changes map-a, which map-b depends on
    let w = replay_workspace("wf-replay-plan-dependent-");
    w.write("repo/tools/cand/map-a/src/lib.rs", "// changed\n");
    w.commit("change map-a");
    // When it is planned, Then map-a's and map-b's replays are planned (map-b through its dependency), and map-big's are not
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(replays(&v).iter().map(|(t, _, _)| t.as_str()).collect::<Vec<_>>(), ["replay-map-a", "replay-map-b"], "{v}");
}

#[test]
fn a_change_to_a_cassette_or_the_candidates_own_files_plans_every_replay_and_a_full_plan_does_too() {
    // Given a commit that changes a cassette
    let w = replay_workspace("wf-replay-plan-cassette-");
    w.write("repo/tools/cand/cassettes/set-1/interactions.jsonl", "{\"row\":\"rpc:a.one\"}\n");
    w.commit("edit a cassette");
    // When it is planned, Then every feature has its replay
    let every = ["replay-map-a", "replay-map-b", "replay-map-big-alpha", "replay-map-big-rest"];
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(replays(&v).iter().map(|(t, _, _)| t.as_str()).collect::<Vec<_>>(), every, "{v}");
    // And so does a change to the candidate's own sources
    w.write("repo/tools/cand/src/main.rs", "// changed\n");
    w.commit("change the candidate");
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(replays(&v).iter().map(|(t, _, _)| t.as_str()).collect::<Vec<_>>(), every, "{v}");
    // And a full plan
    let r = w.wf(&["ci", "plan", "tests", "--all", "--json"], &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let all: Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!(replays(&all).iter().map(|(t, _, _)| t.as_str()).collect::<Vec<_>>(), every, "{all}");
}

#[test]
fn a_change_confined_to_the_files_of_one_named_feature_plans_that_feature_and_the_one_without_files() {
    // Given a crate with two named features, one naming src/alpha.rs, and a commit that changes that file
    let w = replay_workspace("wf-replay-plan-files-");
    w.write("repo/tools/cand/map-big/src/alpha.rs", "// changed\n");
    w.commit("change alpha");
    // When it is planned, Then alpha replays, and the feature without files does with it, and no other crate's
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(replays(&v).iter().map(|(t, _, _)| t.as_str()).collect::<Vec<_>>(), ["replay-map-big-alpha", "replay-map-big-rest"], "{v}");
    // And a file no feature names replays both
    w.write("repo/tools/cand/map-big/src/beta.rs", "// changed\n");
    w.commit("change beta");
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(replays(&v).iter().map(|(t, _, _)| t.as_str()).collect::<Vec<_>>(), ["replay-map-big-alpha", "replay-map-big-rest"], "{v}");
}

#[test]
fn a_crate_with_integration_groups_is_planned_the_rest_and_a_group_entry_each_and_the_shards_deal_the_groups_out() {
    // Given a cargo workspace whose crate names two parts of its integration suite as groups, and a commit that changes the crate
    let w = Workspace::new("wf-integration-plan-groups-");
    let manifest = "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[package.metadata.integration.groups]\nnorth = { filter = \"sets::north::\", files = [\"tests/sets/north/**\"] }\nsouth = { filter = \"sets::south::\", files = [\"tests/sets/south/**\"] }\n";
    w.write("repo/tools/Cargo.toml", "[workspace]\nmembers = [\"app\"]\nresolver = \"2\"\n");
    w.write("repo/tools/app/Cargo.toml", manifest);
    w.write("repo/tools/app/tests/integration.rs", "mod sets;\n");
    w.write("repo/tools/app/src/main.rs", "fn main() {}\n");
    w.commit("a grouped crate");
    w.write("repo/tools/app/src/main.rs", "fn main() {\n}\n");
    w.commit("change it");

    // When the commit is planned
    let (v, _) = w.plan("HEAD~1");
    // Then the suite is one entry that skips both sets, and one entry per set, each its own Nx target, filtered to the set's tests
    let planned: Vec<(String, String, String)> = v["stages"]["integration-tests"].as_array().unwrap().iter().map(|t| (t["group"].as_str().unwrap_or("-").to_string(), t["target"].as_str().unwrap_or("-").to_string(), t["command"].as_str().unwrap_or_default().to_string())).collect();
    let command = "cargo test --locked -p app --test integration --manifest-path tools/Cargo.toml -- --test-threads=1";
    let expected = [
        ("-", "-", format!("{command} --skip sets::north:: --skip sets::south::")),
        ("north", "cargo-integration-north", format!("{command} sets::north::")),
        ("south", "cargo-integration-south", format!("{command} sets::south::")),
    ];
    assert_eq!(planned, expected.map(|(g, t, c)| (g.to_string(), t.to_string(), c)), "{v}");

    // When the first and the second of two shards run the stage from that plan
    let nx_of = |shard: &str| {
        w.s.forget("nx");
        let r = w.wf(&["test", "run", "--stage", "integration-tests", "--plan", &w.path("plan.json"), "--shard", shard], &[]);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        w.s.calls("nx")
    };
    let (first, second) = (nx_of("1/2"), nx_of("2/2"));
    // Then the suite and the south set run in the first, with the crate's whole-suite task, and the north set alone in the second, without it
    assert_eq!(first, ["run-many -t test:integration,cargo-integration,cargo-integration-south -p crate-app --parallel=1 --outputStyle=stream"]);
    assert_eq!(second, ["run-many -t cargo-integration-north -p crate-app --parallel=1 --outputStyle=stream"]);
}

#[test]
fn drift_against_another_environment_reports_how_many_tasks_hash_differently() {
    // Given this environment's hash table (two tasks, from a node stub) and another's in which one of the two hashes differently
    let w = Workspace::new("wf-flow-drift-metrics-");
    let node = w.s.bin_dir.join("node-hasher");
    create_stub(&node, r#"echo '{"runtime":{},"tasks":{"a:lint":{"hash":"1","inputs":{"files":"1"}},"b:lint":{"hash":"2","inputs":{"files":"2"}}}}'"#);
    let theirs = w.path("theirs.json");
    std::fs::write(&theirs, r#"{"runtime":{},"tasks":{"a:lint":{"hash":"1","inputs":{"files":"1"}},"b:lint":{"hash":"3","inputs":{"files":"3"}}}}"#).unwrap();
    let source = MockServer::start(None);

    // When the two are compared
    let r = w.wf(&["test", "drift", "--against", &theirs], &[("WF_NODE", node.display().to_string()), ("WF_CI_METRICS_BASE", format!("http://127.0.0.1:{}", source.port))]);

    // Then the report says one task drifted, an event names the count and its cause, and the source received the count
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(last_json(&r)["drifted"].as_array().map(Vec::len), Some(1), "{}", r.stdout);
    let event = r.stderr.lines().find(|l| l.contains("msg=\"drift.report\"")).unwrap_or_else(|| panic!("no drift event: {}", r.stderr));
    assert!(event.contains("level=warn") && event.contains("drifted=1") && event.contains("causes=files"), "{event}");
    let seen = gauges(&source.requests());
    assert_eq!(seen.iter().find(|g| g.metric == "ci.drift.tasks_drifted").map(|g| g.value), Some(1.0), "{seen:?}");
}

#[test]
fn a_probe_says_why_each_test_it_would_run_misses_against_the_earlier_runs_of_that_test() {
    // Given the plan's android tests, a hasher stub that gives each its input digests, and a history directory that holds an earlier run of
    // `typing` with another env digest and none of `shaky`
    let w = Workspace::new("wf-flow-probe-why-");
    w.plan("HEAD~1");
    let node = w.s.bin_dir.join("node-hasher");
    let inputs = |env: &str| json!({ "env:EXPO_PUBLIC_AGENT_BASE_URL": env, "e2e-typing:e2e/typing.lua": "1" });
    let table = json!({ "runtime": {}, "tasks": { "e2e-typing:e2e-android": { "hash": "H1", "inputs": inputs("2") }, "e2e-shaky:e2e-android": { "hash": "H2", "inputs": { "e2e-shaky:e2e/shaky.lua": "1" } } } });
    std::fs::write(w.path("table.json"), table.to_string()).unwrap();
    create_stub(&node, &format!("cat {}", w.path("table.json")));
    let history = w.path("nx-why");
    let earlier = json!({ "schema": 1, "groups": {}, "tasks": { "e2e-typing:e2e-android": [{ "run": "9", "sha": "", "ts": 1_700_000_000_000i64, "source": "ci", "hash": "H0", "outcome": "ran", "passed": true, "inputs": inputs("1"), "npm": null, "runtime": {} }] } });
    std::fs::create_dir_all(format!("{history}/nx-inputs")).unwrap();
    std::fs::write(format!("{history}/nx-inputs/e2e-android.json"), earlier.to_string()).unwrap();

    // When the stage is probed
    let probe = w.wf(&["test", "run", "--stage", "android-e2e-tests", "--plan", &w.path("plan.json"), "--probe"], &[("WF_NODE", node.display().to_string()), ("WF_NX_WHY_DIR", history.clone())]);

    // Then each test that would run says why: typing names the env var that changed, shaky was never stored, and nothing was written
    assert_eq!(probe.exit_code, 0, "{}{}", probe.stdout, probe.stderr);
    let why = &last_json(&probe)["why"]["misses"];
    let of = |task: &str| why.as_array().unwrap().iter().find(|m| m["task"] == task).unwrap_or_else(|| panic!("no verdict for {task}: {why}")).clone();
    assert_eq!((of("e2e-typing:e2e-android")["verdict"].clone(), of("e2e-typing:e2e-android")["changed"][0]["name"].clone()), (json!("input-changed"), json!("EXPO_PUBLIC_AGENT_BASE_URL")), "{why}");
    assert_eq!(of("e2e-shaky:e2e-android")["verdict"], "never-stored", "{why}");
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(format!("{history}/nx-inputs/e2e-android.json")).unwrap()).unwrap();
    assert_eq!(kept["tasks"]["e2e-typing:e2e-android"].as_array().map(Vec::len), Some(1), "a probe records nothing: {kept}");
}

#[test]
fn given_the_pipeline_when_the_sharded_test_jobs_are_read_then_their_concurrency_groups_name_their_inputs_not_the_run() {
    // Given the pipeline workflow
    let text = std::fs::read_to_string(crate::support::repo_root().join(".github/workflows/pipeline.yml")).unwrap();
    // When the concurrency groups of the sharded test jobs are read
    for (job, keys) in [("core-tests", "core_shard_keys"), ("integration-tests", "integration_shard_keys"), ("cli-e2e-tests", "cli_e2e_shard_keys")] {
        let block = pipeline_job(&text, job);
        let group = block.lines().find(|l| l.trim_start().starts_with("group:")).unwrap_or_else(|| panic!("no group in {job}"));
        // Then the group holds the caller workflow and the shard's input key, so identical work of one workflow waits (and two workflows of one commit never cancel each other) as a pending job and a fix with other inputs starts at once
        assert!(group.contains(keys) && group.contains("github.workflow") && !group.contains("run_id"), "{job}: {group}");
    }
}

/// A cargo workspace of the chain `chain` (an integration suite whose traces refresh the `demo` coverage table) and the unrelated `other`, with the
/// table's `pending` count.
fn evidence_workspace(prefix: &str, pending: u64) -> Workspace {
    let w = Workspace::new(prefix);
    let lib = |name: &str, meta: &str| format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{meta}");
    w.write("repo/tools/Cargo.toml", "[workspace]\nmembers = [\"chain\", \"other\"]\nresolver = \"2\"\n");
    w.write("repo/tools/chain/Cargo.toml", &lib("chain", "\n[package.metadata.integration]\nevidence = \"demo\"\n"));
    w.write("repo/tools/other/Cargo.toml", &lib("other", ""));
    for f in ["chain/src/main.rs", "chain/tests/integration.rs", "other/src/main.rs"] {
        w.write(&format!("repo/tools/{f}"), "fn main() {}\n");
    }
    w.write("repo/docs/plans/rca-parity/coverage/demo.json", &json!({ "summary": { "rows": 9, "served": 9 - pending, "pending": pending } }).to_string());
    w.commit("a chain and an unrelated crate");
    w
}

fn integration_crates(v: &Value) -> Vec<(String, String)> {
    v["stages"]["integration-tests"].as_array().unwrap().iter().map(|t| (t["crate"].as_str().unwrap().to_string(), t["why"][0].as_str().unwrap().to_string())).collect()
}

#[test]
fn a_chain_whose_traces_would_serve_pending_rows_is_planned_for_a_change_that_does_not_reach_it_and_is_never_proven_so_its_bundle_is_published_every_time() {
    // Given a coverage table with two pending rows and a commit that changes only the unrelated crate
    let w = evidence_workspace("wf-evidence-plan-pending-", 2);
    w.write("repo/tools/other/src/main.rs", "fn main() {\n}\n");
    w.commit("change the unrelated crate");

    // When the commit is planned
    let (v, _) = w.plan("HEAD~1");

    // Then the chain's suite is planned once, for the rows, and nothing of the unrelated crate's is
    assert_eq!(integration_crates(&v), [("chain".to_string(), "2 pending row(s) of the demo coverage table wait for the trace bundle of this suite".to_string())], "{v}");
    // And no shard holding it is skipped as proven, whatever the cache says
    let r = w.wf(&["ci", "proven", "--stage", "integration-tests", "--count", "1", "--plan", &w.path("plan.json"), "--no-lookup"], &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let proven = last_json(&r);
    assert_eq!((proven["run"].clone(), proven["proven"].clone()), (json!([1]), json!([])), "{proven}");
}

#[test]
fn a_chain_whose_table_has_every_row_served_is_still_planned_for_a_change_that_does_not_reach_it() {
    // Given a coverage table with every row served, and a commit that changes only the unrelated crate
    let w = evidence_workspace("wf-evidence-plan-served-", 0);
    w.write("repo/tools/other/src/main.rs", "fn main() {\n}\n");
    w.commit("change the unrelated crate");
    // When it is planned, Then the chain's suite is planned once, to keep the evidence current
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(integration_crates(&v), [("chain".to_string(), "its trace bundle keeps the demo coverage table's evidence current".to_string())], "{v}");

    // And a commit that changes the chain plans it for the change, not for the table
    w.write("repo/tools/chain/src/main.rs", "fn main() {\n}\n");
    w.commit("change the chain");
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(integration_crates(&v), [("chain".to_string(), "changed".to_string())], "{v}");
}

#[test]
fn a_chain_naming_an_evidence_table_that_does_not_exist_is_planned_and_says_so() {
    // Given a chain whose `evidence` names no table, and a commit that changes only the unrelated crate
    let w = evidence_workspace("wf-evidence-plan-typo-", 0);
    w.write("repo/docs/plans/rca-parity/coverage/demo.json", "not json");
    w.commit("break the table");
    w.write("repo/tools/other/src/main.rs", "fn main() {\n}\n");
    w.commit("change the unrelated crate");
    // When it is planned, Then the chain is planned and the reason names the missing table
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(integration_crates(&v), [("chain".to_string(), "the demo coverage table named by `evidence` is missing or unreadable, so its rows cannot be shown served".to_string())], "{v}");
}

#[test]
fn a_chain_a_change_reaches_is_planned_once_when_its_table_has_pending_rows() {
    // Given pending rows and a commit that changes the chain itself
    let w = evidence_workspace("wf-evidence-plan-reached-", 3);
    w.write("repo/tools/chain/src/main.rs", "fn main() {\n}\n");
    w.commit("change the chain");
    // When it is planned, Then the chain's suite has one entry, for the change
    let (v, _) = w.plan("HEAD~1");
    assert_eq!(integration_crates(&v), [("chain".to_string(), "changed".to_string())], "{v}");
}
