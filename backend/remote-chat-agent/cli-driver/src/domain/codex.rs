//! The wire of `codex app-server` over stdio (`docs/solutions/architecture/codex-app-server-protocol.md`): one JSON object a line, no `jsonrpc` field.
//! The requests a session sends, what an inbound line is, and what a thread's notifications mean as the neutral events a client shows. Pure.
use super::driver::TurnRequest;
use super::translate::ExecEvent;
use serde_json::{json, Value};
use std::collections::HashSet;

/// What the person said to a request of the CLI that waits for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    AcceptForSession,
    Decline,
    Cancel,
}

impl Verdict {
    fn word(self) -> &'static str {
        match self {
            Verdict::Accept => "accept",
            Verdict::AcceptForSession => "acceptForSession",
            Verdict::Decline => "decline",
            Verdict::Cancel => "cancel",
        }
    }
}

fn line(value: Value) -> String {
    value.to_string()
}

pub fn initialize(id: u64) -> String {
    line(json!({"id": id, "method": "initialize", "params": {"clientInfo": {"name": "rca", "title": "rca", "version": "1.0.0"}}}))
}

pub fn initialized() -> String {
    line(json!({"method": "initialized"}))
}

pub fn thread_start(id: u64, model: Option<&str>) -> String {
    let params = model.map_or_else(|| json!({}), |m| json!({"model": m}));
    line(json!({"id": id, "method": "thread/start", "params": params}))
}

pub fn thread_resume(id: u64, thread: &str) -> String {
    line(json!({"id": id, "method": "thread/resume", "params": {"threadId": thread}}))
}

pub fn thread_unarchive(id: u64, thread: &str) -> String {
    line(json!({"id": id, "method": "thread/unarchive", "params": {"threadId": thread}}))
}

fn text_input(text: &str) -> Value {
    json!([{"type": "text", "text": text, "text_elements": []}])
}

/// The approval policy and sandbox a turn runs under: a plan only reads, the mode that asks has the CLI ask for what it does not trust, the mode
/// that accepts edits asks only when the CLI wants more than its workspace, and full access never asks.
fn policy(req: &TurnRequest) -> (&'static str, Value) {
    match (req.interaction_mode.as_str(), req.runtime_mode.as_str()) {
        ("plan", _) => ("never", json!({"type": "readOnly"})),
        (_, "approval-required") => ("untrusted", json!({"type": "readOnly"})),
        (_, "auto-accept-edits") => ("on-request", json!({"type": "workspaceWrite"})),
        _ => ("never", json!({"type": "dangerFullAccess"})),
    }
}

pub fn turn_start(id: u64, thread: &str, req: &TurnRequest) -> String {
    let (approval, sandbox) = policy(req);
    let mut params = json!({"threadId": thread, "input": text_input(&req.prompt), "approvalPolicy": approval, "approvalsReviewer": "user", "sandboxPolicy": sandbox, "summary": "detailed"});
    if let Some(model) = &req.model {
        params["model"] = json!(model);
    }
    line(json!({"id": id, "method": "turn/start", "params": params}))
}

pub fn turn_steer(id: u64, thread: &str, expected_turn: &str, prompt: &str) -> String {
    line(json!({"id": id, "method": "turn/steer", "params": {"threadId": thread, "expectedTurnId": expected_turn, "input": text_input(prompt)}}))
}

pub fn turn_interrupt(id: u64, thread: &str, turn: &str) -> String {
    line(json!({"id": id, "method": "turn/interrupt", "params": {"threadId": thread, "turnId": turn}}))
}

/// What one line the server printed is.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// The answer to a request of ours: the `result`, or the server's error message.
    Response { id: u64, outcome: Result<Value, String> },
    Notification { method: String, params: Value },
    /// A request of the server's that waits for our answer under its own `id` (a number or a string).
    Request { id: Value, method: String, params: Value },
    /// Not JSON, or JSON that is none of these.
    Other,
}

pub fn parse(text: &str) -> Inbound {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Inbound::Other };
    match (v.get("id"), v["method"].as_str()) {
        (Some(id), Some(method)) => Inbound::Request { id: id.clone(), method: method.to_string(), params: v["params"].clone() },
        (None, Some(method)) => Inbound::Notification { method: method.to_string(), params: v["params"].clone() },
        (Some(id), None) => {
            let Some(id) = id.as_u64() else { return Inbound::Other };
            let outcome = match v.get("error").filter(|e| !e.is_null()) {
                Some(error) => Err(error["message"].as_str().unwrap_or("the server refused the request").to_string()),
                None => Ok(v["result"].clone()),
            };
            Inbound::Response { id, outcome }
        }
        _ => Inbound::Other,
    }
}

/// What a request of the server's asks of the person.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub thread: String,
    /// What the person's answer names: the server's own `approvalId`, else the id of the item it is about.
    pub key: String,
    pub tool: String,
    pub args: Value,
    pub kind: AskKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AskKind {
    Command,
    FileChange,
    /// The permissions the server asked for, echoed back when they are granted.
    Permissions(Value),
}

/// The ask a server request stands for; none for a request that is not an approval.
pub fn ask_of(method: &str, params: &Value) -> Option<Ask> {
    let thread = params["threadId"].as_str()?.to_string();
    let key = params["approvalId"].as_str().or_else(|| params["itemId"].as_str())?.to_string();
    let pick = |names: &[&str]| -> Value { names.iter().filter(|n| !params[**n].is_null()).map(|n| ((*n).to_string(), params[*n].clone())).collect::<serde_json::Map<_, _>>().into() };
    match method {
        "item/commandExecution/requestApproval" => Some(Ask { thread, key, tool: "command".into(), args: pick(&["command", "cwd", "reason"]), kind: AskKind::Command }),
        "item/fileChange/requestApproval" => Some(Ask { thread, key, tool: "file_change".into(), args: pick(&["reason", "grantRoot"]), kind: AskKind::FileChange }),
        "item/permissions/requestApproval" => Some(Ask { thread, key, tool: "permissions".into(), args: pick(&["reason", "permissions", "cwd"]), kind: AskKind::Permissions(params["permissions"].clone()) }),
        _ => None,
    }
}

/// The reply to the server's request `id` for the person's `verdict`.
pub fn answer(id: &Value, kind: &AskKind, verdict: Verdict) -> String {
    let result = match kind {
        AskKind::Command | AskKind::FileChange => json!({"decision": verdict.word()}),
        AskKind::Permissions(asked) => match verdict {
            Verdict::Accept => json!({"permissions": asked, "scope": "turn"}),
            Verdict::AcceptForSession => json!({"permissions": asked, "scope": "session"}),
            Verdict::Decline | Verdict::Cancel => json!({"permissions": {}, "scope": "turn"}),
        },
    };
    line(json!({"id": id, "result": result}))
}

/// The reply to a request of the server's that is no approval: an elicitation is declined, anything else is a method this client does not have.
pub fn refuse(id: &Value, method: &str) -> String {
    if method == "mcpServer/elicitation/request" {
        return line(json!({"id": id, "result": {"action": "decline", "content": null, "_meta": null}}));
    }
    line(json!({"id": id, "error": {"code": -32601, "message": format!("Method not found: {method}")}}))
}

/// What a notification of a thread changes for the turn that runs on it.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Event(ExecEvent),
    /// The server started the turn `id`.
    Started(String),
    /// The turn is over: `completed`, `interrupted` or `failed`.
    Ended(String),
}

/// Reads the notifications of one thread into neutral events, keeping what it needs of the item that is still open.
#[derive(Default)]
pub struct Translator {
    /// The items whose text arrived as deltas, so their completed text is not said again.
    streamed: HashSet<String>,
    spoke: bool,
    failed: bool,
    /// The tool calls of an item still open.
    open: HashSet<String>,
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn tool_name(item: &Value) -> String {
    match item["type"].as_str().unwrap_or_default() {
        "commandExecution" => "command".into(),
        "mcpToolCall" => format!("mcp__{}__{}", text(&item["server"]), text(&item["tool"])),
        "dynamicToolCall" => text(&item["tool"]),
        other => {
            let mut snake = String::new();
            for c in other.chars() {
                if c.is_ascii_uppercase() {
                    snake.push('_');
                }
                snake.push(c.to_ascii_lowercase());
            }
            snake
        }
    }
}

fn tool_input(item: &Value) -> Value {
    match item["type"].as_str().unwrap_or_default() {
        "commandExecution" => json!({"command": item["command"], "cwd": item["cwd"]}),
        "mcpToolCall" | "dynamicToolCall" => item["arguments"].clone(),
        _ => {
            let mut rest = item.clone();
            if let Some(map) = rest.as_object_mut() {
                for key in ["id", "type", "status"] {
                    map.remove(key);
                }
            }
            rest
        }
    }
}

fn tool_result(item: &Value) -> (String, bool) {
    let status = item["status"].as_str().unwrap_or("completed");
    match item["type"].as_str().unwrap_or_default() {
        "commandExecution" => (text(&item["aggregatedOutput"]), status == "completed" && item["exitCode"].as_i64().unwrap_or(0) == 0),
        "mcpToolCall" => {
            if let Some(error) = item["error"].as_object() {
                return (text(&error.get("message").cloned().unwrap_or_default()), false);
            }
            let result = &item["result"];
            let output = if result.is_null() { String::new() } else { result["content"].as_array().map_or_else(|| result.to_string(), |parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n")) };
            (output, status == "completed")
        }
        _ => (String::new(), status == "completed"),
    }
}

/// The items that are no tool: what the person said, what the model answered and thought, and the bookkeeping of the server.
fn is_tool(kind: &str) -> bool {
    !matches!(kind, "userMessage" | "agentMessage" | "reasoning" | "plan" | "fileChange" | "contextCompaction" | "enteredReviewMode" | "exitedReviewMode" | "hookPrompt" | "functionCallOutput" | "sleep")
}

impl Translator {
    /// A new turn begins on the thread.
    pub fn reset(&mut self) {
        *self = Translator::default();
    }

    /// The steps one notification of the thread stands for.
    pub fn notification(&mut self, method: &str, params: &Value) -> Vec<Step> {
        let item = &params["item"];
        let id = text(&item["id"]);
        match method {
            "turn/started" => vec![Step::Started(text(&params["turn"]["id"]))],
            "turn/completed" => {
                let turn = &params["turn"];
                let status = turn["status"].as_str().unwrap_or("completed").to_string();
                let mut steps = Vec::new();
                if status == "failed" && !self.failed {
                    let message = turn["error"]["message"].as_str().unwrap_or("the turn failed");
                    steps.push(Step::Event(ExecEvent::Error(message.to_string())));
                }
                steps.push(Step::Ended(status));
                steps
            }
            "error" if params["willRetry"] == false => {
                self.failed = true;
                vec![Step::Event(ExecEvent::Error(text(&params["error"]["message"])))]
            }
            "item/agentMessage/delta" => {
                let item_id = text(&params["itemId"]);
                let mut events = Vec::new();
                if self.streamed.insert(item_id) && self.spoke {
                    events.push(Step::Event(ExecEvent::Text("\n\n".into())));
                }
                self.spoke = true;
                events.push(Step::Event(ExecEvent::Text(text(&params["delta"]))));
                events
            }
            "item/reasoning/summaryTextDelta" => {
                self.streamed.insert(text(&params["itemId"]));
                vec![Step::Event(ExecEvent::Reasoning(text(&params["delta"])))]
            }
            "item/started" => match item["type"].as_str().unwrap_or_default() {
                kind if is_tool(kind) => {
                    self.open.insert(id.clone());
                    vec![Step::Event(ExecEvent::ToolStarted { id, name: tool_name(item), input: tool_input(item) })]
                }
                _ => Vec::new(),
            },
            "item/completed" => self.completed(item, id),
            _ => Vec::new(),
        }
    }

    fn completed(&mut self, item: &Value, id: String) -> Vec<Step> {
        let kind = item["type"].as_str().unwrap_or_default();
        let streamed = self.streamed.contains(&id);
        match kind {
            "agentMessage" if !streamed && !text(&item["text"]).is_empty() => {
                let mut steps = Vec::new();
                if self.spoke {
                    steps.push(Step::Event(ExecEvent::Text("\n\n".into())));
                }
                self.spoke = true;
                steps.push(Step::Event(ExecEvent::Text(text(&item["text"]))));
                steps
            }
            "reasoning" if !streamed => {
                let summary: Vec<&str> = item["summary"].as_array().map(|all| all.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                if summary.is_empty() {
                    Vec::new()
                } else {
                    vec![Step::Event(ExecEvent::Reasoning(summary.join("\n\n")))]
                }
            }
            "plan" => vec![Step::Event(ExecEvent::Text(text(&item["text"])))],
            "fileChange" => {
                let ok = item["status"].as_str().unwrap_or("completed") == "completed";
                let changes = item["changes"].as_array().cloned().unwrap_or_default();
                changes
                    .iter()
                    .enumerate()
                    .flat_map(|(n, change)| {
                        let call = format!("{id}:{n}");
                        let name = match change["kind"]["type"].as_str() {
                            Some("add") => "create_file",
                            Some("delete") => "delete_file",
                            _ => "Edit",
                        };
                        [Step::Event(ExecEvent::ToolStarted { id: call.clone(), name: name.into(), input: json!({"path": change["path"], "diff": change["diff"]}) }), Step::Event(ExecEvent::ToolFinished { id: call, output: String::new(), ok })]
                    })
                    .collect()
            }
            kind if is_tool(kind) => {
                let (output, ok) = tool_result(item);
                let mut steps = Vec::new();
                if !self.open.remove(&id) {
                    steps.push(Step::Event(ExecEvent::ToolStarted { id: id.clone(), name: tool_name(item), input: tool_input(item) }));
                }
                steps.push(Step::Event(ExecEvent::ToolFinished { id, output, ok }));
                steps
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> TurnRequest {
        TurnRequest { thread_id: "t".into(), run_id: "r".into(), instance_id: "codex".into(), model: Some("gpt-x".into()), cwd: ".".into(), prompt: "go".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: None, tools: None }
    }

    fn feed(translator: &mut Translator, frames: &[Value]) -> Vec<Step> {
        frames.iter().flat_map(|f| translator.notification(f["method"].as_str().unwrap(), &f["params"])).collect()
    }

    fn ev(e: ExecEvent) -> Step {
        Step::Event(e)
    }

    #[test]
    fn given_the_runtime_modes_when_a_turn_is_started_then_each_runs_under_the_policy_of_its_mode_and_the_prompt_and_model_are_sent() {
        // Given one turn per mode of the thread
        let of = |runtime: &str, interaction: &str| serde_json::from_str::<Value>(&turn_start(7, "th", &TurnRequest { runtime_mode: runtime.into(), interaction_mode: interaction.into(), ..req() })).unwrap();
        // When each is built
        let (asks, edits, full, plan) = (of("approval-required", "default"), of("auto-accept-edits", "default"), of("full-access", "default"), of("full-access", "plan"));
        // Then the mode that asks has the CLI ask and only read, edits are accepted within the workspace, full access never asks, and a plan only reads
        let policy = |v: &Value| (v["params"]["approvalPolicy"].as_str().unwrap().to_string(), v["params"]["sandboxPolicy"]["type"].as_str().unwrap().to_string());
        assert_eq!(policy(&asks), ("untrusted".into(), "readOnly".into()));
        assert_eq!(policy(&edits), ("on-request".into(), "workspaceWrite".into()));
        assert_eq!(policy(&full), ("never".into(), "dangerFullAccess".into()));
        assert_eq!(policy(&plan), ("never".into(), "readOnly".into()));
        // And the request names its thread, its prompt as text input, the model and a reviewer that is the person
        assert_eq!((full["id"].clone(), full["method"].clone(), full["params"]["threadId"].clone(), full["params"]["model"].clone(), full["params"]["approvalsReviewer"].clone()), (json!(7), json!("turn/start"), json!("th"), json!("gpt-x"), json!("user")));
        assert_eq!(full["params"]["input"], json!([{"type": "text", "text": "go", "text_elements": []}]));
        assert!(serde_json::from_str::<Value>(&thread_start(1, None)).unwrap()["params"].get("model").is_none());
    }

    #[test]
    fn given_the_lines_a_server_prints_when_parsed_then_a_response_a_notification_and_a_request_of_its_own_are_told_apart() {
        // Given the answer to a request, an error, a notification, an approval request with a number id, one with a string id, and noise
        let ok = parse(r#"{"id":3,"result":{"turn":{"id":"u1"}}}"#);
        let bad = parse(r#"{"id":4,"error":{"code":-32600,"message":"invalid thread id: x"}}"#);
        let note = parse(r#"{"method":"turn/started","params":{"threadId":"th"},"emittedAtMs":1}"#);
        let asked = parse(r#"{"id":0,"method":"item/commandExecution/requestApproval","params":{"threadId":"th"}}"#);
        let named = parse(r#"{"id":"srv-1","method":"mcpServer/elicitation/request","params":{}}"#);
        // Then each is what it is, and a line that is no JSON, or has neither id nor method, is nothing
        assert_eq!(ok, Inbound::Response { id: 3, outcome: Ok(json!({"turn": {"id": "u1"}})) });
        assert_eq!(bad, Inbound::Response { id: 4, outcome: Err("invalid thread id: x".into()) });
        assert!(matches!(note, Inbound::Notification { method, .. } if method == "turn/started"));
        assert!(matches!(asked, Inbound::Request { id, .. } if id == json!(0)));
        assert!(matches!(named, Inbound::Request { id, .. } if id == json!("srv-1")));
        assert_eq!((parse("bye"), parse("{}")), (Inbound::Other, Inbound::Other));
    }

    #[test]
    fn given_an_approval_request_of_each_kind_when_the_person_answers_then_the_reply_has_the_shape_the_server_reads() {
        // Given the command approval of a recorded turn, a file change approval and a permission request
        let command = ask_of("item/commandExecution/requestApproval", &json!({"kind": "command", "threadId": "th", "turnId": "u", "itemId": "exec-1", "reason": "May I?", "command": "/bin/bash -lc \"touch a\"", "cwd": "/w"})).unwrap();
        let patch = ask_of("item/fileChange/requestApproval", &json!({"threadId": "th", "turnId": "u", "itemId": "fc-1", "reason": "edit", "grantRoot": "/w"})).unwrap();
        let sub = ask_of("item/commandExecution/requestApproval", &json!({"threadId": "th", "itemId": "exec-1", "approvalId": "ap-9", "command": "ls"})).unwrap();
        let permission = ask_of("item/permissions/requestApproval", &json!({"threadId": "th", "itemId": "p-1", "reason": "net", "permissions": {"network": {"enabled": true}, "fileSystem": null}})).unwrap();
        // Then the ask names its thread, a key (the approval id when there is one) and what the person is shown
        assert_eq!((command.thread.as_str(), command.key.as_str(), command.tool.as_str()), ("th", "exec-1", "command"));
        assert_eq!(command.args, json!({"command": "/bin/bash -lc \"touch a\"", "cwd": "/w", "reason": "May I?"}));
        assert_eq!((patch.key.as_str(), patch.tool.as_str(), sub.key.as_str()), ("fc-1", "file_change", "ap-9"));
        // And a command or file change is answered with a decision word, a permission with what was asked and its scope, or nothing when refused
        let read = |line: String| serde_json::from_str::<Value>(&line).unwrap();
        assert_eq!(read(answer(&json!(0), &command.kind, Verdict::Accept)), json!({"id": 0, "result": {"decision": "accept"}}));
        assert_eq!(read(answer(&json!("s"), &patch.kind, Verdict::AcceptForSession))["result"], json!({"decision": "acceptForSession"}));
        assert_eq!(read(answer(&json!(0), &command.kind, Verdict::Cancel))["result"], json!({"decision": "cancel"}));
        assert_eq!(read(answer(&json!(2), &permission.kind, Verdict::AcceptForSession))["result"], json!({"permissions": {"network": {"enabled": true}, "fileSystem": null}, "scope": "session"}));
        assert_eq!(read(answer(&json!(2), &permission.kind, Verdict::Decline))["result"], json!({"permissions": {}, "scope": "turn"}));
        // And a request that is no approval has none
        assert!(ask_of("item/tool/call", &json!({"threadId": "th", "itemId": "i"})).is_none());
    }

    #[test]
    fn given_requests_that_are_no_approval_when_refused_then_an_elicitation_is_declined_and_any_other_is_a_method_not_found() {
        let read = |line: String| serde_json::from_str::<Value>(&line).unwrap();
        assert_eq!(read(refuse(&json!("e1"), "mcpServer/elicitation/request")), json!({"id": "e1", "result": {"action": "decline", "content": null, "_meta": null}}));
        assert_eq!(read(refuse(&json!(5), "item/tool/call")), json!({"id": 5, "error": {"code": -32601, "message": "Method not found: item/tool/call"}}));
    }

    #[test]
    fn given_the_notifications_of_a_turn_with_a_command_when_translated_then_the_text_streams_the_tool_has_a_start_and_a_finish_and_the_turn_ends() {
        // Given the notifications of a recorded turn: a message that streams, a command that fails, a final answer that arrives only as its completed text, the end
        let frames = [
            json!({"method": "turn/started", "params": {"threadId": "th", "turn": {"id": "u1"}}}),
            json!({"method": "item/started", "params": {"item": {"type": "userMessage", "id": "m0"}}}),
            json!({"method": "item/started", "params": {"item": {"type": "agentMessage", "id": "m1", "text": ""}}}),
            json!({"method": "item/agentMessage/delta", "params": {"itemId": "m1", "delta": "I'll "}}),
            json!({"method": "item/agentMessage/delta", "params": {"itemId": "m1", "delta": "run it."}}),
            json!({"method": "item/completed", "params": {"item": {"type": "agentMessage", "id": "m1", "text": "I'll run it."}}}),
            json!({"method": "item/started", "params": {"item": {"type": "commandExecution", "id": "c1", "command": "/bin/bash -lc \"false\"", "cwd": "/w", "aggregatedOutput": null, "exitCode": null}}}),
            json!({"method": "item/commandExecution/outputDelta", "params": {"itemId": "c1", "delta": "x"}}),
            json!({"method": "item/completed", "params": {"item": {"type": "commandExecution", "id": "c1", "command": "/bin/bash -lc \"false\"", "status": "failed", "aggregatedOutput": "boom\n", "exitCode": 1}}}),
            json!({"method": "item/completed", "params": {"item": {"type": "agentMessage", "id": "m2", "text": "It failed."}}}),
            json!({"method": "thread/tokenUsage/updated", "params": {"threadId": "th"}}),
            json!({"method": "turn/completed", "params": {"threadId": "th", "turn": {"id": "u1", "status": "completed", "error": null}}}),
        ];
        // When they are translated
        let steps = feed(&mut Translator::default(), &frames);
        // Then the streamed text arrives as its deltas only, the command is one call that failed with its output, the second message follows after a break, and the turn ends completed
        assert_eq!(
            steps,
            vec![
                Step::Started("u1".into()),
                ev(ExecEvent::Text("I'll ".into())),
                ev(ExecEvent::Text("run it.".into())),
                ev(ExecEvent::ToolStarted { id: "c1".into(), name: "command".into(), input: json!({"command": "/bin/bash -lc \"false\"", "cwd": "/w"}) }),
                ev(ExecEvent::ToolFinished { id: "c1".into(), output: "boom\n".into(), ok: false }),
                ev(ExecEvent::Text("\n\n".into())),
                ev(ExecEvent::Text("It failed.".into())),
                Step::Ended("completed".into()),
            ]
        );
    }

    #[test]
    fn given_a_failed_turn_and_an_interrupted_one_when_translated_then_the_failure_is_one_error_and_an_interrupt_has_none() {
        // Given a final error notification followed by the turn's failed end, a failed end with no notification before it, and an interrupted end
        let mut translator = Translator::default();
        let error = json!({"method": "error", "params": {"error": {"message": "usage limit"}, "willRetry": false, "threadId": "th"}});
        let retried = json!({"method": "error", "params": {"error": {"message": "reconnecting"}, "willRetry": true, "threadId": "th"}});
        let failed = json!({"method": "turn/completed", "params": {"turn": {"id": "u", "status": "failed", "error": {"message": "usage limit"}}}});
        // When they are translated
        let with_notice = feed(&mut translator, &[retried, error, failed.clone()]);
        translator.reset();
        let without = feed(&mut translator, &[failed]);
        let interrupted = feed(&mut Translator::default(), &[json!({"method": "turn/completed", "params": {"turn": {"id": "u", "status": "interrupted", "error": null}}})]);
        // Then a retried error says nothing, the error is said once, and an interrupt ends the turn with no error
        assert_eq!(with_notice, vec![ev(ExecEvent::Error("usage limit".into())), Step::Ended("failed".into())]);
        assert_eq!(without, vec![ev(ExecEvent::Error("usage limit".into())), Step::Ended("failed".into())]);
        assert_eq!(interrupted, vec![Step::Ended("interrupted".into())]);
    }

    #[test]
    fn given_file_changes_and_an_mcp_call_when_translated_then_each_change_is_a_call_that_names_its_kind_and_the_mcp_call_names_its_server() {
        // Given a completed patch that adds one file and edits another, and an MCP tool call that answered with text
        let frames = [
            json!({"method": "item/completed", "params": {"item": {"type": "fileChange", "id": "f1", "status": "completed", "changes": [{"path": "a.txt", "kind": {"type": "add"}, "diff": "+x"}, {"path": "b.txt", "kind": {"type": "update"}, "diff": "-a+b"}]}}}),
            json!({"method": "item/started", "params": {"item": {"type": "mcpToolCall", "id": "k1", "server": "docs", "tool": "search", "arguments": {"q": "x"}}}}),
            json!({"method": "item/completed", "params": {"item": {"type": "mcpToolCall", "id": "k1", "server": "docs", "tool": "search", "status": "completed", "arguments": {"q": "x"}, "result": {"content": [{"type": "text", "text": "found"}]}, "error": null}}}),
        ];
        // When they are translated
        let steps = feed(&mut Translator::default(), &frames);
        // Then the patch is two calls (create_file and Edit, which the log turns into file changes), and the MCP call is `mcp__docs__search` with its text
        let names: Vec<(&str, &str)> = steps.iter().filter_map(|s| if let Step::Event(ExecEvent::ToolStarted { id, name, .. }) = s { Some((id.as_str(), name.as_str())) } else { None }).collect();
        assert_eq!(names, [("f1:0", "create_file"), ("f1:1", "Edit"), ("k1", "mcp__docs__search")]);
        assert_eq!(steps.last(), Some(&ev(ExecEvent::ToolFinished { id: "k1".into(), output: "found".into(), ok: true })));
        assert!(steps.contains(&ev(ExecEvent::ToolFinished { id: "f1:1".into(), output: String::new(), ok: true })));
    }
}
