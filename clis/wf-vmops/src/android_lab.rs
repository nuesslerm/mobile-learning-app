//! The device lab as a CI job reaches it: three fixed verbs on the CI port, each a job in agent-shell whose script the server owns, and
//! that job's status. A run needs `WF_VM_CI_PORT` (the lab routes are CI-port routes), never the admin port.
use crate::vmops::{encode_uri_component, request_json, Opts};
use serde_json::Value;
use std::time::Duration;

const POLL: Duration = Duration::from_secs(2);
const POLL_RETRY: Duration = Duration::from_secs(5);
const MAX_FAILED_POLLS: u32 = 30;

/// The start path of a lab verb; none for a verb the lab does not serve.
pub fn path_of(verb: &str, expo_go: bool) -> Option<&'static str> {
    match (verb, expo_go) {
        ("start-device", true) => Some("/android/device/start?expoGo=1"),
        ("start-device", false) => Some("/android/device/start"),
        ("stop-device", _) => Some("/android/device/stop"),
        ("list-devices", _) => Some("/android/device/list"),
        _ => None,
    }
}

/// Runs the lab verb and returns the job's exit code and output once it ends.
pub fn run(verb: &str, expo_go: bool) -> Result<(i64, String), String> {
    let path = path_of(verb, expo_go).ok_or_else(|| format!("the device lab serves start-device, stop-device and list-devices, not {verb}"))?;
    let started = request_json(path, &Opts { ok_statuses: &[202], ..Opts::post() })?;
    let job = started.get("jobId").and_then(Value::as_str).filter(|id| !id.is_empty()).ok_or("the device lab answered no job id")?.to_string();
    let status_path = format!("/android/device/jobs/{}", encode_uri_component(&job));
    let mut failed = 0;
    loop {
        match request_json(&status_path, &Opts::get()) {
            Ok(v) => {
                failed = 0;
                if v.get("running").and_then(Value::as_bool).unwrap_or(false) {
                    std::thread::sleep(POLL);
                    continue;
                }
                let output = v.get("output").and_then(Value::as_str).unwrap_or_default().to_string();
                return Ok((v.get("exitCode").and_then(Value::as_i64).unwrap_or(0), output));
            }
            Err(_) if failed < MAX_FAILED_POLLS => {
                failed += 1;
                std::thread::sleep(POLL_RETRY);
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_lab_verbs_when_their_paths_are_read_then_each_is_a_fixed_ci_route_and_anything_else_has_none() {
        assert_eq!(path_of("start-device", true), Some("/android/device/start?expoGo=1"));
        assert_eq!(path_of("start-device", false), Some("/android/device/start"));
        assert_eq!(path_of("stop-device", false), Some("/android/device/stop"));
        assert_eq!(path_of("list-devices", false), Some("/android/device/list"));
        for other in ["lease", "adb", "exec", "start-device; id", ""] {
            assert_eq!(path_of(other, false), None, "{other}");
        }
        for verb in ["start-device", "stop-device", "list-devices"] {
            assert!(crate::vmops::is_ci_route("POST", path_of(verb, true).unwrap_or_default()), "{verb}");
        }
    }
}
