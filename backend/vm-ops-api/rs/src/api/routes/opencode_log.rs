use crate::api::reply::Reply;
use crate::api::routes::Call;
use crate::api::AppState;
use serde_json::{json, Value};
use vm_ops_api_core::domain::request::Target;
use vm_ops_api_core::domain::validators::is_valid_run_id;

const RUNS: &str = "/opencode-log/api/runs/";

/// `/opencode-log`, `/opencode-log/` and `/opencode-log/<runId>`, which includes `/opencode-log/api`.
pub fn is_dashboard(target: &Target) -> bool {
    let rest = target.pathname.strip_prefix("/opencode-log/");
    target.is_path("/opencode-log") || rest.is_some_and(|rest| rest.is_empty() || is_valid_run_id(rest))
}

/// The run id after `/opencode-log/api/runs/`, whether or not it is a valid one: an invalid one is a run that is not found.
pub fn run_id_of_detail(target: &Target) -> Option<&str> {
    target.pathname.strip_prefix(RUNS)
}

pub fn dashboard(state: &AppState) -> Reply {
    Reply::html(200, state.dashboard.to_vec())
}

pub async fn runs(call: &Call<'_>, state: &AppState) -> Reply {
    let agent = call.target.query_param("agent").unwrap_or_default();
    let q = call.target.query_param("q").unwrap_or_default();
    let runs: Vec<Value> = state.opencode_log.list(&agent, &q).await.into_iter().map(Value::Object).collect();
    Reply::json(200, &json!({ "runs": runs }))
}

pub async fn run(call: &Call<'_>, state: &AppState) -> Reply {
    let run_id = run_id_of_detail(call.target).unwrap_or_default();
    match state.opencode_log.detail(run_id).await {
        Some(detail) => Reply::json(200, &detail),
        None => Reply::json(404, &json!({ "error": "run not found" })),
    }
}
