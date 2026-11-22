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
    /// The CLI asks the person a question (Claude Code's `AskUserQuestion`) and waits: `id` is what the answer names. Only a live session makes it.
    Input { id: String, prompt: String, options: Vec<String> },
}

/// The id of the CLI's conversation a line announces: every line of `opencode run --format json` carries it as `sessionID`. None for every other line,
/// and for the CLIs that live, whose session says it when it starts the conversation.
pub fn session_of(driver: Driver, line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line).ok()?;
    let id = match driver {
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

/// The neutral events one output line stands for (none for a line that is not JSON or says nothing a client shows, and for the CLIs that live, whose
/// processes are read by `domain::claude` and `domain::codex`).
pub fn translate(driver: Driver, line: &str) -> Vec<ExecEvent> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    match driver {
        Driver::ClaudeAgent | Driver::Codex => vec![],
        Driver::Opencode => opencode(&v),
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
    fn given_the_lines_of_each_driver_when_asked_for_a_session_then_only_an_opencode_line_names_one_and_the_drivers_that_live_name_none() {
        // Given Claude Code's init line, its result line and an opencode line that carries its sessionID
        let init = r#"{"type":"system","subtype":"init","session_id":"s-1","cwd":"/w"}"#;
        let opencode = r#"{"type":"text","timestamp":1,"sessionID":"ses_a1","part":{"text":"hi"}}"#;
        // When each is asked for its session
        // Then an opencode line names its own, a line without one names none, and Claude Code and Codex name none: their sessions say it themselves
        assert_eq!((session_of(Driver::Opencode, opencode), session_of(Driver::Opencode, init), session_of(Driver::Opencode, r#"{"sessionID":""}"#)), (Some("ses_a1".to_string()), None, None));
        assert_eq!((session_of(Driver::ClaudeAgent, init), session_of(Driver::Codex, opencode)), (None, None));
        // And translate never turns a line of a driver that lives into an event
        assert!(translate(Driver::ClaudeAgent, init).is_empty() && translate(Driver::ClaudeAgent, r#"{"type":"assistant","message":{"content":[{"type":"text","text":"x"}]}}"#).is_empty());
    }

    #[test]
    fn opencode_lines_become_tool_events_and_codex_has_no_lines_to_translate() {
        assert!(translate(Driver::Codex, r#"{"type":"item.completed","item":{"type":"agent_message","text":"x"}}"#).is_empty());
        let o = translate(Driver::Opencode, r#"{"type":"tool_use","part":{"callID":"o1","tool":"bash","state":{"status":"completed","input":{"c":1},"output":"done"}}}"#);
        assert_eq!(o.len(), 2);
        assert_eq!(translate(Driver::Opencode, r#"{"type":"text","part":{"text":"yo"}}"#), vec![ExecEvent::Text("yo".into())]);
    }
}
