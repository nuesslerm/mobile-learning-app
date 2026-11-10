//! `orchestration.launchThread`: a thread and its first message in one call, idempotent by the launch's command id. A retry
//! writes nothing, answers `resumed` and names the thread the first attempt made.
use super::engine::Failure;
use super::service::Service;
use crate::domain::command::Family;
use crate::domain::launch::{self, LaunchRequest, Workspace};
use serde_json::{json, Value};
use t3_contracts::wire;

#[derive(Debug, PartialEq)]
pub enum LaunchFailure {
    /// A payload the decoder refuses.
    Defect(String),
    /// `OrchestrationV2ThreadLaunchError`, shaped for the wire.
    Refused(Value),
}

fn failure_text(failure: Failure) -> String {
    match failure {
        Failure::Defect(message) | Failure::Refused { message, .. } => message,
        Failure::ProjectNotEmpty { project_id, .. } => wire::project_not_empty(&project_id),
    }
}

impl Service {
    pub fn launch_thread(&self, payload: &Value) -> Result<Value, LaunchFailure> {
        let req = LaunchRequest::parse(payload).map_err(LaunchFailure::Defect)?;
        let refuse = |stage: &'static str, detail: String| {
            self.log("launch.refused", &[("commandId", req.command_id.clone()), ("stage", stage.to_string()), ("detail", detail.clone())]);
            LaunchFailure::Refused(wire::launch_error(&req.command_id, &req.project_id, &format!("Failed to launch thread: {detail}")))
        };
        if let Some(why) = launch::unsupported(&req) {
            return Err(refuse("unsupported", why.to_string()));
        }
        let thread_id = launch::thread_id(&req);
        let claimed = self.engine().accepted(&req.command_id).map_err(|e| refuse("receipt", e.0))?;
        self.engine().read(|state| launch::check(state, &req, &thread_id, claimed)).map_err(|m| refuse("check", m))?;
        let mut command = launch::claim(&req, &thread_id);
        // A thread of the scratch project works in a folder of its own, made for it
        if !claimed && !req.reuse_existing_thread && matches!(req.workspace, Workspace::Root { .. }) {
            let text = req.initial_message.as_ref().map_or(req.title.as_str(), |m| m.text.as_str());
            if let Some(folder) = self.scratch_folder(&req.project_id, &thread_id, text).map_err(|e| refuse("provision-worktree", e))? {
                command["worktreePath"] = json!(folder);
            }
        }
        let claim = self.dispatch_as(Family::Thread, &command, req.initial_message.is_none()).map_err(|f| refuse("claim", failure_text(f)))?;
        let message = match launch::initial_message(&req, &thread_id) {
            Some(command) => Some(self.dispatch(Family::Thread, &command).map_err(|f| refuse("message", failure_text(f)))?),
            None => None,
        };
        let projection = self.engine().thread_projection(&thread_id).ok_or_else(|| refuse("projection", "Thread not found.".into()))?;
        let resumed = message.as_ref().map_or(claim.replayed, |m| m.replayed);
        self.log("launch.accepted", &[("commandId", req.command_id.clone()), ("thread", thread_id.clone()), ("resumed", resumed.to_string())]);
        Ok(wire::launch_result(&thread_id, projection, resumed, message.map(|m| m.sequence)))
    }
}
