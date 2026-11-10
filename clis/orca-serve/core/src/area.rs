//! An `Area` is what one mapping crate registers: its served methods, its not-offered rows and the code that answers them. The core knows no
//! method; the composition root hands it the areas and the table is assembled from what they declare.
use crate::ports::{AgentApi, HostInfo};
use crate::wire::{self, Failure};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Unary,
    Streaming,
}

#[derive(Debug, Clone, Copy)]
pub struct Method {
    pub name: &'static str,
    pub kind: Kind,
}

/// A row the owner decided not to offer: it answers `not_offered` with the reason, never `method_not_found`.
#[derive(Debug, Clone, Copy)]
pub struct NotOffered {
    pub name: &'static str,
    pub reason: &'static str,
    pub decided_by: &'static str,
}

/// An open stream. `poll` returns the events since the last call (the first call includes the opening ones); `end` the closing ones.
pub trait Subscription: Send {
    fn id(&self) -> &str;
    fn poll(&mut self) -> Vec<Value>;
    fn end(&mut self) -> Vec<Value>;
}

struct Active {
    request_id: String,
    sub: Box<dyn Subscription>,
    ending: bool,
}

/// The streams of one connection, ended with it.
#[derive(Default)]
pub struct Subscriptions {
    active: Vec<Active>,
}

impl Subscriptions {
    pub(crate) fn add(&mut self, request_id: &str, sub: Box<dyn Subscription>) {
        self.active.push(Active { request_id: request_id.to_string(), sub, ending: false });
    }

    /// Marks the stream to end on the next flush; false when this connection has no such stream.
    pub fn cancel(&mut self, subscription_id: &str) -> bool {
        match self.active.iter_mut().find(|a| !a.ending && a.sub.id() == subscription_id) {
            Some(a) => {
                a.ending = true;
                true
            }
            None => false,
        }
    }

    /// Marks every stream whose id is `prefix` or `prefix:<more>` to end; how many there were.
    pub fn cancel_prefix(&mut self, prefix: &str) -> usize {
        let child = format!("{prefix}:");
        let mut n = 0;
        for a in self.active.iter_mut().filter(|a| !a.ending && (a.sub.id() == prefix || a.sub.id().starts_with(&child))) {
            a.ending = true;
            n += 1;
        }
        n
    }

    /// Every stream's pending events as `(request id, event)`, and the ended streams' closing events, which drop them.
    pub(crate) fn flush(&mut self) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        for a in &mut self.active {
            let events = if a.ending { a.sub.end() } else { a.sub.poll() };
            out.extend(events.into_iter().map(|e| (a.request_id.clone(), e)));
        }
        self.active.retain(|a| !a.ending);
        out
    }

    pub(crate) fn end_all(&mut self) {
        for a in &mut self.active {
            a.ending = true;
        }
        self.flush();
    }

    pub fn len(&self) -> usize {
        self.active.len()
    }

    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

/// What a handler may touch: the connection it answers and the ports. Nothing here outlives the call.
pub struct Ctx<'a> {
    pub runtime_id: &'a str,
    pub connection_id: &'a str,
    /// The id of the request being answered: the phone names a stream it opened by it.
    pub request_id: &'a str,
    pub device_id: &'a str,
    pub agent: &'a dyn AgentApi,
    /// The same port as an owned handle, for a stream that asks rca again after the call returned.
    pub agent_handle: Arc<dyn AgentApi>,
    pub host: &'a HostInfo,
    /// The capabilities the areas advertise, in area order.
    pub capabilities: &'a [String],
    pub client_capabilities: &'a mut Vec<String>,
    pub subscriptions: &'a mut Subscriptions,
    pub(crate) seq: &'a AtomicU64,
}

impl Ctx<'_> {
    /// `<prefix><connection id>-<n>`: ids are counted across connections, so one never repeats within a process.
    pub fn subscription_id(&self, prefix: &str) -> String {
        format!("{prefix}{}-{}", self.connection_id, self.seq.fetch_add(1, Ordering::SeqCst) + 1)
    }
}

pub trait Area: Send + Sync {
    fn name(&self) -> &'static str;
    fn methods(&self) -> &'static [Method];
    fn not_offered(&self) -> &'static [NotOffered] {
        &[]
    }
    /// Capabilities `status.get` advertises because this area serves what they promise.
    fn capabilities(&self) -> &'static [&'static str] {
        &[]
    }
    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure>;
    fn open(&self, _ctx: &mut Ctx, method: &str, _params: &Value) -> Result<Box<dyn Subscription>, Failure> {
        Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {method}")))
    }
}
