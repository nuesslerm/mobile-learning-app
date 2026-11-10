//! The front door: the axum router and its edge layers (`routes`), the WebSocket transport (`ws`), the shared state the handlers
//! reach (`app`), who gets in (`auth`), the origin policy (`cors`) and the wire-format checks of the thread view's commands (`wire`).

pub mod app;
pub mod auth;
pub mod cors;
pub mod routes;
pub mod wire;
pub mod ws;
