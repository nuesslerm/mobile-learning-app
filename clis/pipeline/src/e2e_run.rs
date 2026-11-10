//! `wf test run`: the e2e runs a change needs, one unit after another, each through its harness (`wf android test` / `wf browser test`)
//! under the test's timeout. An Android unit against `update` loads the EAS Update CI published, checked to carry the tree; a
//! browser unit gets the web export served on loopback.
use wf_kit::args::{parse_spec, Positional, Spec};
use crate::device_run::{self, UpdateTarget};
use crate::e2e_tests::{self as catalog, E2eTest};
use crate::export_site;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const SPEC: Spec = Spec { values: &["base", "sha", "out", "serial", "platform"], flags: &["all", "changed", "json"], positional: Positional::Any, ..Spec::NONE };

/// One run of an e2e test: the whole script, or one of its `units:`.
#[derive(Debug, Clone, PartialEq)]
pub struct Unit {
    pub test: String,
    pub script: String,
    pub platform: String,
    pub against: String,
    pub timeout_s: u64,
    pub args: Vec<String>,
}

/// The units of the picked tests, in test order: one per `units:` entry, or one for a test without units.
pub fn units_of(books: &[&E2eTest]) -> Vec<Unit> {
    books
        .iter()
        .filter_map(|r| {
            let script = r.script.clone()?;
            let unit = |args: Vec<String>| Unit { test: r.name.clone(), script: script.clone(), platform: r.platform.clone(), against: r.against.clone(), timeout_s: r.timeout_s.unwrap_or(180), args };
            Some(if r.units.is_empty() { vec![unit(vec![])] } else { r.units.iter().map(|u| unit(u.clone())).collect() })
        })
        .flatten()
        .collect()
}

/// The cold start (`e2e/warm-app.lua`) is the one script that names the link it opens.
const WARM_SCRIPT: &str = "e2e/warm-app.lua";

/// The harness command line for a unit: its platform's `test`, the script, the test output dir, the unit's env, then the
/// caller's test flags (`--update-goldens`, `--logpoint file:line`) as they were given.
fn command_line(unit: &Unit, dir: &Path, serial: Option<&str>, flags: &[String], target: Option<&Target>) -> Vec<String> {
    let mut a: Vec<String> = vec![unit.platform.clone(), "test".into(), unit.script.clone(), "--test-output-dir".into(), dir.join("rec").display().to_string()];
    // screenshots inside the test output dir, so the published bundle carries them
    a.extend(["--env".into(), format!("out={}", dir.join("rec").join("out").display()), "--env".into(), format!("against={}", unit.against)]);
    // the browser opens the export this run serves on loopback, Android the update CI published
    if let Some(target) = target {
        a.extend(["--env".into(), format!("base={}", target.base())]);
        // an update opens bare: its manifest answers 404 under a /--/ path, so the cold start is told the link
        if let (Target::Update(u), true) = (target, unit.script == WARM_SCRIPT) {
            a.extend(["--env".into(), format!("open={}", u.open)]);
        }
    }
    for kv in &unit.args {
        a.extend(["--env".into(), kv.clone()]);
    }
    if let Some(s) = serial.filter(|_| unit.platform == "android") {
        a.extend(["--device".into(), s.to_string()]);
    }
    // WF_TEST_PUBLISH=1 (the workflow): each run becomes a run bundle in the bucket, which also posts its test_runs row
    if std::env::var("WF_TEST_PUBLISH").as_deref() == Ok("1") && !flags.iter().any(|f| f == "--publish") {
        a.push("--publish".into());
    }
    a.extend(flags.iter().cloned());
    a
}

fn slug(unit: &Unit) -> String {
    let tail: String = unit.args.iter().map(|a| a.replace(['=', '/', ' '], "-")).collect::<Vec<_>>().join("-");
    if tail.is_empty() { unit.test.clone() } else { format!("{}-{tail}", unit.test) }
}

/// The export a run serves its browser units from: where, and what it was built from.
struct ExportRun {
    base: String,
    built: Value,
}

/// What a unit's harness opens, resolved once per run: the export served on loopback (browser) or the EAS Update (Android).
enum Target {
    Export(ExportRun),
    Update(UpdateTarget),
}

impl Target {
    fn base(&self) -> &str {
        match self {
            Target::Export(e) => &e.base,
            Target::Update(u) => &u.base,
        }
    }
}

/// Runs one unit under its timeout (SIGTERM, then the grace, then SIGKILL).
fn run_unit(unit: &Unit, sha: &str, root: &Path, serial: Option<&str>, flags: &[String], target: Option<&Target>) -> Value {
    let dir = root.join(slug(unit));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let (verdict, note, wall_ms) = run_on_harness(unit, sha, &dir, serial, flags, target);
    let log = std::fs::read_to_string(dir.join("run.log")).unwrap_or_default();
    let timing = wf_harness::run_report::timing_summary(&std::fs::read_to_string(dir.join("timing.jsonl")).unwrap_or_default(), wall_ms);
    let report = device_run::report_in(&log).or_else(|| Some(dir.join("rec/report.html")).filter(|p| p.exists()).map(|p| p.display().to_string()));
    let tail: Vec<&str> = log.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect();
    let bundle = log.lines().find_map(|l| l.strip_prefix("bundle: ")).map(str::to_string);
    let mut result = json!({
        "test": unit.test, "args": unit.args, "sha": sha, "script": unit.script, "platform": unit.platform, "verdict": verdict,
        "note": note, "durationS": wall_ms as f64 / 1000.0, "timing": timing, "report": report, "recording": device_run::recordings_in(&log),
        "bundle": bundle, "attempt": 1, "dir": dir.display().to_string(), "logTail": tail,
    });
    match target {
        Some(Target::Export(e)) => {
            result["base"] = json!(e.base);
            result["export"] = e.built.clone();
        }
        Some(Target::Update(u)) => {
            result["base"] = json!(u.base);
            result["updateGroup"] = json!(u.group);
            result["updateCommit"] = json!(u.commit);
        }
        None => {}
    }
    let _ = std::fs::write(dir.join("result.json"), serde_json::to_string_pretty(&result).unwrap_or_default());
    result
}

/// The unit on its harness; an Android one holds the device's lease while it runs.
fn run_on_harness(unit: &Unit, sha: &str, dir: &Path, serial: Option<&str>, flags: &[String], target: Option<&Target>) -> (&'static str, String, u64) {
    let serial = match (unit.platform.as_str(), serial) {
        ("android", Some(s)) if std::env::var("WF_TEST_NO_ADB").is_ok() => Some(s.to_string()),
        ("android", Some(s)) => match device_run::connect_serial(s) {
            Ok(s) => Some(s),
            Err(e) => return ("error", e, 0),
        },
        _ => None,
    };
    // the device's lease keeps another session's `wf android` verbs off it while the unit runs
    let leased = unit.platform == "android" && std::env::var("WF_TEST_NO_LEASE").is_err();
    let device = wf_harness::android::container(None);
    let lease_id = format!("device-{}", std::process::id());
    if leased {
        if let Err(e) = wf_harness::android_lease::acquire_blocking(&device, &lease_id, &format!("wf test run:{}", unit.test), 1_800_000, 1_800_000) {
            return ("error", e, 0);
        }
    }
    let outcome = spawn_and_wait(unit, sha, dir, serial.as_deref(), flags, leased.then_some(lease_id.as_str()), target);
    if leased {
        wf_harness::android_lease::release_and_hand_over(&device, &lease_id, None);
    }
    outcome
}

fn spawn_and_wait(unit: &Unit, sha: &str, dir: &Path, serial: Option<&str>, flags: &[String], lease: Option<&str>, target: Option<&Target>) -> (&'static str, String, u64) {
    let mut words = device_run::runner();
    let program = words.remove(0);
    let mut cmd = Command::new(program);
    cmd.args(words).args(command_line(unit, dir, serial, flags, target));
    if let Ok(log) = std::fs::File::create(dir.join("run.log")) {
        if let Ok(err) = log.try_clone() {
            cmd.stderr(err);
        }
        cmd.stdout(log);
    }
    cmd.env("WF_RUN_TIMING", dir.join("timing.jsonl")).env("WF_RUN_SHA", sha);
    // the run's own `wf android` verbs hold the lease this process took for it
    if let Some(id) = lease {
        cmd.env("WF_ANDROID_LEASE_ID", id);
    }
    // its own process group: a timeout stops the run and everything it started (adb, ffmpeg)
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let timeout = Duration::from_secs(std::env::var("WF_TEST_TIMEOUT_S").ok().and_then(|v| v.parse().ok()).unwrap_or(unit.timeout_s));
    let start = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ("error", format!("cannot start the runner: {e}"), 0),
    };
    let (mut verdict, mut note) = ("error", String::new());
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                verdict = if status.success() { "pass" } else { "fail" };
                break;
            }
            Ok(None) if start.elapsed() > timeout => {
                device_run::stop_run(&mut child);
                note = format!("timed out after {}s", timeout.as_secs());
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(500)),
            Err(e) => {
                note = format!("lost the runner: {e}");
                break;
            }
        }
    }
    (verdict, note, start.elapsed().as_millis() as u64)
}

/// Whether the Expo Go the units reuse is known good: started cold by a passing warm run, against this update, with every unit
/// since then passed. `at` names the update: `update:<group>`.
#[derive(Default)]
struct Warm {
    ok: bool,
    at: Option<String>,
}

/// A unit's target as a warm start names it: the update's group.
fn target_key(update: Option<&UpdateTarget>) -> String {
    format!("update:{}", update.map_or("", |u| u.group.as_str()))
}

/// Android units reuse the running app, started cold first when it is not known good: before the first, after a unit that did not
/// pass, and when the update's group changed (16 Oct: a cold start was 57 to 76 s of every unit).
fn needs_warm(unit: &Unit, warm: &Warm, key: &str) -> bool {
    unit.platform == "android" && (!warm.ok || warm.at.as_deref() != Some(key))
}

/// `WF_E2E_STATE`: the file a stage keeps the warm state in, so each test's own `wf test run` (one per Nx task) starts
/// from the app the last one left instead of starting it cold again.
fn load_warm() -> Warm {
    let state = std::env::var("WF_E2E_STATE").ok().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok());
    state.map_or_else(Warm::default, |v| Warm { ok: v["ok"] == true, at: v["at"].as_str().map(str::to_string) })
}

fn save_warm(warm: &Warm) {
    if let Ok(f) = std::env::var("WF_E2E_STATE") {
        let _ = std::fs::write(f, json!({ "ok": warm.ok, "at": warm.at }).to_string());
    }
}

/// A unit's target must be its harness's: Android runs on an EAS Update, the browser on a web export (the error, and the next step).
fn unsupported(u: &Unit) -> Option<(String, String)> {
    match (u.platform.as_str(), u.against.as_str()) {
        ("android", "update") | ("browser", "export") => None,
        ("android", "metro") => Some((format!("{} runs on Android against the dev Metro, which is gone: Android tests run against the EAS Update CI published", u.test), format!("set `against: update` (or `against: android=update, browser=export`) in {}", u.script))),
        ("browser", "metro") => Some((format!("{} runs in the browser against the dev Metro, which is gone: browser tests run against a web export", u.test), format!("set `against: export` (or `against: android=update, browser=export`) in {}", u.script))),
        ("browser", "update") => Some((format!("{} runs in the browser against an EAS Update: an update has no web build, browser tests run against a web export", u.test), format!("set `against: android=update, browser=export` in {}", u.script))),
        (platform, against) => Some((format!("{}: `against: {against}` is not a target of the {platform} harness", u.test), format!("`against:` in {} is update for android and export for browser", u.script))),
    }
}

/// What a result records of the export its browser unit ran against: what it was built from, the baked names and a digest of
/// their values (a result is published and cached, the hosts and credential hashes stay in the stamp).
fn export_built(stamp: &Value, dir: &Path) -> Value {
    json!({ "dir": dir.display().to_string(), "sha": stamp["sha"], "inputs": stamp["inputs"], "builtAt": stamp["builtAt"], "buildS": stamp["buildS"], "flags": stamp["flags"], "baked": export_site::baked_summary(&stamp["baked"]) })
}

/// The cold start as its own recorded and published run: `e2e/warm-app.lua` with `--cold-start`, against the target of the unit it precedes.
fn warm_unit(n: usize, against: &str) -> Unit {
    Unit { test: "warm-app".into(), script: WARM_SCRIPT.into(), platform: "android".into(), against: against.into(), timeout_s: 300, args: vec![format!("warm={n}")] }
}

/// `wf test run [<test>... | --changed [--base <ref>] | --all] [--sha S] [--out DIR] [--serial S] [-- <test flags>]`
pub fn run(rest: &[String]) -> i32 {
    let split = rest.iter().position(|a| a == "--").unwrap_or(rest.len());
    let (own, flags) = (&rest[..split], rest.get(split + 1..).unwrap_or_default().to_vec());
    let mut with_verb = vec!["device".to_string()];
    with_verb.extend_from_slice(own);
    let Some(p) = parse_spec(&with_verb, &SPEC) else { return 2 };
    let sha = p.val("sha").map(str::to_string).unwrap_or_else(catalog::head_sha);
    let books = catalog::e2e_tests();
    let picked = match catalog::pick(&p, &books) {
        Ok(v) => v,
        Err(e) => {
            println!("{}", json!({ "ok": false, "error": e }));
            return 2;
        }
    };
    let chosen: Vec<&E2eTest> = picked.iter().map(|i| &books[*i]).collect();
    let mut units = units_of(&chosen);
    // `--platform android|browser`: one harness per CI job (browser e2e tests before the Android ones, each its own job)
    if let Some(platform) = p.val("platform") {
        units.retain(|u| u.platform == platform);
    }
    let root = p.val("out").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".agents/tmp/device-runs").join(&sha[..sha.len().min(9)]));
    let serial = p.val("serial").map(str::to_string).or_else(|| std::env::var("WF_ANDROID_SERIAL").ok());
    if units.is_empty() {
        println!("{}", json!({ "ok": true, "sha": sha, "units": [], "note": "no focused e2e test was picked" }));
        return 0;
    }
    if let Some((error, next)) = units.iter().find_map(unsupported) {
        println!("{}", json!({ "ok": false, "error": error, "next": next }));
        return 2;
    }
    // an Android run loads the EAS Update, which has no inspector: a logpoint is a browser run's
    if let (true, Some(u)) = (flags.iter().any(|f| f.starts_with("--logpoint")), units.iter().find(|u| u.platform == "android")) {
        let error = format!("{} runs on Android, and --logpoint is browser only: an Android run loads the EAS Update, which has no inspector", u.test);
        println!("{}", json!({ "ok": false, "error": error, "next": "set a logpoint in a browser run (`--platform browser`), or drop --logpoint for this Android run (there is no Android debugger yet)" }));
        return 2;
    }
    // the export the browser units run against: checked once, served for the whole run; a missing or stale one fails those units
    let export = units.iter().any(|u| u.against == "export").then(|| export_site::serve_current(&catalog::repo()));
    // the update the Android units load, read once: a failed or stale one fails those units
    let update = units.iter().any(|u| u.against == "update").then(|| device_run::update_target(&sha));
    let mut results: Vec<Value> = Vec::new();
    let mut warms: Vec<Value> = Vec::new();
    let mut warm = load_warm();
    // a failed warm-up means the app cannot start against that update: its other units are reported, not each waited on
    let mut cannot_start: Option<String> = None;
    // failures in a row mean something shared is broken (the device, a backend): the rest would only burn the job's time
    let max_fails: usize = std::env::var("WF_TEST_MAX_FAILS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let mut fails_in_row = 0;
    for u in &units {
        if fails_in_row >= max_fails {
            let note = format!("not run: the {fails_in_row} units before it failed in a row");
            println!("{}", json!({ "unit": slug(u), "verdict": "skipped", "durationS": 0.0, "bundle": null, "note": note }));
            results.push(json!({ "test": u.test, "args": u.args, "sha": sha, "script": u.script, "platform": u.platform, "verdict": "skipped", "note": note, "durationS": 0.0 }));
            continue;
        }
        if let (Some(note), true) = (&cannot_start, u.platform == "android") {
            println!("{}", json!({ "unit": slug(u), "verdict": "error", "durationS": 0.0, "bundle": null, "note": note }));
            results.push(json!({ "test": u.test, "args": u.args, "sha": sha, "script": u.script, "platform": u.platform, "verdict": "error", "note": note, "durationS": 0.0 }));
            continue;
        }
        let mut target = None;
        let problem = match (u.against.as_str(), &export, &update) {
            ("export", Some(Ok((server, stamp))), _) => {
                target = Some(Target::Export(ExportRun { base: server.base_url.clone(), built: export_built(stamp, &export_site::dir(&catalog::repo())) }));
                None
            }
            ("export", Some(Err(p)), _) => Some((p.error.clone(), p.next.clone())),
            ("update", _, Some(Ok(t))) => {
                target = Some(Target::Update(t.clone()));
                None
            }
            ("update", _, Some(Err(p))) => Some((p.error.clone(), p.next.clone())),
            _ => None,
        };
        if let Some((error, next)) = problem {
            println!("{}", json!({ "unit": slug(u), "verdict": "error", "durationS": 0.0, "bundle": null, "note": error, "next": next }));
            results.push(json!({ "test": u.test, "args": u.args, "sha": sha, "script": u.script, "platform": u.platform, "verdict": "error", "note": error, "next": next, "durationS": 0.0 }));
            continue;
        }
        let key = (u.platform == "android").then(|| target_key(update.as_ref().and_then(|r| r.as_ref().ok())));
        if key.as_deref().is_some_and(|k| needs_warm(u, &warm, k)) {
            let w = run_unit(&warm_unit(warms.len() + 1, &u.against), &sha, &root, serial.as_deref(), &["--cold-start".to_string()], target.as_ref());
            println!("{}", json!({ "warm": warms.len() + 1, "verdict": w["verdict"], "durationS": w["durationS"], "bundle": w["bundle"], "note": w["note"] }));
            warm = Warm { ok: w["verdict"] == "pass", at: Some(format!("update:{}", w["updateGroup"].as_str().unwrap_or(""))) };
            save_warm(&warm);
            let failed = (!warm.ok).then(|| format!("the app did not start cold before this unit: {}", w["note"].as_str().unwrap_or("")));
            warms.push(w);
            cannot_start = failed.clone();
            if let Some(note) = failed {
                let r = json!({ "test": u.test, "args": u.args, "sha": sha, "script": u.script, "platform": u.platform, "verdict": "error", "note": note, "durationS": 0.0 });
                println!("{}", json!({ "unit": slug(u), "verdict": "error", "durationS": 0.0, "bundle": null, "note": r["note"] }));
                results.push(r);
                continue;
            }
        }
        let r = run_unit(u, &sha, &root, serial.as_deref(), &flags, target.as_ref());
        println!("{}", json!({ "unit": slug(u), "verdict": r["verdict"], "durationS": r["durationS"], "bundle": r["bundle"], "note": r["note"] }));
        // a unit that did not pass may leave the app broken: the next one starts it cold again
        warm.ok &= r["verdict"] == "pass";
        save_warm(&warm);
        fails_in_row = if r["verdict"] == "pass" { 0 } else { fails_in_row + 1 };
        results.push(r);
    }
    if let Some(Ok((server, _))) = &export {
        server.stop();
    }
    let ok = results.iter().all(|r| r["verdict"] == "pass");
    let summary = json!({ "ok": ok, "sha": sha, "out": root.display().to_string(), "units": results, "warms": warms });
    let _ = std::fs::create_dir_all(&root);
    let _ = std::fs::write(root.join("summary.json"), serde_json::to_string_pretty(&summary).unwrap_or_default());
    println!("{summary}");
    i32::from(!ok)
}
