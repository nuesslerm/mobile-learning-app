//! `GET /agent/v1/socket`: the rca agent API (`rca_agent_core`). One JSON request per text frame, one response frame for each, and the events of
//! the streams a request opens (`thread.subscribe`) pushed as event frames. The bearer is a device bearer (`POST /agent/v1/bearers` mints one);
//! the service secret opens the socket no longer. The socket's lifecycle is `infrastructure::agent_socket`'s, reached through `App`; this handler authenticates and hands over.

use super::Req;
use crate::api::app::App;
use crate::api::auth::{agent_socket_bearer, unauthorized};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

pub async fn socket(State(app): State<Arc<App>>, req: Req, upgrade: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>) -> Response {
    let Some(bearer) = agent_socket_bearer(app.bearers.as_ref(), &req) else {
        return unauthorized();
    };
    match upgrade {
        Ok(upgrade) => {
            tracing::info!(event = "agent_socket.opened", bearer = %bearer.id);
            upgrade.on_upgrade(move |socket| async move { app.serve_agent_socket(socket).await })
        }
        Err(rejection) => rejection.into_response(),
    }
}
