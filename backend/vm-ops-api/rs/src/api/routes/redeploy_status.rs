//! `GET /redeploy/status`: the log and the history of the redeploys, read from the host by two siblings. 502 carries the same body
//! when the log's sibling failed.
use crate::api::reply::Reply;
use crate::api::AppState;
use serde_json::Value;
use vm_ops_api_core::domain::js;

pub async fn answer(state: &AppState) -> Reply {
    let status = state.redeploy_status.read().await;
    let history = js::stringify(&Value::Array(status.history));
    let body = format!(r#"{{"log":{},"stderr":{},"history":{history}}}"#, status.log.to_json(), status.stderr.to_json());
    Reply::json_text(if status.ok { 200 } else { 502 }, body)
}
