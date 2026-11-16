//! Doubles of the core's ports.
use crate::ports::{AgentApi, AgentError, EventStream, Principals, ServiceStatus, StreamEvent};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// An agent API that answers what it was given and remembers what it was asked: the flow tests' stand-in for rca.
pub struct ScriptedAgent {
    status: Mutex<Result<ServiceStatus, AgentError>>,
    state: Mutex<HashMap<(String, String), Value>>,
    calls: Mutex<Vec<String>>,
    replies: Mutex<HashMap<String, Reply>>,
    requests: Mutex<Vec<(String, Value)>>,
    streams: Mutex<HashMap<String, Scripted>>,
    follows: Mutex<Vec<(String, Value)>>,
    followers: Mutex<HashMap<String, Follower>>,
}

/// What a scripted follower answers: the service's answer and a stream, both made from the params.
pub type Follower = Box<dyn Fn(&Value) -> Result<(Value, Box<dyn EventStream>), AgentError> + Send + Sync>;

/// A scripted follow: what the call answers and the events its stream still holds, which the test may add to while the stream is open.
struct Scripted {
    answer: Result<Value, AgentError>,
    events: Arc<Mutex<VecDeque<Result<StreamEvent, AgentError>>>>,
}

/// The test's hand on a scripted stream: events pushed here come out of the stream's `next`.
#[derive(Clone)]
pub struct StreamFeed(Arc<Mutex<VecDeque<Result<StreamEvent, AgentError>>>>);

impl StreamFeed {
    pub fn push(&self, seq: u64, kind: &str, data: Value) {
        self.0.lock().unwrap().push_back(Ok(StreamEvent { seq, kind: kind.into(), data }));
    }

    pub fn fail(&self, error: AgentError) {
        self.0.lock().unwrap().push_back(Err(error));
    }
}

struct ScriptedStream(Arc<Mutex<VecDeque<Result<StreamEvent, AgentError>>>>);

impl EventStream for ScriptedStream {
    fn next(&mut self, _wait: Duration) -> Result<Option<StreamEvent>, AgentError> {
        self.0.lock().unwrap().pop_front().transpose()
    }
}

/// What a scripted method answers: a fixed result, or a function of the params it was sent.
pub type Reply = Box<dyn Fn(&Value) -> Result<Value, AgentError> + Send + Sync>;

impl ScriptedAgent {
    pub fn serving(status: ServiceStatus) -> Self {
        Self { status: Mutex::new(Ok(status)), state: Mutex::default(), calls: Mutex::default(), replies: Mutex::default(), requests: Mutex::default(), streams: Mutex::default(), follows: Mutex::default(), followers: Mutex::default() }
    }

    pub fn set_status(&self, status: Result<ServiceStatus, AgentError>) {
        *self.status.lock().unwrap() = status;
    }

    pub fn set_state(&self, scope: &str, key: &str, value: Value) {
        self.state.lock().unwrap().insert((scope.into(), key.into()), value);
    }

    /// Makes `method` answer `reply`; a method with no reply is refused with `UNKNOWN_METHOD`, as the service refuses one it does not serve.
    pub fn reply(&self, method: &str, reply: impl Fn(&Value) -> Result<Value, AgentError> + Send + Sync + 'static) {
        self.replies.lock().unwrap().insert(method.to_string(), Box::new(reply));
    }

    /// Makes `follow(method, params)` answer `answer` and open a stream that holds `events`; the feed adds more later. A follow with params
    /// no script names is refused with `UNKNOWN_METHOD`. Following again replaces the earlier script of the same params.
    pub fn script_stream(&self, method: &str, params: &Value, answer: Result<Value, AgentError>, events: Vec<StreamEvent>) -> StreamFeed {
        let events = Arc::new(Mutex::new(events.into_iter().map(Ok).collect::<VecDeque<_>>()));
        self.streams.lock().unwrap().insert(stream_key(method, params), Scripted { answer, events: events.clone() });
        StreamFeed(events)
    }

    /// Makes every `follow` of `method` that no `script_stream` names answer from `follower`, which sees the params.
    pub fn follower(&self, method: &str, follower: impl Fn(&Value) -> Result<(Value, Box<dyn EventStream>), AgentError> + Send + Sync + 'static) {
        self.followers.lock().unwrap().insert(method.to_string(), Box::new(follower));
    }

    /// Every `follow` opened so far with the params it was sent, in order.
    pub fn follows(&self) -> Vec<(String, Value)> {
        self.follows.lock().unwrap().clone()
    }

    /// Every `request` sent so far with its params, in order.
    pub fn requests(&self) -> Vec<(String, Value)> {
        self.requests.lock().unwrap().clone()
    }

    /// The agent methods asked so far, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl AgentApi for ScriptedAgent {
    fn status(&self) -> Result<ServiceStatus, AgentError> {
        self.calls.lock().unwrap().push("service.status".into());
        self.status.lock().unwrap().clone()
    }

    fn state_get(&self, scope: &str, key: &str) -> Result<Option<Value>, AgentError> {
        self.calls.lock().unwrap().push("state.get".into());
        Ok(self.state.lock().unwrap().get(&(scope.to_string(), key.to_string())).cloned())
    }

    fn request(&self, method: &'static str, params: Value) -> Result<Value, AgentError> {
        self.calls.lock().unwrap().push(method.to_string());
        let answer = self.answer(method, &params);
        self.requests.lock().unwrap().push((method.to_string(), params));
        answer
    }

    fn follow(&self, method: &'static str, _topic: &str, params: Value, _seq_param: &'static str) -> Result<(Value, Box<dyn EventStream>), AgentError> {
        self.calls.lock().unwrap().push(method.to_string());
        let key = stream_key(method, &params);
        self.follows.lock().unwrap().push((method.to_string(), params.clone()));
        if let Some(Scripted { answer, events }) = self.streams.lock().unwrap().get(&key) {
            return Ok((answer.clone()?, Box::new(ScriptedStream(events.clone()))));
        }
        let followers = self.followers.lock().unwrap();
        match followers.get(method) {
            Some(follower) => follower(&params),
            None => Err(AgentError::Refused { code: "UNKNOWN_METHOD".into(), message: format!("{method} is not a method of this service") }),
        }
    }
}

fn stream_key(method: &str, params: &Value) -> String {
    format!("{method} {params}")
}

impl ScriptedAgent {
    fn answer(&self, method: &str, params: &Value) -> Result<Value, AgentError> {
        match self.replies.lock().unwrap().get(method) {
            Some(reply) => reply(params),
            None if method == "state.set" => {
                let (scope, key) = (params["scope"].as_str().unwrap_or_default(), params["key"].as_str().unwrap_or_default());
                self.state.lock().unwrap().insert((scope.into(), key.into()), params["value"].clone());
                Ok(serde_json::json!({}))
            }
            None => Err(AgentError::Refused { code: "UNKNOWN_METHOD".into(), message: format!("{method} is not a method of this service") }),
        }
    }
}

/// Every device is paired until it is revoked by id.
#[derive(Default)]
pub struct RevocablePrincipals(Mutex<Vec<String>>);

impl RevocablePrincipals {
    pub fn revoke(&self, device_id: &str) {
        self.0.lock().unwrap().push(device_id.to_string());
    }
}

impl Principals for RevocablePrincipals {
    fn still_paired(&self, device_id: &str, _token: &str) -> bool {
        !self.0.lock().unwrap().iter().any(|d| d == device_id)
    }
}
