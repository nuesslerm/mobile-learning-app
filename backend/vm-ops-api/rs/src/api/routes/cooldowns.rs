use crate::api::reply::Reply;
use crate::api::AppState;
use serde_json::json;

pub async fn answer(state: &AppState) -> Reply {
    let state = state.cooldown_ledger.read().await;
    Reply::json(200, &json!({ "ledger": state.ledger, "version": state.version }))
}
