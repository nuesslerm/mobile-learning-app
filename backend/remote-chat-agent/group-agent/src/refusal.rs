//! What the group's methods answer with: a result, or a refusal with the code and message the contract gives it.
use crate::port::{EngineError, Refused};
use agent_api_agent::{code, message};
use agent_api_proto::{code as shared, ParamError, Response};
use serde_json::Value;

/// A request the group refuses, with the code it is answered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: String,
    pub message: String,
}

impl Refusal {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Refusal {
        Refusal { code: code.into(), message: message.into() }
    }

    pub fn not_found(thread: &str) -> Refusal {
        Refusal::new(code::THREAD_NOT_FOUND, message::thread_not_found(thread))
    }

    pub fn invalid(message: impl Into<String>) -> Refusal {
        Refusal::new(shared::INVALID_PARAMS, message)
    }
}

impl From<ParamError> for Refusal {
    fn from(error: ParamError) -> Refusal {
        Refusal::invalid(error.0)
    }
}

impl From<EngineError> for Refusal {
    fn from(error: EngineError) -> Refusal {
        Refusal::new(code::THREADS_UNAVAILABLE, error.0)
    }
}

impl From<Refused> for Refusal {
    fn from(refused: Refused) -> Refusal {
        Refusal { code: refused.code, message: refused.message }
    }
}

pub type Answer = Result<Value, Refusal>;

pub fn to_value<T: serde::Serialize>(result: &T) -> Answer {
    serde_json::to_value(result).map_err(|e| Refusal::new(code::THREADS_UNAVAILABLE, e.to_string()))
}

/// The response frame of `answer` to the request `id`.
pub fn respond(id: Value, answer: Answer) -> Response {
    match answer {
        Ok(result) => Response::result(id, result),
        Err(refusal) => Response::error(id, &refusal.code, refusal.message),
    }
}
