//! `wf android test` / `wf browser test <script.lua>`: run a test script against a device (Maestro's `maestro test`). The
//! script gets a `device` (the command table's methods, `crate::device::api`) and can hand open-ended steps to a local agent
//! with `agent.run{}`. The run is recorded by default; `--api` prints the API reference. Every run, pass or fail, ends with
//! `<test-output-dir>/report.html` and `report.json` (`crate::run_report`).
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};
use crate::device::golden::Goldens;
use crate::device::{lua, Level, Logpoint};
use crate::platform::Platform;
use crate::run_report::{self, Facts, Trail};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

/// Uploads a finished run's directory as a bundle and returns the bundle's base URL (the directory, the site it ran on, whether to post its row).
type Publisher = fn(&Path, &str, bool) -> Result<String, String>;

static PUBLISHER: OnceLock<Publisher> = OnceLock::new();

/// Sets what `--publish` calls: the full `wf` has the bucket client, this crate does not.
pub fn set_publisher(publisher: Publisher) {
    assert!(PUBLISHER.set(publisher).is_ok(), "run::set_publisher runs once, at start");
}

/// Uploads a finished run's directory through the publisher `wf` set, and returns the bundle's base URL.
pub fn publish(dir: &Path, ran_on: &str, record: bool) -> Result<String, String> {
    match PUBLISHER.get() {
        Some(publish) => publish(dir, ran_on, record),
        None => Err("this binary has no bucket client".to_string()),
    }
}

fn usage(platform: &dyn Platform) -> String {
    let (logpoint, extra) = platform.usage_options();
    let platform = platform.name();
    format!("wf {platform} test <script.lua> [--env K=V]... [--publish] [--test-output-dir D] [--no-record] [--update-goldens]{logpoint} [--device D] [--speed N]{extra} | wf {platform} test --api [--json]")
}

/// The flags `run` took that `test` does not, each with why.
const DROPPED_FLAGS: &[(&str, &str)] = &[
    ("--goldens", "assert_screenshot compares with its golden on every run; --update-goldens writes them"),
    ("--platform", "the noun is the platform"),
];

/// `wf browser|android test <script.lua> ...` runs a script; `test --api [--json]` prints the API from the command table
/// (`--json`: the table itself).
pub fn test_for(platform: &dyn Platform, args: &[String]) -> Option<i32> {
    if args.iter().any(|a| a == "--api") {
        if args.iter().any(|a| a != "--api" && a != "--json") {
            println!("{}", json!({ "ok": false, "error": "test --api takes only --json", "usage": usage(platform) }));
            return Some(2);
        }
        if args.iter().any(|a| a == "--json") {
            println!("{}", crate::device::api::json(platform.name()));
        } else {
            print!("{}", crate::device::api::text(platform.name()));
        }
        return Some(0);
    }
    let flag = |a: &str| a.split('=').next().unwrap_or(a).to_string();
    for a in args.iter().filter(|a| a.starts_with("--")) {
        if let Some((old, why)) = DROPPED_FLAGS.iter().find(|(f, _)| *f == flag(a)) {
            println!("{}", json!({ "ok": false, "error": format!("{old} is gone: {why}"), "usage": usage(platform) }));
            return Some(2);
        }
    }
    harness(platform, args)
}

/// `wf run` was the old spelling of the two harness verbs.
pub fn gone(_: &[String]) -> Option<i32> {
    println!("{}", json!({ "error": "wf run is gone", "next": "wf android test <script.lua> | wf browser test <script.lua>" }));
    Some(2)
}

fn harness(platform: &dyn Platform, args: &[String]) -> Option<i32> {
    let Some(p) = parse_spec(args, &Spec { values: &["device", "env", "test-output-dir", "logpoint", "goldens-dir", "screen-size", "frame", "speed"], flags: &["no-record", "update-goldens", "publish", "cold-start"], positional: Positional::Any, ..Spec::NONE }) else {
        let e = wf_kit::args::take_error().unwrap_or_default();
        let said = e.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().trim_start_matches("error: ").to_string();
        println!("{}", json!({ "ok": false, "error": said, "usage": usage(platform) }));
        return Some(2);
    };
    if let Err(e) = platform.check_options(&p).and_then(|()| speed(&p)) {
        println!("{}", json!({ "ok": false, "error": e, "usage": usage(platform) }));
        return Some(2);
    }
    let script = match p.positional.as_slice() {
        [one] => one,
        [] => {
            println!("{}", json!({ "ok": false, "error": "test needs the script: its first word", "usage": usage(platform) }));
            return Some(2);
        }
        more => {
            println!("{}", json!({ "ok": false, "error": format!("test runs one script, got {}", more.join(" ")), "usage": usage(platform) }));
            return Some(2);
        }
    };
    let stem = Path::new(script).file_stem().and_then(|s| s.to_str()).unwrap_or("run").to_string();
    let dir = p.val("test-output-dir").map(str::to_string).unwrap_or_else(|| format!(".agents/tmp/runs/{stem}-{}", wf_kit::time::now_ms()));
    // every report carries the timing table: the lines go into the record dir unless the caller names another file
    let timing_file = Some(std::env::var_os("WF_RUN_TIMING").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("{dir}/timing.jsonl"))));
    if std::env::var_os("WF_RUN_TIMING").is_none() {
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("WF_RUN_TIMING", format!("{dir}/timing.jsonl"));
    }
    let timing_from = timing_file.as_deref().and_then(|t| std::fs::metadata(t).ok()).map_or(0, |m| m.len() as usize);
    // the spans of this run count their t0 from here
    std::env::set_var("WF_RUN_T0_MS", wf_kit::time::now_ms().to_string());
    let mut f = Facts { script: script.clone(), platform: platform.name().to_string(), sha: head_sha(), speed: Some(crate::device::speed()), started_ms: wf_kit::time::now_ms(), verdict: "error", ..Facts::default() };
    let sampler = wf_vmops::vm_sampler::start_from_env();
    let scope = platform.begin_run(&stem, f.started_ms, &p);
    let code = execute(&p, script, platform, &dir, &mut f);
    scope.finish(&p, &mut f);
    f.vm = sampler.map(wf_vmops::vm_sampler::Sampler::finish);
    f.exit_code = code;
    f.finished_ms = wf_kit::time::now_ms();
    let report_span = crate::spans::begin("phase", "report", None);
    f.timing_lines = timing_file.clone().and_then(|t| std::fs::read(t).ok()).map(|b| String::from_utf8_lossy(b.get(timing_from..).unwrap_or_default()).into_owned());
    let written = run_report::write(Path::new(&dir), &f);
    drop(report_span);
    match written {
        Ok(page) => {
            println!("report: {}", page.display());
            if p.flag("publish") {
                let _publish = crate::spans::begin("phase", "publish", None);
                // the run becomes a bundle in the bucket, and that posts the row with the bundle's report as report_path
                let published = publish(Path::new(&dir), &std::env::var("WF_SITE").unwrap_or_else(|_| wf_kit::proc::hostname()), true);
                match published {
                    Ok(base) => println!("bundle: {base}/bundle.json"),
                    Err(e) => wf_kit::wf_err!("wf: the run's bundle was not published: {e}"),
                }
            }
        }
        Err(e) => wf_kit::wf_err!("wf: cannot write the report: {e}"),
    }
    Some(code)
}

/// The run's speed: `--speed` becomes WF_TEST_SPEED, and either must be a number from 1 to 4 before anything runs.
fn speed(p: &Parsed) -> Result<(), String> {
    if let Some(v) = p.val("speed") {
        crate::device::parse_speed(v).ok_or_else(|| format!("--speed is a number from 1 to 4, got '{v}'"))?;
        std::env::set_var("WF_TEST_SPEED", v.trim());
    }
    crate::device::test_speed().map(|_| ())
}

/// The commit under test: `WF_RUN_SHA` (the queue sets it), else the checkout's HEAD.
fn head_sha() -> Option<String> {
    if let Ok(s) = std::env::var("WF_RUN_SHA") {
        return Some(s).filter(|s| !s.is_empty());
    }
    let o = std::process::Command::new("git").args(["rev-parse", "HEAD"]).stderr(std::process::Stdio::null()).output().ok()?;
    Some(String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| o.status.success() && !s.is_empty())
}

fn refuse(f: &mut Facts, msg: String) -> i32 {
    wf_kit::wf_err!("wf: {msg}");
    f.message = Some(msg);
    1
}

/// Runs the script and fills `f`; returns the exit code. Everything it learns goes into the report, whatever happens.
fn execute(p: &Parsed, script: &str, platform: &dyn Platform, dir: &str, f: &mut Facts) -> i32 {
    crate::run_stop::arm();
    let setup = crate::spans::begin("phase", "setup", None);
    let serial = p.val("device").map(str::to_string);
    let record_wanted = !p.flag("no-record");
    let _ = std::fs::create_dir_all(dir);
    let inputs = crate::video_check::input_log_path_for(&format!("{dir}/run.mp4"));
    let device = platform.open_run_device(serial.clone(), inputs, f);
    if let Some(why) = platform.refuse_option(p) {
        return refuse(f, why);
    }
    let mut debug_points = Vec::new();
    for spec in p.values("logpoint") {
        let mut parts = spec.splitn(3, ':');
        let (file, line, condition) = (parts.next().unwrap_or(""), parts.next().and_then(|l| l.parse::<u32>().ok()), parts.next().map(str::to_string));
        match line {
            Some(line) if !file.is_empty() => debug_points.push(Logpoint { file: file.to_string(), line, condition, capture: vec![], defer: true }),
            _ => return refuse(f, format!("--logpoint wants file:line[:condition], got '{spec}'")),
        }
    }
    if !debug_points.is_empty() {
        if let Err(e) = device.arm_logpoints(&debug_points) {
            return refuse(f, format!("cannot arm the logpoints: {e}"));
        }
    }
    let source = match std::fs::read_to_string(script) {
        Ok(s) => s,
        Err(e) => return refuse(f, format!("cannot read {script}: {e}")),
    };
    let mut script_args = BTreeMap::new();
    for kv in p.values("env") {
        match kv.split_once('=') {
            Some((k, v)) => {
                script_args.insert(k.to_string(), v.to_string());
            }
            None => return refuse(f, format!("--env wants K=V, got '{kv}'")),
        }
    }
    f.args = script_args.clone();
    f.extra_dirs = script_args.get("out").map(PathBuf::from).into_iter().collect();
    platform.before_script(&*device, p, f);
    let record = record_wanted;
    if record {
        if let Err(e) = device.start_recording(&format!("{dir}/run.mp4")) {
            return refuse(f, format!("cannot start the recording: {e}"));
        }
    }
    let goldens_dir = p.val("goldens-dir").map(PathBuf::from).unwrap_or_else(|| wf_kit::root::repo_root().join("docs/runbooks/goldens").join(platform.name()));
    let mode = if p.flag("update-goldens") { crate::golden::Mode::Update } else { crate::golden::Mode::Compare };
    let goldens = Rc::new(Goldens::new(goldens_dir, mode, dir.into()));
    let trail = Rc::new(Trail::default());
    drop(setup);
    let script_span = crate::spans::begin("phase", "script", None);
    let result = lua::run(device.clone(), lua::Script { path: script, source: &source, env: script_args, serial, goldens: goldens.clone(), trail: trail.clone(), out_dir: PathBuf::from(dir) });
    drop(script_span);
    let _teardown = crate::spans::begin("phase", "teardown", None);
    if let Some(line) = goldens.finish() {
        println!("{line}");
    }
    if crate::run_stop::requested() {
        provisional_report(dir, f, &result, &goldens, &trail);
    }
    if let Ok(w) = device.get_logs(Level::Warn) {
        trail.saw_warnings(&w);
    }
    // read before the recording stops: closing the page to write the video ends the session that holds the hits
    if let Ok(hits) = device.get_debug_hits() {
        if !hits.is_empty() || !p.values("logpoint").is_empty() {
            let body: String = hits.iter().map(|h| format!("{h}\n")).collect();
            match std::fs::write(format!("{dir}/debug.jsonl"), body) {
                Ok(()) => println!("debug: {dir}/debug.jsonl ({} hits)", hits.len()),
                Err(e) => wf_kit::wf_err!("wf: cannot write the debug hits: {e}"),
            }
        }
        f.debug_hits = hits;
    }
    // read before the page closes: the hits live in the browser session
    let declared = trail.stubs.borrow().clone();
    if !declared.is_empty() {
        let hits = device.get_network_stub_hits().unwrap_or_default();
        let mut stubs = serde_json::json!({ "declared": declared, "hits": hits });
        if let Some(routing) = device.stub_routing() {
            stubs["routing"] = routing;
        }
        f.stubs = Some(stubs);
    }
    let mut code = 0;
    if record {
        match device.stop_recording() {
            Ok(files) if !files.is_empty() => {
                files.iter().for_each(|r| println!("recording: {r}"));
                f.recordings = files;
            }
            Ok(_) => {
                wf_kit::wf_err!("wf: the run produced no recording");
                f.notes.push("the run produced no recording".into());
                code = 1;
            }
            Err(e) => {
                wf_kit::wf_err!("wf: cannot stop the recording: {e}");
                f.notes.push(format!("cannot stop the recording: {e}"));
                code = 1;
            }
        }
    }
    device.shutdown();
    f.goldens = goldens.outcomes.borrow().clone();
    f.screenshots = trail.screenshots.borrow().clone();
    f.warnings = trail.warnings.borrow().clone();
    match result {
        Err(e) => {
            wf_kit::wf_err!("wf: {}", e.text);
            let (message, traceback) = run_report::split_traceback(&e.text);
            if let Some(shot) = failure_screenshot(&message) {
                f.screenshots.push(shot);
            }
            f.evidence.extend(failure_evidence(&message));
            if crate::run_stop::requested() {
                // the queue's timeout or a cancel: the report says so, and 124 tells the worker it was a stop, not a failure
                f.verdict = "error";
                f.message = Some(format!("{}; last step: {message}", crate::run_stop::reason()));
                f.traceback = traceback;
                return 124;
            }
            f.verdict = e.verdict;
            f.message = Some(message);
            f.traceback = traceback;
            1
        }
        Ok(()) if code != 0 => {
            f.message = f.notes.first().cloned();
            code
        }
        Ok(()) => {
            f.verdict = "pass";
            0
        }
    }
}

/// A stopped run gets its grace period to pull the recording, which a loaded VM can outlast; the report written here
/// stands if the run is killed first, and the full one replaces it if not.
fn provisional_report(dir: &str, f: &mut Facts, result: &Result<(), lua::ScriptError>, goldens: &Goldens, trail: &Trail) {
    if let Err(e) = result {
        let (message, traceback) = run_report::split_traceback(&e.text);
        f.message = Some(format!("{}; last step: {message}", crate::run_stop::reason()));
        f.traceback = traceback;
    }
    f.goldens = goldens.outcomes.borrow().clone();
    f.screenshots = trail.screenshots.borrow().clone();
    f.exit_code = 124;
    f.finished_ms = wf_kit::time::now_ms();
    match run_report::write(Path::new(dir), f) {
        Ok(page) => println!("report (before the recording stops): {}", page.display()),
        Err(e) => wf_kit::wf_err!("wf: cannot write the report: {e}"),
    }
}

/// The `screenshot: <file>.png` a wait timeout names (android takes it outside the record dir).
fn failure_screenshot(message: &str) -> Option<String> {
    named(message, "screenshot: ", ".png")
}

/// The `evidence: <file>.json` a failed wait names, beside its screenshot.
fn failure_evidence(message: &str) -> Option<String> {
    named(message, "evidence: ", ".json")
}

fn named(message: &str, label: &str, ext: &str) -> Option<String> {
    let rest = &message[message.rfind(label)? + label.len()..];
    let path = rest.split_whitespace().next()?.trim_end_matches([';', ',']);
    path.ends_with(ext).then(|| path.to_string())
}
