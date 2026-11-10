//! `POST /api/edit-stats`: the client half of an edit turn's latency record, validated, written as a `side = 'client'` row, and
//! answered 202 whether or not the table is configured (the client must never wait on, or fail because of, a stats row).

use super::{respond, Call};
use rca_service_core::application::edit_stats::client_stat_to_row;
use rca_service_core::application::log;
use axum::response::Response;
use serde_json::{json, Value};

pub async fn post(Call { app, body, .. }: Call) -> Response {
    let body = body.unwrap_or(Value::Null);
    match client_stat_to_row(&body, &app.edit_stats.host) {
        Err(message) => respond(400, &json!({ "error": "invalid_edit_stat", "message": message })),
        Ok(row) => {
            let field = |name: &str| body.get(name).cloned().unwrap_or(Value::Null);
            log::info!("edit_stats.client_turn", editId = field("editId"), editSource = field("source"), outcome = field("outcome"), transcribeWallMs = field("transcribeWallMs"), planWallMs = field("planWallMs"), postConfirmWaitMs = field("postConfirmWaitMs"), speculativeHit = field("speculativeHit"), localFastPath = field("localFastPath"));
            app.edit_stats.record(row);
            respond(202, &json!({ "ok": true, "stored": app.edit_stats.enabled() }))
        }
    }
}
