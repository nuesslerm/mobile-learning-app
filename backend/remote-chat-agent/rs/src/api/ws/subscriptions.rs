//! A connection's subscriptions (`ws/subscriptions.ts`): each has the client's id, a task of its own that forwards what the engine
//! sends as `{ kind: 'event', id, item }`, and, for a thread, a `complete` once the thread is deleted. The shell subscription never
//! completes. Subscribing under an id that is held ends the old subscription first.

use super::{Context, Socket};
use remote_chat_agent_core::domain::types::{ShellItem, ThreadItem};
use rca_service_core::domain::schema::{parse, NonEmpty};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::unbounded_channel;
use tokio_util::sync::CancellationToken;

type Failure = (&'static str, String);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSubscription {
    thread_id: NonEmpty,
    after_sequence: Option<u64>,
}

struct Active {
    /// Tells a newer subscription under the same id apart from the one that ended by itself.
    generation: u64,
    cancel: CancellationToken,
}

pub struct Subscriptions {
    socket: Socket,
    ctx: Arc<Context>,
    active: Mutex<HashMap<String, Active>>,
    next_generation: AtomicU64,
}

fn event(id: &str, item: Value) -> Value {
    json!({ "kind": "event", "id": id, "item": item })
}

impl Subscriptions {
    pub fn new(socket: Socket, ctx: Arc<Context>) -> Self {
        Self { socket, ctx, active: Mutex::new(HashMap::new()), next_generation: AtomicU64::new(0) }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<String, Active>> {
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn subscribe(self: &Arc<Self>, id: &str, method: &str, params: &Value) -> Result<(), Failure> {
        let invalid = |message: String| ("invalid_params", message);
        let active = match method {
            "orchestration.subscribeShell" => {
                let empty = json!({});
                let params = if params.is_null() { &empty } else { params };
                parse::<Map<String, Value>>(params).map_err(invalid)?;
                self.replace(id);
                self.shell(id)
            }
            "orchestration.subscribeThread" => {
                let ThreadSubscription { thread_id, after_sequence } = parse(params).map_err(invalid)?;
                let after = after_sequence.map(|n| i64::try_from(n).unwrap_or(i64::MAX));
                self.replace(id);
                self.thread(id, &String::from(thread_id), after).map_err(|e| ("subscription_failed", e))?
            }
            other => return Err(("unknown_subscription", format!("Unknown subscription: {other}"))),
        };
        self.held().insert(id.to_string(), active);
        Ok(())
    }

    /// Ends the subscription held under `id` before its successor registers with the engine.
    fn replace(&self, id: &str) {
        if let Some(previous) = self.held().remove(id) {
            previous.cancel.cancel();
        }
    }

    /// Sends a frame unless its subscription was ended: the check and the send happen under the lock that ending one takes, so no
    /// frame of a replaced subscription follows its replacement's first.
    fn deliver(&self, token: &CancellationToken, socket: &Socket, frame: &Value) -> bool {
        let _held = self.held();
        if token.is_cancelled() {
            return false;
        }
        socket.send_json(frame);
        true
    }

    fn start(&self) -> (u64, CancellationToken) {
        (self.next_generation.fetch_add(1, Ordering::SeqCst), CancellationToken::new())
    }

    fn thread(self: &Arc<Self>, id: &str, thread_id: &str, after: Option<i64>) -> Result<Active, String> {
        let (tx, mut rx) = unbounded_channel::<ThreadItem>();
        self.ctx.orch.engine.subscribe_thread_with(thread_id, after, tx)?;
        let (generation, cancel) = self.start();
        let (me, socket, id, token) = (Arc::clone(self), self.socket.clone(), id.to_string(), cancel.clone());
        tokio::spawn(async move {
            loop {
                let item = tokio::select! {
                    biased;
                    () = token.cancelled() => return,
                    item = rx.recv() => match item {
                        Some(item) => item,
                        None => return,
                    },
                };
                if !me.deliver(&token, &socket, &event(&id, item.to_json())) {
                    return;
                }
                if matches!(&item, ThreadItem::Event(e) if e.kind == "thread.deleted") {
                    me.deliver(&token, &socket, &json!({ "kind": "complete", "id": id }));
                    me.forget(&id, generation);
                    return;
                }
            }
        });
        Ok(Active { generation, cancel })
    }

    fn shell(self: &Arc<Self>, id: &str) -> Active {
        let (tx, mut rx) = unbounded_channel::<ShellItem>();
        self.ctx.orch.engine.subscribe_shell_with(tx);
        let (generation, cancel) = self.start();
        let (me, socket, id, token) = (Arc::clone(self), self.socket.clone(), id.to_string(), cancel.clone());
        tokio::spawn(async move {
            loop {
                let item = tokio::select! {
                    biased;
                    () = token.cancelled() => return,
                    item = rx.recv() => match item {
                        Some(item) => item,
                        None => return,
                    },
                };
                if !me.deliver(&token, &socket, &event(&id, item.to_json())) {
                    return;
                }
            }
        });
        Active { generation, cancel }
    }

    /// Drops a subscription that ended by itself, unless the id has been taken by a newer one.
    fn forget(&self, id: &str, generation: u64) {
        let mut held = self.held();
        if held.get(id).is_some_and(|a| a.generation == generation) {
            held.remove(id);
        }
    }

    pub fn unsubscribe(&self, id: &str) {
        if let Some(active) = self.held().remove(id) {
            active.cancel.cancel();
        }
    }

    pub fn cleanup(&self) {
        for (_, active) in self.held().drain() {
            active.cancel.cancel();
        }
    }
}
