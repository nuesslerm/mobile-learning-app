//! The executor channel: how rca asks the workspace's executor (`rca-exec`) to perform the actions a model takes (`docs/plans/rca-client/protocol.md`).
//! `ExecutorChannel` is the port the MCP bridge and the agent API's file methods call; `Channel` is its implementation over a `FrameSink` (the
//! transport, a WebSocket in the service) that matches result and stream frames to the requests by id.
use async_trait::async_trait;
use exec_proto::{Code, Reply, Request};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, watch};

pub use exec_proto::REPLACED;

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteError {
    /// No channel is open for the workspace.
    NoExecutor,
    /// The channel closed while the action was pending.
    ExecutorGone,
    /// The executor answered `ok: false`.
    Action { code: String, message: String },
}

impl RemoteError {
    pub fn code(&self) -> &str {
        match self {
            RemoteError::NoExecutor => Code::NoExecutor.as_str(),
            RemoteError::ExecutorGone => Code::ExecutorGone.as_str(),
            RemoteError::Action { code, .. } => code,
        }
    }

    pub fn message(&self) -> String {
        match self {
            RemoteError::NoExecutor => "no executor is connected for this workspace".into(),
            RemoteError::ExecutorGone => "the executor disconnected before the action finished".into(),
            RemoteError::Action { message, .. } => message.clone(),
        }
    }
}

/// What a finished action gave back: its result and, for `run_command`, the streamed output in arrival order.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionOutput {
    pub result: Value,
    pub output: String,
}

/// What a streaming action sends before its result: the `event` of a subscribe frame, or output bytes of a pty at their byte offset (a binary
/// frame of `exec_pty::encode_frame`).
#[derive(Debug, Clone, PartialEq)]
pub enum Streamed {
    Event(Value),
    Bytes { offset: u64, data: Vec<u8> },
}

#[async_trait]
pub trait ExecutorChannel: Send + Sync {
    /// Performs one action at the executor. Dropping the returned future before it finishes cancels the action there.
    async fn call(&self, tool: &str, args: Value) -> Result<ActionOutput, RemoteError>;
    /// Like `call`, and the `event` of each subscribe frame the action sends is handed to `events` in order, before the result (an action that
    /// reports progress, `worktree_setup`). A channel with no such frames sends none.
    async fn call_with_events(&self, tool: &str, args: Value, events: mpsc::UnboundedSender<Value>) -> Result<ActionOutput, RemoteError> {
        drop(events);
        self.call(tool, args).await
    }
    /// Like `call_with_events`, and the binary frames the action sends (`pty_attach`) arrive as `Streamed::Bytes` in order with its events. A
    /// channel that cannot stream sends nothing.
    async fn call_stream(&self, tool: &str, args: Value, stream: mpsc::UnboundedSender<Streamed>) -> Result<ActionOutput, RemoteError> {
        drop(stream);
        self.call(tool, args).await
    }
    /// Closes the channel with a WebSocket close code; pending actions fail with `ExecutorGone`.
    fn close(&self, code: u16);
    fn is_closed(&self) -> bool;
    /// Resolves when the channel has closed (at once if it already has).
    async fn closed(&self);
}

/// The transport under a channel: text frames out, and a close.
pub trait FrameSink: Send + Sync {
    /// `false` when the frame could not be handed to the transport.
    fn send(&self, frame: String) -> bool;
    fn close(&self, code: u16);
}

enum Incoming {
    Stream(String),
    Event(Value),
    Bytes(u64, Vec<u8>),
    Done(Result<Value, RemoteError>),
}

#[derive(Default)]
struct State {
    closed: bool,
    pending: HashMap<String, mpsc::UnboundedSender<Incoming>>,
}

pub struct Channel {
    sink: Arc<dyn FrameSink>,
    state: Arc<Mutex<State>>,
    next: AtomicU64,
    closed: watch::Sender<bool>,
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|p| p.into_inner())
}

impl Channel {
    pub fn new(sink: Arc<dyn FrameSink>) -> Arc<Channel> {
        Arc::new(Channel { sink, state: Arc::new(Mutex::new(State::default())), next: AtomicU64::new(1), closed: watch::channel(false).0 })
    }

    /// A frame the executor sent: a stream chunk or the result of a pending action. Anything else is ignored.
    pub fn on_frame(&self, text: &str) {
        let (id, incoming) = match Reply::parse(text) {
            Some(Reply::Output(frame)) => (frame.id, Incoming::Stream(frame.data)),
            Some(Reply::Done { id, outcome }) => (id, Incoming::Done(outcome.map_err(|e| RemoteError::Action { code: e.code, message: e.message }))),
            Some(Reply::Event(frame)) => (frame.id, Incoming::Event(frame.event)),
            None => return,
        };
        let done = matches!(incoming, Incoming::Done(_));
        let mut state = lock(&self.state);
        if let Some(waiting) = state.pending.get(&id) {
            let _ = waiting.send(incoming);
        }
        if done {
            state.pending.remove(&id);
        }
    }

    /// A binary frame the executor sent: pty output for the pending action it names. Bytes that are no frame, or name no pending action, are ignored.
    pub fn on_binary(&self, bytes: &[u8]) {
        let Some((id, offset, data)) = exec_pty::decode_frame(bytes) else { return };
        if let Some(waiting) = lock(&self.state).pending.get(&id) {
            let _ = waiting.send(Incoming::Bytes(offset, data.to_vec()));
        }
    }

    /// The transport is gone (or the channel was closed here): pending actions fail and later calls are refused.
    pub fn shutdown(&self) {
        let pending = {
            let mut state = lock(&self.state);
            state.closed = true;
            std::mem::take(&mut state.pending)
        };
        for (_, waiting) in pending {
            let _ = waiting.send(Incoming::Done(Err(RemoteError::ExecutorGone)));
        }
        self.closed.send_replace(true);
    }
}

/// Sends `cancel` for an action whose caller went away before its result came.
struct CancelOnDrop<'a> {
    channel: &'a Channel,
    id: String,
    armed: bool,
}

impl Drop for CancelOnDrop<'_> {
    fn drop(&mut self) {
        let mut state = lock(&self.channel.state);
        if self.armed && state.pending.remove(&self.id).is_some() && !state.closed {
            let cancel = Request::new(format!("c{}", self.channel.next.fetch_add(1, Ordering::SeqCst)), exec_proc::CANCEL, json!({"id": self.id}));
            self.channel.sink.send(cancel.encode());
        }
    }
}

impl Channel {
    async fn run(&self, tool: &str, args: Value, events: Option<mpsc::UnboundedSender<Streamed>>) -> Result<ActionOutput, RemoteError> {
        let id = format!("a{}", self.next.fetch_add(1, Ordering::SeqCst));
        let (tx, mut rx) = mpsc::unbounded_channel();
        {
            let mut state = lock(&self.state);
            if state.closed {
                return Err(RemoteError::ExecutorGone);
            }
            state.pending.insert(id.clone(), tx);
        }
        let mut guard = CancelOnDrop { channel: self, id: id.clone(), armed: true };
        if !self.sink.send(Request::new(id.clone(), tool, args).encode()) {
            return Err(RemoteError::ExecutorGone);
        }
        let mut output = String::new();
        loop {
            match rx.recv().await {
                Some(Incoming::Stream(data)) => output.push_str(&data),
                Some(Incoming::Event(event)) => {
                    if let Some(events) = &events {
                        let _ = events.send(Streamed::Event(event));
                    }
                }
                Some(Incoming::Bytes(offset, data)) => {
                    if let Some(events) = &events {
                        let _ = events.send(Streamed::Bytes { offset, data });
                    }
                }
                Some(Incoming::Done(result)) => {
                    guard.armed = false;
                    return result.map(|result| ActionOutput { result, output });
                }
                None => {
                    guard.armed = false;
                    return Err(RemoteError::ExecutorGone);
                }
            }
        }
    }

}

#[async_trait]
impl ExecutorChannel for Channel {
    async fn call(&self, tool: &str, args: Value) -> Result<ActionOutput, RemoteError> {
        self.run(tool, args, None).await
    }

    async fn call_with_events(&self, tool: &str, args: Value, events: mpsc::UnboundedSender<Value>) -> Result<ActionOutput, RemoteError> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let forward = async move {
            while let Some(item) = rx.recv().await {
                if let Streamed::Event(event) = item {
                    if events.send(event).is_err() {
                        break;
                    }
                }
            }
        };
        let (done, ()) = tokio::join!(self.run(tool, args, Some(tx)), forward);
        done
    }

    async fn call_stream(&self, tool: &str, args: Value, stream: mpsc::UnboundedSender<Streamed>) -> Result<ActionOutput, RemoteError> {
        self.run(tool, args, Some(stream)).await
    }

    fn close(&self, code: u16) {
        self.sink.close(code);
        self.shutdown();
    }

    fn is_closed(&self) -> bool {
        lock(&self.state).closed
    }

    async fn closed(&self) {
        let mut seen = self.closed.subscribe();
        let _ = seen.wait_for(|closed| *closed).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::RecordingSink;

    async fn first_frame(sink: &RecordingSink) -> Value {
        for _ in 0..100 {
            if let Some(f) = sink.frames().first().cloned() {
                return f;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("no frame was sent");
    }

    #[tokio::test]
    async fn a_call_gets_the_streamed_output_in_order_and_the_result_of_its_own_id() {
        let sink = Arc::new(RecordingSink::default());
        let channel = Channel::new(sink.clone());
        let caller = tokio::spawn({
            let channel = channel.clone();
            async move { channel.call("run_command", json!({"command": "x"})).await }
        });
        let id = first_frame(&sink).await["id"].as_str().unwrap().to_string();
        channel.on_frame(&json!({"id": "other", "ok": true, "result": {}}).to_string());
        channel.on_frame(&json!({"id": id, "stream": "stdout", "data": "a"}).to_string());
        channel.on_frame(&json!({"id": id, "stream": "stderr", "data": "b"}).to_string());
        channel.on_frame(&json!({"id": id, "ok": true, "result": {"exit_code": 0}}).to_string());
        let done = caller.await.unwrap().unwrap();
        assert_eq!((done.output.as_str(), done.result), ("ab", json!({"exit_code": 0})));
    }

    #[tokio::test]
    async fn given_an_action_that_reports_progress_when_called_with_events_then_each_event_arrives_in_order_before_the_result_and_a_plain_call_ignores_them() {
        // Given a channel and two calls of the same action, one with an events receiver and one plain
        let sink = Arc::new(RecordingSink::default());
        let channel = Channel::new(sink.clone());
        let (tx, mut seen) = mpsc::unbounded_channel();
        let with = tokio::spawn({
            let channel = channel.clone();
            async move { channel.call_with_events("worktree_setup", json!({}), tx).await }
        });
        let id = first_frame(&sink).await["id"].as_str().unwrap().to_string();
        // When the executor sends two subscribe frames, a frame of another request, and the result
        channel.on_frame(&json!({"id": id, "seq": 1, "event": {"kind": "setup.started", "steps": 1}}).to_string());
        channel.on_frame(&json!({"id": "other", "seq": 1, "event": {"kind": "stray"}}).to_string());
        channel.on_frame(&json!({"id": id, "seq": 2, "event": {"kind": "setup.finished", "ok": true}}).to_string());
        channel.on_frame(&json!({"id": id, "ok": true, "result": {"ok": true}}).to_string());
        // Then the receiver got the two events in order and nothing else, and the call's result is the executor's
        assert_eq!(with.await.unwrap().unwrap().result, json!({"ok": true}));
        assert_eq!((seen.recv().await, seen.recv().await, seen.recv().await), (Some(json!({"kind": "setup.started", "steps": 1})), Some(json!({"kind": "setup.finished", "ok": true})), None));
        // And a plain call of the same action finishes on its result, the events it was sent going nowhere
        let plain = tokio::spawn({
            let channel = channel.clone();
            async move { channel.call("worktree_setup", json!({})).await }
        });
        let frames = loop {
            let frames = sink.frames();
            if frames.len() >= 2 {
                break frames;
            }
            tokio::task::yield_now().await;
        };
        let id = frames[1]["id"].as_str().unwrap().to_string();
        channel.on_frame(&json!({"id": id, "seq": 1, "event": {"kind": "setup.started"}}).to_string());
        channel.on_frame(&json!({"id": id, "ok": true, "result": {"ok": false}}).to_string());
        assert_eq!(plain.await.unwrap().unwrap().result, json!({"ok": false}));
    }

    #[tokio::test]
    async fn closing_fails_the_pending_call_and_every_later_one_with_executor_gone() {
        let sink = Arc::new(RecordingSink::default());
        let channel = Channel::new(sink.clone());
        let caller = tokio::spawn({
            let channel = channel.clone();
            async move { channel.call("read_file", json!({})).await }
        });
        first_frame(&sink).await;
        channel.shutdown();
        assert_eq!(caller.await.unwrap(), Err(RemoteError::ExecutorGone));
        assert_eq!(channel.call("read_file", json!({})).await, Err(RemoteError::ExecutorGone));
        channel.closed().await;
    }

    #[tokio::test]
    async fn a_dropped_call_sends_cancel_for_its_action() {
        let sink = Arc::new(RecordingSink::default());
        let channel = Channel::new(sink.clone());
        let call = tokio::spawn({
            let channel = channel.clone();
            async move { channel.call("run_command", json!({})).await }
        });
        let id = first_frame(&sink).await["id"].clone();
        call.abort();
        let _ = call.await;
        let frames = sink.frames();
        assert_eq!((frames[1]["tool"].clone(), frames[1]["args"]["id"].clone()), (json!("cancel"), id));
    }

    #[tokio::test]
    async fn an_executor_that_answers_with_an_error_frame_fails_the_call_with_its_code_and_message() {
        // Given a call waiting on an executor
        let sink = Arc::new(RecordingSink::default());
        let channel = Channel::new(sink.clone());
        let caller = tokio::spawn({
            let channel = channel.clone();
            async move { channel.call("read_file", json!({"path": "x"})).await }
        });
        let id = first_frame(&sink).await["id"].clone();
        // When the executor answers ok: false
        channel.on_frame(&json!({"id": id, "ok": false, "error": {"code": "NOT_FOUND", "message": "x is not there"}}).to_string());
        // Then the call fails with that code and message, which the service relays unchanged
        let failed = caller.await.unwrap().unwrap_err();
        assert_eq!((failed.code(), failed.message()), ("NOT_FOUND", "x is not there".to_string()));
    }

    #[tokio::test]
    async fn given_a_streaming_action_when_binary_frames_arrive_then_they_reach_the_stream_in_order_with_the_events_and_a_stranger_frame_is_ignored() {
        // Given a pty attach pending on a channel
        let sink = Arc::new(RecordingSink::default());
        let channel = Channel::new(sink.clone());
        let (tx, mut seen) = mpsc::unbounded_channel();
        let call = tokio::spawn({
            let channel = channel.clone();
            async move { channel.call_stream("pty_attach", json!({"pty": "p"}), tx).await }
        });
        let id = first_frame(&sink).await["id"].as_str().unwrap().to_string();
        // When the executor sends an event, two binary frames (one with bytes no text can hold), a frame of another request, garbage and the result
        channel.on_frame(&json!({"id": id, "seq": 1, "event": {"kind": "pty.attached"}}).to_string());
        channel.on_binary(&exec_pty::encode_frame(&id, 0, &[0, 255, b'a']));
        channel.on_binary(&exec_pty::encode_frame("other", 3, b"x"));
        channel.on_binary(&[9]);
        channel.on_binary(&exec_pty::encode_frame(&id, 3, b"bc"));
        channel.on_frame(&json!({"id": id, "ok": true, "result": {"exited": true}}).to_string());
        // Then the stream got the event and both frames in order with their offsets, and the result is the executor's
        assert_eq!(call.await.unwrap().unwrap().result, json!({"exited": true}));
        let got: Vec<Streamed> = std::iter::from_fn(|| seen.try_recv().ok()).collect();
        assert_eq!(got, [Streamed::Event(json!({"kind": "pty.attached"})), Streamed::Bytes { offset: 0, data: vec![0, 255, b'a'] }, Streamed::Bytes { offset: 3, data: b"bc".to_vec() }]);
    }
}
