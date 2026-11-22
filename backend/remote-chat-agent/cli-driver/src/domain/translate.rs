//! The neutral events a client shows, whatever CLI makes them: each live session turns its CLI's own protocol into these (`domain::claude`, `domain::codex`,
//! `domain::opencode`).
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum ExecEvent {
    Text(String),
    Reasoning(String),
    ToolStarted { id: String, name: String, input: Value },
    ToolFinished { id: String, output: String, ok: bool },
    Done,
    Error(String),
    /// The CLI's own id of the conversation the turn runs in, which a live session says when it starts a thread.
    Session(String),
    /// The CLI asks the person whether `tool` may run with `args`, and waits: `id` is what the answer names. Only a live session makes it.
    Approval { id: String, tool: String, args: Value },
    /// The CLI asks the person a question (Claude Code's `AskUserQuestion`) and waits: `id` is what the answer names. Only a live session makes it.
    Input { id: String, prompt: String, options: Vec<String> },
}
