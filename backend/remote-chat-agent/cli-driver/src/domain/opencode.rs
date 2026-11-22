//! opencode's server as the service speaks it (`docs/solutions/architecture/opencode-server-protocol.md`): the lines of the executor's bridge (a request goes
//! out as one line, its answer, the server's events and the end of its event stream come back as lines), the bodies of the requests a turn makes, and what
//! the events of a session mean as neutral events. Pure: nothing here reaches a process or the network.
use super::codex::Verdict;
use super::translate::ExecEvent;
use serde_json::{json, Value};
use std::collections::HashMap;

/// One line of the bridge's output.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    /// The server listens and its event stream is open.
    Ready,
    /// The answer to the request numbered `id`: its status (0 when it could not be made, `body` then says why) and its body as text.
    Response { id: u64, status: u16, body: String },
    /// One frame of the event stream: the kind of event and its properties.
    Event { kind: String, properties: Value },
    /// The event stream ended.
    Closed(String),
    Other,
}

pub fn parse(line: &str) -> Inbound {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return Inbound::Other };
    if v["ready"] == true {
        return Inbound::Ready;
    }
    if let Some(why) = v["closed"].as_str() {
        return Inbound::Closed(why.to_string());
    }
    if let Some(id) = v["id"].as_u64() {
        let status = v["status"].as_u64().unwrap_or(0) as u16;
        let body = v["body"].as_str().or_else(|| v["error"].as_str()).unwrap_or_default().to_string();
        return Inbound::Response { id, status, body };
    }
    let payload = &v["event"]["payload"];
    match payload["type"].as_str() {
        Some(kind) => Inbound::Event { kind: kind.to_string(), properties: payload["properties"].clone() },
        None => Inbound::Other,
    }
}

/// The line that asks the bridge for `method` `path` with `body`, numbered `id`.
pub fn request(id: u64, method: &str, path: &str, body: Option<&Value>) -> String {
    let mut line = json!({"id": id, "method": method, "path": path});
    if let Some(body) = body {
        line["body"] = body.clone();
    }
    line.to_string()
}

/// `text` as a query value: everything but the unreserved characters is percent-encoded.
fn encode(text: &str) -> String {
    text.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The query that names a directory the session works in: only an absolute one is named (the server runs in its workspace's directory, which is
/// every other request's default).
pub fn directory_query(cwd: &str) -> String {
    if cwd.starts_with('/') {
        format!("?directory={}", encode(cwd))
    } else {
        String::new()
    }
}

pub fn session_path(session: &str, tail: &str, cwd: &str) -> String {
    format!("/session/{}{tail}{}", encode(session), directory_query(cwd))
}

pub fn create_path(cwd: &str) -> String {
    format!("/session{}", directory_query(cwd))
}

/// The permission rules of a session for the thread's runtime mode, as the T3 adapter builds them: everything allowed in full access; otherwise what could
/// change the machine is asked about (or, for edits when the mode accepts them, allowed) and the reading tools are allowed.
pub fn rules(runtime_mode: &str) -> Value {
    if runtime_mode == "full-access" {
        return json!([{"permission": "*", "pattern": "*", "action": "allow"}]);
    }
    let edits = if runtime_mode == "auto-accept-edits" { "allow" } else { "ask" };
    let mut rules = vec![json!({"permission": "*", "pattern": "*", "action": "deny"})];
    for permission in ["bash", "edit", "webfetch", "websearch", "codesearch", "external_directory", "doom_loop"] {
        let action = if permission == "edit" { edits } else { "ask" };
        rules.push(json!({"permission": permission, "pattern": "*", "action": action}));
    }
    for permission in ["question", "read", "glob", "grep", "lsp", "todowrite", "task", "skill"] {
        rules.push(json!({"permission": permission, "pattern": "*", "action": "allow"}));
    }
    for pattern in ["*.env", "*.env.*"] {
        rules.push(json!({"permission": "read", "pattern": pattern, "action": "ask"}));
    }
    rules.push(json!({"permission": "read", "pattern": "*.env.example", "action": "allow"}));
    Value::Array(rules)
}

fn split_model(model: &str) -> Option<(&str, &str)> {
    let at = model.find('/').filter(|&i| i >= 1 && i + 1 < model.len())?;
    Some((&model[..at], &model[at + 1..]))
}

/// The body of a prompt: the text, the agent the interaction mode names and the model, when it names a provider.
pub fn prompt_body(prompt: &str, model: Option<&str>, plan: bool) -> Value {
    let mut body = json!({"parts": [{"type": "text", "text": prompt}], "agent": if plan { "plan" } else { "build" }});
    if let Some((provider, id)) = model.and_then(split_model) {
        body["model"] = json!({"providerID": provider, "modelID": id});
    }
    body
}

pub fn reply_body(verdict: Verdict) -> Value {
    json!({"reply": match verdict { Verdict::Accept => "once", Verdict::AcceptForSession => "always", Verdict::Decline | Verdict::Cancel => "reject" }})
}

/// The text of the error a response carries (`{"data":{"message"}}`, `{"message"}`), else its body cut short.
pub fn failure(status: u16, body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let said = v["data"]["message"].as_str().or_else(|| v["message"].as_str()).map(str::to_string);
    let said = said.unwrap_or_else(|| body.chars().take(200).collect());
    if status == 0 {
        said
    } else {
        format!("{status}: {said}")
    }
}

/// What a request of the session asks the person.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Permission,
    /// A question tool's request with this many questions.
    Question(usize),
}

/// What one event does to the turn of its session.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Event(ExecEvent),
    /// The session asks the person; the id is the request's.
    Asked { id: String, ask: Ask, event: ExecEvent },
    /// A request was answered, by the person or by anyone else.
    Settled(String),
    /// The session is idle: the turn is over.
    Idle,
    /// A user message was stored: a prompt was taken.
    Admitted,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Reasoning,
}

/// The session id an event names.
pub fn session_of(properties: &Value) -> Option<&str> {
    properties["sessionID"].as_str().or_else(|| properties["part"]["sessionID"].as_str()).or_else(|| properties["info"]["sessionID"].as_str()).filter(|s| !s.is_empty())
}

/// The events of one session as a turn: what the assistant says and calls, in the order the server sends it. Only an assistant message's parts are the
/// turn's output (the prompt's own text comes back as a part of a user message), and a part is told by what it adds to what was said of it.
#[derive(Debug, Default)]
pub struct Translator {
    roles: HashMap<String, String>,
    said: HashMap<String, String>,
    kinds: HashMap<String, Kind>,
    called: HashMap<String, bool>,
    error: Option<String>,
    aborted: bool,
}

impl Translator {
    /// A new turn starts: nothing of the one before is kept, but the roles of the messages it stored.
    pub fn reset(&mut self) {
        self.said.clear();
        self.kinds.clear();
        self.called.clear();
        self.error = None;
        self.aborted = false;
    }

    fn assistant(&self, message: &str) -> bool {
        self.roles.get(message).is_some_and(|r| r == "assistant")
    }

    /// The text a part has grown to: what it adds.
    fn grown(&mut self, part: &str, kind: Kind, text: &str) -> Option<ExecEvent> {
        self.kinds.insert(part.to_string(), kind);
        let before = self.said.entry(part.to_string()).or_default();
        let add = text.strip_prefix(before.as_str()).filter(|a| !a.is_empty()).map(str::to_string);
        if text.starts_with(before.as_str()) || before.is_empty() {
            *before = text.to_string();
        }
        add.map(|t| if kind == Kind::Text { ExecEvent::Text(t) } else { ExecEvent::Reasoning(t) })
    }

    pub fn event(&mut self, kind: &str, p: &Value) -> Vec<Step> {
        match kind {
            "message.updated" => {
                let info = &p["info"];
                if let (Some(id), Some(role)) = (info["id"].as_str(), info["role"].as_str()) {
                    let first = self.roles.insert(id.to_string(), role.to_string()).is_none();
                    if first && role == "user" {
                        return vec![Step::Admitted];
                    }
                }
                vec![]
            }
            "message.part.updated" => self.part(&p["part"]),
            "message.part.delta" => {
                let (Some(message), Some(part)) = (p["messageID"].as_str(), p["partID"].as_str()) else { return vec![] };
                let (Some(kind), true) = (self.kinds.get(part).copied(), p["field"] == "text" && self.assistant(message)) else { return vec![] };
                let delta = p["delta"].as_str().unwrap_or_default();
                self.said.entry(part.to_string()).or_default().push_str(delta);
                match (delta.is_empty(), kind) {
                    (true, _) => vec![],
                    (false, Kind::Text) => vec![Step::Event(ExecEvent::Text(delta.into()))],
                    (false, Kind::Reasoning) => vec![Step::Event(ExecEvent::Reasoning(delta.into()))],
                }
            }
            "permission.asked" => {
                let Some(id) = p["id"].as_str() else { return vec![] };
                let tool = p["permission"].as_str().unwrap_or("tool").to_string();
                let args = json!({"patterns": p["patterns"], "metadata": p["metadata"]});
                vec![Step::Asked { id: id.into(), ask: Ask::Permission, event: ExecEvent::Approval { id: id.into(), tool, args } }]
            }
            "question.asked" => {
                let Some(id) = p["id"].as_str() else { return vec![] };
                let questions = p["questions"].as_array().cloned().unwrap_or_default();
                let prompt = questions.iter().filter_map(|q| q["question"].as_str()).collect::<Vec<_>>().join("\n\n");
                let options = questions.first().and_then(|q| q["options"].as_array()).map(|o| o.iter().filter_map(|o| o["label"].as_str().map(str::to_string)).collect()).unwrap_or_default();
                vec![Step::Asked { id: id.into(), ask: Ask::Question(questions.len().max(1)), event: ExecEvent::Input { id: id.into(), prompt, options } }]
            }
            "permission.replied" | "question.replied" | "question.rejected" => p["requestID"].as_str().map(|id| vec![Step::Settled(id.into())]).unwrap_or_default(),
            "session.error" => {
                let error = &p["error"];
                if error["name"] == "MessageAbortedError" {
                    self.aborted = true;
                } else {
                    let said = error["data"]["message"].as_str().or_else(|| error["name"].as_str()).unwrap_or("the session failed");
                    self.error.get_or_insert_with(|| said.to_string());
                }
                vec![]
            }
            "session.status" if p["status"]["type"] == "idle" => self.idle(),
            "session.idle" => self.idle(),
            _ => vec![],
        }
    }

    fn idle(&mut self) -> Vec<Step> {
        let mut steps: Vec<Step> = self.error.take().map(|e| Step::Event(ExecEvent::Error(e))).into_iter().collect();
        steps.push(Step::Idle);
        steps
    }

    fn part(&mut self, part: &Value) -> Vec<Step> {
        let (Some(id), Some(message)) = (part["id"].as_str(), part["messageID"].as_str()) else { return vec![] };
        if !self.assistant(message) {
            return vec![];
        }
        let events: Vec<ExecEvent> = match part["type"].as_str() {
            Some("text") => self.grown(id, Kind::Text, part["text"].as_str().unwrap_or_default()).into_iter().collect(),
            Some("reasoning") => self.grown(id, Kind::Reasoning, part["text"].as_str().unwrap_or_default()).into_iter().collect(),
            Some("tool") => self.tool(part),
            _ => vec![],
        };
        events.into_iter().map(Step::Event).collect()
    }

    fn tool(&mut self, part: &Value) -> Vec<ExecEvent> {
        let (call, state) = (part["callID"].as_str().unwrap_or_default().to_string(), &part["state"]);
        let status = state["status"].as_str().unwrap_or_default();
        if status == "pending" {
            return vec![];
        }
        let mut out = Vec::new();
        if !self.called.contains_key(&call) {
            self.called.insert(call.clone(), false);
            out.push(ExecEvent::ToolStarted { id: call.clone(), name: part["tool"].as_str().unwrap_or_default().to_string(), input: state["input"].clone() });
        }
        let output = |v: &Value| match v {
            Value::String(t) => t.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        let finished = self.called.get_mut(&call).filter(|done| !**done);
        match (status, finished) {
            ("completed", Some(done)) => {
                *done = true;
                out.push(ExecEvent::ToolFinished { id: call, output: output(&state["output"]), ok: true });
            }
            ("error", Some(done)) => {
                *done = true;
                out.push(ExecEvent::ToolFinished { id: call, output: output(&state["error"]), ok: false });
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(t: &mut Translator, events: &[(&str, Value)]) -> Vec<Step> {
        events.iter().flat_map(|(k, p)| t.event(k, p)).collect()
    }

    fn assistant(id: &str) -> (&'static str, Value) {
        ("message.updated", json!({"sessionID": "s", "info": {"id": id, "role": "assistant"}}))
    }

    #[test]
    fn given_the_lines_of_the_bridge_when_parsed_then_each_is_what_it_says_and_a_frame_that_is_no_event_is_nothing() {
        // Given a ready line, an answer, an answer that could not be made, a session event, the end of the stream and noise
        let answer = parse(r#"{"id":4,"status":200,"body":"{\"id\":\"ses_1\"}"}"#);
        let failed = parse(r#"{"id":5,"status":0,"error":"the opencode server does not answer"}"#);
        let event = parse(r#"{"event":{"directory":"/w","payload":{"id":"evt_1","type":"session.idle","properties":{"sessionID":"ses_1"}}}}"#);
        // Then each reads as what it is
        assert_eq!(parse(r#"{"ready":true}"#), Inbound::Ready);
        assert_eq!(answer, Inbound::Response { id: 4, status: 200, body: "{\"id\":\"ses_1\"}".into() });
        assert_eq!(failed, Inbound::Response { id: 5, status: 0, body: "the opencode server does not answer".into() });
        assert_eq!(event, Inbound::Event { kind: "session.idle".into(), properties: json!({"sessionID": "ses_1"}) });
        assert_eq!(parse(r#"{"closed":"the event stream ended"}"#), Inbound::Closed("the event stream ended".into()));
        assert_eq!((parse("not json"), parse(r#"{"event":{"payload":{}}}"#)), (Inbound::Other, Inbound::Other));
    }

    #[test]
    fn given_the_modes_of_a_thread_when_its_session_is_made_then_the_rules_and_the_prompt_follow_them() {
        // Given the three runtime modes and a model that names its provider
        let full = rules("full-access");
        let asking = rules("approval-required");
        let editing = rules("auto-accept-edits");
        let rule = |rules: &Value, permission: &str| rules.as_array().unwrap().iter().rev().find(|r| r["permission"] == permission && r["pattern"] == "*").map(|r| r["action"].as_str().unwrap().to_string());
        // Then full access allows everything, asking asks for the shell and edits, accepting edits allows them, and the prompt names agent and model
        assert_eq!(full, json!([{"permission": "*", "pattern": "*", "action": "allow"}]));
        assert_eq!((rule(&asking, "bash"), rule(&asking, "edit"), rule(&asking, "read")), (Some("ask".into()), Some("ask".into()), Some("allow".into())));
        assert_eq!((rule(&editing, "bash"), rule(&editing, "edit")), (Some("ask".into()), Some("allow".into())));
        assert_eq!(prompt_body("go", Some("opencode-go/kimi-k2"), true), json!({"parts": [{"type": "text", "text": "go"}], "agent": "plan", "model": {"providerID": "opencode-go", "modelID": "kimi-k2"}}));
        assert!(prompt_body("go", Some("kimi"), false).get("model").is_none());
        // And a directory is named only when it is absolute, percent-encoded
        assert_eq!((directory_query("."), directory_query("/w/my repo")), (String::new(), "?directory=%2Fw%2Fmy%20repo".to_string()));
    }

    #[test]
    fn given_the_events_of_an_assistant_turn_when_translated_then_only_its_own_text_tools_and_end_come_out_in_order() {
        // Given the prompt's user message and text, then an assistant that streams a text, calls a tool and finishes
        let mut t = Translator::default();
        let steps = feed(&mut t, &[
            ("message.updated", json!({"sessionID": "s", "info": {"id": "u1", "role": "user"}})),
            ("message.part.updated", json!({"sessionID": "s", "part": {"id": "p0", "messageID": "u1", "type": "text", "text": "go"}})),
            assistant("a1"),
            ("message.part.updated", json!({"sessionID": "s", "part": {"id": "p1", "messageID": "a1", "type": "text", "text": ""}})),
            ("message.part.delta", json!({"sessionID": "s", "messageID": "a1", "partID": "p1", "field": "text", "delta": "Hel"})),
            ("message.part.updated", json!({"sessionID": "s", "part": {"id": "p1", "messageID": "a1", "type": "text", "text": "Hello"}})),
            ("message.part.updated", json!({"sessionID": "s", "part": {"id": "t1", "messageID": "a1", "type": "tool", "callID": "c1", "tool": "bash", "state": {"status": "pending", "input": {}}}})),
            ("message.part.updated", json!({"sessionID": "s", "part": {"id": "t1", "messageID": "a1", "type": "tool", "callID": "c1", "tool": "bash", "state": {"status": "running", "input": {"command": "ls"}}}})),
            ("message.part.updated", json!({"sessionID": "s", "part": {"id": "t1", "messageID": "a1", "type": "tool", "callID": "c1", "tool": "bash", "state": {"status": "completed", "input": {"command": "ls"}, "output": "a.txt"}}})),
            ("message.part.updated", json!({"sessionID": "s", "part": {"id": "t1", "messageID": "a1", "type": "tool", "callID": "c1", "tool": "bash", "state": {"status": "completed", "input": {"command": "ls"}, "output": "a.txt"}}})),
            ("session.status", json!({"sessionID": "s", "status": {"type": "idle"}})),
        ]);
        // Then the user's message was admitted, the text came once ("Hel" then the rest), the tool started with its input and finished once, and the session ended
        assert_eq!(steps, vec![
            Step::Admitted,
            Step::Event(ExecEvent::Text("Hel".into())),
            Step::Event(ExecEvent::Text("lo".into())),
            Step::Event(ExecEvent::ToolStarted { id: "c1".into(), name: "bash".into(), input: json!({"command": "ls"}) }),
            Step::Event(ExecEvent::ToolFinished { id: "c1".into(), output: "a.txt".into(), ok: true }),
            Step::Idle,
        ]);
    }

    #[test]
    fn given_a_failed_aborted_or_asking_session_when_translated_then_a_failure_is_an_error_an_abort_is_none_and_requests_are_events_to_answer() {
        // Given a session that fails, one that is aborted, and a permission and a question request
        let (mut failed, mut aborted, mut asks) = (Translator::default(), Translator::default(), Translator::default());
        let failure = feed(&mut failed, &[("session.error", json!({"sessionID": "s", "error": {"name": "APIError", "data": {"message": "rate limited"}}})), ("session.idle", json!({"sessionID": "s"}))]);
        let abort = feed(&mut aborted, &[("session.error", json!({"sessionID": "s", "error": {"name": "MessageAbortedError", "data": {"message": "aborted"}}})), ("session.idle", json!({"sessionID": "s"}))]);
        let asked = feed(&mut asks, &[
            ("permission.asked", json!({"id": "per_1", "sessionID": "s", "permission": "bash", "patterns": ["git status"], "metadata": {}, "always": ["bash"]})),
            ("question.asked", json!({"id": "que_1", "sessionID": "s", "questions": [{"question": "Which?", "header": "h", "options": [{"label": "a", "description": ""}, {"label": "b", "description": ""}]}]})),
            ("permission.replied", json!({"sessionID": "s", "requestID": "per_1", "reply": "once"})),
        ]);
        // Then the failure is an error before the end, the abort only the end, the requests are asks the person answers, and the reply settles one
        assert_eq!(failure, vec![Step::Event(ExecEvent::Error("rate limited".into())), Step::Idle]);
        assert_eq!(abort, vec![Step::Idle]);
        assert_eq!(asked, vec![
            Step::Asked { id: "per_1".into(), ask: Ask::Permission, event: ExecEvent::Approval { id: "per_1".into(), tool: "bash".into(), args: json!({"patterns": ["git status"], "metadata": {}}) } },
            Step::Asked { id: "que_1".into(), ask: Ask::Question(1), event: ExecEvent::Input { id: "que_1".into(), prompt: "Which?".into(), options: vec!["a".into(), "b".into()] } },
            Step::Settled("per_1".into()),
        ]);
        assert_eq!(reply_body(Verdict::AcceptForSession), json!({"reply": "always"}));
        assert_eq!(failure_text(), "404: Session not found: ses_x");
    }

    fn failure_text() -> String {
        failure(404, r#"{"name":"NotFoundError","data":{"message":"Session not found: ses_x"}}"#)
    }
}
