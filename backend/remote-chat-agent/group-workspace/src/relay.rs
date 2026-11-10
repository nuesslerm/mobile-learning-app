//! What the groups share: an answer that is a result or a refusal, the executor and place of a workspace, and the args of an executor action
//! for the params of a file method.
use agent_api_proto::{code, Response};
use agent_api_workspace::{edit, FileParams, FILE_DIFF, FILE_LIST, FILE_READ};
use exec_proto::Code;
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError};
use rca_exec_channel::{Executors, Place, RouteError};
use serde_json::{json, Value};
use std::sync::Arc;

/// Why a method answers an error: its code and message.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Refusal {
    pub code: String,
    pub message: String,
}

impl Refusal {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Refusal {
        Refusal { code: code.into(), message: message.into() }
    }

    pub fn invalid(message: impl Into<String>) -> Refusal {
        Refusal::new(code::INVALID_PARAMS, message)
    }
}

impl From<agent_api_proto::ParamError> for Refusal {
    fn from(e: agent_api_proto::ParamError) -> Refusal {
        Refusal::invalid(e.0)
    }
}

impl From<RemoteError> for Refusal {
    fn from(e: RemoteError) -> Refusal {
        Refusal::new(e.code(), e.message())
    }
}

impl From<RouteError> for Refusal {
    fn from(e: RouteError) -> Refusal {
        match e {
            RouteError::NoExecutor => Refusal::new(Code::NoExecutor.as_str(), "no executor is attached"),
            RouteError::UnknownExecutor(id) => Refusal::new(Code::NoExecutor.as_str(), format!("no executor {id} is attached")),
            RouteError::AmbiguousExecutor(ids) => Refusal::new(agent_api_workspace::code::AMBIGUOUS_EXECUTOR, format!("several executors are attached, name one of: {}", ids.join(", "))),
            RouteError::UnknownWorkspace(w) => Refusal::new(agent_api_workspace::code::UNKNOWN_WORKSPACE, format!("no workspace {w}")),
        }
    }
}

pub(crate) type Answer = Result<Value, Refusal>;

pub(crate) fn respond(id: Value, answer: Answer) -> Response {
    match answer {
        Ok(result) => Response::result(id, result),
        Err(refusal) => Response::error(id, &refusal.code, refusal.message),
    }
}

/// The channel of the executor that owns `workspace`, and where the workspace lies at it.
pub(crate) struct Target {
    pub channel: Arc<dyn ExecutorChannel>,
    pub place: Place,
}

impl Target {
    pub async fn call(&self, action: &str, args: Value) -> Result<ActionOutput, Refusal> {
        Ok(self.channel.call(action, args).await?)
    }
}

pub(crate) fn target(executors: &dyn Executors, workspace: &str) -> Result<Target, Refusal> {
    match executors.get(workspace) {
        Some(channel) => Ok(Target { channel, place: executors.place(workspace) }),
        None => Err(Refusal::new(Code::NoExecutor.as_str(), format!("no executor is attached for workspace {workspace}"))),
    }
}

/// The args of the executor action for a file method: its params, with the path moved under the workspace's place, a search scoped to it,
/// and a diff aimed at its worktree.
pub(crate) fn executor_args(method: agent_api_proto::Method, mut params: FileParams, place: &Place) -> Result<Value, Refusal> {
    let place_path = |path: &str| place.path(path).map_err(Refusal::invalid);
    match method {
        FILE_DIFF | edit::TURN_DIFF | edit::THREAD_DIFF => {
            if let Some(worktree) = &place.worktree {
                let mut args = params.executor_args();
                args["worktree"] = json!(worktree);
                return Ok(args);
            }
            params.path = match (params.path.take(), &place.prefix) {
                (Some(path), _) => Some(place_path(&path)?),
                (None, prefix) => prefix.clone(),
            };
            Ok(params.executor_args())
        }
        edit::FILE_SEARCH => {
            params.path = place.prefix.clone();
            Ok(params.executor_args())
        }
        edit::FILE_BROWSE => {
            // An absolute path is the executor's to allow or refuse; a relative one is the workspace's
            if let Some(path) = params.path.clone().filter(|p| !std::path::Path::new(p).is_absolute()) {
                params.path = Some(place_path(&path)?);
            }
            Ok(params.executor_args())
        }
        FILE_READ | FILE_LIST | edit::FILE_READ_CHUNK | edit::FILE_WRITE | edit::FILE_CREATE | edit::FILE_STAT | edit::FILE_MKDIR => {
            params.path = params.path.take().map(|p| place_path(&p)).transpose()?;
            Ok(params.executor_args())
        }
        other => unreachable!("{} is no file method", other.name()),
    }
}
