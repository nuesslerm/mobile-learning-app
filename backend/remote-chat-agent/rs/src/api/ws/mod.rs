//! The WebSocket transport (`ws/server.ts`): `/ws` on the same port as the HTTP routes, a shared-secret check on the upgrade, a hello,
//! requests and subscriptions in the envelopes `envelope.rs` checks, and a server-driven heartbeat (a ping every 15 s; a client that
//! has not answered two of them in a row is cut off). Framing and the handshake are axum's.

pub mod envelope;
pub mod methods;
mod subscriptions;

use rca_service_core::application::opencode_client::Client;
use remote_chat_agent_core::application::reactor::Orchestrator;
use rca_service_core::application::log;
use axum::extract::ws::{Message, WebSocket};
use envelope::{error_response, ok_response, Envelope};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use subscriptions::Subscriptions;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub const PATH: &str = "/ws";
pub const PROTOCOL_VERSION: u32 = 1;
/// How often the server pings a connection, unless the app is built with another interval.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(15_000);
const MAX_MISSED_PONGS: u32 = 2;

/// What every connection needs: the engine behind the thread view, opencode for its models, the version it reports and the secret
/// the upgrade is checked against (none: anyone may connect).
pub struct Context {
    pub orch: Arc<Orchestrator>,
    pub opencode: Client,
    pub version: &'static str,
    pub secret: Option<String>,
    pub heartbeat: Duration,
}

/// The write half of a connection, shared by whoever has something to send: messages queue here and one task writes them out.
#[derive(Clone)]
pub struct Socket {
    outgoing: mpsc::UnboundedSender<Message>,
}

impl Socket {
    /// A closing socket is the receiver's business: a failed send is dropped.
    pub fn send_json(&self, value: &Value) {
        let _ = self.outgoing.send(Message::Text(value.to_string().into()));
    }

    fn ping(&self) -> bool {
        self.outgoing.send(Message::Ping(Vec::new().into())).is_ok()
    }
}

/// A ping every `interval`; two unanswered in a row cancels `cut`.
async fn heartbeat(socket: Socket, missed: Arc<AtomicU32>, cut: CancellationToken, interval: Duration) {
    let mut ticks = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    loop {
        ticks.tick().await;
        if missed.load(Ordering::SeqCst) >= MAX_MISSED_PONGS {
            cut.cancel();
            return;
        }
        missed.fetch_add(1, Ordering::SeqCst);
        if !socket.ping() {
            return;
        }
    }
}

pub async fn run(ctx: Arc<Context>, connection: WebSocket) {
    let (mut sink, mut incoming) = connection.split();
    let (outgoing, mut queued) = mpsc::unbounded_channel::<Message>();
    let mut writer = tokio::spawn(async move {
        while let Some(message) = queued.recv().await {
            if sink.send(message).await.is_err() {
                return;
            }
        }
        let _ = sink.close().await;
    });
    let socket = Socket { outgoing };
    socket.send_json(&json!({ "kind": "hello", "serverVersion": ctx.version, "protocolVersion": PROTOCOL_VERSION, "heartbeatIntervalMs": ctx.heartbeat.as_millis() as u64 }));

    let missed = Arc::new(AtomicU32::new(0));
    let cut = CancellationToken::new();
    let beating = tokio::spawn(heartbeat(socket.clone(), Arc::clone(&missed), cut.clone(), ctx.heartbeat));
    let subscriptions = Arc::new(Subscriptions::new(socket.clone(), Arc::clone(&ctx)));

    loop {
        let message = tokio::select! {
            () = cut.cancelled() => break,
            message = incoming.next() => message,
        };
        match message {
            Some(Ok(Message::Text(text))) => handle_message(&ctx, &socket, &subscriptions, text.as_bytes()).await,
            Some(Ok(Message::Binary(bytes))) => handle_message(&ctx, &socket, &subscriptions, &bytes).await,
            Some(Ok(Message::Pong(_))) => missed.store(0, Ordering::SeqCst),
            // The library answers a ping with a pong by itself.
            Some(Ok(Message::Ping(_))) => {}
            Some(Ok(Message::Close(_)) | Err(_)) | None => break,
        }
    }
    subscriptions.cleanup();
    beating.abort();
    drop(socket);
    drop(subscriptions);
    // What is queued is flushed (the close reply included); a request still running holds the queue open, so the wait is bounded.
    if tokio::time::timeout(Duration::from_secs(1), &mut writer).await.is_err() {
        writer.abort();
    }
}

async fn handle_message(ctx: &Arc<Context>, socket: &Socket, subscriptions: &Arc<Subscriptions>, bytes: &[u8]) {
    let bad = |message: &str| socket.send_json(&error_response("", "bad_envelope", message));
    let Ok(parsed) = serde_json::from_slice::<Value>(bytes) else { return bad("Invalid JSON") };
    match envelope::parse_envelope(&parsed) {
        Err(message) => bad(&message),
        Ok(Envelope::Ping(ts)) => socket.send_json(&json!({ "kind": "pong", "ts": ts })),
        Ok(Envelope::Unsubscribe { id }) => subscriptions.unsubscribe(&id),
        Ok(Envelope::Subscribe { id, method, params }) => {
            if let Err((code, message)) = subscriptions.subscribe(&id, &method, &params) {
                socket.send_json(&error_response(&id, code, &message));
            }
        }
        Ok(Envelope::Request { id, method, params }) => {
            let (ctx, socket) = (Arc::clone(ctx), socket.clone());
            let slow = methods::is_slow(&method);
            let answer = async move {
                let outcome = tokio::spawn({
                    let ctx = Arc::clone(&ctx);
                    async move { methods::handle(&ctx, &method, &params).await }
                })
                .await;
                socket.send_json(&match outcome {
                    Ok(Ok(result)) => ok_response(&id, result),
                    Ok(Err(e)) => error_response(&id, e.code, &e.message),
                    Err(_) => {
                        log::error!("ws.request.panicked");
                        error_response(&id, "internal_error", "the request failed")
                    }
                });
            };
            // A method that waits on the network must not hold up the messages behind it; the rest answer in the order they came.
            if slow {
                tokio::spawn(answer);
            } else {
                answer.await;
            }
        }
    }
}
