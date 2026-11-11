//! The agent-shell routes: thin handlers over `application::agent_shell`, which maps each refusal to Node's status and message.
use crate::api::reply::Reply;
use crate::api::routes::{Call, Failure};
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::agent_shell::Failed;
use vm_ops_api_core::domain::agent_shell::known_tools;
use vm_ops_api_core::domain::js::JsString;
use vm_ops_api_core::domain::request::Target;

const BAD_ARTIFACT_PATH: &str = "path must be a relative path (no \"..\", only [A-Za-z0-9_-./]) under ~/e2e-artifacts";

/// `/services/agent-shell/<action>` with this many segments in all.
fn is_agent_shell_path(target: &Target, length: usize, action: &str) -> bool {
    matches!(target.parts.as_slice(), [services, app, found, ..] if target.parts.len() == length && services == "services" && app == "agent-shell" && found == action)
}

pub fn is_job_status(target: &Target) -> bool {
    is_agent_shell_path(target, 4, "exec-script")
}

pub fn is_job_cancel(target: &Target) -> bool {
    is_agent_shell_path(target, 5, "exec-script") && target.parts[4] == "cancel"
}

pub fn is_ledger(target: &Target) -> bool {
    is_agent_shell_path(target, 4, "vm-ledger")
}

/// A refusal is the route's own reply; an Engine failure is the dispatcher's 502.
pub(super) fn refused(failed: Failed) -> Result<Reply, Failure> {
    let error = |status: u16, message: &str| Reply::json(status, &json!({ "error": message }));
    Ok(match failed {
        Failed::NotRunning => error(503, "agent-shell container is not running"),
        Failed::EmptyScript => error(400, "request body (the script to run) must not be empty"),
        Failed::BadJobId => error(400, "jobId must be a UUID"),
        Failed::NotALabJob => error(404, "not a device-lab job"),
        Failed::UnknownLedger => error(404, "unknown ledger"),
        Failed::LedgerNotFound => error(404, "ledger not found"),
        Failed::BadArtifactPath => error(400, BAD_ARTIFACT_PATH),
        Failed::ArtifactNotFound(path) => Reply::json(404, &json!({ "error": "artifact not found", "path": path })),
        Failed::ArtifactEscapes => error(400, "resolved path escapes the allowed artifact directory"),
        Failed::NotJson => error(400, "body must be JSON"),
        Failed::UnknownTool => Reply::json(400, &json!({ "error": "unknown tool", "knownTools": known_tools() })),
        Failed::EmptyContent => error(400, "content must be a non-empty base64 string"),
        Failed::InvalidBase64 => error(400, "content is not valid base64"),
        Failed::MkdirFailed { exit_code, output } => Reply::json(502, &json!({ "error": "mkdir failed inside agent-shell", "exitCode": exit_code, "output": output })),
        Failed::ArchiveFailed { status, body } => Reply::json_text(502, format!(r#"{{"error":"docker archive write failed","statusCode":{status},"body":{}}}"#, body.to_json())),
        Failed::Engine(error) => return Err(error.into()),
    })
}

pub async fn start(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    match state.agent_shell.start_job(call.body).await {
        Ok(job_id) => Ok(Reply::json(202, &json!({ "jobId": job_id }))),
        Err(failed) => refused(failed),
    }
}

pub async fn status(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    let job_id = &call.target.parts[3];
    match state.agent_shell.job_status(job_id).await {
        Ok(job) => {
            let (id, output) = (JsString::from(job_id.as_str()).to_json(), job.output.to_json());
            Ok(Reply::json_text(200, format!(r#"{{"jobId":{id},"running":{},"exitCode":{},"output":{output}}}"#, job.running, job.exit_code_json())))
        }
        Err(failed) => refused(failed),
    }
}

pub async fn cancel(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    match state.agent_shell.cancel_job(&call.target.parts[3]).await {
        Ok(()) => Ok(Reply::json(200, &json!({ "ok": true }))),
        Err(failed) => refused(failed),
    }
}

pub async fn ledger(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    match state.agent_shell.read_ledger(&call.target.parts[3]).await {
        Ok(ledger) => Ok(Reply::text(200, ledger.content_type, ledger.text)),
        Err(failed) => refused(failed),
    }
}

pub async fn artifact(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    let path = call.target.query_param("path").unwrap_or_default();
    match state.agent_shell.read_artifact(&path).await {
        Ok(found) => Ok(Reply::json(200, &json!({ "path": found.path, "base64": found.base64 }))),
        Err(failed) => refused(failed),
    }
}

pub async fn credentials(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    match state.agent_shell.write_credentials(call.body).await {
        Ok(written) => Ok(Reply::json(200, &json!({ "ok": true, "tool": written.tool, "path": written.path, "bytes": written.bytes }))),
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
    fn given_agent_shell_paths_when_matched_then_each_route_takes_its_own_segment_count_and_doubled_slashes_do_not_matter() {
        let job = "/services/agent-shell/exec-script/0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab";
        assert!(is_job_status(&target(job)));
        assert!(is_job_status(&target("/services//agent-shell//exec-script//0b9f5c2e-8a1d-4c3e-9f7a-1234567890ab")));
        assert!(is_job_cancel(&target(&format!("{job}/cancel"))));
        assert!(!is_job_status(&target(&format!("{job}/cancel"))));
        assert!(!is_job_cancel(&target(&format!("{job}/stop"))));
        assert!(is_ledger(&target("/services/agent-shell/vm-ledger/pool-cooldowns.json")));
        assert!(!is_ledger(&target("/services/leetcode-backend/vm-ledger/x")));
        assert!(!is_job_status(&target("/services/agent-shell/exec-script")));
    }

    #[test]
    fn given_the_refusals_when_mapped_then_each_has_nodes_status_and_a_lone_surrogate_survives_in_the_archive_body() {
        let status = |failed: Failed| refused(failed).ok().map(|reply| reply.status.as_u16());
        assert_eq!(status(Failed::NotRunning), Some(503));
        assert_eq!(status(Failed::LedgerNotFound), Some(404));
        assert_eq!(status(Failed::InvalidBase64), Some(400));
        let cut = JsString::from("\u{1f600}").first(1);
        let reply = refused(Failed::ArchiveFailed { status: 500, body: cut }).ok().expect("a reply");
        assert_eq!((reply.status.as_u16(), String::from_utf8_lossy(&reply.body).to_string()), (502, r#"{"error":"docker archive write failed","statusCode":500,"body":"\ud83d"}"#.to_string()));
    }
}
