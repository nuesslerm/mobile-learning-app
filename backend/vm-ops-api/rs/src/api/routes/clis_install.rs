use crate::api::reply::Reply;
use crate::api::routes::{Call, Failure};
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::clis_install::Refused;

/// `POST /clis/install?sha256=<64 hex>`: 200 with the installed tree once the release asset hashes to what the caller named and `current` points at it.
pub async fn answer(call: &Call<'_>, state: &AppState) -> Result<Reply, Failure> {
    let sha256 = call.target.query_param("sha256");
    match state.clis_install.install(sha256.as_deref()).await {
        Ok(tree) => Ok(Reply::json(200, &json!({ "installed": true, "tree": tree }))),
        Err(Refused::BadSha) => Ok(Reply::json(400, &json!({ "error": "sha256 must be 64 lowercase hex digits" }))),
        Err(Refused::Failed(message)) => Ok(Reply::json(502, &json!({ "error": message }))),
    }
}
