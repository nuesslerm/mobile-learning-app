//! Doubles of the core's ports.
use crate::ports::{AgentApi, AgentError, Principals, ServiceStatus};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

/// An agent API that answers what it was given and remembers what it was asked: the flow tests' stand-in for rca.
pub struct ScriptedAgent {
    status: Mutex<Result<ServiceStatus, AgentError>>,
    state: Mutex<HashMap<(String, String), Value>>,
    calls: Mutex<Vec<String>>,
    replies: Mutex<HashMap<String, Reply>>,
    requests: Mutex<Vec<(String, Value)>>,
}

/// What a scripted method answers: a fixed result, or a function of the params it was sent.
pub type Reply = Box<dyn Fn(&Value) -> Result<Value, AgentError> + Send + Sync>;

impl ScriptedAgent {
    pub fn serving(status: ServiceStatus) -> Self {
        Self { status: Mutex::new(Ok(status)), state: Mutex::default(), calls: Mutex::default(), replies: Mutex::default(), requests: Mutex::default() }
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
