//! One socket's side of a request: where a method that opens a stream sends its events, and the work that must start only once the answer to
//! the request that opened it is on its way. The transport (the service's WebSocket) implements `EventSink` and runs what a request left in
//! `Connection::follow`, so a group never touches a socket and a test of one needs none.
use agent_api_proto::Event;
use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// Where a stream's events go: the peer's socket.
#[async_trait]
pub trait EventSink: Send + Sync {
    /// Queues a live `event` for the peer, in the order of the calls. False when the peer is gone or cannot keep up (the transport closes it,
    /// and the peer resumes from the last `seq` it saw), which is when whoever sends should stop.
    fn send(&self, event: &Event) -> bool;

    /// Queues an event of a replay, which the peer asked for and is all in hand already: it waits while the peer is reading the ones before it
    /// instead of counting a long replay as a peer that cannot keep up. False when the peer is gone.
    async fn send_replay(&self, event: &Event) -> bool;
}

/// The work of one stream: it pushes events to the sink until the sink refuses one or the stream ends.
pub type Job = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A stream a request opened, named by its topic.
pub struct Following {
    pub topic: String,
    pub job: Job,
}

/// A sink that carries nothing: for a caller that wants the answers only.
struct Nowhere;

#[async_trait]
impl EventSink for Nowhere {
    fn send(&self, _event: &Event) -> bool {
        false
    }

    async fn send_replay(&self, _event: &Event) -> bool {
        false
    }
}

pub struct Connection {
    sink: Arc<dyn EventSink>,
    following: Mutex<Vec<Following>>,
}

impl Connection {
    pub fn new(sink: Arc<dyn EventSink>) -> Connection {
        Connection { sink, following: Mutex::new(Vec::new()) }
    }

    /// A connection that carries no events, so a method that opens a stream finds it refused at once.
    pub fn answers_only() -> Connection {
        Connection::new(Arc::new(Nowhere))
    }

    pub fn sink(&self) -> Arc<dyn EventSink> {
        Arc::clone(&self.sink)
    }

    /// Opens the stream of `topic`: `job` starts after the answer to the current request was queued, so the peer reads the answer before the
    /// first event. A stream of the same topic that is already open on this connection ends, whoever runs the jobs sees to it.
    pub fn follow(&self, topic: impl Into<String>, job: impl Future<Output = ()> + Send + 'static) {
        self.following.lock().unwrap_or_else(|p| p.into_inner()).push(Following { topic: topic.into(), job: Box::pin(job) });
    }

    /// The streams the request opened, in order; the transport runs them once the answer is queued.
    pub fn take_following(&self) -> Vec<Following> {
        std::mem::take(&mut *self.following.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_connection_that_carries_nothing_when_an_event_is_sent_then_the_sender_is_told_to_stop() {
        // Given a connection for a caller that wants answers only
        let connection = Connection::answers_only();
        let event = Event { seq: 1, thread: Some("t".into()), turn: None, kind: "turn.started".into(), data: Default::default() };
        // When an event is sent, Then the sink refuses it
        assert!(!connection.sink().send(&event));
    }

    #[test]
    fn given_streams_opened_by_a_request_when_taken_then_they_come_in_the_order_opened_and_only_once() {
        // Given a request that opened two streams
        let connection = Connection::answers_only();
        connection.follow("thread/a", async {});
        connection.follow("thread/b", async {});
        // When the transport takes them, Then it gets both in order, and a second take finds none left
        assert_eq!(connection.take_following().iter().map(|f| f.topic.as_str()).collect::<Vec<_>>(), ["thread/a", "thread/b"]);
        assert!(connection.take_following().is_empty());
    }
}
