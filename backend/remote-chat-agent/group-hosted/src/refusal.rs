use agent_api_proto::{code, ParamError};
use exec_proto::Code;
use rca_exec_channel::remote::RemoteError;
use rca_exec_channel::Executors;
use std::sync::Arc;

/// An error code and message, as the peer reads them.
#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    pub code: String,
    pub message: String,
}

pub(crate) fn refuse(code: &str, message: impl Into<String>) -> Refused {
    Refused { code: code.into(), message: message.into() }
}

impl From<ParamError> for Refused {
    fn from(e: ParamError) -> Refused {
        refuse(code::INVALID_PARAMS, e.0)
    }
}

/// The executor's refusal with the code a caller can act on when it says the CLI is missing or not signed in, else as it came.
impl From<RemoteError> for Refused {
    fn from(e: RemoteError) -> Refused {
        let message = e.message();
        let code = match &e {
            RemoteError::Action { .. } => agent_api_hosted::code_of(&message),
            _ => None,
        };
        refuse(code.unwrap_or_else(|| e.code()), message)
    }
}

pub(crate) fn channel_of(executors: &Arc<dyn Executors>, workspace: &str) -> Result<Arc<dyn rca_exec_channel::remote::ExecutorChannel>, Refused> {
    executors.get(workspace).ok_or_else(|| refuse(Code::NoExecutor.as_str(), format!("no executor is attached for workspace {workspace}")))
}
