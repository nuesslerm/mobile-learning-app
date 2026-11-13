//! `POST /state/{verb}` (CI port only): starts the fixed backup or verify script on agent-shell, 202 with the job id; `GET /state/jobs/{job}`: how it is going.
use crate::api::reply::Reply;
use crate::api::routes::{Call, Failure};
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::agent_shell::Failed;
use vm_ops_api_core::application::state_job::Refused;
use vm_ops_api_core::domain::js::JsString;
use vm_ops_api_core::domain::request::Target;

pub fn is_start(target: &Target) -> bool {
    matches!(target.parts.as_slice(), [state, _] if state == "state")
}

pub fn is_status(target: &Target) -> bool {
    matches!(target.parts.as_slice(), [state, jobs, _] if state == "state" && jobs == "jobs")
}

fn refused(refusal: Refused) -> Result<Reply, Failure> {
    let error = |status: u16, message: &str| Reply::json(status, &json!({ "error": message }));
    Ok(match refusal {
        Refused::BadVerb => error(400, "the verb must be backup or verify"),
        Refused::UnknownJob => error(404, "no such state job"),
        Refused::Failed(Failed::NotRunning) => error(503, "agent-shell container is not running"),
        Refused::Failed(Failed::BadJobId) => error(400, "jobId must be a UUID"),
        Refused::Failed(Failed::Engine(error)) => return Err(error.into()),
        Refused::Failed(_) => error(502, "agent-shell could not run the job"),
    })
}

pub async fn start(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    match state.state_jobs.start(&call.target.parts[1]).await {
        Ok(job_id) => Ok(Reply::json(202, &json!({ "jobId": job_id }))),
        Err(refusal) => refused(refusal),
    }
}

pub async fn status(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    let job_id = &call.target.parts[2];
    match state.state_jobs.status(job_id).await {
        Ok(job) => {
            let (id, output) = (JsString::from(job_id.as_str()).to_json(), job.output.to_json());
            Ok(Reply::json_text(200, format!(r#"{{"jobId":{id},"running":{},"exitCode":{},"output":{output}}}"#, job.running, job.exit_code_json())))
        }
        Err(refusal) => refused(refusal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(raw: &str) -> Target {
        Target::parse(raw).expect("a target")
    }

    #[test]
    fn given_state_paths_when_matched_then_a_start_has_two_segments_and_a_status_three() {
        assert!(is_start(&target("/state/backup")));
        assert!(!is_start(&target("/state/jobs/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab")));
        assert!(is_status(&target("/state/jobs/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab")));
        assert!(!is_status(&target("/state/other/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab")));
        assert!(!is_start(&target("/services/backup")));
    }
}
