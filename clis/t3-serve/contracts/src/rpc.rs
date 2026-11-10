//! The Effect RPC JSON framing: one JSON value (or an array of them) per WebSocket text message. Client to server: Request, Ack,
//! Interrupt, Ping, Eof. Server to client: Chunk, Exit, Defect, ClientProtocolError, Pong. Streams are ack-paced: a Chunk is
//! followed by the next one only after the client's Ack (`StreamPacer`). Byte details at Effect 4.0.5 are unverified (protocol.md 12.1).
use serde_json::{json, Map, Value};
use std::collections::VecDeque;

/// The RPCs this server answers for real; every other tag of the scope table answers `UnsupportedOperationError` (protocol.md table).
pub const IMPLEMENTED_RPCS: [&str; 136] = [
    "pullRequests.updateComment",
    "pullRequests.diffFileContents",
    "pullRequests.reviewGuide",
    "pullRequests.generateReviewGuide",
    "git.preparePullRequestThread",
    "projects.ensureScratch",
    "projects.createNew",
    "projects.createFromThread",
    "preview.open",
    "preview.resize",
    "preview.adjust",
    "preview.close",
    "preview.list",
    "preview.reportStatus",
    "subscribePreviewEvents",
    "assets.createUrl",
    "assets.persistChatAttachments",
    "attachments.createUploadUrl",
    "attachments.delete",
    "scheduledTasks.subscribe",
    "scheduledTasks.upsert",
    "scheduledTasks.setEnabled",
    "scheduledTasks.delete",
    "scheduledTasks.runNow",
    "scheduledTasks.rotateWebhookToken",
    "pullRequests.list",
    "pullRequests.listStats",
    "pullRequests.filterOptions",
    "pullRequests.summary",
    "pullRequests.preview",
    "pullRequests.routing",
    "pullRequests.routingIdentity",
    "pullRequests.detail",
    "pullRequests.checks",
    "pullRequests.activity",
    "pullRequests.threadComments",
    "pullRequests.changeBreakdown",
    "pullRequests.labelCandidates",
    "pullRequests.reviewerCandidates",
    "pullRequests.filesViewed",
    "pullRequests.linkedThreads",
    "pullRequests.comment",
    "pullRequests.replyToThread",
    "pullRequests.setThreadResolution",
    "pullRequests.setReaction",
    "pullRequests.requestReviewers",
    "pullRequests.setLabels",
    "pullRequests.submitReview",
    "pullRequests.update",
    "pullRequests.runAction",
    "pullRequests.setFilesViewed",
    "pullRequests.invalidate",
    "pullRequests.reportState",
    "pullRequests.stack",
    "pullRequests.subscribeRefreshes",
    "sourceControl.lookupRepository",
    "sourceControl.publishRepository",
    "sourceControl.appAccess",
    "sourceControl.signInStart",
    "sourceControl.signInWait",
    "sourceControl.signInCancel",
    "projectClone.start",
    "projectClone.cancel",
    "projectClone.retry",
    "subscribeProjectClones",
    "server.discoverSourceControl",
    "server.getDatabaseStorage",
    "provider.auth.start",
    "provider.auth.respond",
    "provider.auth.complete",
    "provider.auth.cancel",
    "provider.auth.logout",
    "provider.auth.subscribe",
    "provider.chatgpt.reconnect-profile",
    "provider.chatgpt.import-profile",
    "provider.chatgpt.handoff.subscribe",
    "provider.consumeResetCredit",
    "server.refreshProviders",
    "server.updateProvider",
    "server.updateSettings",
    "server.getUsageSummary",
    "server.refreshUsageRates",
    "server.getHostResources",
    "vcs.refreshStatus",
    "vcs.listRefs",
    "vcs.switchRef",
    "vcs.createRef",
    "vcs.pull",
    "review.getDiffPreview",
    "review.getDiffFileContents",
    "subscribeVcsStatus",
    "git.runStackedAction",
    "terminal.open",
    "terminal.attach",
    "terminal.observe",
    "terminal.write",
    "terminal.resize",
    "terminal.clear",
    "terminal.close",
    "subscribeTerminalMetadata",
    "server.probe",
    "server.reportClientActivity",
    "peerLinks.list",
    "peerLinks.link",
    "peerLinks.unlink",
    "peerLinks.answerRequest",
    "server.getConfig",
    "subscribeServerConfig",
    "subscribeServerLifecycle",
    "orchestration.subscribeShell",
    "orchestration.subscribeThread",
    "orchestration.getThreadProjection",
    "orchestration.dispatchCommand",
    "orchestration.launchThread",
    "projects.mutate",
    "projects.listEntries",
    "projects.readFile",
    "projects.writeFile",
    "filesystem.browse",
    "orchestration.getTurnItem",
    "orchestration.searchThreads",
    "orchestration.getThreadFamily",
    "orchestration.getOlderSettledThreads",
    "orchestration.getArchivedShellSnapshot",
    "orchestration.forceStopThread",
    "orchestration.stopWorkflow",
    "orchestration.getWorkflowScript",
    "secrets.answerRequest",
    "orchestration.subscribeCommandOutput",
    "filesystem.getMetadata",
    "projects.searchEntries",
    "orchestration.getTurnDiff",
    "orchestration.getFullThreadDiff",
    "vcs.createWorktree",
    "worktreeSetup.cancel",
    "subscribeWorktreeSetup",
];

#[derive(Debug, Clone, PartialEq)]
pub enum FromClient {
    Request { id: Value, tag: String, payload: Value },
    Ack { request_id: Value },
    Interrupt { request_id: Value },
    Ping,
    Eof,
}

/// A message that is not valid JSON, or a frame with no known `_tag`.
#[derive(Debug, PartialEq)]
pub struct ProtocolError(pub String);

/// Every frame in one text message, which holds one object or an array of them.
pub fn parse_client_message(text: &str) -> Result<Vec<FromClient>, ProtocolError> {
    let value: Value = serde_json::from_str(text).map_err(|e| ProtocolError(format!("not JSON: {e}")))?;
    match value {
        Value::Array(items) => items.iter().map(parse_frame).collect(),
        other => Ok(vec![parse_frame(&other)?]),
    }
}

fn parse_frame(frame: &Value) -> Result<FromClient, ProtocolError> {
    let tag = frame.get("_tag").and_then(Value::as_str).ok_or_else(|| ProtocolError("a frame without _tag".into()))?;
    let request_id = || frame.get("requestId").cloned().ok_or_else(|| ProtocolError(format!("{tag} without requestId")));
    match tag {
        "Request" => {
            let id = frame.get("id").cloned().ok_or_else(|| ProtocolError("Request without id".into()))?;
            let rpc = frame.get("tag").and_then(Value::as_str).ok_or_else(|| ProtocolError("Request without tag".into()))?;
            Ok(FromClient::Request { id, tag: rpc.to_string(), payload: frame.get("payload").cloned().unwrap_or(Value::Null) })
        }
        "Ack" => Ok(FromClient::Ack { request_id: request_id()? }),
        "Interrupt" => Ok(FromClient::Interrupt { request_id: request_id()? }),
        "Ping" => Ok(FromClient::Ping),
        "Eof" => Ok(FromClient::Eof),
        other => Err(ProtocolError(format!("unknown frame {other}"))),
    }
}

pub fn pong() -> Value {
    json!({"_tag": "Pong"})
}

pub fn chunk(request_id: &Value, values: Vec<Value>) -> Value {
    json!({"_tag": "Chunk", "requestId": request_id, "values": values})
}

/// A success without a value omits the key, as `JSON.stringify` does for `undefined` (the schema of a void result decodes it; `null` would not).
pub fn exit_success(request_id: &Value, value: Option<Value>) -> Value {
    let mut exit = Map::new();
    exit.insert("_tag".into(), "Success".into());
    if let Some(value) = value {
        exit.insert("value".into(), value);
    }
    json!({"_tag": "Exit", "requestId": request_id, "exit": Value::Object(exit)})
}

/// A declared failure: the method's tagged error, as one `Fail` reason of the cause.
pub fn exit_failure(request_id: &Value, error: Value) -> Value {
    json!({"_tag": "Exit", "requestId": request_id, "exit": {"_tag": "Failure", "cause": [{"_tag": "Fail", "error": error}]}})
}

pub fn exit_defect(request_id: &Value, message: &str) -> Value {
    json!({"_tag": "Exit", "requestId": request_id, "exit": {"_tag": "Failure", "cause": [{"_tag": "Die", "defect": message}]}})
}

/// The end of a stream the client interrupted: one `Interrupt` reason naming the fiber (the real server repeats it; a client reads the first).
pub fn exit_interrupted(request_id: &Value, fiber_id: u64) -> Value {
    json!({"_tag": "Exit", "requestId": request_id, "exit": {"_tag": "Failure", "cause": [{"_tag": "Interrupt", "fiberId": fiber_id}]}})
}

pub fn protocol_error(message: &str) -> Value {
    json!({"_tag": "ClientProtocolError", "error": {"_tag": "RpcClientError", "reason": "Protocol", "message": message}})
}

/// What a follower sends through the connection's channel in place of a frame to say its stream is over: the stream ends once what it queued
/// has been sent and acked.
pub fn end_marker() -> Value {
    json!({"_end": true})
}

/// One stream's outbound side: values wait here, and one Chunk carries everything queued whenever the client has acked the last one.
#[derive(Debug, Default)]
pub struct StreamPacer {
    queued: VecDeque<Value>,
    awaiting_ack: bool,
}

impl StreamPacer {
    pub fn push(&mut self, value: Value) {
        self.queued.push_back(value);
    }

    /// Nothing is queued and nothing is owed an ack.
    pub fn drained(&self) -> bool {
        self.queued.is_empty() && !self.awaiting_ack
    }

    pub fn ack(&mut self) {
        self.awaiting_ack = false;
    }

    /// The next Chunk's values, or `None` while an ack is owed or nothing is queued.
    pub fn next_chunk(&mut self) -> Option<Vec<Value>> {
        if self.awaiting_ack || self.queued.is_empty() {
            return None;
        }
        self.awaiting_ack = true;
        Some(self.queued.drain(..).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batched_message_yields_every_frame_in_order() {
        // Given one text message holding a Request and a Ping
        let text = r#"[{"_tag":"Request","id":"1","tag":"server.probe","payload":{},"headers":[]},{"_tag":"Ping"}]"#;
        // When it is parsed
        let frames = parse_client_message(text).unwrap();
        // Then both frames come out, request first
        assert_eq!(frames, vec![FromClient::Request { id: json!("1"), tag: "server.probe".into(), payload: json!({}) }, FromClient::Ping]);
    }

    #[test]
    fn a_frame_without_a_known_tag_is_a_protocol_error_not_a_panic() {
        // Given frames the protocol does not define
        for text in ["not json", r#"{"x":1}"#, r#"{"_tag":"Chunk"}"#, r#"{"_tag":"Ack"}"#] {
            // When parsed, Then each is refused
            assert!(parse_client_message(text).is_err(), "{text}");
        }
    }

    #[test]
    fn a_stream_sends_one_chunk_per_ack_and_batches_what_queued_meanwhile() {
        // Given a stream with two values queued
        let mut pacer = StreamPacer::default();
        pacer.push(json!(1));
        pacer.push(json!(2));
        // When the first chunk is taken
        // Then it carries both and a second chunk waits for the ack
        assert_eq!(pacer.next_chunk(), Some(vec![json!(1), json!(2)]));
        pacer.push(json!(3));
        assert_eq!(pacer.next_chunk(), None);
        // And after the ack the next chunk carries the value queued meanwhile
        pacer.ack();
        assert_eq!(pacer.next_chunk(), Some(vec![json!(3)]));
        assert_eq!(pacer.next_chunk(), None);
    }

    #[test]
    fn a_void_success_has_no_value_key() {
        assert!(exit_success(&json!("1"), None)["exit"].get("value").is_none());
    }
}
