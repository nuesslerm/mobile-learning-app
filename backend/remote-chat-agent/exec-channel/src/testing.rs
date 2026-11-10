//! The doubles of the crate's ports for tests: a transport that records what it was given, and a channel that answers from a function.
use crate::remote::{ActionOutput, ExecutorChannel, FrameSink, RemoteError};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

/// A `FrameSink` that keeps the frames sent (parsed) and the close codes.
#[derive(Default)]
pub struct RecordingSink {
    frames: Mutex<Vec<Value>>,
    closes: Mutex<Vec<u16>>,
}

impl RecordingSink {
    pub fn frames(&self) -> Vec<Value> {
        self.frames.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn closes(&self) -> Vec<u16> {
        self.closes.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

impl FrameSink for RecordingSink {
    fn send(&self, frame: String) -> bool {
        match serde_json::from_str(&frame) {
            Ok(parsed) => {
                self.frames.lock().unwrap_or_else(|p| p.into_inner()).push(parsed);
                true
            }
            Err(_) => false,
        }
    }

    fn close(&self, code: u16) {
        self.closes.lock().unwrap_or_else(|p| p.into_inner()).push(code);
    }
}

type Answers = Box<dyn Fn(&str, &Value) -> Result<ActionOutput, RemoteError> + Send + Sync>;

/// An executor that answers each action with `answers(tool, args)` and notes the call.
pub struct ScriptedChannel {
    answers: Answers,
    calls: Mutex<Vec<(String, Value)>>,
    closed: watch::Sender<bool>,
}

impl ScriptedChannel {
    pub fn new(answers: impl Fn(&str, &Value) -> Result<ActionOutput, RemoteError> + Send + Sync + 'static) -> Arc<ScriptedChannel> {
        Arc::new(ScriptedChannel { answers: Box::new(answers), calls: Mutex::new(Vec::new()), closed: watch::channel(false).0 })
    }

    /// The actions asked for so far, with their args, in order.
    pub fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

#[async_trait]
impl ExecutorChannel for ScriptedChannel {
    async fn call(&self, tool: &str, args: Value) -> Result<ActionOutput, RemoteError> {
        if *self.closed.borrow() {
            return Err(RemoteError::ExecutorGone);
        }
        let answered = (self.answers)(tool, &args);
        self.calls.lock().unwrap_or_else(|p| p.into_inner()).push((tool.to_string(), args));
        answered
    }

    fn close(&self, _code: u16) {
        self.closed.send_replace(true);
    }

    fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    async fn closed(&self) {
        let mut seen = self.closed.subscribe();
        let _ = seen.wait_for(|closed| *closed).await;
    }
}
