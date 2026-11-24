//! The wire of one `claude -p --input-format stream-json --output-format stream-json` process over stdio (`docs/solutions/architecture/claude-code-stream-json-protocol.md`):
//! one JSON object a line. The frames a session writes, what an inbound line is, and what the process's output means as the neutral events a client shows. Pure.
use super::codex::Verdict;
use super::driver::TurnRequest;
use super::translate::ExecEvent;
use serde_json::{json, Value};
use std::collections::HashSet;

#[derive(PartialEq, Eq, Hash)]
pub(crate) struct TaskId {
    pub task: String,
    pub run: Option<String>,
}

pub(crate) enum TaskFrame {
    Started(TaskId),
    Finished { id: TaskId, status: String, summary: String, stdout: Option<TaskStdout> },
}

#[derive(Debug, PartialEq, Eq)]
pub struct TaskStdout {
    pub text: String,
    pub truncated: bool,
}

pub(crate) fn task_frame(frame: &Value) -> Option<TaskFrame> {
    if frame["type"] != "system" {
        return None;
    }
    let id = || Some(TaskId { task: frame["task_id"].as_str()?.to_string(), run: frame["run_id"].as_str().map(str::to_string) });
    match frame["subtype"].as_str()? {
        "task_started" if frame["is_backgrounded"] == true => Some(TaskFrame::Started(id()?)),
        "task_notification" => {
            let status = frame["status"].as_str()?;
            matches!(status, "completed" | "failed" | "stopped").then_some(())?;
            let output = &frame["rca_task_stdout"];
            let stdout = output["text"].as_str().and_then(|text| Some(TaskStdout {
                text: text.to_string(),
                truncated: output["truncated"].as_bool()?,
            }));
            Some(TaskFrame::Finished { id: id()?, status: status.to_string(), summary: frame["summary"].as_str()?.to_string(), stdout })
        }
        _ => None,
    }
}

/// The permission mode of the CLI for a turn: a plan only plans, the mode that asks has the CLI ask (as a `can_use_tool` request the session
/// hands to the person), the mode that accepts edits asks for the rest, and full access never asks.
pub fn permission_mode(req: &TurnRequest) -> &'static str {
    if req.interaction_mode == "plan" {
        return "plan";
    }
    match req.runtime_mode.as_str() {
        "approval-required" => "default",
        "auto-accept-edits" => "acceptEdits",
        _ => "bypassPermissions",
    }
}

/// The per-process flags of one thread's CLI: the conversation (a new id on the thread's first start, the stored one after), its mode and model, and the
/// service's tool server.
pub fn session_args(req: &TurnRequest, resume: bool, conversation: &str) -> Vec<String> {
    let mut args = vec![if resume { "--resume" } else { "--session-id" }.to_string(), conversation.to_string(), "--permission-mode".into(), permission_mode(req).into()];
    if let Some(model) = &req.model {
        args.extend(["--model".to_string(), model.clone()]);
    }
    if let Some(tools) = &req.tools {
        let config = json!({"mcpServers": {"executor": {"type": "http", "url": tools.url, "headers": {"Authorization": format!("Bearer {}", tools.token)}}}});
        args.extend(["--mcp-config".to_string(), config.to_string()]);
    }
    args
}

fn line(value: Value) -> String {
    value.to_string()
}

fn request(id: &str, body: Value) -> String {
    line(json!({"type": "control_request", "request_id": id, "request": body}))
}

pub fn initialize(id: &str) -> String {
    request(id, json!({"subtype": "initialize"}))
}

/// One message of the person. `priority` is `now` for a steer, which the CLI runs next in the same process and conversation, aborting what it was doing.
pub fn user_message(uuid: &str, text: &str, priority: Option<&str>) -> String {
    let mut message = json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}, "parent_tool_use_id": null, "uuid": uuid});
    if let Some(priority) = priority {
        message["priority"] = json!(priority);
    }
    line(message)
}

/// Stops the turn; messages still queued in the CLI are cancelled with it, because the service keeps the queue.
pub fn interrupt(id: &str) -> String {
    request(id, json!({"subtype": "interrupt", "cancel_queued": true}))
}

pub fn set_permission_mode(id: &str, mode: &str) -> String {
    request(id, json!({"subtype": "set_permission_mode", "mode": mode}))
}

pub fn set_model(id: &str, model: &str) -> String {
    request(id, json!({"subtype": "set_model", "model": model}))
}

fn success(request_id: &str, body: Value) -> String {
    line(json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": body}}))
}

/// The answer to a request of the CLI's that is no business of ours: an error, never silence.
pub fn refuse(request_id: &str, subtype: &str) -> String {
    line(json!({"type": "control_response", "response": {"subtype": "error", "request_id": request_id, "error": format!("Unsupported control request subtype: {subtype}"), "error_code": "unsupported_request"}}))
}

/// What the CLI asked about a tool call, kept to answer it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolAsk {
    pub request: String,
    pub tool: String,
    pub input: Value,
    pub tool_use: String,
    pub suggestions: Option<Value>,
}

/// The tool call a `control_request` asks permission for; none for a request that is anything else.
pub fn tool_ask(request_id: &str, body: &Value) -> Option<ToolAsk> {
    if body["subtype"] != "can_use_tool" {
        return None;
    }
    let tool = body["tool_name"].as_str()?.to_string();
    let tool_use = body["tool_use_id"].as_str().filter(|id| !id.is_empty()).unwrap_or(request_id).to_string();
    Some(ToolAsk { request: request_id.to_string(), tool, input: body["input"].clone(), tool_use, suggestions: body.get("permission_suggestions").filter(|s| s.as_array().is_some_and(|a| !a.is_empty())).cloned() })
}

/// The answer to a `can_use_tool` request for the person's `verdict`: the call runs as asked, runs and stays allowed for the rest of the session, is declined
/// (the model reads why), or is declined and ends the turn.
pub fn answer_tool(ask: &ToolAsk, verdict: Verdict) -> String {
    let id = json!(ask.tool_use);
    let body = match verdict {
        Verdict::Accept => json!({"behavior": "allow", "updatedInput": ask.input, "toolUseID": id, "decisionClassification": "user_temporary"}),
        Verdict::AcceptForSession => {
            let rules = json!([{"type": "addRules", "rules": [{"toolName": ask.tool}], "behavior": "allow", "destination": "session"}]);
            let permissions = match &ask.suggestions {
                Some(all) => Value::Array(all.as_array().into_iter().flatten().cloned().map(|mut s| {
                    s["destination"] = json!("session");
                    s
                }).collect()),
                None => rules,
            };
            json!({"behavior": "allow", "updatedInput": ask.input, "updatedPermissions": permissions, "toolUseID": id, "decisionClassification": "user_permanent"})
        }
        Verdict::Decline => json!({"behavior": "deny", "message": "User declined tool execution.", "toolUseID": id, "decisionClassification": "user_reject"}),
        Verdict::Cancel => json!({"behavior": "deny", "message": "User declined tool execution and ended the turn.", "interrupt": true, "toolUseID": id, "decisionClassification": "user_reject"}),
    };
    success(&ask.request, body)
}

/// A question the CLI asks the person through its `AskUserQuestion` tool: each has a text, a short list of labels and whether several may be chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub text: String,
    pub options: Vec<String>,
}

/// The questions of an `AskUserQuestion` call, in order.
pub fn questions_of(input: &Value) -> Vec<Question> {
    input["questions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| Some(Question { text: q["question"].as_str()?.to_string(), options: q["options"].as_array().into_iter().flatten().filter_map(|o| o["label"].as_str().or_else(|| o.as_str())).map(str::to_string).collect() }))
        .collect()
}

/// The allow of an `AskUserQuestion` call with every answer given, keyed by the question's text.
pub fn answer_questions(ask: &ToolAsk, answers: &[(String, String)]) -> String {
    let mut input = ask.input.clone();
    input["answers"] = answers.iter().map(|(q, a)| (q.clone(), json!(a))).collect::<serde_json::Map<_, _>>().into();
    success(&ask.request, json!({"behavior": "allow", "updatedInput": input, "toolUseID": ask.tool_use}))
}

/// The deny of an `AskUserQuestion` call whose question the person closed.
pub fn dismiss_questions(ask: &ToolAsk) -> String {
    success(&ask.request, json!({"behavior": "deny", "message": "User cancelled tool execution.", "toolUseID": ask.tool_use, "decisionClassification": "user_reject"}))
}

/// What one line the process printed is.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// The answer to a control request of ours: the response body, or the CLI's error.
    Response { id: String, outcome: Result<Value, String> },
    /// A request of the CLI's that waits for our answer under its own id.
    Request { id: String, body: Value },
    /// The CLI no longer needs the answer to its request.
    Withdrawn { id: String },
    Frame(Value),
    /// Not JSON, a keep-alive, or a frame that is none of these.
    Other,
}

pub fn parse(text: &str) -> Inbound {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return Inbound::Other };
    match v["type"].as_str() {
        Some("control_response") => {
            let r = &v["response"];
            let Some(id) = r["request_id"].as_str() else { return Inbound::Other };
            let outcome = if r["subtype"] == "error" { Err(r["error"].as_str().unwrap_or("the CLI refused the request").to_string()) } else { Ok(r["response"].clone()) };
            Inbound::Response { id: id.to_string(), outcome }
        }
        Some("control_request") => match v["request_id"].as_str() {
            Some(id) => Inbound::Request { id: id.to_string(), body: v["request"].clone() },
            None => Inbound::Other,
        },
        Some("control_cancel_request") => v["request_id"].as_str().map_or(Inbound::Other, |id| Inbound::Withdrawn { id: id.to_string() }),
        Some("keep_alive") | None => Inbound::Other,
        Some(_) => Inbound::Frame(v),
    }
}

/// What a `result` frame says of the turn it ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The messages of ours the turn consumed; none when the frame does not say, an empty list for a turn nobody on the phone started.
    pub messages: Option<Vec<String>>,
    /// Stopped by an interrupt or a steer (`terminal_reason` `aborted_*`), which is no failure.
    pub aborted: bool,
    /// The CLI's own words when the turn failed.
    pub error: Option<String>,
}

/// What a frame of the process means to the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Event(ExecEvent),
    /// The conversation the process runs in (the first frame of every turn).
    Conversation(String),
    Result(Outcome),
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        other => other.to_string(),
    }
}

fn outcome_of(v: &Value) -> Outcome {
    let messages = match &v["user_message_uuids"] {
        Value::Array(all) => Some(all.iter().filter_map(Value::as_str).map(str::to_string).collect()),
        Value::Null if v.get("user_message_uuids").is_some() => Some(Vec::new()),
        _ => None,
    };
    let aborted = v["terminal_reason"].as_str().is_some_and(|r| r.starts_with("aborted"));
    let error = (v["is_error"] == true && !aborted).then(|| {
        let said = text_of(&v["result"]);
        if !said.is_empty() {
            said
        } else {
            let listed = v["errors"].as_array().map(|all| all.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; ")).unwrap_or_default();
            if listed.is_empty() { v["subtype"].as_str().unwrap_or("the turn failed").to_string() } else { listed }
        }
    });
    Outcome { messages, aborted, error }
}

/// Reads the frames of one process: the text the model streams reaches the client as it comes, so the complete frame that follows it says it again
/// only when nothing streamed.
#[derive(Default)]
pub struct Translator {
    streamed: HashSet<String>,
    message: String,
}

impl Translator {
    pub fn reset(&mut self) {
        self.streamed.clear();
        self.message.clear();
    }

    pub fn frame(&mut self, v: &Value) -> Vec<Step> {
        match v["type"].as_str() {
            Some("system") if v["subtype"] == "init" => v["session_id"].as_str().filter(|id| !id.is_empty()).map(|id| Step::Conversation(id.to_string())).into_iter().collect(),
            Some("stream_event") if v["parent_tool_use_id"].is_null() => self.stream(&v["event"]),
            Some("assistant") => self.assistant(v),
            Some("user") if v["isReplay"] != true => v["message"]["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "tool_result")
                .map(|b| Step::Event(ExecEvent::ToolFinished { id: text_of(&b["tool_use_id"]), output: text_of(&b["content"]), ok: b["is_error"] != true }))
                .collect(),
            Some("result") => vec![Step::Result(outcome_of(v))],
            _ => vec![],
        }
    }

    fn stream(&mut self, event: &Value) -> Vec<Step> {
        match event["type"].as_str() {
            Some("message_start") => {
                self.message = event["message"]["id"].as_str().unwrap_or_default().to_string();
                vec![]
            }
            Some("content_block_delta") => {
                let delta = &event["delta"];
                let streamed = match delta["type"].as_str() {
                    Some("text_delta") => ExecEvent::Text(text_of(&delta["text"])),
                    Some("thinking_delta") => ExecEvent::Reasoning(text_of(&delta["thinking"])),
                    _ => return vec![],
                };
                self.streamed.insert(self.message.clone());
                vec![Step::Event(streamed)]
            }
            _ => vec![],
        }
    }

    fn assistant(&mut self, v: &Value) -> Vec<Step> {
        let said = v["message"]["id"].as_str().is_some_and(|id| self.streamed.contains(id));
        v["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| match b["type"].as_str()? {
                "text" if !said => Some(ExecEvent::Text(text_of(&b["text"]))),
                "thinking" if !said => Some(ExecEvent::Reasoning(text_of(&b["thinking"]))),
                "tool_use" => Some(ExecEvent::ToolStarted { id: text_of(&b["id"]), name: text_of(&b["name"]), input: b["input"].clone() }),
                _ => None,
            })
            .map(Step::Event)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::driver::ToolServer;

    fn req() -> TurnRequest {
        TurnRequest { thread_id: "t".into(), run_id: "r".into(), instance_id: "claudeAgent".into(), model: None, cwd: ".".into(), prompt: "go".into(), runtime_mode: "full-access".into(), interaction_mode: "default".into(), resume: None, tools: None }
    }

    fn frame(t: &mut Translator, v: Value) -> Vec<Step> {
        t.frame(&v)
    }

    #[test]
    fn given_the_modes_of_a_turn_when_the_cli_s_flags_are_built_then_a_new_conversation_names_its_id_a_stored_one_is_resumed_and_the_tool_server_is_told_with_its_token() {
        // Given a turn that asks before each tool with a model and a tool server, a plan, and one in the mode that accepts edits
        let asking = TurnRequest { runtime_mode: "approval-required".into(), model: Some("opus".into()), tools: Some(ToolServer { url: "http://x/mcp/t".into(), token: "tok-1".into() }), ..req() };
        let plan = TurnRequest { interaction_mode: "plan".into(), ..asking.clone() };
        let edits = TurnRequest { runtime_mode: "auto-accept-edits".into(), ..req() };
        // When the flags are built for a first start and for a restart
        let first = session_args(&asking, false, "11111111-1111-4111-8111-111111111111");
        let again = session_args(&edits, true, "s-1");
        // Then the first names a new id, the mode, the model and the tool server's address with its bearer, and the restart resumes without a model or tool server
        assert_eq!(&first[..4], ["--session-id", "11111111-1111-4111-8111-111111111111", "--permission-mode", "default"]);
        assert_eq!(&first[4..6], ["--model", "opus"]);
        assert!(first[7].contains("http://x/mcp/t") && first[7].contains("Bearer tok-1") && !first[7].contains("approval_prompt"), "{first:?}");
        assert_eq!(again, ["--resume", "s-1", "--permission-mode", "acceptEdits"]);
        assert_eq!((permission_mode(&plan), permission_mode(&req())), ("plan", "bypassPermissions"));
    }

    #[test]
    fn given_streamed_text_when_the_complete_frame_follows_then_it_is_not_said_twice_and_a_tool_call_always_is() {
        // Given a message whose text streams as deltas, and then arrives complete beside a tool call, and a second message that was never streamed
        let mut t = Translator::default();
        let mut steps = Vec::new();
        for v in [
            json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "message_start", "message": {"id": "m1"}}}),
            json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hel"}}}),
            json!({"type": "stream_event", "parent_tool_use_id": null, "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "lo"}}}),
            json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "text", "text": "Hello"}]}}),
            json!({"type": "assistant", "message": {"id": "m1", "content": [{"type": "tool_use", "id": "k", "name": "Bash", "input": {"command": "ls"}}]}}),
            json!({"type": "assistant", "message": {"id": "m2", "content": [{"type": "text", "text": "later"}]}}),
        ] {
            steps.extend(frame(&mut t, v));
        }
        // Then the text came as its deltas once, the tool call once, and the unstreamed message as it was
        let events: Vec<ExecEvent> = steps.into_iter().filter_map(|s| if let Step::Event(e) = s { Some(e) } else { None }).collect();
        assert_eq!(events, vec![ExecEvent::Text("Hel".into()), ExecEvent::Text("lo".into()), ExecEvent::ToolStarted { id: "k".into(), name: "Bash".into(), input: json!({"command": "ls"}) }, ExecEvent::Text("later".into())]);
    }

    #[test]
    fn given_the_other_frames_of_a_turn_when_read_then_a_tool_result_is_a_finish_a_replay_and_noise_are_nothing_and_the_init_names_the_conversation() {
        let mut t = Translator::default();
        assert_eq!(frame(&mut t, json!({"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "k", "content": [{"type": "text", "text": "a.txt"}], "is_error": false}]}})), vec![Step::Event(ExecEvent::ToolFinished { id: "k".into(), output: "a.txt".into(), ok: true })]);
        assert!(frame(&mut t, json!({"type": "user", "isReplay": true, "message": {"content": [{"type": "text", "text": "hi"}]}})).is_empty());
        assert!(frame(&mut t, json!({"type": "rate_limit_event"})).is_empty() && frame(&mut t, json!({"type": "system", "subtype": "status"})).is_empty());
        assert_eq!(frame(&mut t, json!({"type": "system", "subtype": "init", "session_id": "s-1"})), vec![Step::Conversation("s-1".into())]);
    }

    #[test]
    fn given_results_of_each_kind_when_read_then_a_failure_carries_the_cli_s_words_an_abort_is_none_and_the_messages_consumed_are_listed_when_said() {
        let mut t = Translator::default();
        let result = |v: Value| match frame(&mut Translator::default(), v).pop() {
            Some(Step::Result(o)) => o,
            other => panic!("{other:?}"),
        };
        let _ = &mut t;
        let ok = result(json!({"type": "result", "subtype": "success", "is_error": false, "user_message_uuids": ["u1"]}));
        let failed = result(json!({"type": "result", "subtype": "success", "is_error": true, "result": "rate limited", "user_message_uuids": ["u1"]}));
        let aborted = result(json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "terminal_reason": "aborted_streaming", "result": null}));
        let wake = result(json!({"type": "result", "subtype": "success", "is_error": false, "user_message_uuids": null}));
        let broken = result(json!({"type": "result", "subtype": "error_during_execution", "is_error": true, "errors": ["No conversation found with session ID: x"]}));
        assert_eq!((ok.messages, ok.error), (Some(vec!["u1".to_string()]), None));
        assert_eq!(failed.error.as_deref(), Some("rate limited"));
        assert_eq!((aborted.aborted, aborted.error, aborted.messages), (true, None, None));
        assert_eq!(wake.messages, Some(vec![]));
        assert_eq!(broken.error.as_deref(), Some("No conversation found with session ID: x"));
    }

    #[test]
    fn given_the_lines_a_process_prints_when_parsed_then_control_frames_are_told_from_turn_frames_and_a_keep_alive_is_nothing() {
        assert_eq!(parse(r#"{"type":"control_response","response":{"subtype":"success","request_id":"r1","response":{"mode":"plan"}}}"#), Inbound::Response { id: "r1".into(), outcome: Ok(json!({"mode": "plan"})) });
        assert_eq!(parse(r#"{"type":"control_response","response":{"subtype":"error","request_id":"r2","error":"no"}}"#), Inbound::Response { id: "r2".into(), outcome: Err("no".into()) });
        assert!(matches!(parse(r#"{"type":"control_request","request_id":"c1","request":{"subtype":"can_use_tool"}}"#), Inbound::Request { id, .. } if id == "c1"));
        assert_eq!(parse(r#"{"type":"control_cancel_request","request_id":"c1"}"#), Inbound::Withdrawn { id: "c1".into() });
        assert!(matches!(parse(r#"{"type":"assistant"}"#), Inbound::Frame(_)));
        assert_eq!((parse(r#"{"type":"keep_alive"}"#), parse("not json")), (Inbound::Other, Inbound::Other));
    }

    #[test]
    fn given_a_permission_request_when_answered_then_each_verdict_is_the_allow_or_deny_the_cli_reads_under_its_request_id() {
        // Given a request to write a file, with a suggestion of the CLI's
        let body = json!({"subtype": "can_use_tool", "tool_name": "Write", "input": {"file_path": "a"}, "tool_use_id": "toolu_1", "permission_suggestions": [{"type": "setMode", "mode": "acceptEdits", "destination": "userSettings"}]});
        let ask = tool_ask("b3a9", &body).unwrap();
        let said = |v: Verdict| serde_json::from_str::<Value>(&answer_tool(&ask, v)).unwrap();
        // Then accept allows with the input echoed, for the session it allows with the suggestion kept to the session, decline denies with the reason, and cancel also ends the turn
        let accepted = said(Verdict::Accept);
        assert_eq!((accepted["response"]["request_id"].clone(), accepted["response"]["response"]["behavior"].clone(), accepted["response"]["response"]["updatedInput"].clone()), (json!("b3a9"), json!("allow"), json!({"file_path": "a"})));
        assert_eq!(said(Verdict::AcceptForSession)["response"]["response"]["updatedPermissions"], json!([{"type": "setMode", "mode": "acceptEdits", "destination": "session"}]));
        let declined = said(Verdict::Decline);
        assert_eq!((declined["response"]["response"]["behavior"].clone(), declined["response"]["response"]["message"].clone(), declined["response"]["response"].get("interrupt").cloned()), (json!("deny"), json!("User declined tool execution."), None));
        assert_eq!(said(Verdict::Cancel)["response"]["response"]["interrupt"], json!(true));
        // And a request with no suggestion is allowed for the session by a rule for the tool, and another request subtype is no tool ask
        let plain = tool_ask("r", &json!({"subtype": "can_use_tool", "tool_name": "Bash", "input": {}})).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&answer_tool(&plain, Verdict::AcceptForSession)).unwrap()["response"]["response"]["updatedPermissions"][0]["rules"], json!([{"toolName": "Bash"}]));
        assert_eq!(plain.tool_use, "r");
        assert!(tool_ask("r", &json!({"subtype": "elicitation"})).is_none());
    }

    #[test]
    fn given_an_ask_user_question_when_answered_then_the_answers_are_keyed_by_the_question_text_and_a_closed_question_is_a_cancelled_tool() {
        let input = json!({"questions": [{"question": "Which library?", "header": "Library", "multiSelect": false, "options": [{"label": "date-fns", "description": "x"}, {"label": "dayjs", "description": "y"}]}]});
        let ask = tool_ask("c41d", &json!({"subtype": "can_use_tool", "tool_name": "AskUserQuestion", "input": input, "tool_use_id": "toolu_2"})).unwrap();
        assert_eq!(questions_of(&ask.input), vec![Question { text: "Which library?".into(), options: vec!["date-fns".into(), "dayjs".into()] }]);
        let allowed = serde_json::from_str::<Value>(&answer_questions(&ask, &[("Which library?".into(), "dayjs".into())])).unwrap();
        assert_eq!(allowed["response"]["response"]["updatedInput"]["answers"], json!({"Which library?": "dayjs"}));
        assert_eq!(allowed["response"]["response"]["updatedInput"]["questions"], input["questions"]);
        assert_eq!(serde_json::from_str::<Value>(&dismiss_questions(&ask)).unwrap()["response"]["response"]["message"], json!("User cancelled tool execution."));
    }

    #[test]
    fn given_a_message_with_and_without_a_priority_when_built_then_only_the_steer_carries_one_and_both_carry_the_uuid() {
        let plain: Value = serde_json::from_str(&user_message("u1", "go", None)).unwrap();
        let steer: Value = serde_json::from_str(&user_message("u2", "left", Some("now"))).unwrap();
        assert_eq!((plain["uuid"].clone(), plain.get("priority").cloned(), plain["message"]["content"][0]["text"].clone()), (json!("u1"), None, json!("go")));
        assert_eq!((steer["uuid"].clone(), steer["priority"].clone()), (json!("u2"), json!("now")));
        assert_eq!(serde_json::from_str::<Value>(&interrupt("i1")).unwrap()["request"], json!({"subtype": "interrupt", "cancel_queued": true}));
    }
}
