//! The scripted `claude` as a live process (`-p --input-format stream-json`): the frames it prints around a script's lines. The script decides what a turn
//! says; this module only makes the frames the real CLI adds to it (the replay of a message, the init, the result that names the messages it consumed).
use serde_json::{json, Value};

/// The value after `name` in `args`.
pub fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|at| args.get(at + 1)).map(String::as_str)
}

/// Whether the process was started to be spoken to over stream-json, as the service starts it.
pub fn is_live(args: &[String]) -> bool {
    flag(args, "--input-format") == Some("stream-json")
}

pub fn init(session: &str) -> String {
    json!({"type": "system", "subtype": "init", "session_id": session, "model": "scripted", "permissionMode": "default"}).to_string()
}

/// The echo of a message the process was sent (`--replay-user-messages`).
pub fn replay(message: &Value) -> String {
    let mut echo = message.clone();
    echo["isReplay"] = json!(true);
    echo.to_string()
}

pub fn control_success(request_id: &Value, body: Value) -> String {
    json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": body}}).to_string()
}

/// The result frame of a turn that consumed `messages`: successful, or the aborted one an interrupt or a steer causes.
pub fn result(messages: &[String], aborted: bool) -> String {
    json!({
        "type": "result",
        "subtype": if aborted { "error_during_execution" } else { "success" },
        "is_error": aborted,
        "terminal_reason": if aborted { "aborted_streaming" } else { "completed" },
        "result": if aborted { Value::Null } else { json!("") },
        "num_turns": 1,
        "user_message_uuids": messages,
    })
    .to_string()
}

/// A line of a script that is a result frame, with the messages the turn consumed added to it when it names none.
pub fn name_messages(line: &str, messages: &[String]) -> Option<String> {
    let mut frame: Value = serde_json::from_str(line).ok().filter(|v: &Value| v["type"] == "result")?;
    if frame.get("user_message_uuids").is_none() {
        frame["user_message_uuids"] = json!(messages);
    }
    Some(frame.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_result_line_of_a_recording_when_the_turn_consumed_messages_then_they_are_named_once_and_other_lines_are_not_results() {
        let line = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#;
        let named = name_messages(line, &["u1".to_string()]).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&named).unwrap()["user_message_uuids"], json!(["u1"]));
        let own = r#"{"type":"result","user_message_uuids":["x"]}"#;
        assert_eq!(serde_json::from_str::<Value>(&name_messages(own, &["u1".to_string()]).unwrap()).unwrap()["user_message_uuids"], json!(["x"]));
        assert_eq!((name_messages(r#"{"type":"assistant"}"#, &[]), name_messages("not json", &[])), (None, None));
    }

    #[test]
    fn given_the_arguments_of_a_start_when_read_then_a_stream_json_start_is_live_and_a_flag_has_its_value() {
        let args: Vec<String> = ["-p", "--input-format", "stream-json", "--resume", "c-1"].iter().map(|s| s.to_string()).collect();
        assert!(is_live(&args) && !is_live(&["-p".to_string(), "hi".to_string()]));
        assert_eq!((flag(&args, "--resume"), flag(&args, "--session-id")), (Some("c-1"), None));
    }
}
