//! The commands whose work is the agent API's: each is asked of it first, in T3's words of refusal, and only an accepted one is recorded on the
//! thread's rows. A command that a person's device keeps (pins, visits, modes, the order of the lists) is also kept in the agent API's `state`.
use super::service::Service;
use crate::domain::command::Command;
use crate::ports::{AgentError, AgentThread, Op};
use serde_json::{json, Value};

/// The thread commands this module answers; every other type is decided by the engine alone.
pub const FORWARDED: [&str; 31] = [
    "thread.archive",
    "thread.unarchive",
    "thread.delete",
    "thread.settle",
    "thread.unsettle",
    "thread.snooze",
    "thread.unsnooze",
    "thread.fork",
    "thread.merge_back",
    "subagent.stop",
    "prepared-run.retry",
    "provider-session.detach",
    "queue.resume",
    "queued-run.start",
    "queued-run.reorder",
    "queued-run.cancel",
    "queued-run.edit",
    "queued-message.promote-to-steer",
    "runtime-request.respond",
    "thread.user-input.dismiss",
    "checkpoint.rollback",
    "thread.pin",
    "thread.unpin",
    "thread.pin.reorder",
    "thread.active.reorder",
    "thread.auto-settle.set",
    "thread.visit",
    "thread.mark-unread",
    "thread.pull-request.link",
    "thread.pull-request.unlink",
    "thread.pull-request.watch",
];

/// Commands kept in `state` as well, by the key of the value that changed.
fn state_key(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "thread.pin" | "thread.unpin" | "thread.pin.reorder" => "thread.pins",
        "thread.active.reorder" => "thread.activeOrder",
        "thread.auto-settle.set" => "thread.autoSettle",
        "thread.visit" | "thread.mark-unread" => "thread.visits",
        "thread.metadata.update" | "thread.runtime-mode.set" | "thread.interaction-mode.set" | "thread.model-selection.set" => "thread.settings",
        _ => return None,
    })
}

/// What the agent API answered for a command.
#[derive(Debug, PartialEq)]
pub enum Forwarded {
    /// Accepted: record it on the rows.
    Apply,
    /// Accepted, and nothing is left to record (`thread.delete` of a thread that had been used and so stays).
    Kept,
}

pub fn is_forwarded(kind: &str) -> bool {
    FORWARDED.contains(&kind) || state_key(kind).is_some()
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

impl Service {
    pub(crate) fn thread_for_agent(&self, id: &str) -> Option<AgentThread> {
        self.engine().read(|state| {
            let shell = state.threads.get(id)?;
            Some(AgentThread { thread_id: id.to_string(), instance_id: text(&Value::Object(shell.clone()), "providerInstanceId").unwrap_or_default(), model: shell["modelSelection"]["model"].as_str().map(str::to_string) })
        })
    }

    /// A thread t3 has made exists in the agent API from then on, so the other devices that read it see it before its first turn. A failure is
    /// logged and left: the thread is made again by whatever asks of it first.
    pub(super) fn thread_created(&self, cmd: &Command) {
        let Some(id) = cmd.str("threadId").filter(|_| self.agent().configured()) else { return };
        if let Err(message) = self.ensure(id) {
            self.log("thread.create.agent.failed", &[("thread", id.to_string()), ("error", message)]);
        }
    }

    /// The thread exists in the agent API before a request about it: a thread t3 made that never ran there is made now.
    fn ensure(&self, id: &str) -> Result<(), String> {
        let thread = self.thread_for_agent(id).ok_or_else(|| format!("thread {id} does not exist"))?;
        self.agent().ensure_thread(&thread).map(drop).map_err(|e| e.message())
    }

    fn turn_of(&self, run: &str) -> Option<String> {
        self.turn_of_run(run)
    }

    fn ask(&self, op: &Op) -> Result<Value, String> {
        self.agent().run(op).map_err(|e| match e {
            AgentError::NoThread => "the agent API has no such thread".to_string(),
            AgentError::Failed(message) | AgentError::Refused { message, .. } => message,
        })
    }

    /// The requests a command stands for, in order, or the refusal T3 itself gives without asking.
    fn ops_for(&self, cmd: &Command) -> Result<Vec<Op>, String> {
        let thread = cmd.str("threadId").unwrap_or_default().to_string();
        let item = |key: &str| cmd.str(key).map(str::to_string);
        let run = |key: &str| item(key).ok_or_else(|| format!("{} needs {key}", cmd.kind));
        Ok(match cmd.kind.as_str() {
            "thread.archive" => vec![Op::ThreadArchive { thread, archived: true }],
            "thread.unarchive" => vec![Op::ThreadArchive { thread, archived: false }],
            "thread.delete" => vec![Op::ThreadDelete { thread, only_if_unused: cmd.body.get("onlyIfUnused").and_then(Value::as_bool).unwrap_or(false) }],
            "thread.settle" => vec![Op::ThreadSettle { thread, state: "settled", until_ms: None, at_ms: item("settledAt").and_then(|t| t3_contracts::time::parse_iso(&t)).and_then(|ms| u64::try_from(ms).ok()) }],
            "thread.unsettle" => vec![Op::ThreadSettle { thread, state: "active", until_ms: None, at_ms: None }],
            "thread.snooze" => {
                let until = item("snoozedUntil").and_then(|t| t3_contracts::time::parse_iso(&t)).and_then(|ms| u64::try_from(ms).ok()).ok_or("thread.snooze needs snoozedUntil")?;
                vec![Op::ThreadSettle { thread, state: "snoozed", until_ms: Some(until), at_ms: None }]
            }
            "thread.unsnooze" => vec![Op::ThreadSettle { thread, state: "awake", until_ms: None, at_ms: None }],
            "thread.fork" => vec![Op::ThreadFork { thread: run("sourceThreadId")?, fork: run("targetThreadId")?, turn: None, title: item("title") }],
            "thread.merge_back" => vec![Op::ThreadMergeBack { thread: run("sourceThreadId")?, into: run("targetThreadId")?, turn: None }],
            "subagent.stop" => {
                let run_id = run("runId")?;
                vec![Op::WorkflowStop { thread, turn: self.turn_of(&run_id).unwrap_or(run_id), workflow: item("subagentId") }]
            }
            "prepared-run.retry" => return Err(format!("Run {} has no failed workspace preparation to retry.", run("runId")?)),
            "provider-session.detach" => vec![Op::ThreadDetach { thread, session: run("providerSessionId")? }],
            "queue.resume" => vec![Op::QueueResume { thread, item: None }],
            "queued-run.start" => vec![Op::QueueResume { thread, item: Some(run("runId")?) }],
            "queued-run.reorder" => vec![Op::QueueMove { thread, item: run("runId")?, before: cmd.str("beforeRunId").map(str::to_string) }],
            "queued-run.cancel" => vec![Op::QueueDelete { thread, item: run("runId")? }],
            "queued-run.edit" => vec![Op::QueueEdit { thread, item: run("runId")?, prompt: cmd.body.get("text").and_then(Value::as_str).unwrap_or_default().to_string() }],
            "queued-message.promote-to-steer" => {
                let target = run("targetRunId")?;
                vec![Op::TurnSteer { thread, turn: Some(self.turn_of(&target).unwrap_or(target)), item: Some(run("queuedRunId")?) }]
            }
            "runtime-request.respond" => {
                let request = run("requestId")?;
                match (cmd.str("decision"), cmd.body.get("answers").filter(|a| a.is_object())) {
                    (Some(decision), _) => vec![Op::ApprovalAnswer { thread, approval: request, decision: snake(decision) }],
                    (None, Some(answers)) => vec![Op::InputAnswer { thread, request, answer: answer_text(answers) }],
                    (None, None) => return Err("runtime-request.respond needs a decision or answers".into()),
                }
            }
            "thread.user-input.dismiss" => vec![Op::InputDismiss { thread, request: run("requestId")? }],
            "checkpoint.rollback" => {
                let checkpoint = run("checkpointId")?;
                vec![Op::ThreadRollback { thread, turn: self.turn_of(&checkpoint).unwrap_or(checkpoint), restore_files: cmd.body.get("restoreFiles").and_then(Value::as_bool).unwrap_or(true) }]
            }
            _ => vec![],
        })
    }

    /// Asks the agent API what a command needs; an `Err` is the refusal T3 answers with.
    pub fn forward(&self, cmd: &Command) -> Result<Forwarded, String> {
        let primary = cmd.str("threadId").or_else(|| cmd.str("sourceThreadId")).unwrap_or_default();
        if !self.engine().thread_exists(primary) {
            return Err(format!("thread {primary} does not exist"));
        }
        if super::pr::PULL_REQUEST_COMMANDS.contains(&cmd.kind.as_str()) {
            return self.pull_request_command(cmd).map(|()| Forwarded::Apply);
        }
        let ops = self.ops_for(cmd)?;
        if ops.is_empty() {
            return Ok(Forwarded::Apply);
        }
        if cmd.kind == "thread.fork" && cmd.str("targetThreadId").is_some_and(|t| self.engine().thread_exists(t)) {
            return Err(format!("thread {} already exists", cmd.str("targetThreadId").unwrap_or_default()));
        }
        self.ensure(primary)?;
        if let Some(target) = cmd.str("targetThreadId").filter(|_| cmd.kind == "thread.merge_back") {
            self.ensure(target)?;
        }
        let mut kept = false;
        for op in &ops {
            let answer = self.ask(op)?;
            kept |= matches!(op, Op::ThreadDelete { .. }) && answer["deleted"] == json!(false);
        }
        Ok(if kept { Forwarded::Kept } else { Forwarded::Apply })
    }

    /// Keeps what a device command changed in the agent API's `state`; a failure is logged, since the thread's rows already say it.
    pub fn mirror(&self, cmd: &Command) {
        let (Some(key), Some(thread)) = (state_key(&cmd.kind), cmd.str("threadId")) else { return };
        let value = self.engine().read(|state| {
            let shell = state.threads.get(thread)?;
            let pick = |keys: &[&str]| Value::Object(keys.iter().map(|k| (k.to_string(), shell.get(*k).cloned().unwrap_or(Value::Null))).collect());
            Some(match key {
                "thread.pins" => pick(&["pinnedAt", "pinOrderKey"]),
                "thread.autoSettle" => pick(&["autoSettleDisabledAt"]),
                "thread.visits" => pick(&["lastVisitedAt"]),
                "thread.settings" => pick(&["title", "runtimeMode", "interactionMode", "modelSelection", "providerInstanceId"]),
                _ => json!({"orderKey": cmd.body.get("orderKey")}),
            })
        });
        let Some(value) = value else { return };
        if let Err(e) = self.agent().run(&Op::StateSet { scope: "t3".into(), key: format!("{key}:{thread}"), value }) {
            self.log("state.mirror.failed", &[("key", key.to_string()), ("thread", thread.to_string()), ("error", e.message())]);
        }
    }
}

/// T3's `acceptForSession` is the contract's `accept_for_session`.
fn snake(decision: &str) -> String {
    decision.chars().fold(String::new(), |mut out, c| {
        if c.is_ascii_uppercase() {
            out.push('_');
        }
        out.push(c.to_ascii_lowercase());
        out
    })
}

/// The answers of a question, as the one text the agent API takes: each answer's text on its own line, in the order of the question ids.
fn answer_text(answers: &Value) -> String {
    let mut ids: Vec<&String> = answers.as_object().map(|o| o.keys().collect()).unwrap_or_default();
    ids.sort();
    ids.into_iter()
        .map(|id| match &answers[id.as_str()] {
            Value::String(s) => s.clone(),
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}
