//! The device-lab routes: thin handlers over `application::android_lab`. Each starts one fixed verb as a job and answers its id; the
//! status route answers only for a job this lab started.
use crate::api::reply::Reply;
use crate::api::routes::agent_shell::refused;
use crate::api::routes::{Call, Failure};
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::android_lab::Verb;
use vm_ops_api_core::domain::js::JsString;
use vm_ops_api_core::domain::request::Target;

pub fn is_job_status(target: &Target) -> bool {
    matches!(target.parts.as_slice(), [android, device, jobs, _] if android == "android" && device == "device" && jobs == "jobs")
}

/// `?expoGo=1` asks the start verb to install Expo Go once the device has booted; any other query is ignored.
fn wants_expo_go(call: &Call<'_>) -> bool {
    call.target.query_param("expoGo").is_some_and(|value| value == "1")
}

pub async fn start(verb_of: fn(&Call<'_>) -> Verb, call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    match state.android_lab.start(verb_of(call)).await {
        Ok(job_id) => Ok(Reply::json(202, &json!({ "jobId": job_id }))),
        Err(failed) => refused(failed),
    }
}

pub fn start_verb(call: &Call<'_>) -> Verb {
    Verb::Start { expo_go: wants_expo_go(call) }
}

pub fn stop_verb(_: &Call<'_>) -> Verb {
    Verb::Stop
}

pub fn list_verb(_: &Call<'_>) -> Verb {
    Verb::List
}

pub async fn status(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    let job_id = &call.target.parts[3];
    match state.android_lab.status(job_id).await {
        Ok(job) => {
            let (id, output) = (JsString::from(job_id.as_str()).to_json(), job.output.to_json());
            Ok(Reply::json_text(200, format!(r#"{{"jobId":{id},"running":{},"exitCode":{},"output":{output}}}"#, job.running, job.exit_code_json())))
        }
        Err(failed) => refused(failed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(raw: &str) -> Target {
        Target::parse(raw).expect("a target")
    }

    #[test]
    fn given_lab_paths_when_matched_then_only_a_job_status_of_exactly_four_segments_is_one() {
        assert!(is_job_status(&target("/android/device/jobs/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab")));
        assert!(!is_job_status(&target("/android/device/jobs")));
        assert!(!is_job_status(&target("/android/device/jobs/x/y")));
        assert!(!is_job_status(&target("/services/agent-shell/exec-script/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab")));
    }
}
