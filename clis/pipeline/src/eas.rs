use std::path::Path;
use std::time::Duration;

pub fn seconds(d: Duration) -> f64 {
    (d.as_millis() as f64 / 100.0).round() / 10.0
}

const UPDATES_URL: &str = "https://u.expo.dev";
const SERVED_TIMEOUT: Duration = Duration::from_secs(120);

/// The largest process the publish ran (the export's Node, in practice), in MB.
pub fn children_peak_rss_mb() -> Option<f64> {
    // SAFETY: `rusage` is plain data that getrusage fills in; zeroed is a valid start.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: writes only into the local above.
    if unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut usage) } != 0 {
        return None;
    }
    // ru_maxrss is in KB on Linux and in bytes on macOS
    let kb = if cfg!(target_os = "macos") { usage.ru_maxrss as f64 / 1024.0 } else { usage.ru_maxrss as f64 };
    Some((kb / 1024.0).round())
}

fn served_timeout() -> Duration {
    std::env::var("WF_EAS_SERVED_TIMEOUT_S").ok().and_then(|s| s.trim().parse::<u64>().ok()).filter(|s| *s > 0).map_or(SERVED_TIMEOUT, Duration::from_secs)
}

/// The branch's pointer file name, in the pointer dir and under `eas/` of the state ref.
pub fn latest_file(branch: &str) -> String {
    format!("latest-{}.json", branch.replace('/', "-"))
}

/// The text of the error `latest_pointer` returns when no update was stored yet (as against one it could not read).
pub const NO_POINTER: &str = "is not in refs/wf/state yet";

/// The tailnet names the pointer's bundle baked in that have since moved (`wf vm net resolve` answers otherwise now): the device
/// dials exactly those names, so such an update is stale although the app's code is not.
pub fn moved_hosts(pointer: &serde_json::Value) -> Vec<String> {
    let baked = pointer["hosts"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_str);
    baked.filter_map(|h| wf_vmops::tailnet_resolve::drifted(h).map(|now| format!("the tailnet name {h} the update baked in is now {now}"))).collect()
}

pub fn pointer_dir() -> Option<String> {
    std::env::var("WF_EAS_POINTER_DIR").ok().map(|d| d.trim().to_string()).filter(|d| !d.is_empty())
}

/// The pointer for the branch: the file the same run's eas-update job left in `WF_EAS_POINTER_DIR` when that is set, else the one
/// that job pushed to `eas/` of the state ref (`wf bucket state`). The update the tests and the planner count as latest. No ref
/// or no file yet is an error containing `NO_POINTER`, which the planner reads as "publish".
pub fn latest_pointer(branch: &str) -> Result<serde_json::Value, String> {
    if let Some(dir) = pointer_dir() {
        let path = Path::new(&dir).join(latest_file(branch));
        let body = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        return serde_json::from_slice(&body).map_err(|e| format!("{} is not JSON: {e}", path.display()));
    }
    let key = format!("eas/{}", latest_file(branch));
    let body = wf_bucket::state::read(&key)?.ok_or_else(|| format!("{key} {NO_POINTER}"))?;
    serde_json::from_str(&body).map_err(|e| format!("{key} is not JSON: {e}"))
}

/// A string field of the app's `app.config.js` (`projectId`, `runtimeVersion`), read as text.
pub fn config_field(app: &Path, name: &str) -> Option<String> {
    let config = std::fs::read_to_string(app.join("app.config.js")).ok()?;
    regex::Regex::new(&format!(r#"{name}:\s*['"]([^'"]+)['"]"#)).ok()?.captures(&config)?.get(1).map(|m| m.as_str().to_string())
}

/// Polls the group's manifest as a device asks for it until EAS serves it: `s` is the seconds after the upload returned.
pub fn served(app: &Path, group: &str, platform: &str) -> serde_json::Value {
    let (Some(project), Some(runtime)) = (config_field(app, "projectId"), config_field(app, "runtimeVersion")) else {
        return serde_json::json!({ "platform": platform, "error": format!("{} names no projectId or runtimeVersion", app.join("app.config.js").display()) });
    };
    let base = std::env::var("WF_EAS_UPDATES_URL").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| UPDATES_URL.to_string());
    let url = format!("{}/{project}/group/{group}", base.trim_end_matches('/'));
    let (started, timeout) = (std::time::Instant::now(), served_timeout());
    let mut last = String::new();
    while started.elapsed() < timeout {
        let asked = cli_http::Request::get(&url)
            .header("expo-platform", platform)
            .header("expo-protocol-version", "1")
            .header("expo-runtime-version", &runtime)
            .header("accept", "multipart/mixed,application/expo+json,application/json")
            .timeout(Duration::from_secs(15))
            .send();
        match asked {
            Ok(r) if r.status == 200 => return serde_json::json!({ "platform": platform, "runtimeVersion": runtime, "url": url, "status": 200, "s": seconds(started.elapsed()) }),
            Ok(r) => last = format!("HTTP {}", r.status),
            Err(e) => last = e,
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    serde_json::json!({ "platform": platform, "runtimeVersion": runtime, "url": url, "error": format!("not served within {} s, last answer: {last}", timeout.as_secs()) })
}
