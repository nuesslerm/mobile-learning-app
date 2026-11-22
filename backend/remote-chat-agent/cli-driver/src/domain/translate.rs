//! What a CLI's output lines mean: each driver's JSON lines become the neutral events a client shows.
use super::driver::Driver;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum ExecEvent {
    Text(String),
    Reasoning(String),
    ToolStarted { id: String, name: String, input: Value },
    ToolFinished { id: String, output: String, ok: bool },
    Done,
    Error(String),
    /// The CLI's own id of the conversation this process is part of. `translate` never makes it: `Turns` sends it from `session_of`, and a live
    /// session when it starts a thread.
    Session(String),
    /// The CLI asks the person whether `tool` may run with `args`, and waits: `id` is what the answer names. Only a live session makes it.
    Approval { id: String, tool: String, args: Value },
}

/// The id of the CLI's conversation a line announces: Claude Code's `system` `init` line names it, and every line of `opencode run --format json`
/// carries it as `sessionID`. None for every other line, and for Codex, whose server says it in the answer to `thread/start`.
pub fn session_of(driver: Driver, line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    let id = match driver {
        Driver::ClaudeAgent if v["type"] == "system" && v["subtype"] == "init" => &v["session_id"],
        Driver::Opencode => &v["sessionID"],
        _ => return None,
    };
    id.as_str().filter(|id| !id.is_empty()).map(str::to_string)
}

fn s(v: &Value) -> String {
    match v {
        Value::String(t) => t.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The neutral events one output line stands for (none for a line that is not JSON or says nothing a client shows, and for Codex, whose
/// server is read by `domain::codex`).
pub fn translate(driver: Driver, line: &str) -> Vec<ExecEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    match driver {
        Driver::ClaudeAgent => claude(&v),
        Driver::Codex => vec![],
        Driver::Opencode => opencode(&v),
    }
}

fn claude(v: &Value) -> Vec<ExecEvent> {
    let blocks = v["message"]["content"].as_array().cloned().unwrap_or_default();
    match v["type"].as_str() {
        Some("assistant") => blocks
            .iter()
            .filter_map(|b| match b["type"].as_str()? {
                "text" => Some(ExecEvent::Text(s(&b["text"]))),
                "thinking" => Some(ExecEvent::Reasoning(s(&b["thinking"]))),
                "tool_use" => Some(ExecEvent::ToolStarted { id: s(&b["id"]), name: s(&b["name"]), input: b["input"].clone() }),
                _ => None,
            })
            .collect(),
        Some("user") => blocks.iter().filter(|b| b["type"] == "tool_result").map(|b| ExecEvent::ToolFinished { id: s(&b["tool_use_id"]), output: s(&b["content"]), ok: b["is_error"] != true }).collect(),
        Some("result") if v["is_error"] == true => vec![ExecEvent::Error(s(&v["result"]))],
        _ => vec![],
    }
}

fn opencode(v: &Value) -> Vec<ExecEvent> {
    let part = &v["part"];
    match v["type"].as_str() {
        Some("text") => vec![ExecEvent::Text(s(&part["text"]))],
        Some("tool_use") => {
            let (id, state) = (s(&part["callID"]), &part["state"]);
            let started = ExecEvent::ToolStarted { id: id.clone(), name: s(&part["tool"]), input: state["input"].clone() };
            match state["status"].as_str() {
                Some("completed") => vec![started, ExecEvent::ToolFinished { id, output: s(&state["output"]), ok: true }],
                Some("error") => vec![started, ExecEvent::ToolFinished { id, output: s(&state["error"]), ok: false }],
                _ => vec![started],
            }
        }
        Some("error") => {
            let message = s(&v["error"]["data"]["message"]);
            vec![ExecEvent::Error(if message.is_empty() { s(&v["error"]["name"]) } else { message })]
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_lines_become_text_a_tool_start_its_result_and_a_failed_result_an_error() {
        let text = translate(Driver::ClaudeAgent, r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hi"},{"type":"tool_use","id":"k","name":"Bash","input":{"command":"ls"}}]}}"#);
        assert_eq!(text, vec![ExecEvent::Text("Hi".into()), ExecEvent::ToolStarted { id: "k".into(), name: "Bash".into(), input: serde_json::json!({"command": "ls"}) }]);
        let done = translate(Driver::ClaudeAgent, r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"k","content":"a.txt","is_error":false}]}}"#);
        assert_eq!(done, vec![ExecEvent::ToolFinished { id: "k".into(), output: "a.txt".into(), ok: true }]);
        assert_eq!(translate(Driver::ClaudeAgent, r#"{"type":"result","is_error":true,"result":"rate limited"}"#), vec![ExecEvent::Error("rate limited".into())]);
        assert!(translate(Driver::ClaudeAgent, "not json").is_empty());
    }

    #[test]
    fn given_the_lines_of_each_driver_when_asked_for_a_session_then_each_drivers_own_announcing_line_names_one() {
        // Given Claude Code's init line, its other lines and an opencode line that carries its sessionID
        let init = r#"{"type":"system","subtype":"init","session_id":"s-1","cwd":"/w"}"#;
        let result = r#"{"type":"result","is_error":false,"result":"ok","session_id":"s-1"}"#;
        let empty = r#"{"type":"system","subtype":"init","session_id":""}"#;
        // When each is asked for its session
        // Then only the init line of Claude Code names it, an opencode line names its own, and Codex and a line without one name none
        assert_eq!(session_of(Driver::ClaudeAgent, init), Some("s-1".to_string()));
        assert_eq!((session_of(Driver::ClaudeAgent, result), session_of(Driver::ClaudeAgent, empty), session_of(Driver::ClaudeAgent, "not json")), (None, None, None));
        let opencode = r#"{"type":"text","timestamp":1,"sessionID":"ses_a1","part":{"text":"hi"}}"#;
        assert_eq!((session_of(Driver::Opencode, opencode), session_of(Driver::Opencode, init), session_of(Driver::Opencode, r#"{"sessionID":""}"#)), (Some("ses_a1".to_string()), None, None));
        assert_eq!((session_of(Driver::Codex, init), session_of(Driver::Codex, opencode)), (None, None));
        // And translate never turns it into an event
        assert!(translate(Driver::ClaudeAgent, init).is_empty());
    }

    #[test]
    fn opencode_lines_become_tool_events_and_codex_has_no_lines_to_translate() {
        assert!(translate(Driver::Codex, r#"{"type":"item.completed","item":{"type":"agent_message","text":"x"}}"#).is_empty());
        let o = translate(Driver::Opencode, r#"{"type":"tool_use","part":{"callID":"o1","tool":"bash","state":{"status":"completed","input":{"c":1},"output":"done"}}}"#);
        assert_eq!(o.len(), 2);
        assert_eq!(translate(Driver::Opencode, r#"{"type":"text","part":{"text":"yo"}}"#), vec![ExecEvent::Text("yo".into())]);
    }
}
