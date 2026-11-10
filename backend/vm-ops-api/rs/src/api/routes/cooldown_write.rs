use crate::api::reply::Reply;
use crate::api::routes::Call;
use crate::api::AppState;
use serde_json::json;
use vm_ops_api_core::application::cooldown_writes::{write_cooldowns, Refused};

pub async fn answer(call: &Call<'_>, state: &AppState) -> Reply {
    match write_cooldowns(state.cooldown_ledger.as_ref(), call.body).await {
        Ok(written) if written.ok => Reply::json(200, &json!({ "version": written.version })),
        Ok(stale) => Reply::json(409, &json!({ "error": "stale", "version": stale.version })),
        Err(Refused::NotJson) => Reply::json(400, &json!({ "error": "invalid JSON body" })),
        Err(Refused::BadShape) => Reply::json(400, &json!({ "error": "expected { ledger, version }" })),
    }
}
