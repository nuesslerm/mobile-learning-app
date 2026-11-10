//! The entity rows the real server writes, as templates recorded from `t3@0.1.6` (data/*.json): the shell row of a thread, the
//! two narrower rows (projection and event payload) cut from it, the project row, and the command types of the decoder's union.
use serde_json::{Map, Value};

const THREAD_SHELL: &str = include_str!("../data/thread-shell.json");
const PROJECT_SHELL: &str = include_str!("../data/project-shell.json");
const PROJECTION_KEYS: &str = include_str!("../data/thread-projection-keys.json");
const EVENT_KEYS: &str = include_str!("../data/thread-event-keys.json");
const COMMAND_UNION_DEFECT: &str = include_str!("../data/command-union-defect.txt");

fn object(text: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(text).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// A thread shell row with every key at its default; the caller sets id, project, title, model and times.
pub fn thread_defaults() -> Map<String, Value> {
    object(THREAD_SHELL)
}

pub fn project_defaults() -> Map<String, Value> {
    object(PROJECT_SHELL)
}

fn cut(shell: &Map<String, Value>, keys: &str) -> Value {
    let keys: Vec<String> = serde_json::from_str(keys).unwrap_or_default();
    Value::Object(keys.into_iter().map(|k| (k.clone(), shell.get(&k).cloned().unwrap_or(Value::Null))).collect())
}

/// `projection.thread` of a thread snapshot.
pub fn thread_projection_row(shell: &Map<String, Value>) -> Value {
    cut(shell, PROJECTION_KEYS)
}

/// The payload of a thread lifecycle event: the row after the change.
pub fn thread_event_row(shell: &Map<String, Value>) -> Value {
    cut(shell, EVENT_KEYS)
}

/// What a command of a type outside the union is refused with (a decoder defect, not a typed error).
pub fn command_union_defect() -> &'static str {
    COMMAND_UNION_DEFECT
}

/// The command types the decoder's union lists, in its order.
pub fn command_types() -> Vec<&'static str> {
    const MARK: &str = "readonly \"type\": \"";
    COMMAND_UNION_DEFECT.split(MARK).skip(1).filter_map(|rest| rest.split('"').next()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_union_lists_the_55_types_the_decoder_knows() {
        let types = command_types();
        assert_eq!(types.len(), 55);
        assert!(types.contains(&"thread.create") && types.contains(&"message.dispatch"));
    }

    #[test]
    fn the_cut_rows_hold_exactly_the_recorded_keys_and_null_for_a_key_the_shell_lacks() {
        let shell = thread_defaults();
        let event = thread_event_row(&shell);
        assert_eq!(event.as_object().unwrap().len(), 26);
        assert_eq!(event["guestShare"], Value::Null);
        assert_eq!(thread_projection_row(&shell).as_object().unwrap().len(), 24);
    }
}
