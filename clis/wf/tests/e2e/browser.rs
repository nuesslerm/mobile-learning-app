//! `wf check --visual` in a real Chromium: the vision tier screenshots the routes a change touched and judges them against baselines.
//! `wf browser` itself is `clis/wf-harness`.
use crate::support::{run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn chromium_path() -> Option<PathBuf> {
    let browsers = wf_harness::testing::browsers_dir();
    let mut dirs: Vec<String> = std::fs::read_dir(&browsers).ok()?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|d| d.starts_with("chromium-")).collect();
    dirs.sort();
    dirs.reverse();
    dirs.iter().flat_map(|d| ["chrome-linux/chrome", "chrome-linux64/chrome"].map(|rel| PathBuf::from(&browsers).join(d).join(rel))).find(|p| p.exists())
}


fn page(body: &str) -> String {
    format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body style=\"margin:0;background:#fff\">{body}</body></html>")
}

fn stub_bin(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::Builder::new().prefix("wf-flow-visual-bin-").tempdir().unwrap();
    for (name, body) in files {
        crate::support::create_sh_stub(&dir.path().join(name), body);
    }
    dir
}

#[test]
fn the_visual_tier_screenshots_changed_routes_and_judges_them_against_baselines() {
    assert!(chromium_path().is_some(), "no Chromium under PLAYWRIGHT_BROWSERS_PATH (default /opt/pw-browsers)");
    // Given a repo with three routes and a web export of them
    let dir = tempfile::Builder::new().prefix("wf-flow-visual-").tempdir().unwrap();
    let root = dir.path();
    assert!(Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(root).status().unwrap().success());
    let (app, dist, work) = (root.join("frontend/mla-tamagui/app"), root.join("dist"), root.join("work"));
    std::fs::create_dir_all(&app).unwrap();
    std::fs::create_dir_all(&dist).unwrap();
    for (name, body) in [("ok", "export default function Ok() {}"), ("broken", "export default function B() {}"), ("empty", "export default function E() {}")] {
        std::fs::write(app.join(format!("{name}.tsx")), body).unwrap();
    }
    std::fs::write(dist.join("index.html"), page("<p>home</p>")).unwrap();
    std::fs::write(dist.join("ok.html"), page("<h1 style=\"font:48px sans-serif;padding:24px\">Hello visual tier</h1><div style=\"height:200px;background:#2a6\"></div>")).unwrap();
    std::fs::write(dist.join("broken.html"), page("<h1 style=\"font:48px sans-serif\">half rendered</h1><script>console.error(\"kaboom from broken route\")</script>")).unwrap();
    std::fs::write(dist.join("empty.html"), page("")).unwrap();
    let (ok, broken) = ("frontend/mla-tamagui/app/ok.tsx", "frontend/mla-tamagui/app/broken.tsx");
    let visual = |args: &[&str], env: &[(&str, String)], bin: Option<&Path>| {
        let mut opts = RunOpts::default().cwd(root).timeout(Duration::from_secs(120));
        for (k, v) in env {
            opts = opts.env(k, v);
        }
        if let Some(b) = bin {
            opts = opts.bin_dir(b);
        }
        run_wf(&[&["check", "--visual"][..], args].concat(), &opts)
    };
    let common = |out: &str| vec!["--export-dir".to_string(), dist.display().to_string(), "--out-dir".to_string(), work.join(out).display().to_string(), "--baseline-dir".to_string(), work.join("baselines").display().to_string()];
    let run = |files: &str, out: &str, extra: &[&str], env: &[(&str, String)], bin: Option<&Path>| {
        let common = common(out);
        let mut args: Vec<&str> = vec!["--files", files];
        args.extend(common.iter().map(String::as_str));
        args.extend_from_slice(extra);
        visual(&args, env, bin)
    };
    let tier = |r: &RunResult| -> Value { serde_json::from_str(&r.stdout).unwrap_or_else(|e| panic!("stdout {:?}: {e}\n{}", r.stdout, r.stderr)) };
    let detail = |r: &RunResult| tier(r)["items"][0]["detail"].as_str().unwrap_or_default().to_string();

    // When a clean route and one that logs a console error run with --update-baselines
    let first = run(&format!("{ok},{broken}"), "out", &["--update-baselines"], &[], None);
    let t = tier(&first);
    let item = |name: &str| t["items"].as_array().unwrap().iter().find(|i| i["name"] == name).cloned().unwrap_or(Value::Null);
    // Then only the clean one passes and only its baseline is written, both screenshotted
    assert_eq!((first.exit_code, t["tier"].clone(), t["status"].clone()), (1, json!(2), json!("fail")), "{}", first.stderr);
    assert_eq!((item("/ok")["status"].clone(), item("/broken")["status"].clone()), (json!("pass"), json!("fail")));
    assert!(item("/broken")["detail"].as_str().unwrap().contains("kaboom from broken route"));
    assert!(work.join("out/ok.png").exists() && work.join("out/broken.png").exists() && work.join("baselines/ok.png").exists() && !work.join("baselines/broken.png").exists());

    // When the route runs again against its baseline, and against a corrupt one
    let again = run(ok, "out2", &[], &[], None);
    std::fs::copy(work.join("baselines/ok.png"), work.join("ok.png.keep")).unwrap();
    std::fs::write(work.join("baselines/ok.png"), "not a png").unwrap();
    let corrupt = run(ok, "out3", &[], &[], None);
    std::fs::rename(work.join("ok.png.keep"), work.join("baselines/ok.png")).unwrap();
    // Then it passes with a zero diff, and the corrupt baseline fails the route
    assert!(again.exit_code == 0 && detail(&again).contains("diff 0.000% vs baseline"), "{}", again.stdout);
    assert!(corrupt.exit_code == 1 && detail(&corrupt).contains("diff could not be computed"), "{}", corrupt.stdout);

    // When a route renders nothing, and when no frontend file changed
    let blank = run("frontend/mla-tamagui/app/empty.tsx", "out4", &[], &[], None);
    let skipped = run("README.md", "out5", &[], &[], None);
    // Then the blank route fails and the tier is skipped
    assert!(blank.exit_code == 1 && detail(&blank).contains("blank"), "{}", blank.stdout);
    assert_eq!((skipped.exit_code, tier(&skipped)["status"].clone(), tier(&skipped)["items"].clone()), (0, json!("skip"), json!([])));

    // When the export is missing, no files are given, or the baseline store is unknown
    let no_export = visual(&["--files", ok, "--export-dir", &root.join("nope").display().to_string(), "--out-dir", &work.join("out6").display().to_string()], &[], None);
    let no_files = visual(&[], &[], None);
    let store = visual(&["--files", ok, "--baseline-store", "gcs"], &[], None);
    // Then each exits 2 saying how to fix it
    assert!(no_export.exit_code == 2 && no_export.stderr.contains("no web export at") && no_export.stderr.contains("export:web"), "{}", no_export.stderr);
    assert!(no_files.exit_code == 2 && no_files.stderr.contains("--files is required") && no_files.stderr.contains("usage:"), "{}", no_files.stderr);
    assert!(store.exit_code == 2 && store.stderr.contains("--baseline-store must be fs or oci"), "{}", store.stderr);

    // Given a ladder of three vision judges across two pools, an acceptance note, and the route changed since its baseline
    std::fs::create_dir_all(root.join("libraries/rca-core/plugins/ladder/data")).unwrap();
    std::fs::write(root.join("libraries/rca-core/plugins/ladder/data/model-ladders.json"), json!({ "models": { "m-a": { "pool": "p1" }, "m-b": { "pool": "p2" }, "m-c": { "pool": "p1" } }, "ladders": { "judge:vision": ["m-a", "m-c", "m-b"] } }).to_string()).unwrap();
    std::fs::write(work.join("acceptance.md"), "The heading is green.").unwrap();
    std::fs::write(dist.join("ok.html"), page("<h1 style=\"font:48px sans-serif;padding:24px;color:#c00\">Changed heading</h1><div style=\"height:400px;background:#06a\"></div>")).unwrap();
    let rca = wf_testkit::MockServer::start(None);
    let verdict = |text: &str| {
        let attempt = json!({ "index": 0, "choice": { "model": "m-a" }, "verdict": { "ok": true } });
        wf_testkit::MockReply::json(200, json!({ "ok": true, "output": { "text": text, "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt] }))
    };
    let judge_env = vec![
        ("VITEST", "true".to_string()),
        ("SECRETS_BACKEND", "macos".to_string()),
        ("SECRETS_LOG_BETTERSTACK", "false".to_string()),
        ("OPENCODE_DISPATCH_RUN_LOG", work.join("runs.jsonl").display().to_string()),
        ("OPENCODE_POOL_LEDGER", work.join("pool.json").display().to_string()),
        // The judges run on the rca service, here a local listener
        ("WF_TAILNET_SOCKS", String::new()),
        ("RCA_HOST", "127.0.0.1".to_string()),
        ("RCA_PORT", rca.port.to_string()),
        ("CHAT_AGENT_API_SECRET", "s3cret".to_string()),
        ("XDG_CACHE_HOME", work.join("cache").display().to_string()),
    ];
    let acceptance = work.join("acceptance.md").display().to_string();
    // When judges from different pools both say FAIL, when one says PASS, and when there is no acceptance
    rca.routes(&[("POST /runs", verdict("FAIL because"))]);
    let violated = run(ok, "judged", &["--acceptance", &acceptance], &judge_env, None);
    let sent = rca.requests();
    rca.clear();
    rca.routes(&[("POST /runs", verdict("PASS because"))]);
    let disagree = run(ok, "judged2", &["--acceptance", &acceptance], &judge_env, None);
    let unjudged = run(ok, "judged3", &[], &[], None);
    // Then each judge was sent the screenshot as a staged file the prompt names, two agreeing judges fail the route naming the verdicts, a disagreement passes with a note, and no acceptance skips them
    let body: Value = serde_json::from_str(&sent[0].body).unwrap();
    assert!(body["files"][0]["path"].as_str().is_some_and(|p| p.starts_with("screenshots/")) && body["files"][0]["contentBase64"].is_string() && body["prompt"].as_str().unwrap().contains("screenshots/"), "{body}");
    assert!(violated.exit_code == 1 && detail(&violated).contains("vision judges agree it violates acceptance: m-a: FAIL (because) | m-b: FAIL (because)"), "{}", detail(&violated));
    assert!(disagree.exit_code == 0 && detail(&disagree).contains("vision judges do not agree on a violation"), "{}", detail(&disagree));
    assert!(unjudged.exit_code == 0 && detail(&unjudged).contains("no --acceptance, vision judge skipped"), "{}", detail(&unjudged));

    // When baselines go to an OCI bucket under a sha, with 22 older ones there, and when the bucket variable is missing
    let oci = stub_bin(&[("oci", "echo \"$@\" >> \"$0.log\"\ncase \"$*\" in\n  *\"object list\"*) i=0; printf '{\"data\":['; while [ $i -lt 22 ]; do [ $i -gt 0 ] && printf ','; printf '{\"name\":\"baselines/ok/s%02d.png\",\"time-created\":\"2026-01-%02dT00:00:00Z\"}' $i $((i+1)); i=$((i+1)); done; printf ']}';;\n  *) ;;\nesac")]);
    let bucket = run(ok, "oci", &["--baseline-store", "oci", "--sha", "abc123", "--update-baselines"], &[("GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX", "https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/ns1/b/shots/o/".to_string())], Some(oci.path()));
    let no_bucket = run(ok, "oci2", &["--baseline-store", "oci"], &[("GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX", String::new())], None);
    // Then the baseline is put under its sha, the oldest past 20 are pruned, the tier is recorded, and the missing variable is named
    let log = std::fs::read_to_string(oci.path().join("oci.log")).unwrap();
    assert_eq!(bucket.exit_code, 0, "{}\n{}", bucket.stdout, bucket.stderr);
    assert!(log.contains("os object put --namespace-name ns1 --bucket-name shots --name baselines/ok/abc123.png") && log.contains("--name verify/abc123.tier2.json"), "{log}");
    assert!(log.contains("--name baselines/ok/s00.png --force") && log.contains("--name baselines/ok/s01.png --force") && !log.contains("--name baselines/ok/s02.png --force"), "{log}");
    assert!(no_bucket.exit_code == 2 && no_bucket.stderr.contains("GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX is unset"), "{}", no_bucket.stderr);
}
