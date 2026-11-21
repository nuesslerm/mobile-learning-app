//! What runs on the VM, as vm-ops-api lists it: the apps it manages, their containers, the commit each was built from, the health
//! each one answers on the tailnet, the image of the on-demand Android device, and vm-ops-api's own redeploy (`wf ship` and
//! `wf vm ops` both start it).
use crate::vmops::{encode_form, encode_uri_component, request_json, Opts};
use serde_json::Value;

/// The apps `wf ship --status` and `wf vm recover` sweep: vm-ops-api's `KNOWN_APPS`.
pub const RECOVER_APPS: &[&str] = &[
    "leetcode-backend",
    "remote-chat-agent",
    "leetcode-auth-cookie-capture",
    "agent-shell",
    "vnc-auth-session",
    "reporting-service",
    "t3-serve",
    "orca-serve",
];

/// The control plane every app is deployed through. It is no app of its own: it lists no containers and reports its commit itself.
pub const VM_OPS_API: &str = "vm-ops-api";

/// The redroid image of the on-demand Android device; an image build waits while a container of it is up.
pub const ANDROID_IMAGE: &str = "redroid/redroid:14.0.0_64only-latest";

/// The image an app runs, when its compose project is not named after it: the desktop app runs `vnc-auth-capture`.
pub fn app_image(app: &str) -> &str {
    match app {
        "vnc-auth-session" => "vnc-auth-capture",
        _ => app,
    }
}

/// Where an app answers `/health` on the tailnet; the apps without an HTTP health have none.
pub fn health_url(app: &str) -> Option<&'static str> {
    match app {
        "leetcode-backend" => Some("http://leetcode-backend-prod:8789/health"),
        "remote-chat-agent" => Some("http://remote-chat-agent-prod:8787/health"),
        "reporting-service" => Some("http://reporting-service:8791/health"),
        "t3-serve" => Some("http://t3-serve:8797/health"),
        "orca-serve" => Some("http://orca-serve:6768/health"),
        _ => None,
    }
}

/// The spare port a smoke test's throwaway container listens on, beside the app's own.
pub const SMOKE_PORT: u16 = 18999;

/// `/health` of an app's smoke throwaway: the app's tailnet host, on the spare smoke port.
pub fn smoke_health_url(app: &str) -> Option<String> {
    let url = health_url(app)?;
    let (host, rest) = url.strip_prefix("http://")?.split_once(':')?;
    let path = rest.split_once('/')?.1;
    Some(format!("http://{host}:{SMOKE_PORT}/{path}"))
}

/// vm-ops-api answers a smoke test after it provisions, syncs the env, pulls the image and waits up to 60 s for the container
/// (`SMOKE_TRIES` in its redeploy script); a typical one takes 20 s, so 30 s cut slow pulls off while the server was still fine.
const SMOKE_TIMEOUT_SECS: u32 = 240;

/// Starts the smoke test of `app`'s image at `tag` (built at `commit`): the container is left running on the spare port. A failed test is
/// an `Err` carrying the container's log.
pub fn smoke_app(app: &str, commit: &str, tag: &str) -> Result<Value, String> {
    let opts = Opts { timeout_sec: SMOKE_TIMEOUT_SECS, ..Opts::post() };
    request_json(&format!("/services/{}/smoke?tag={}&ref={}", encode_uri_component(app), encode_form(tag), encode_form(commit)), &opts)
}

/// Starts vm-ops-api's own smoke test of the image at `tag`: the throwaway container is left running on the spare port.
pub fn smoke_self(commit: &str, tag: &str) -> Result<Value, String> {
    request_json(&format!("/redeploy/smoke?ref={}&tag={}", encode_form(commit), encode_form(tag)), &Opts { timeout_sec: SMOKE_TIMEOUT_SECS, ..Opts::post() })
}

pub fn smoke_stop_self() -> Result<Value, String> {
    request_json("/redeploy/smoke/stop", &Opts { timeout_sec: 30, ..Opts::post() })
}

/// A signed `/health` of vm-ops-api's smoke container, over the tailnet, on the spare port of the caller's lane.
pub fn probe_self_smoke() -> Result<String, String> {
    let lane = crate::vmops::lane_for("POST", "/redeploy/smoke");
    let reply = crate::vmops::send_on(lane, lane.smoke_port(), "GET", "/health", None, std::time::Duration::from_secs(5))?;
    if reply.status == 200 { Ok(reply.text()) } else { Err(format!("HTTP {}: {}", reply.status, reply.text())) }
}

/// Removes the smoke container of `app`, whether or not one is running.
pub fn smoke_stop_app(app: &str) -> Result<Value, String> {
    request_json(&format!("/services/{}/smoke/stop", encode_uri_component(app)), &Opts { timeout_sec: 30, ..Opts::post() })
}

pub fn ps(app: &str) -> Result<Vec<Value>, String> {
    let v = request_json(&format!("/services/{}/ps", encode_uri_component(app)), &Opts::get())?;
    Ok(v.get("containers").and_then(Value::as_array).cloned().unwrap_or_default())
}

/// The commit a service was built from, or none: vm-ops-api reports its own in `/health`, any other app's is the `sha-<short>` tag of its
/// running image. A container that was created or stopped runs nothing: a redeploy that died half-way leaves the tag on containers that never started.
pub fn running_commit(app: &str) -> Option<String> {
    if app == VM_OPS_API {
        return request_json("/health", &Opts::get()).ok()?["commit"].as_str().filter(|c| !c.is_empty()).map(str::to_string);
    }
    let tag = format!("/{}:sha-", app_image(app));
    match ps(app) {
        Ok(containers) => {
            let running: Vec<&str> = containers.iter().filter(|c| c.get("state").and_then(Value::as_str) == Some("running")).filter_map(|c| c.get("image").and_then(Value::as_str)).collect();
            if running.is_empty() {
                wf_kit::wf_err!("wf: {app} has no running container, planning it as changed");
            }
            running.iter().find_map(|image| image.rsplit_once(&tag[..]).map(|(_, sha)| sha).filter(|sha| !sha.is_empty() && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))).map(str::to_string))
        }
        Err(e) => {
            wf_kit::wf_err!("wf: {app} status unavailable, planning it as changed: {e}");
            None
        }
    }
}

/// vm-ops-api swaps itself to the images CI pushed at `tag` for `commit`. It smoke-tests the pulled image beside
/// the live one and answers once that proof is in: 202 with the swap spawned, or 502 with the test's own output and nothing
/// swapped. The swap ends the server that answered, so its outcome is read from `/redeploy/status` and `/health`.
pub fn redeploy_self(commit: &str, tag: &str) -> Result<Value, String> {
    let opts = Opts { timeout_sec: 30, ok_statuses: &[200, 202], ..Opts::post() };
    request_json(&format!("/redeploy?ref={}&tag={}", encode_form(commit), encode_form(tag)), &opts)
}

/// The recorded failure of the last redeploy of vm-ops-api at `commit`, from `/redeploy/status`: none while it has recorded none, or
/// while the status cannot be read (the swap is replacing the server that serves it).
pub fn recorded_self_failure(commit: &str) -> Option<String> {
    let status = request_json("/redeploy/status", &Opts { timeout_sec: 20, ..Opts::get() }).ok()?;
    let failed = status["history"].as_array()?.iter().rev().find(|entry| entry["key"] == VM_OPS_API && entry["ref"] == commit && entry["outcome"] == "redeploy.failed")?;
    let log: String = status["log"].as_str().unwrap_or_default().lines().rev().take(15).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    Some(format!("vm-ops-api redeploy failed at {commit} (exit {}, restored {}); the swap log ends:\n{log}", failed["exitCode"], failed["restored"].as_str().unwrap_or("none")))
}
