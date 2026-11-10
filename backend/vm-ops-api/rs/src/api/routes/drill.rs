use crate::api::reply::Reply;
use crate::api::routes::{Call, Failure};
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::drill::Refused;

/// `POST /drill?action=stop|freeze`: 202 before the sibling acts, naming this service's container.
pub fn answer(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    let action = call.target.query_param("action");
    match state.drill.start(action.as_deref()) {
        Ok(accepted) => Ok(Reply::json(202, &json!({ "accepted": true, "action": accepted.action.as_str(), "container": accepted.container }))),
        Err(Refused::UnknownAction) => Ok(Reply::json(400, &json!({ "error": "action must be stop or freeze" }))),
        Err(Refused::NotStarted(message)) => Err(Failure(message)),
    }
}
