//! What a device run needs besides the e2e catalog (`pipeline`): the harness process, stopping a run, and the EAS Update an Android
//! test loads. `e2e_run.rs` uses them.
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

/// The package files a result carries: `recording: <file>` lines of the harness run.
pub fn recordings_in(log: &str) -> Vec<String> {
    log.lines().filter_map(|l| l.trim().strip_prefix("recording:")).map(|f| f.trim().to_string()).filter(|f| !f.is_empty()).collect()
}

/// The run's report page: the last `report: <file>` line of the harness run.
pub fn report_in(log: &str) -> Option<String> {
    log.lines().rev().filter_map(|l| l.trim().strip_prefix("report:")).map(|f| f.trim().to_string()).find(|f| !f.is_empty())
}

/// A timed-out run is told to stop (SIGTERM to its process group) and given `STOP_GRACE` to end its recording and write its
/// report, which the hard rule asks of every run; only then is it killed.
const STOP_GRACE: Duration = Duration::from_secs(15);

pub fn stop_run(child: &mut std::process::Child) {
    let pgid = nix::unistd::Pid::from_raw(child.id() as i32);
    let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGTERM);
    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let _ = nix::sys::signal::killpg(pgid, nix::sys::signal::Signal::SIGKILL);
    let _ = child.wait();
}

/// The words of a runner command (`WF_TEST_RUNNER`, default this binary's `run`).
pub fn runner() -> Vec<String> {
    match std::env::var("WF_TEST_RUNNER") {
        Ok(r) if !r.trim().is_empty() => r.split_whitespace().map(str::to_string).collect(),
        _ => vec![std::env::current_exe().map(|e| e.display().to_string()).unwrap_or_else(|_| "wf".to_string())],
    }
}

/// The EAS Update an `against: update` unit loads, and where it opens.
#[derive(Debug, Clone)]
pub struct UpdateTarget {
    /// `exp://u.expo.dev/<project>/group/<group>/--`: what a script puts a route after.
    pub base: String,
    /// The same without `/--`: an update opens bare, its manifest answers 404 under a `/--/` path.
    pub open: String,
    pub group: String,
    pub commit: String,
}

/// Why no update can stand for this commit, and the step that fixes it. Never a reason to load the dev Metro instead.
#[derive(Debug, Clone)]
pub struct UpdateProblem {
    pub error: String,
    pub next: String,
}

/// What an update must bake in: the lab flag, the leetcode backend and its token, and the agent voice-edit speaks to.
const UPDATE_ENV: [&str; 4] = ["EXPO_PUBLIC_DEVICE_LAB", "EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL", "EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN", "EXPO_PUBLIC_AGENT_BASE_URL"];

const PUBLISH_NEXT: &str = "re-run this CI run so its eas-update job publishes this commit's update (the plan publishes whenever the app changed since the latest one); a `wf ship eas` by hand lacks the lab flag, backend hosts and token that job bakes in and is refused here too; an update test never falls back to the dev Metro";

fn same_commit(a: &str, b: &str) -> bool {
    !a.is_empty() && !b.is_empty() && (a.starts_with(b) || b.starts_with(a))
}

/// The update units of commit `sha` load, given how this push's eas-update job ended (`result`, empty on a dispatch): its own publish,
/// else the pointer's, which must carry this commit's app by `crate::legs::app_changed_since`, the rule that decides to publish.
fn resolve_update(sha: &str, result: &str, pointer: Result<Value, String>, app: &Path, changed: impl Fn(&str) -> Result<Vec<String>, String>) -> Result<UpdateTarget, UpdateProblem> {
    let problem = |error: String, next: &str| UpdateProblem { error, next: next.to_string() };
    let ran = if result.is_empty() { "no eas-update job ran".to_string() } else { format!("eas-update: {result}") };
    let word = |p: &Value, key: &str| p[key].as_str().unwrap_or_default().to_string();
    if matches!(result, "failure" | "cancelled") {
        let last = pointer.as_ref().map_or(String::new(), |p| format!(" (group {} from commit {})", word(p, "group"), word(p, "commit")));
        return Err(problem(format!("this push's EAS publish ended {result}, so latest-dev.json still names the last one{last}: the update tests have no update of this commit to load"), PUBLISH_NEXT));
    }
    let pointer = pointer.map_err(|e| problem(format!("cannot read the latest update ({ran}): {e}"), PUBLISH_NEXT))?;
    let (group, commit) = (word(&pointer, "group"), word(&pointer, "commit"));
    let named = format!("group {group} from commit {commit}");
    if group.is_empty() || commit.is_empty() || !group.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(problem(format!("latest-dev.json names no usable update ({ran}): {}", pointer.to_string().chars().take(200).collect::<String>()), PUBLISH_NEXT));
    }
    if result == "success" {
        if !same_commit(&commit, sha) {
            return Err(problem(format!("latest-dev.json names commit {commit}, but this push published {sha}: the pointer write failed or another publish moved it"), PUBLISH_NEXT));
        }
    } else if !same_commit(&commit, sha) {
        match changed(&commit) {
            Ok(why) if why.is_empty() => {}
            Ok(why) => {
                let shown: Vec<String> = why.into_iter().take(3).collect();
                return Err(problem(format!("latest-dev.json names {named}, and the app changed since: {}. That update is not this commit's app, and this push published none ({ran})", shown.join("; ")), PUBLISH_NEXT));
            }
            Err(e) => return Err(problem(format!("cannot tell whether the update from commit {commit} carries this commit's app ({ran}): {e}"), "fetch the history of both commits (`git fetch --unshallow origin dev`), run `pnpm install --frozen-lockfile` and rerun")),
        }
    }
    let platforms: Vec<&str> = pointer["platforms"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !platforms.contains(&"android") {
        return Err(problem(format!("latest-dev.json names an update without Android: {named}, platforms [{}]", platforms.join(", ")), PUBLISH_NEXT));
    }
    let route = pointer["initialRoute"].as_str().unwrap_or_default();
    if route != "/" {
        return Err(problem(format!("latest-dev.json names an update that opens on {route:?}, not on /: {named}"), PUBLISH_NEXT));
    }
    let baked: Vec<&str> = pointer["env"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if let Some(missing) = UPDATE_ENV.iter().find(|n| !baked.contains(n)) {
        return Err(problem(format!("the update did not bake in {missing}, which the Android tests need ({UPDATE_ENV:?}): {named}"), PUBLISH_NEXT));
    }
    if let Some(moved) = crate::eas::moved_hosts(&pointer).first() {
        return Err(problem(format!("{moved}: the update dials a backend name that no longer answers ({named})"), PUBLISH_NEXT));
    }
    let app_dir = app.join("frontend/mla-tamagui");
    // the publish gave up waiting for EAS to serve the group (the pointer is stored anyway): it may serve it by now, the manifest decides
    if pointer["served"]["status"] != 200 {
        if let Some(error) = crate::eas::served(&app_dir, &group, "android")["error"].as_str() {
            return Err(problem(format!("EAS does not serve {named} to an Android device: {error}"), PUBLISH_NEXT));
        }
    }
    let Some(project) = crate::eas::config_field(&app_dir, "projectId") else {
        return Err(problem("frontend/mla-tamagui/app.config.js names no projectId".to_string(), "restore the EAS projectId in app.config.js"));
    };
    let open = format!("exp://u.expo.dev/{project}/group/{group}");
    Ok(UpdateTarget { base: format!("{open}/--"), open, group, commit })
}

/// One resolution per process and job: the first update unit reads the pointer, checks the tree against it and caches the answer.
static UPDATE: std::sync::Mutex<Option<(String, Result<UpdateTarget, UpdateProblem>)>> = std::sync::Mutex::new(None);

pub fn update_target(sha: &str) -> Result<UpdateTarget, UpdateProblem> {
    let result = std::env::var("WF_EAS_UPDATE_RESULT").unwrap_or_default().trim().to_string();
    let key = format!("{sha} {result}");
    if let Some((k, answer)) = UPDATE.lock().ok().and_then(|g| g.clone()) {
        if k == key {
            return answer;
        }
    }
    let root = crate::e2e_tests::repo();
    let answer = resolve_update(sha, &result, crate::eas::latest_pointer("dev"), &root, |commit| crate::legs::app_changed_since(commit, Some(&root)));
    if let Ok(mut g) = UPDATE.lock() {
        *g = Some((key, answer.clone()));
    }
    answer
}

/// Connects adb to a TCP serial (`127.0.0.1:5555`) and waits up to 60 s for the device to answer.
pub fn connect_serial(serial: &str) -> Result<String, String> {
    let serial = serial.to_string();
    let adb = |args: &[&str]| Command::new("adb").args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let start = Instant::now();
    loop {
        adb(&["connect", &serial]);
        if adb(&["-s", &serial, "get-state"]) == "device" {
            return Ok(serial);
        }
        if start.elapsed() > Duration::from_secs(60) {
            return Err(format!("adb cannot reach the device at {serial}"));
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}
