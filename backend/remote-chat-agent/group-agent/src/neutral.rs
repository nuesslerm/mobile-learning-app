//! What a CLI's turn becomes in the thread's log: the neutral events of `rca-cli-driver` (what `claude`, `codex` and `opencode` print, in one
//! vocabulary) as the agent API's thirteen. Pure: the turn's task records what these return.
use agent_api_agent::{FileOp, Role, ThreadEvent};
use rca_cli_driver::domain::ExecEvent;
use serde_json::{json, Value};
use std::collections::HashMap;

/// The executor actions a model's call comes through under each CLI's name for the tool: Claude Code's `mcp__executor__<action>` and opencode's
/// `executor_<action>`.
const TOOL_PREFIXES: [&str; 2] = ["mcp__executor__", "executor_"];

/// The executor action a tool call names, whatever the CLI called it.
pub fn action_of(tool: &str) -> &str {
    TOOL_PREFIXES.iter().find_map(|prefix| tool.strip_prefix(prefix)).unwrap_or(tool)
}

/// The kind of change a successful executor action makes to a file, if it makes one.
fn file_op(action: &str) -> Option<FileOp> {
    match action {
        "write_file" => Some(FileOp::Write),
        "edit_file" => Some(FileOp::Edit),
        "create_file" => Some(FileOp::Create),
        _ => None,
    }
}

/// The code an `error` event gets when the CLI reported the failure itself.
pub const CLI_ERROR: &str = "CLI_ERROR";
/// The code of the error that ends a turn whose CLI could not be started.
pub const CLI_START_FAILED: &str = "CLI_START_FAILED";
/// The code of the error that ends a turn whose executor went away.
pub const EXECUTOR_GONE: &str = "EXECUTOR_GONE";

/// The calls a turn has started and not finished: what a call's finish needs of its start (the tool it was and the path it was given).
#[derive(Default)]
pub struct Calls(HashMap<String, (String, Value)>);

impl Calls {
    /// The events one thing the CLI printed stands for, in the order they happened. `Done` and what ends a turn are the turn's own.
    pub fn events(&mut self, event: ExecEvent) -> Vec<ThreadEvent> {
        match event {
            ExecEvent::Text(text) => vec![ThreadEvent::MessageDelta { role: Role::Assistant, text }],
            ExecEvent::Reasoning(text) => vec![ThreadEvent::ReasoningDelta { text }],
            ExecEvent::ToolStarted { id, name, input } => {
                let tool = action_of(&name).to_string();
                self.0.insert(id.clone(), (tool.clone(), input.clone()));
                vec![ThreadEvent::ToolStarted { call: id, tool, args: input }]
            }
            ExecEvent::ToolFinished { id, output, ok } => {
                let started = self.0.remove(&id);
                let mut events = Vec::new();
                if !output.is_empty() {
                    events.push(ThreadEvent::ToolOutput { call: id.clone(), stream: agent_api_agent::OutputStream::Stdout, data: output });
                }
                events.push(ThreadEvent::ToolFinished { call: id, ok, result: json!({}) });
                if let (true, Some((tool, args))) = (ok, started) {
                    if let (Some(op), Some(path)) = (file_op(&tool), args["path"].as_str()) {
                        events.push(ThreadEvent::FileChanged { path: path.to_string(), op });
                    }
                }
                events
            }
            ExecEvent::Error(message) => vec![ThreadEvent::Error { code: CLI_ERROR.into(), message }],
            ExecEvent::Done | ExecEvent::Session(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_api_agent::OutputStream;

    fn kinds(events: &[ThreadEvent]) -> Vec<&'static str> {
        events.iter().map(ThreadEvent::kind).collect()
    }

    #[test]
    fn given_the_names_each_cli_gives_a_tool_when_read_then_they_are_the_executor_action_and_a_name_no_cli_prefixes_is_kept() {
        // Given the names Claude Code and opencode give the executor's write_file, and tools of no executor
        // Then both are write_file, and the others are what they were
        assert_eq!((action_of("mcp__executor__write_file"), action_of("executor_write_file")), ("write_file", "write_file"));
        assert_eq!((action_of("Bash"), action_of("command"), action_of("")), ("Bash", "command", ""));
    }

    #[test]
    fn given_a_turn_that_writes_a_file_and_runs_a_command_when_translated_then_each_call_is_its_start_output_and_finish_and_only_the_successful_write_is_a_file_change() {
        // Given a turn: some thinking, a write of notes/a.txt that works, a command that fails with output, then text
        let mut calls = Calls::default();
        let turn = [
            ExecEvent::Reasoning("look".into()),
            ExecEvent::ToolStarted { id: "k1".into(), name: "mcp__executor__write_file".into(), input: json!({"path": "notes/a.txt", "content": "x"}) },
            ExecEvent::ToolFinished { id: "k1".into(), output: "wrote 1 bytes".into(), ok: true },
            ExecEvent::ToolStarted { id: "k2".into(), name: "mcp__executor__run_command".into(), input: json!({"command": "exit 3"}) },
            ExecEvent::ToolFinished { id: "k2".into(), output: "[exit code 3]".into(), ok: false },
            ExecEvent::Text("done".into()),
        ];
        // When each is translated
        let events: Vec<ThreadEvent> = turn.into_iter().flat_map(|e| calls.events(e)).collect();
        // Then the write gives a start, its output, its finish and the file it changed, the failed command no file change, then the text
        assert_eq!(kinds(&events), ["reasoning.delta", "tool.started", "tool.output", "tool.finished", "file.changed", "tool.started", "tool.output", "tool.finished", "message.delta"]);
        assert_eq!(events[1], ThreadEvent::ToolStarted { call: "k1".into(), tool: "write_file".into(), args: json!({"path": "notes/a.txt", "content": "x"}) });
        assert_eq!(events[2], ThreadEvent::ToolOutput { call: "k1".into(), stream: OutputStream::Stdout, data: "wrote 1 bytes".into() });
        assert_eq!(events[4], ThreadEvent::FileChanged { path: "notes/a.txt".into(), op: FileOp::Write });
        assert_eq!(events[7], ThreadEvent::ToolFinished { call: "k2".into(), ok: false, result: json!({}) });
    }

    #[test]
    fn given_a_failed_write_a_finish_with_no_start_and_no_output_when_translated_then_no_file_change_is_claimed_and_an_empty_output_has_no_event() {
        // Given a write that failed, a finish of a call that never started, and a finish with no output
        let mut calls = Calls::default();
        calls.events(ExecEvent::ToolStarted { id: "k1".into(), name: "executor_write_file".into(), input: json!({"path": "a"}) });
        let failed = calls.events(ExecEvent::ToolFinished { id: "k1".into(), output: "refused".into(), ok: false });
        let orphan = calls.events(ExecEvent::ToolFinished { id: "never".into(), output: String::new(), ok: true });
        // Then neither changed a file, and the orphan is only its finish
        assert_eq!(kinds(&failed), ["tool.output", "tool.finished"]);
        assert_eq!(kinds(&orphan), ["tool.finished"]);
    }

    #[test]
    fn given_an_edit_a_create_and_the_cli_s_own_error_when_translated_then_they_are_the_matching_file_ops_and_an_error_with_the_cli_s_message() {
        let mut calls = Calls::default();
        let mut run = |name: &str, path: &str| {
            calls.events(ExecEvent::ToolStarted { id: name.into(), name: name.into(), input: json!({"path": path}) });
            calls.events(ExecEvent::ToolFinished { id: name.into(), output: String::new(), ok: true })
        };
        assert_eq!(run("edit_file", "e.rs").last(), Some(&ThreadEvent::FileChanged { path: "e.rs".into(), op: FileOp::Edit }));
        assert_eq!(run("create_file", "c.rs").last(), Some(&ThreadEvent::FileChanged { path: "c.rs".into(), op: FileOp::Create }));
        assert_eq!(calls.events(ExecEvent::Error("rate limited".into())), vec![ThreadEvent::Error { code: "CLI_ERROR".into(), message: "rate limited".into() }]);
        assert!(calls.events(ExecEvent::Done).is_empty());
    }
}
