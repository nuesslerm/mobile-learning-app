//! The CI test pipeline as `wf test` serves it: `run <tests>` runs focused e2e tests unit by unit through a harness, `export` builds
//! the web export the browser units load, `publish` turns a run into a bucket bundle and a metrics row, and `results` reads them back
//! (what `plan` and `run --stage` do is in `pipeline`). `pnpm`, `nx`, `gh`, `curl` and the harness are stubs; the bucket and the
//! ORDS table are mock HTTP services.
use crate::support::{last_json, MockReply, MockServer, RunResult, SlotSandbox};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

/// A temp tree with a small pnpm workspace in a git repo of two commits, focused e2e tests and a fake harness.
struct Workspace {
    s: SlotSandbox,
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
case "$target" in lint) exit 1;; e2e-*) ;; *) exit 0;; esac
for p in $projects; do t=${p#e2e-}
  if [ -n "$WF_E2E_PROBE" ]; then echo "$t" >> "$WF_E2E_PROBE"; continue; fi
  mkdir -p "$WF_E2E_OUT/$t"; ok=true; [ "$t" = shaky ] && ok=false
  printf '{"ok":%s,"sha":"%s","units":[{"test":"%s","verdict":"pass"}]}' "$ok" "$WF_E2E_SHA" "$t" > "$WF_E2E_OUT/$t/summary.json"
done
[ -n "$WF_E2E_PROBE" ] && exit 1
case "$projects" in *e2e-shaky*) exit 1;; esac
exit 0"#;

/// A harness that opens the base it was given the way a browser would: it fetches a few paths of the export and writes each
/// status, content type and first bytes to `served.txt` in its output dir.
const EXPORT_RUNNER: &str = "#!/bin/sh\n\
echo \"args: $*\"\n\
echo \"args: $*\" >> \"$(dirname \"$0\")/runner.log\"\n\
for a in \"$@\"; do case \"$a\" in base=*) base=${a#base=};; esac; [ \"$prev\" = --test-output-dir ] && dir=$a; prev=$a; done\n\
mkdir -p \"$dir\"\n\
if [ -n \"$base\" ]; then\n\
  origin=${base%/mla-tamagui}\n\
  for p in /mla-tamagui /mla-tamagui/ /mla-tamagui/leetcode/two-sum /mla-tamagui/leetcode/two-sum/description /mla-tamagui/tamil/uyir-1 /mla-tamagui/tamil/uyir-2 /mla-tamagui/no/such/route /mla-tamagui/_expo/static/js/web/entry.js /mla-tamagui/missing.png /elsewhere /mla-tamagui/../../etc/passwd /mla-tamagui/%2e%2e/%2e%2e/etc/passwd; do\n\
    printf '%s => %s | %s\\n' \"$p\" \"$(curl --noproxy '*' --path-as-is -s -o \"$dir/body\" -w '%{http_code} %{content_type}' \"$origin$p\")\" \"$(head -c 20 \"$dir/body\" | tr -d '\\n')\" >> \"$dir/served.txt\"\n\
  done\n\
fi\n\
echo report > \"$dir/report.html\"; echo \"report: $dir/report.html\"; echo \"recording: $dir/run.mp4\"\n\
case \"$*\" in *e2e/broken.lua*) exit 1;; esac\n\
exit 0\n";

/// The lockfile of a workspace whose app links to a contracts package under backend/, through one of its features.
const LOCK: &str = "lockfileVersion: '9.0'\nimporters:\n  frontend/mla-tamagui:\n    dependencies:\n      '@feature/x':\n        specifier: workspace:*\n        version: link:features/x\n  frontend/mla-tamagui/features/x:\n    dependencies:\n      '@app/contracts':\n        specifier: workspace:*\n        version: link:../../../../backend/contracts\n";

/// The harness `wf test run` starts per unit: logs its argv and lease, writes a report, a recording line and timing lines, and fails,
/// hangs or passes by test; a cold start fails for every unit (`STUB_WARM_FAIL`) or for the ones whose argv names a target
/// (`STUB_WARM_FAIL_ON=update`).
const RUNNER: &str = "#!/bin/sh\n\
echo \"args: $*\"\n\
echo \"args: $*\" >> \"$(dirname \"$0\")/runner.log\"\n\
echo \"lease: ${WF_ANDROID_LEASE_ID:-none}\"\n\
for a in \"$@\"; do [ \"$prev\" = --test-output-dir ] && dir=$a; prev=$a; done\n\
mkdir -p \"$dir\"; echo report > \"$dir/report.html\"; echo \"report: $dir/report.html\"; echo \"recording: $dir/run.mp4\"\n\
[ -n \"$WF_RUN_TIMING\" ] && printf '{\"verb\":\"tap\",\"ms\":1200}\\n{\"verb\":\"find\",\"ms\":300}\\n' >> \"$WF_RUN_TIMING\"\n\
case \"$*\" in *e2e/slow.lua*) trap 'echo stopped; exit 124' TERM; sleep 30 & wait;; esac\n\
case \"$*\" in *e2e/broken.lua*) exit 1;; esac\n\
case \"$*\" in *e2e/warm-app.lua*) [ -n \"$STUB_WARM_FAIL\" ] && exit 1; [ -n \"$STUB_WARM_FAIL_ON\" ] && case \"$*\" in *\"against=$STUB_WARM_FAIL_ON\"*) exit 1;; esac;; esac\n\
case \"$*\" in *problem=fail*) exit 1;; esac\n\
exit 0\n";

impl Workspace {
    fn new(prefix: &str) -> Workspace {
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
        w.write("runner.sh", RUNNER);
        Command::new("chmod").args(["+x", &w.path("runner.sh")]).status().unwrap();
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

    fn path(&self, sub: &str) -> String {
        self.root.join(sub).display().to_string()
    }

    /// `wf test prepare --web`: the stub pnpm writes the export the browser units run against.
    fn export(&self) -> RunResult {
        self.wf(&["test", "prepare", "--web"], &[])
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git").args(args).current_dir(self.root.join("repo")).env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-qm", message]);
    }

    /// The app's EAS project, committed: what an update's base is made of.
    fn commit_eas_project(&self) {
        self.write("repo/frontend/mla-tamagui/app.config.js", "module.exports = { expo: { runtimeVersion: 'exposdk:57.0.0', extra: { eas: { projectId: 'proj-1' } } } };\n");
        self.commit("the app's EAS project");
    }

    /// `wf <args>` on this workspace, with `extra` environment last.
    fn wf(&self, args: &[&str], extra: &[(&str, String)]) -> RunResult {
        let mut envs = vec![
            ("WF_TEST_E2E_DIR", self.path("e2e")),
            ("WF_TEST_REPO", self.path("repo")),
            ("WF_TEST_RUNNER", self.path("runner.sh")),
            ("WF_TEST_NO_ADB", "1".to_string()),
            ("WF_NX_BIN", self.s.bin_dir.join("nx").display().to_string()),
            // a closed port unless the test serves a bucket: an Android unit that forgot one fails, never reads the real pointer
            ("VERIFICATION_ARTIFACTS_READ_URL", "http://127.0.0.1:9/o".to_string()),
        ];
        if !extra.iter().any(|(k, _)| *k == "WF_ANDROID_LEASE_DIR") {
            envs.push(("WF_TEST_NO_LEASE", "1".to_string()));
        }
        // the tailnet answers the backends' own names (no DNS in a test), and a pointer read that fails is tried again at once
        envs.push(("WF_TEST_TAILNET_NAMES", "leetcode-backend-prod,remote-chat-agent-prod".to_string()));
        envs.push(("WF_EAS_POINTER_BACKOFF_MS", "10".to_string()));
        envs.extend(extra.iter().cloned());
        let refs: Vec<(&str, &str)> = envs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.s.run(args, &refs)
    }

    fn summary(r: &RunResult) -> Value {
        r.stdout.lines().rev().find(|l| l.starts_with("{\"ok\"")).and_then(|l| serde_json::from_str(l).ok()).unwrap_or_else(|| panic!("no summary: {}{}", r.stdout, r.stderr))
    }
}

#[test]
fn focused_e2e_tests_run_unit_by_unit_through_their_harness() {
    // Given focused tests on the app and on services, one unfocused, a commit that changed only the library the app uses, and the
    // update this push's publish put in the bucket for the Android ones
    let w = Workspace::new("wf-flow-e2e-");
    let (_bucket, update) = publish_update(&w);
    assert_eq!(w.export().exit_code, 0);
    let out = w.path("out");
    let run = |args: &[&str]| {
        let mut full = vec!["test", "run"];
        full.extend_from_slice(args);
        w.wf(&full, &update)
    };

    // When everything is listed
    let listed = w.wf(&["test", "plan", "--all", "--json"], &[]);
    // Then every e2e test is there (the helper without a header is none) and only the focused ones are selected
    let v: Value = serde_json::from_str(listed.stdout.trim()).unwrap_or_else(|_| panic!("{}{}", listed.stdout, listed.stderr));
    let names: Vec<&str> = v["tests"].as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["broken", "idle", "shaky", "slow", "svc", "typing"]);
    assert!(v["tests"].as_array().unwrap().iter().all(|b| b["selected"] == b["focus"] && b["against"] == if b["platform"] == "browser" { "export" } else { "update" }));
    assert_eq!(v["tests"].as_array().unwrap().iter().filter(|b| b["selected"] == true).count(), 5);

    // When a test with two units runs with test flags after --
    let typing = run(&["typing", "--out", &out, "--serial", "127.0.0.1:5555", "--", "--update-goldens"]);
    // Then the app is started cold once, then each unit runs in turn on the android harness with its args, the serial and the flags
    assert_eq!(typing.exit_code, 0, "{}{}", typing.stdout, typing.stderr);
    let v = Workspace::summary(&typing);
    let units = v["units"].as_array().unwrap();
    assert_eq!(units.iter().map(|u| u["args"][0].as_str().unwrap()).collect::<Vec<_>>(), ["problem=two-sum", "problem=valid-palindrome"]);
    let first = units[0]["logTail"][0].as_str().unwrap();
    assert!(first.starts_with("args: android test e2e/typing.lua --test-output-dir ") && first.contains("--env problem=two-sum") && first.ends_with("--device 127.0.0.1:5555 --update-goldens"), "{first}");
    assert_eq!(units[0]["logTail"][1], "lease: none");
    let args_of = |dir: &Value| std::fs::read_to_string(PathBuf::from(dir.as_str().unwrap()).join("run.log")).unwrap().lines().find_map(|l| l.strip_prefix("args: ")).unwrap_or_default().to_string();
    let warms = v["warms"].as_array().unwrap();
    assert_eq!(warms.len(), 1, "{v}");
    assert!(args_of(&warms[0]["dir"]).contains("android test e2e/warm-app.lua") && args_of(&warms[0]["dir"]).contains("--cold-start"), "{v}");
    assert!(typing.stdout.lines().next().unwrap_or("").contains("\"warm\":1"), "{}", typing.stdout);
    // and each result carries its verdict, wall time, a per-verb timing table, the report and the recording, beside a summary file
    for u in units {
        assert_eq!(u["verdict"], "pass");
        assert!(!args_of(&u["dir"]).contains("--cold-start"), "{u}");
        assert!(u["timing"]["wallS"].as_f64().is_some() && !u["timing"]["byVerb"].as_array().unwrap().is_empty(), "{u}");
        assert!(u["report"].as_str().unwrap().ends_with("rec/report.html") && !u["recording"].as_array().unwrap().is_empty(), "{u}");
    }
    assert!(w.root.join("out/summary.json").is_file());

    // When a test whose first unit fails runs
    let shaky = run(&["shaky", "--out", &out]);
    // Then the app is started cold again before the next unit
    assert_eq!(shaky.exit_code, 1, "{}{}", shaky.stdout, shaky.stderr);
    let v = Workspace::summary(&shaky);
    assert_eq!(v["warms"].as_array().unwrap().len(), 2, "{v}");
    assert_eq!(v["units"].as_array().unwrap().iter().map(|u| u["verdict"].as_str().unwrap()).collect::<Vec<_>>(), ["fail", "pass"]);
    // and the failing unit ran once: a failure is reported, never run again
    assert_eq!(v["units"].as_array().unwrap().iter().map(|u| u["attempt"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 1], "{v}");
    assert!(!shaky.stdout.contains("\"verdict\":\"retry\"") && std::fs::read_to_string(w.root.join("runner.log")).unwrap().matches("problem=fail").count() == 1, "{}", shaky.stdout);

    // When the app cannot start (the warm-up fails) before a test with two units
    let mut dead_env = update.clone();
    dead_env.push(("STUB_WARM_FAIL", "1".to_string()));
    let dead = w.wf(&["test", "run", "typing", "--out", &out], &dead_env);
    // Then it is started once, not before each unit, and both units are errors that say why
    assert_eq!(dead.exit_code, 1, "{}{}", dead.stdout, dead.stderr);
    let v = Workspace::summary(&dead);
    assert_eq!(v["warms"].as_array().unwrap().len(), 1, "{v}");
    assert!(v["units"].as_array().unwrap().iter().all(|u| u["verdict"] == "error" && u["note"].as_str().unwrap().starts_with("the app did not start cold")), "{v}");

    // When a failing test and one that outlasts its 1 s timeout run
    let failing = run(&["broken", "slow", "--out", &out]);
    // Then the command fails, the failure is a fail and the slow one was stopped with its report
    assert_eq!(failing.exit_code, 1, "{}{}", failing.stdout, failing.stderr);
    let v = Workspace::summary(&failing);
    let by = |n: &str| v["units"].as_array().unwrap().iter().find(|u| u["test"] == n).cloned().unwrap();
    assert_eq!(by("broken")["verdict"], "fail");
    let slow = by("slow");
    assert_eq!((slow["verdict"].as_str(), slow["note"].as_str()), (Some("error"), Some("timed out after 1s")), "{slow}");
    assert!(slow["report"].as_str().is_some_and(|p| p.ends_with("rec/report.html")), "{slow}");

    // When failures in a row reach the limit
    let capped = w.wf(&["test", "run", "broken", "slow", "--out", &out], &[("WF_TEST_MAX_FAILS", "1".to_string())]);
    // Then the rest are not run but reported as skipped with the reason, so a broken Metro or device costs one unit, not the job
    let v = Workspace::summary(&capped);
    let slow = v["units"].as_array().unwrap().iter().find(|u| u["test"] == "slow").cloned().unwrap();
    assert_eq!((capped.exit_code, slow["verdict"].as_str(), slow["note"].as_str()), (1, Some("skipped"), Some("not run: the 1 units before it failed in a row")), "{v}");

    // When the change is run on Android, then a range that changed nothing, then the unfocused test is named
    let changed = run(&["--changed", "--base", "HEAD~1", "--platform", "android", "--out", &out]);
    let nothing = run(&["--changed", "--base", "HEAD", "--platform", "android", "--out", &out]);
    let named = run(&["idle", "--out", &out]);
    // Then both run every focused Android test, the change or none alike, and naming the unfocused one is refused with the fix
    let tests = |r: &RunResult| Workspace::summary(r)["units"].as_array().unwrap().iter().map(|u| u["test"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(tests(&changed), ["shaky", "shaky", "typing", "typing"], "{}{}", changed.stdout, changed.stderr);
    assert_eq!(tests(&nothing), tests(&changed));
    assert_ne!(named.exit_code, 0);
    assert!(format!("{}{}", named.stdout, named.stderr).contains("idle is not focused: add `focus: true`"), "{}{}", named.stdout, named.stderr);

    // When a platform is chosen
    let browser = run(&["typing", "svc", "--platform", "browser", "--out", &out]);
    // Then only that harness's tests run and no app is started cold for Android
    let v = Workspace::summary(&browser);
    assert_eq!(v["units"].as_array().unwrap().iter().map(|u| u["test"].as_str().unwrap()).collect::<Vec<_>>(), ["svc"], "{v}");
    assert_eq!(v["warms"].as_array().unwrap().len(), 0, "{v}");

    // When a test on both harnesses is named with the browser chosen
    w.write("e2e/both.lua", "--[[---\nplatform: android, browser\nagainst: android=update, browser=export\npackages: backend/svc\nfocus: true\n---]]\nlog(1)\n");
    let both = run(&["both", "--platform", "browser", "--out", &out]);
    // Then its browser entry runs, not only the first (Android) one the name matches
    let v = Workspace::summary(&both);
    assert_eq!(v["units"].as_array().unwrap().iter().map(|u| (u["test"].as_str().unwrap(), u["platform"].as_str().unwrap())).collect::<Vec<_>>(), [("both", "browser")], "{v}");
    // and each harness's entry has its own target
    let planned: Value = serde_json::from_str(w.wf(&["test", "plan", "--all", "--json"], &[]).stdout.trim()).unwrap();
    let targets: Vec<(&str, &str)> = planned["tests"].as_array().unwrap().iter().filter(|b| b["name"] == "both").map(|b| (b["platform"].as_str().unwrap(), b["against"].as_str().unwrap())).collect();
    assert_eq!(targets, [("android", "update"), ("browser", "export")], "{planned}");
    std::fs::remove_file(w.path("e2e/both.lua")).unwrap();

    // When the Android test runs under the device lease kept in a local directory, as on the VM
    let mut leased_env = update.clone();
    leased_env.push(("WF_ANDROID_LEASE_DIR", w.path("leases")));
    let leased = w.wf(&["test", "run", "typing", "--out", &out], &leased_env);
    // Then each harness run holds the lease this process took
    assert_eq!(leased.exit_code, 0, "{}{}", leased.stdout, leased.stderr);
    for u in Workspace::summary(&leased)["units"].as_array().unwrap() {
        assert!(u["logTail"][1].as_str().is_some_and(|l| l.starts_with("lease: device-")), "{u}");
    }

    // When the verbs that were retired run
    for (verb, says) in [("enqueue", "wf test run"), ("metrics", "wf test publish"), ("record", "test-report skill"), ("device", "wf test run")] {
        let r = w.wf(&["test", verb, "list"], &[]);
        // Then each exits 2 saying it is gone and naming what replaced it
        assert!(r.exit_code == 2 && r.stdout.contains(&format!("wf test {verb} is gone")) && r.stdout.contains(says), "{verb}: {}", r.stdout);
    }
}

#[test]
fn browser_units_run_against_the_export_wf_serves_and_that_export_must_be_current() {
    // Given a workspace whose browser tests run against an export, a harness that opens the base it is given, and the update this
    // push published for its Android test
    let w = Workspace::new("wf-flow-export-");
    let (bucket, update) = publish_update(&w);
    w.write("runner-export.sh", EXPORT_RUNNER);
    Command::new("chmod").args(["+x", &w.path("runner-export.sh")]).status().unwrap();
    let out = w.path("out");
    let run = |args: &[&str]| {
        let mut full = vec!["test", "run"];
        full.extend_from_slice(args);
        let mut env = update.clone();
        env.push(("WF_TEST_RUNNER", w.path("runner-export.sh")));
        w.wf(&full, &env)
    };
    let unit = |r: &RunResult, test: &str| Workspace::summary(r)["units"].as_array().unwrap().iter().find(|u| u["test"] == test).cloned().unwrap_or_else(|| panic!("{}{}", r.stdout, r.stderr));
    let pnpm_exports = || std::fs::read_to_string(w.s.bin_dir.join("pnpm.log")).unwrap_or_default().lines().filter(|l| l.contains("run export:web")).count();

    // When a browser unit runs before any export was built
    let missing = run(&["svc", "--out", &out]);
    // Then it is an error naming the missing export and the command that builds it, and the harness never started
    let u = unit(&missing, "svc");
    assert_eq!((missing.exit_code, u["verdict"].as_str(), u["next"].as_str()), (1, Some("error"), Some("wf test prepare --web")), "{}", missing.stdout);
    assert!(u["note"].as_str().unwrap().contains("no web export at"), "{u}");
    assert!(!w.root.join("runner.log").exists(), "the harness started without an export");

    // When the export is built, and built again with nothing changed
    let built = w.export();
    let again = w.export();
    // Then the second is the first, kept: the build ran once
    let (b, a) = (last_json(&built), last_json(&again));
    assert_eq!((built.exit_code, b["ok"].clone(), b["reused"].clone()), (0, json!(true), json!(false)), "{}{}", built.stdout, built.stderr);
    assert_eq!((a["reused"].clone(), a["inputs"].clone(), pnpm_exports()), (json!(true), b["inputs"].clone(), 1), "{again:?}");
    assert!(w.root.join("repo/frontend/mla-tamagui/dist/wf-export.json").is_file());
    // and the build carried source maps (the browser debugger resolves logpoints through them) and measured its memory
    assert_eq!((b["flags"].as_str(), std::fs::read_to_string(w.root.join("repo/frontend/mla-tamagui/dist/flags.txt")).unwrap().trim()), (Some("--source-maps"), "--source-maps"), "{built:?}");
    assert!(b["memory"]["peakRssMb"].is_number(), "{built:?}");

    // When it runs with the backends baked in (a credential among them), and once more with the same values
    let baked = [("EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL", "http://backend:8789".to_string()), ("EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN", "s3cret-token".to_string())];
    let rebuilt = w.wf(&["test", "prepare", "--web"], &baked);
    let kept = w.wf(&["test", "prepare", "--web"], &baked);
    // Then other baked values are a new build, the credential is never printed or stamped, and the same values keep it
    assert_eq!((last_json(&rebuilt)["reused"].clone(), last_json(&kept)["reused"].clone(), pnpm_exports()), (json!(false), json!(true), 2), "{}", rebuilt.stdout);
    let shown = last_json(&rebuilt)["baked"]["EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN"].as_str().unwrap().to_string();
    assert!(shown.starts_with("set:") && shown.len() == 12 && last_json(&rebuilt)["baked"]["EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL"] == "http://backend:8789", "{}", rebuilt.stdout);
    let stamp = std::fs::read_to_string(w.root.join("repo/frontend/mla-tamagui/dist/wf-export.json")).unwrap();
    assert!(!format!("{}{}{stamp}", rebuilt.stdout, rebuilt.stderr).contains("s3cret-token"), "{stamp}");
    // and a rotated credential is a new build too, though its stamp still shows only that it is set
    let rotated = w.wf(&["test", "prepare", "--web"], &[baked[0].clone(), ("EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN", "another-token".to_string())]);
    assert_eq!((last_json(&rotated)["reused"].clone(), pnpm_exports()), (json!(false), 3), "{}", rotated.stdout);
    assert_ne!(last_json(&rotated)["baked"]["EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN"].as_str().unwrap(), shown);

    // When the browser unit runs, and a failing one
    let ran = run(&["svc", "broken", "--out", &out]);
    // Then each opens the base of an export served on loopback under /mla-tamagui, and the result says which export that was
    let (svc, broken) = (unit(&ran, "svc"), unit(&ran, "broken"));
    assert_eq!((svc["verdict"].as_str(), broken["verdict"].as_str(), ran.exit_code), (Some("pass"), Some("fail"), 1), "{}", ran.stdout);
    let args = std::fs::read_to_string(PathBuf::from(svc["dir"].as_str().unwrap()).join("run.log")).unwrap().lines().find_map(|l| l.strip_prefix("args: ")).unwrap().to_string();
    let base = args.split_whitespace().find_map(|t| t.strip_prefix("base=")).unwrap_or_else(|| panic!("no base in {args}")).to_string();
    assert!(base.starts_with("http://127.0.0.1:") && base.ends_with("/mla-tamagui") && args.contains("--env against=export"), "{args}");
    assert!(["metroCommit", "metroWarmS", "metroBundle"].iter().all(|k| svc.get(*k).is_none()), "{svc}");
    let rotated_stamp = last_json(&rotated);
    let baked_names: Vec<&String> = rotated_stamp["baked"].as_object().unwrap().keys().collect();
    assert_eq!((svc["base"].as_str(), svc["export"]["inputs"].clone(), svc["export"]["baked"]["keys"].clone()), (Some(base.as_str()), last_json(&rotated)["inputs"].clone(), json!(baked_names)), "{svc}");
    // and the result, which gets published and cached, names what was baked but shows no host and no credential hash
    let result_text = std::fs::read_to_string(PathBuf::from(svc["dir"].as_str().unwrap()).join("result.json")).unwrap();
    assert!(svc["export"]["baked"]["digest"].as_str().is_some_and(|d| d.len() == 12) && svc["export"]["flags"] == "--source-maps", "{svc}");
    assert!(!result_text.contains("backend:8789") && !result_text.contains(&shown) && !result_text.contains("another-token"), "{result_text}");
    // and the server answered that harness as a static host does: a route's own page, a dynamic route's page at its nested path,
    // the page generateStaticParams wrote for one id before the template page every other id gets, the SPA fallback for a route no file names, a script with its type, and nothing outside the base path, for a missing asset or
    // for a path that climbs out of the export (plain or percent-encoded)
    let served = std::fs::read_to_string(PathBuf::from(svc["dir"].as_str().unwrap()).join("rec/served.txt")).unwrap();
    let rows: Vec<(String, String, String)> = served
        .lines()
        .map(|l| {
            let (path, rest) = l.split_once(" => ").unwrap();
            let (status, body) = rest.split_once(" | ").unwrap();
            (path.to_string(), status.split(' ').next().unwrap().to_string(), body.to_string())
        })
        .collect();
    let row = |path: &str, status: &str, body: &str| (path.to_string(), status.to_string(), body.to_string());
    assert_eq!(
        rows,
        [
            row("/mla-tamagui", "200", "INDEX"),
            row("/mla-tamagui/", "200", "INDEX"),
            row("/mla-tamagui/leetcode/two-sum", "200", "LEETCODE"),
            row("/mla-tamagui/leetcode/two-sum/description", "200", "DESCRIPTION"),
            row("/mla-tamagui/tamil/uyir-1", "200", "UYIR1"),
            row("/mla-tamagui/tamil/uyir-2", "200", "TEMPLATE"),
            row("/mla-tamagui/no/such/route", "200", "SPA"),
            row("/mla-tamagui/_expo/static/js/web/entry.js", "200", "console.log(1)"),
            row("/mla-tamagui/missing.png", "404", ""),
            row("/elsewhere", "404", ""),
            row("/mla-tamagui/../../etc/passwd", "400", ""),
            row("/mla-tamagui/%2e%2e/%2e%2e/etc/passwd", "400", ""),
        ],
        "{served}"
    );
    assert!(served.contains("entry.js => 200 text/javascript") || served.contains("entry.js => 200 application/javascript"), "{served}");

    // When an app source file changes after the build, and when only a test, a doc and a build output change
    w.write("repo/frontend/app/screen.tsx", "export const s = 1");
    let stale = run(&["svc", "--out", &out]);
    // Then the unit is an error that says what the export was built from and what the tree is, and the next step; the harness did not run
    let u = unit(&stale, "svc");
    assert_eq!((stale.exit_code, u["verdict"].as_str(), u["next"].as_str()), (1, Some("error"), Some("wf test prepare --web")), "{}", stale.stdout);
    let note = u["note"].as_str().unwrap();
    assert!(note.contains("is stale") && note.contains("built at commit") && note.contains("crashes the app") && note.contains(last_json(&rebuilt)["inputs"].as_str().unwrap()), "{note}");
    assert!(unit(&stale, "svc")["logTail"].is_null(), "{u}");
    // and rebuilding makes it run again, and an edit that cannot reach the bundle leaves the export current
    let fresh = w.export();
    assert_eq!((last_json(&fresh)["reused"].clone(), pnpm_exports()), (json!(false), 4), "{}", fresh.stdout);
    w.write("repo/frontend/app/tests/screen.test.tsx", "test");
    w.write("repo/frontend/app/NOTES.md", "notes");
    w.write("repo/frontend/mla-tamagui/dist.rejected/index.html", "a rejected export");
    w.write("repo/README.md", "docs");
    assert_eq!((last_json(&w.export())["reused"].clone(), run(&["svc", "--out", &out]).exit_code), (json!(true), 0));

    // When a committed app file is edited after the build, and when a committed library file is deleted
    w.write("repo/.gitignore", "frontend/mla-tamagui/dist/\nfrontend/mla-tamagui/dist.rejected/\n");
    w.commit("the tree so far");
    let is_stale = |what: &str| {
        let r = run(&["svc", "--out", &out]);
        let u = unit(&r, "svc");
        assert_eq!((r.exit_code, u["verdict"].as_str()), (1, Some("error")), "{what}: {}", r.stdout);
        assert!(u["note"].as_str().unwrap().contains("is stale"), "{what}: {u}");
    };
    let rebuilt_now = |what: &str| {
        let r = w.export();
        assert_eq!(last_json(&r)["reused"], json!(false), "{what}: {}", r.stdout);
        assert_eq!(run(&["svc", "--out", &out]).exit_code, 0, "{what}");
    };
    let manifest = w.root.join("repo/frontend/app/package.json");
    std::fs::write(&manifest, std::fs::read_to_string(&manifest).unwrap().replacen("tsc", "tsd", 1)).unwrap();
    // Then the edit makes the export stale (the hash follows content, not names or sizes), and so does the deletion
    is_stale("an edited committed file");
    rebuilt_now("after the edit");
    std::fs::remove_file(w.root.join("repo/libraries/lib/index.ts")).unwrap();
    is_stale("a deleted committed file");
    rebuilt_now("after the deletion");

    // When a library's tracked dist (the app bundles it) and a contracts package the lockfile links the app to are edited
    w.write("repo/frontend/libraries/dsl/dist/index.js", "exports.v = 1");
    w.write("repo/backend/contracts/src/index.ts", "export const v = 1");
    w.write("repo/pnpm-lock.yaml", LOCK);
    w.commit("a built library, the contracts and the lockfile that links the app to them");
    rebuilt_now("after the commit");
    w.write("repo/frontend/libraries/dsl/dist/index.js", "exports.v = 2");
    // Then each makes the export stale; a backend package the app is not linked to does not
    is_stale("a library's tracked dist");
    rebuilt_now("after the dist edit");
    w.write("repo/backend/contracts/src/index.ts", "export const v = 2");
    is_stale("a contracts package the lockfile links the app to");
    rebuilt_now("after the contracts edit");
    w.write("repo/backend/other/src/index.ts", "export const v = 1");
    assert_eq!((last_json(&w.export())["reused"].clone(), run(&["svc", "--out", &out]).exit_code), (json!(true), 0));

    // When the build fails after a good export, and when the caller's export flags differ from the stamp's
    let failed = w.wf(&["test", "prepare", "--web", "--force"], &[("STUB_EXPORT_FAIL", "1".into())]);
    // Then the failure names the build and the command to run by hand, and the old export no longer passes for current
    let f = last_json(&failed);
    assert_eq!((failed.exit_code, f["ok"].clone(), f["next"].as_str()), (1, json!(false), Some("pnpm --filter mla-tamagui run export:web")), "{}", failed.stdout);
    assert!(f["error"].as_str().unwrap().contains("export:web failed"), "{f}");
    assert!(!w.root.join("repo/frontend/mla-tamagui/dist/wf-export.json").exists(), "a failed build left the old stamp");
    let orphan = unit(&run(&["svc", "--out", &out]), "svc");
    assert!(orphan["note"].as_str().unwrap().contains("has no wf-export.json") && orphan["next"] == "wf test prepare --web --force", "{orphan}");
    // and a caller's flag is added to the source maps and rebuilds, and so does going back
    let flagged = last_json(&w.wf(&["test", "prepare", "--web"], &[("EXPO_EXPORT_FLAGS", "--clear".into())]));
    let plain = last_json(&w.export());
    assert_eq!((flagged["reused"].clone(), flagged["flags"].as_str(), plain["reused"].clone(), plain["flags"].as_str()), (json!(false), Some("--clear --source-maps"), json!(false), Some("--source-maps")), "{flagged} {plain}");

    // When Android tests say no `against:` at all
    w.write("e2e/plain.lua", "--[[---\nplatform: android\nfocus: true\n---]]\nlog(1)\n");
    let planned: Value = serde_json::from_str(w.wf(&["test", "plan", "--all", "--json"], &[]).stdout.trim()).unwrap();
    // Then theirs is the update CI published
    assert_eq!(planned["tests"].as_array().unwrap().iter().find(|b| b["name"] == "plain").map(|b| b["against"].clone()), Some(json!("update")), "{planned}");
    std::fs::remove_file(w.path("e2e/plain.lua")).unwrap();

    // When an Android unit runs beside a browser unit in one run, the bucket naming this commit's update
    serve(&bucket, pointer("g-1", &w.git(&["rev-parse", "HEAD"])));
    let mixed = run(&["svc", "typing", "--out", &out]);
    // Then Android loads the update and carries no export; the browser unit carries its own export and no update
    let (typing, svc) = (unit(&mixed, "typing"), unit(&mixed, "svc"));
    let args_of = |u: &Value| std::fs::read_to_string(PathBuf::from(u["dir"].as_str().unwrap()).join("run.log")).unwrap().lines().find_map(|l| l.strip_prefix("args: ")).unwrap().to_string();
    let (android, base) = (args_of(&typing), "exp://u.expo.dev/proj-1/group/g-1/--");
    assert!(android.contains("--env against=update") && android.contains(&format!("--env base={base}")) && !android.contains("127.0.0.1:"), "{android}");
    assert!(typing["export"].is_null() && typing["base"] == base && typing["updateGroup"] == "g-1", "{typing}");
    assert!(svc["export"]["inputs"].is_string() && svc["updateGroup"].is_null(), "{svc}");

    // When a browser test is declared against the dev Metro, one against a target its harness has none of, and an Android test against the dev Metro
    let harness_runs = || std::fs::read_to_string(w.root.join("runner.log")).unwrap().lines().count();
    let started = harness_runs();
    w.write("e2e/old.lua", "--[[---\nplatform: browser\nagainst: metro\nfocus: true\n---]]\nlog(1)\n");
    let old = run(&["old", "--out", &out]);
    w.write("e2e/old.lua", "--[[---\nplatform: android\nagainst: export\nfocus: true\n---]]\nlog(1)\n");
    let wrong = run(&["old", "--out", &out]);
    w.write("e2e/old.lua", "--[[---\nplatform: android\nagainst: metro\nfocus: true\n---]]\nlog(1)\n");
    let gone = run(&["old", "--out", &out]);
    // Then each is refused before any harness starts, naming the header line that fixes it
    let (o, x, g) = (last_json(&old), last_json(&wrong), last_json(&gone));
    assert_eq!((old.exit_code, wrong.exit_code, gone.exit_code, o["ok"].clone()), (2, 2, 2, json!(false)), "{}{}{}", old.stdout, wrong.stdout, gone.stdout);
    assert!(o["error"].as_str().unwrap().contains("against the dev Metro") && o["next"].as_str().unwrap().contains("against: export"), "{o}");
    assert!(x["error"].as_str().unwrap().contains("`against: export` is not a target of the android harness"), "{x}");
    assert!(g["ok"] == json!(false) && g["error"].as_str().unwrap().contains("against the dev Metro") && g["next"].as_str().unwrap().contains("against: update"), "{g}");
    assert_eq!(harness_runs(), started, "a refused test started a harness");
}

/// The pointer CI's publish stores for the branch: `commit`'s group for iOS and Android, opening on /, with the lab's env baked in.
fn pointer(group: &str, commit: &str) -> Value {
    json!({ "published": true, "branch": "dev", "initialRoute": "/", "commit": commit, "group": group, "platform": "all", "platforms": ["ios", "android"],
            "env": ["EXPO_PUBLIC_APP_ENV", "EXPO_PUBLIC_DEVICE_LAB", "EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL", "EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN", "EXPO_PUBLIC_AGENT_BASE_URL"],
            "hosts": ["leetcode-backend-prod", "remote-chat-agent-prod"], "served": { "platform": "android", "status": 200, "s": 1.2 }, "timing": { "exportS": 60.1, "uploadS": 40.2, "servedS": 1.2 } })
}

const POINTER_ROUTE: &str = "GET /o/verification/eas-updates/latest-dev.json";

/// A workspace whose app has an EAS project, `upd` (two units) an Android test against the update beside its browser twin against
/// the export, and a bucket that serves what `serve` gives it as the branch's latest-update pointer.
fn update_workspace(prefix: &str) -> (Workspace, MockServer) {
    let w = Workspace::new(prefix);
    w.write("e2e/upd.lua", "--[[---\nplatform: android, browser\nagainst: android=update, browser=export\ntimeout: 60\nunits: a=1; a=2\nfocus: true\n---]]\nlog(1)\n");
    w.commit_eas_project();
    (w, MockServer::start(None))
}

/// The bucket naming group g-1, published from the workspace's head commit by this push's own publish, and the environment that
/// makes an Android unit read it: what the Android tests of the other workspaces load.
fn publish_update(w: &Workspace) -> (MockServer, Vec<(&'static str, String)>) {
    w.commit_eas_project();
    let bucket = MockServer::start(None);
    serve(&bucket, pointer("g-1", &w.git(&["rev-parse", "HEAD"])));
    let env = vec![("VERIFICATION_ARTIFACTS_READ_URL", format!("http://127.0.0.1:{}/o", bucket.port)), ("WF_EAS_UPDATE_RESULT", "success".to_string())];
    (bucket, env)
}

fn serve(bucket: &MockServer, pointer: Value) {
    bucket.routes(&[(POINTER_ROUTE, MockReply::json(200, pointer))]);
}

/// The `args:` line a unit's (or warm-up's) harness run logged.
fn runner_args(unit: &Value) -> String {
    std::fs::read_to_string(PathBuf::from(unit["dir"].as_str().unwrap()).join("run.log")).unwrap().lines().find_map(|l| l.strip_prefix("args: ")).unwrap_or_default().to_string()
}

#[test]
fn android_units_load_the_update_ci_published() {
    // Given an update test and the bucket naming group g-1 published from this commit by this push's own publish
    let (w, bucket) = update_workspace("wf-flow-update-");
    let (head, behind) = (w.git(&["rev-parse", "HEAD"]), w.git(&["rev-parse", "HEAD~1"]));
    serve(&bucket, pointer("g-1", &head));
    let read = format!("http://127.0.0.1:{}/o", bucket.port);
    let env = |result: &str, extra: &[(&'static str, &str)]| -> Vec<(&'static str, String)> {
        let mut e = vec![
            ("VERIFICATION_ARTIFACTS_READ_URL", read.clone()),
            ("WF_EAS_UPDATE_RESULT", result.to_string()),
        ];
        e.extend(extra.iter().map(|(k, v)| (*k, v.to_string())));
        e
    };
    let out = w.path("out");
    let run = |result: &str, extra: &[(&'static str, &str)]| w.wf(&["test", "run", "upd", "--platform", "android", "--out", &out], &env(result, extra));
    let base = "exp://u.expo.dev/proj-1/group/g-1/--";

    // When the Android entry of the test runs
    let r = run("success", &[]);
    // Then each unit's harness got the update as its base (and the target as `against`), and the one cold start opened it bare
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let v = Workspace::summary(&r);
    let units = v["units"].as_array().unwrap();
    assert_eq!(units.iter().map(|u| u["args"][0].as_str().unwrap()).collect::<Vec<_>>(), ["a=1", "a=2"], "{v}");
    for u in units {
        let args = runner_args(u);
        assert!(args.starts_with("android test e2e/upd.lua ") && args.contains("--env against=update") && args.contains(&format!("--env base={base}")), "{args}");
        assert!(!args.contains("metro-runner") && !args.contains("open="), "{args}");
        assert_eq!((u["updateGroup"].as_str(), u["updateCommit"].as_str(), u["base"].as_str()), (Some("g-1"), Some(head.as_str()), Some(base)), "{u}");
        assert!(["metroCommit", "metroWarmS", "metroBundle"].iter().all(|k| u.get(*k).is_none()), "{u}");
    }
    let warms = v["warms"].as_array().unwrap();
    assert_eq!(warms.len(), 1, "{v}");
    let cold = runner_args(&warms[0]);
    assert!(cold.contains("android test e2e/warm-app.lua") && cold.contains("--cold-start") && cold.contains("--env against=update") && cold.contains(&format!("--env base={base}")), "{cold}");
    assert!(cold.split_whitespace().any(|t| t == "open=exp://u.expo.dev/proj-1/group/g-1"), "an update opens bare, without /--/: {cold}");
    assert_eq!((warms[0]["updateGroup"].as_str(), warms[0].get("metroBundle").is_none()), (Some("g-1"), true), "{v}");

    // When the same test is planned, and when its browser entry runs
    let planned: Value = serde_json::from_str(w.wf(&["test", "plan", "--all", "--json"], &[]).stdout.trim()).unwrap();
    let targets: Vec<(&str, &str)> = planned["tests"].as_array().unwrap().iter().filter(|b| b["name"] == "upd").map(|b| (b["platform"].as_str().unwrap(), b["against"].as_str().unwrap())).collect();
    // Then each harness has its own target: an update has no web build
    assert_eq!(targets, [("android", "update"), ("browser", "export")], "{planned}");
    w.write("e2e/web-update.lua", "--[[---\nplatform: android, browser\nagainst: update\nfocus: true\n---]]\nlog(1)\n");
    let wrong = w.wf(&["test", "run", "web-update", "--platform", "browser", "--out", &out], &[]);
    assert_eq!(wrong.exit_code, 2, "{}", wrong.stdout);
    let wrong = last_json(&wrong);
    assert!(wrong["error"].as_str().unwrap().contains("an update has no web build") && wrong["next"].as_str().unwrap().contains("against: android=update, browser=export"), "{wrong}");
    std::fs::remove_file(w.path("e2e/web-update.lua")).unwrap();

    // Given the harness has not run since
    let started = std::fs::read_to_string(w.root.join("runner.log")).unwrap().lines().count();
    // When an update the run cannot treat as this commit's is what the bucket names, or this push's publish did not succeed
    let old_update = pointer("g-1", &behind);
    let app_changed = r#"[{"name":"mla-tamagui"}]"#;
    let mut no_android = pointer("g-1", &head);
    no_android["platforms"] = json!(["ios"]);
    let mut routed = pointer("g-1", &head);
    routed["initialRoute"] = json!("/leetcode/two-sum");
    let mut no_commit = pointer("g-1", &head);
    no_commit["commit"] = json!("");
    let mut bad_group = pointer("g-1", &head);
    bad_group["group"] = json!("g;x");
    let mut moved = pointer("g-1", &head);
    moved["hosts"] = json!(["leetcode-backend-prod-1", "remote-chat-agent-prod"]);
    let mut unserved = pointer("g-1", &head);
    unserved["served"] = json!({ "platform": "android", "error": "not served within 120 s, last answer: HTTP 404" });
    // the manifest EAS answers a device from: nothing is served here, so a pointer whose publish did not see it served stays refused
    let updates = MockServer::start(None);
    let updates_url = format!("http://127.0.0.1:{}", updates.port);
    let without = |name: &str| {
        let mut p = pointer("g-1", &head);
        p["env"] = json!(p["env"].as_array().unwrap().iter().filter(|n| *n != name).collect::<Vec<_>>());
        p
    };
    let pnpm_failed = |out: &'static str| vec![("STUB_LIST_FAIL", "1"), ("STUB_LIST_FAIL_OUT", out)];
    let publish = ("re-run this CI run", "eas-update job");
    let unknown = ("git fetch --unshallow", "pnpm install");
    type Case<'a> = (String, &'static str, Value, Vec<(&'static str, &'a str)>, String, (&'static str, &'static str));
    let mut cases: Vec<Case> = vec![
        ("the publish failed".into(), "failure", pointer("g-1", &head), vec![], "this push's EAS publish ended failure, so latest-dev.json still names the last one (group g-1 from commit".into(), publish),
        ("the publish was cancelled".into(), "cancelled", pointer("g-1", &head), vec![], "this push's EAS publish ended cancelled".into(), publish),
        ("the publish succeeded but the pointer is behind".into(), "success", old_update.clone(), vec![], format!("latest-dev.json names commit {behind}, but this push published {head}"), publish),
        ("no publish and the app changed since the update".into(), "skipped", old_update.clone(), vec![("STUB_AFFECTED", app_changed)], "the app changed since: mla-tamagui changed since".into(), publish),
        ("a dispatch (no eas-update job) and the app changed since the update".into(), "", old_update.clone(), vec![("STUB_AFFECTED", app_changed)], "this push published none (no eas-update job ran)".into(), publish),
        ("an update without Android".into(), "success", no_android, vec![], "latest-dev.json names an update without Android: group g-1 from commit".into(), publish),
        ("an update that opens on another route".into(), "success", routed, vec![], "opens on \"/leetcode/two-sum\", not on /".into(), publish),
        ("a pointer with no commit".into(), "success", no_commit, vec![], "latest-dev.json names no usable update".into(), publish),
        ("a group that is no manifest id".into(), "success", bad_group, vec![], "latest-dev.json names no usable update".into(), publish),
        ("a baked backend name the tailnet has since moved".into(), "success", moved.clone(), vec![], "the tailnet name leetcode-backend-prod-1 the update baked in is now leetcode-backend-prod".into(), publish),
        ("an update EAS does not serve".into(), "success", unserved.clone(), vec![("WF_EAS_UPDATES_URL", updates_url.as_str()), ("WF_EAS_SERVED_TIMEOUT_S", "1")], "EAS does not serve group g-1 from commit".into(), publish),
        // pnpm cannot say what changed: that is never "nothing changed"
        ("pnpm failing with an error object".into(), "skipped", old_update.clone(), pnpm_failed(r#"{"error":{"code":"ERR_PNPM_FILTER_CHANGED"}}"#), format!("cannot tell whether the update from commit {behind} carries this commit's app (eas-update: skipped): pnpm exited 1 listing the packages changed since {behind}: {{\"error\":{{\"code\":\"ERR_PNPM_FILTER_CHANGED\""), unknown),
        ("pnpm failing without a word".into(), "", old_update.clone(), pnpm_failed(""), "(no eas-update job ran): pnpm exited 1 listing the packages".into(), unknown),
        ("pnpm printing no package list".into(), "skipped", old_update.clone(), vec![("STUB_AFFECTED", r#"{"error":{"code":"E"}}"#)], "pnpm list printed no package list".into(), unknown),
    ];
    // the update must carry each of what the Android tests need
    for name in ["EXPO_PUBLIC_DEVICE_LAB", "EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL", "EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN", "EXPO_PUBLIC_AGENT_BASE_URL"] {
        cases.push((format!("an update without {name}"), "success", without(name), vec![], format!("the update did not bake in {name}"), publish));
    }
    for (what, result, p, extra, says, (next_has, next_also)) in cases {
        serve(&bucket, p);
        let r = run(result, &extra);
        // Then every unit is an error naming why and the step that fixes it, the harness never starts, and nothing is started cold
        assert_eq!(r.exit_code, 1, "{what}: {}{}", r.stdout, r.stderr);
        let v = Workspace::summary(&r);
        let notes: Vec<(&str, &str)> = v["units"].as_array().unwrap().iter().map(|u| (u["verdict"].as_str().unwrap(), u["note"].as_str().unwrap())).collect();
        assert!(notes.len() == 2 && notes.iter().all(|(verdict, note)| *verdict == "error" && note.contains(says.as_str())), "{what}: {notes:?}");
        let next = v["units"][0]["next"].as_str().unwrap_or_default();
        assert!(next.contains(next_has) && next.contains(next_also) && !next.contains("--pointer"), "{what}: {next}");
        assert!(v["warms"].as_array().unwrap().is_empty(), "{what}: {v}");
        assert_eq!(std::fs::read_to_string(w.root.join("runner.log")).unwrap().lines().count(), started, "{what}");
    }
    // and an app without an EAS project has no update to open
    let config = "repo/frontend/mla-tamagui/app.config.js";
    let with_project = std::fs::read_to_string(w.path(config)).unwrap();
    w.write(config, "module.exports = { expo: { runtimeVersion: 'exposdk:57.0.0' } };\n");
    serve(&bucket, pointer("g-1", &head));
    let no_project = run("success", &[]);
    assert!(Workspace::summary(&no_project)["units"][0]["note"].as_str().unwrap().contains("app.config.js names no projectId"), "{}", no_project.stdout);
    w.write(config, &with_project);
    // and with no pointer in the bucket, or a bucket that is not there
    bucket.routes(&[]);
    let gone = run("success", &[]);
    assert_eq!(gone.exit_code, 1);
    assert!(Workspace::summary(&gone)["units"][0]["note"].as_str().unwrap().contains("cannot read the latest update (eas-update: success)"), "{}", gone.stdout);
    assert_eq!(std::fs::read_to_string(w.root.join("runner.log")).unwrap().lines().count(), started);
    // and a failed publish is reported as that, whether or not the pointer can be read
    let failed = run("failure", &[]);
    assert_eq!(failed.exit_code, 1);
    assert!(Workspace::summary(&failed)["units"][0]["note"].as_str().unwrap().starts_with("this push's EAS publish ended failure, so latest-dev.json still names the last one:"), "{}", failed.stdout);

    // When the publish was skipped (nothing the app bundles changed) and the pointer names an older update, pnpm finding the app unchanged since it
    serve(&bucket, old_update);
    w.s.forget("pnpm");
    let skipped = run("skipped", &[]);
    let dispatched = run("", &[]);
    // Then that update is this commit's, its units run, and the question to pnpm was the planner's: the same ignore patterns
    for r in [&skipped, &dispatched] {
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        assert_eq!(Workspace::summary(r)["units"].as_array().unwrap().iter().map(|u| u["updateGroup"].as_str().unwrap()).collect::<Vec<_>>(), ["g-1", "g-1"]);
    }
    let pnpm_log = std::fs::read_to_string(w.s.bin_dir.join("pnpm.log")).unwrap_or_default();
    assert!(pnpm_log.contains(&format!("--filter ...[{behind}] --changed-files-ignore-pattern=**/__tests__/** --changed-files-ignore-pattern=**/*.test.* --changed-files-ignore-pattern=**/*.md list --depth -1 --json")), "{pnpm_log}");
    // and an update published from this very commit is current without asking pnpm
    serve(&bucket, pointer("g-1", &head));
    w.s.forget("pnpm");
    assert_eq!(run("skipped", &[]).exit_code, 0);
    assert!(!std::fs::read_to_string(w.s.bin_dir.join("pnpm.log")).unwrap_or_default().contains("list --depth"));

    // When a baked backend name has moved but the tailnet still answers it, and when the publish saw no 200 from EAS but EAS serves the group now
    serve(&bucket, moved);
    let answered = run("success", &[("WF_TEST_TAILNET_NAMES", "leetcode-backend-prod-1,remote-chat-agent-prod")]);
    updates.routes(&[("GET /proj-1/group/g-1", MockReply::json(200, json!({})))]);
    serve(&bucket, unserved);
    let late = run("success", &[("WF_EAS_UPDATES_URL", updates_url.as_str()), ("WF_EAS_SERVED_TIMEOUT_S", "1")]);
    // Then the units run: only a name that proves drift, and a group EAS does not serve, refuse an update
    assert_eq!((answered.exit_code, late.exit_code), (0, 0), "{}{}{}{}", answered.stdout, answered.stderr, late.stdout, late.stderr);

    // When the bucket answers 503 once before the pointer, then 503 three times, then 404
    serve(&bucket, pointer("g-1", &head));
    let seen_by_bucket = |before: usize| bucket.requests().len() - before;
    let before = bucket.requests().len();
    bucket.queue(vec![MockReply::json(503, json!({})), MockReply::json(200, pointer("g-1", &head))], false);
    let flaky = run("success", &[]);
    let tries = seen_by_bucket(before);
    let before = bucket.requests().len();
    bucket.queue(vec![MockReply::json(503, json!({})); 3], false);
    let down = run("success", &[]);
    let down_tries = seen_by_bucket(before);
    bucket.queue(vec![], false);
    bucket.routes(&[]);
    let before = bucket.requests().len();
    let absent = run("success", &[]);
    let absent_tries = seen_by_bucket(before);
    // Then a blip is tried again and the run goes on, an outage is an error naming the tries, and a 404 (no pointer yet) is asked once
    assert_eq!((flaky.exit_code, tries), (0, 2), "{}{}", flaky.stdout, flaky.stderr);
    assert_eq!((down.exit_code, down_tries), (1, 3), "{}", down.stdout);
    assert!(Workspace::summary(&down)["units"][0]["note"].as_str().unwrap().contains("answered HTTP 503 (3 tries)"), "{}", down.stdout);
    assert_eq!((absent.exit_code, absent_tries), (1, 1), "{}", absent.stdout);

    // When a logpoint is asked of an Android run, which has no debugger
    serve(&bucket, pointer("g-1", &head));
    let harness_runs = || std::fs::read_to_string(w.root.join("runner.log")).unwrap().lines().count();
    let started = harness_runs();
    let logpoint = w.wf(&["test", "run", "upd", "--platform", "android", "--out", &out, "--", "--logpoint", "features/x.tsx:1"], &env("success", &[]));
    // Then it is refused before anything starts, saying why and what to do
    let refused = last_json(&logpoint);
    assert_eq!((logpoint.exit_code, refused["ok"].clone()), (2, json!(false)), "{}", logpoint.stdout);
    assert!(refused["error"].as_str().unwrap().contains("--logpoint") && refused["error"].as_str().unwrap().contains("upd runs on Android") && refused["error"].as_str().unwrap().contains("browser only") && refused["next"].as_str().unwrap().contains("--platform browser"), "{refused}");
    assert_eq!(harness_runs(), started);
    // while the same flag on the test's browser entry is passed on to its harness
    assert_eq!(w.export().exit_code, 0);
    let browser = w.wf(&["test", "run", "upd", "--platform", "browser", "--out", &out, "--", "--logpoint", "features/x.tsx:1"], &env("success", &[]));
    assert_eq!(browser.exit_code, 0, "{}{}", browser.stdout, browser.stderr);
    let args = runner_args(&Workspace::summary(&browser)["units"][0]);
    assert!(args.starts_with("browser test e2e/upd.lua ") && args.contains("--logpoint features/x.tsx:1"), "{args}");

    // When an Android test is declared against the dev Metro
    w.write("e2e/old.lua", "--[[---\nplatform: android\nagainst: metro\nfocus: true\n---]]\nlog(1)\n");
    let started = harness_runs();
    let old = w.wf(&["test", "run", "old", "--platform", "android", "--out", &out], &env("success", &[]));
    // Then it is refused before any harness starts, naming the header line that fixes it
    let refused = last_json(&old);
    assert_eq!((old.exit_code, refused["ok"].clone()), (2, json!(false)), "{}", old.stdout);
    assert!(refused["error"].as_str().unwrap().contains("against the dev Metro") && refused["next"].as_str().unwrap().contains("set `against: update`"), "{refused}");
    assert_eq!(harness_runs(), started);
    std::fs::remove_file(w.path("e2e/old.lua")).unwrap();
}

#[test]
fn the_cold_start_is_keyed_on_the_update_group_the_units_run_against() {
    // Given two update tests sharing one device state file, as the tests of one Android job do
    let (w, bucket) = update_workspace("wf-flow-warm-key-");
    serve(&bucket, pointer("g-1", &w.git(&["rev-parse", "HEAD"])));
    let (state, out, read) = (w.path("warm.json"), w.path("out"), format!("http://127.0.0.1:{}/o", bucket.port));
    let run = |tests: &[&str], extra: &[(&str, &str)]| -> Value {
        let mut args = vec!["test", "run"];
        args.extend_from_slice(tests);
        args.extend(["--platform", "android", "--out", &out]);
        let mut env = vec![("WF_E2E_STATE", state.as_str()), ("VERIFICATION_ARTIFACTS_READ_URL", read.as_str()), ("WF_EAS_UPDATE_RESULT", "success")];
        env.extend_from_slice(extra);
        let envs: Vec<(&str, String)> = env.iter().map(|(k, v)| (*k, v.to_string())).collect();
        Workspace::summary(&w.wf(&args, &envs))
    };
    let warms = |v: &Value| v["warms"].as_array().unwrap().len();
    let at = || serde_json::from_str::<Value>(&std::fs::read_to_string(&state).unwrap()).unwrap()["at"].clone();

    // When one test runs, then the other, then the other again, all against group g-1
    let first = run(&["typing"], &[]);
    let second = run(&["upd"], &[]);
    let again = run(&["upd"], &[]);
    // Then the first starts the app cold once, opening the update bare, and the next two reuse the app it left
    assert_eq!((warms(&first), warms(&second), warms(&again)), (1, 0, 0), "{first} {second} {again}");
    assert!(runner_args(&first["warms"][0]).split_whitespace().any(|t| t == "open=exp://u.expo.dev/proj-1/group/g-1"), "{first}");
    assert_eq!((first["warms"][0]["updateGroup"].as_str(), again["ok"].clone()), (Some("g-1"), json!(true)));
    assert_eq!(at(), json!("update:g-1"));

    // When CI publishes another update (group g-2) and a test runs, then the other
    serve(&bucket, pointer("g-2", &w.git(&["rev-parse", "HEAD"])));
    let moved = run(&["upd"], &[]);
    let same = run(&["typing"], &[]);
    // Then the new group starts the app cold, and the other test against it reuses that app
    assert_eq!((warms(&moved), warms(&same)), (1, 0), "{moved} {same}");
    assert_eq!((moved["warms"][0]["updateGroup"].as_str(), at()), (Some("g-2"), json!("update:g-2")));

    // When a third update's cold start fails, in one run of both tests
    serve(&bucket, pointer("g-3", &w.git(&["rev-parse", "HEAD"])));
    let broken = run(&["upd", "typing"], &[("STUB_WARM_FAIL_ON", "update")]);
    // Then it is tried once, and every unit is an error that says the app did not start
    let verdicts: Vec<(String, String)> = broken["units"].as_array().unwrap().iter().map(|u| (u["test"].as_str().unwrap().to_string(), u["verdict"].as_str().unwrap().to_string())).collect();
    assert_eq!(verdicts, [("upd", "error"), ("upd", "error"), ("typing", "error"), ("typing", "error")].map(|(a, b)| (a.to_string(), b.to_string())), "{broken}");
    assert_eq!(warms(&broken), 1, "{broken}");
    assert!(broken["units"].as_array().unwrap().iter().all(|u| u["note"].as_str().unwrap().starts_with("the app did not start cold")), "{broken}");
    assert_eq!(at(), json!("update:g-3"));
    // and the next run does not take that app for a good one: it starts it cold again, and passes
    let healed = run(&["typing"], &[]);
    assert_eq!((warms(&healed), healed["ok"].clone()), (1, json!(true)), "{healed}");
}

fn index_run(id: &str, chunk: &str, at: &str, verdict: &str, find_p95: u64) -> Value {
    json!({ "runId": id, "harness": "android", "suiteOrRunbook": "e2e/voice-edit.lua", "sha": "abc1234", "verdict": verdict, "durationS": 58.0,
            "publishedAt": at, "args": { "chunk": chunk, "out": "/tmp/x" }, "readBack": true, "wallS": 58.0, "recordings": 1, "warnings": 0,
            "timing": { "phases": { "setup": 9.0 }, "verbs": [{ "verb": "find", "calls": 30, "s": 20.0, "p95Ms": find_p95 }] } })
}

fn percent_decoded(v: &str) -> String {
    let b = v.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            out.push(u8::from_str_radix(&v[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

#[test]
fn published_runs_are_found_by_a_query_over_the_index_and_the_table() {
    // Given a focused test with three units, an index with two runs of chunk 1 and one of chunk 2, and a table holding chunk 2's row
    // and an older run the index dropped
    let s = SlotSandbox::new("wf-flow-results-");
    s.file("e2e/voice-edit.lua", "--[[---\nplatform: android\nagainst: update\nunits: chunk=1; chunk=2; chunk=3\nfocus: true\n---]]\n");
    let bucket = MockServer::start(None);
    bucket.queue(vec![MockReply::json(200, json!({ "schema": 1, "runs": [index_run("r3", "2", "2026-10-16T12:00:00.000Z", "pass", 1500), index_run("r2", "1", "2026-10-16T11:00:00.000Z", "pass", 800), index_run("r1", "1", "2026-10-16T10:00:00.000Z", "fail", 1200)] }))], true);
    let table = MockServer::start(None);
    table.queue(vec![MockReply::json(200, json!({ "items": [{ "run_id": "r3", "ts": "2026-10-16T12:00:01Z", "ran_on": "vm", "suite_or_runbook": "e2e/voice-edit.lua" }, { "run_id": "r0", "ts": "2026-10-01T00:00:00Z", "suite_or_runbook": "e2e/voice-edit.lua", "harness": "android", "verdict": "fail" }] }))], true);
    let read = format!("http://127.0.0.1:{}/o", bucket.port);
    let ords = format!("http://127.0.0.1:{}/ords/health_rpt/test_runs/", table.port);
    let env = [("VERIFICATION_ARTIFACTS_READ_URL", read.as_str()), ("TEST_RUNS_ORDS_BASE_URL", ords.as_str()), ("HEALTH_REPORTS_ORDS_PASSWORD", "pw")];
    let query = |q: &str| {
        table.clear();
        let r = s.run(&["test", "results", "--query", q, "--json"], &env);
        assert_eq!(r.exit_code, 0, "{q}: {}{}", r.stdout, r.stderr);
        serde_json::from_str::<Value>(r.stdout.trim()).unwrap()
    };
    let ids = |v: &Value| v["runs"].as_array().unwrap().iter().map(|r| (r["runId"].as_str().map(str::to_string), r["unit"].as_str().map(str::to_string))).collect::<Vec<_>>();
    let some = |id: Option<&str>, unit: &str| (id.map(str::to_string), Some(unit.to_string()));

    // When the newest passing run of every voice-edit unit since 2 Oct is asked for
    let newest = query("test=voice-edit platform=android verdict=pass newest units since=2026-10-02");
    // Then chunk 2 is r3 joined to its row, chunk 1 is r2 (the failed r1 is filtered), chunk 3 is missing, and r0 is too old
    assert_eq!((newest["sources"]["index"].as_str(), newest["sources"]["table"].as_str()), (Some("ok"), Some("ok")), "{newest}");
    assert_eq!(ids(&newest), [some(Some("r3"), "chunk=2"), some(Some("r2"), "chunk=1"), some(None, "chunk=3")], "{newest}");
    assert_eq!((newest["runs"][0]["inTable"].as_bool(), newest["runs"][0]["ranOn"].as_str(), newest["runs"][0]["byVerb"][0]["verb"].as_str()), (Some(true), Some("vm"), Some("find")));
    assert_eq!(newest["runs"][2]["missing"], true);
    // and the table got the equality terms and the window as its filter, with the stored credentials
    let q = table.requests().into_iter().find(|q| q.method == "GET").expect("a table read");
    let filter: Value = serde_json::from_str(&percent_decoded(q.url.split("q=").nth(1).and_then(|v| v.split('&').next()).unwrap())).unwrap();
    assert_eq!((filter["verdict"].as_str(), filter["harness"].as_str(), filter["ts"]["$gt"]["$date"].as_str(), filter["suite_or_runbook"]["$like"].as_str()), (Some("pass"), Some("android"), Some("2026-10-02T00:00:00Z"), Some("%voice-edit%")), "{filter}");
    assert!(q.auth.is_some_and(|a| a.starts_with("Basic ")));

    // When runs are selected by a verb's percentile and a phase, by an --arg term, by a sha prefix and with a limit
    let slow = query("timing.find.p95>1 timing.setup=9");
    let renamed = query("timing.get_element.p95>1 timing.setup=9");
    let chunk = query("chunk=2");
    let by_sha = query("sha=abc limit=1");
    let sha_filter: Value = serde_json::from_str(&percent_decoded(table.requests()[0].url.split("q=").nth(1).and_then(|v| v.split('&').next()).unwrap())).unwrap();
    // Then the comparisons read the timing, the arg picks the unit, and the sha is a prefix match for the table
    assert_eq!(ids(&slow).iter().map(|(id, _)| id.clone().unwrap()).collect::<Vec<_>>(), ["r3", "r1"], "{slow}");
    // and a verb named by the command that replaced today's name reads the same row
    assert_eq!(ids(&renamed), ids(&slow), "{renamed}");
    assert_eq!(ids(&chunk).iter().map(|(id, _)| id.clone().unwrap()).collect::<Vec<_>>(), ["r3"], "{chunk}");
    assert_eq!(by_sha["runs"].as_array().unwrap().len(), 1, "{by_sha}");
    assert_eq!(sha_filter["sha"]["$like"], "abc%", "{sha_filter}");

    // When the bucket has no index and the table is off
    bucket.queue(vec![MockReply::text(404, "not found")], true);
    let r = s.run(&["test", "results", "--query", "durationS>60", "--json"], &[("VERIFICATION_ARTIFACTS_READ_URL", read.as_str())]);
    // Then it exits 0 with no runs and names both sources' state
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert!(v["sources"]["index"] == "absent" && v["sources"]["table"].as_str().unwrap().starts_with("disabled") && v["runs"] == json!([]), "{v}");

    // When a term has no operator, or a window is not a date or a span
    for bad in ["voice-edit", "since=soon"] {
        let r = s.run(&["test", "results", "--query", bad], &[]);
        // Then it is refused with an example
        assert_eq!(r.exit_code, 2, "{bad}: {}{}", r.stdout, r.stderr);
        let v: Value = serde_json::from_str(r.stdout.trim()).unwrap();
        assert!(v["error"].as_str().is_some() && v["next"].as_str().unwrap().contains("wf test results --query"), "{v}");
    }
}
