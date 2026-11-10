//! The checkpoint of a workspace's files that each turn starts from, and the restore a rollback asks of it. The executor keeps it (`git_checkpoint`
//! writes a ref in the workspace's repository), so a service restart loses none; this module only names it and asks.
use crate::refusal::Refusal;
use agent_api_agent::lifecycle as api;
use agent_api_proto::Event;
use exec_proto::Code;
use rca_exec_channel::remote::ExecutorChannel;
use rca_exec_channel::Executors;
use serde_json::{json, Value};
use std::sync::Arc;

/// The args aimed at the workspace's worktree; none when git cannot act on the workspace (a directory inside a repository).
fn aimed(executors: &dyn Executors, workspace: &str, mut args: Value) -> Option<Value> {
    let place = executors.place(workspace);
    match place.worktree {
        Some(worktree) => args["worktree"] = json!(worktree),
        None if place.prefix.is_some() => return None,
        None => {}
    }
    Some(args)
}

/// Records the workspace's files before the turn's CLI runs. A workspace that is no repository, or an executor without the action, has no
/// checkpoint and the turn runs all the same: only a rollback that restores files is the worse for it.
pub async fn take(executors: &dyn Executors, channel: &Arc<dyn ExecutorChannel>, workspace: &str, thread: &str, turn: &str) {
    let Some(args) = aimed(executors, workspace, json!({"thread": thread, "turn": turn})) else {
        tracing::info!(event = "agent_threads.checkpoint_skipped", thread = %thread, turn = %turn, reason = "workspace is a directory inside a repository");
        return;
    };
    if let Err(why) = channel.call(exec_git::GIT_CHECKPOINT, args).await {
        tracing::info!(event = "agent_threads.checkpoint_skipped", thread = %thread, turn = %turn, reason = %why.message());
    }
}

/// Puts the workspace's files back to the checkpoint taken as `turn` started: tracked files restored, ones the turn added removed.
pub async fn restore(executors: &dyn Executors, workspace: &str, thread: &str, turn: &str) -> Result<(), Refusal> {
    let missing = || Refusal::new(api::code::NO_CHECKPOINT, api::message::no_checkpoint(thread, turn));
    let args = json!({"paths": ["."], "scope": "all", "untracked": true, "source": exec_git::checkpoint_ref(thread, turn)});
    let Some(args) = aimed(executors, workspace, args) else { return Err(missing()) };
    let channel = executors.get(workspace).ok_or_else(|| Refusal::new(Code::NoExecutor.as_str(), format!("no executor is attached for workspace {workspace}")))?;
    channel.call(exec_git::GIT_DISCARD, args).await.map(drop).map_err(|why| if why.code() == Code::NotFound.as_str() { missing() } else { Refusal::new(why.code(), why.message()) })
}

/// The turns of a thread that a rollback has not taken out, oldest first.
pub fn live_turns(events: &[Event]) -> Vec<String> {
    let mut turns: Vec<String> = Vec::new();
    for event in events {
        match (event.kind.as_str(), &event.turn) {
            ("turn.started", Some(turn)) if !turns.contains(turn) => turns.push(turn.clone()),
            ("thread.rolled_back", _) => {
                let dropped: Vec<&str> = event.data.get("dropped").and_then(Value::as_array).map(|all| all.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                turns.retain(|t| !dropped.contains(&t.as_str()));
            }
            _ => {}
        }
    }
    turns
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str, turn: Option<&str>, data: Value) -> Event {
        Event { seq: 0, thread: Some("t".into()), turn: turn.map(str::to_string), kind: kind.into(), data: data.as_object().cloned().unwrap_or_default() }
    }

    #[test]
    fn given_turns_and_a_rollback_in_the_log_when_the_live_ones_are_read_then_the_dropped_are_gone_and_a_turn_that_ran_again_counts_once() {
        // Given three turns, a rollback that dropped the last two, and a new turn after it
        let log = [
            event("turn.started", Some("u1"), json!({})),
            event("message.delta", Some("u1"), json!({})),
            event("turn.started", Some("u2"), json!({})),
            event("turn.started", Some("u3"), json!({})),
            event("thread.rolled_back", None, json!({"to_turn": "u1", "dropped": ["u2", "u3"]})),
            event("turn.started", Some("u4"), json!({})),
            event("turn.started", Some("u4"), json!({})),
        ];
        // When the live turns are read
        let live = live_turns(&log);
        // Then the first and the newest remain, in order
        assert_eq!(live, ["u1", "u4"]);
    }
}
