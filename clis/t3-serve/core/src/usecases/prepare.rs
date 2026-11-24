//! The workspace a run waits for, prepared: the launch that asks for a worktree leaves its run `preparing`, and this makes the worktree (the agent API's
//! `worktree.create`, which starts the executor's `worktree_setup`), follows the setup, records the worktree on the thread and releases the run, or
//! fails it with the reason. Each step reports as the pin's `ThreadLaunchService` does: `prepared-run.progress`, `release` and `fail`, which the engine
//! decides like any command. A retry (`prepared-run.retry`) takes the failure back and comes here again.
use super::service::Service;
use super::workspace::{worktree_name, SetupFold};
use crate::domain::command::Family;
use crate::ports::Op;
use serde_json::{json, Value};
use std::time::Duration;

/// How long one look at the setup waits for news, and how many looks at most (a setup script that runs for half an hour is cut off).
const LOOK: Duration = Duration::from_secs(2);
const LOOKS: u32 = 900;

impl Service {
    /// Starts the preparation of the preparing run on a thread of its own: the launch that asked for it answers the projection with the run still
    /// preparing, as the pin's does, and the thread's rows show the rest as it happens.
    pub(super) fn prepare_run(&self, thread: &str, run: &str) {
        let (svc, thread, run) = (self.clone(), thread.to_string(), run.to_string());
        std::thread::spawn(move || svc.prepare_and_report(&thread, &run));
    }

    fn command(&self, thread: &str, run: &str, kind: &str, id: String, extra: Value) -> Result<(), String> {
        let mut body = json!({"type": kind, "commandId": id, "threadId": thread, "runId": run});
        if let (Some(map), Some(more)) = (body.as_object_mut(), extra.as_object()) {
            map.extend(more.clone());
        }
        self.dispatch(Family::Thread, &body).map(drop).map_err(|f| format!("{f:?}"))
    }

    /// A token that is new for each preparation of the run: the engine replays a command id it already accepted, and the failure row's id is fixed per run,
    /// so nothing counted from the rows grows with every retry.
    fn attempt(&self) -> String {
        hex::encode(self.0.ids.random_bytes(6))
    }

    fn prepare_and_report(&self, thread: &str, run: &str) {
        let attempt = self.attempt();
        let outcome = self.prepare_workspace(thread, run, &attempt);
        let reported = match &outcome {
            Ok(()) => self.command(thread, run, "prepared-run.release", format!("{run}:release:{attempt}"), json!({})),
            Err(message) => {
                self.log("run.preparation.failed", &[("thread", thread.to_string()), ("run", run.to_string()), ("message", message.clone())]);
                self.discard_worktree(thread, run, &attempt);
                self.command(thread, run, "prepared-run.fail", format!("{run}:fail:{attempt}"), json!({"failure": {"class": "validation_error", "message": message, "code": null, "retryable": false}}))
            }
        };
        if let Err(message) = reported {
            self.log("run.preparation.unreported", &[("thread", thread.to_string()), ("run", run.to_string()), ("message", message)]);
        }
    }

    fn progress(&self, thread: &str, run: &str, attempt: &str, phase: &str) -> Result<(), String> {
        self.command(thread, run, "prepared-run.progress", format!("{run}:progress:{phase}:{attempt}"), json!({"phase": phase}))
    }

    fn prepare_workspace(&self, thread: &str, run: &str, attempt: &str) -> Result<(), String> {
        let (strategy, shell) = self.engine().read(|state| (state.entities.get(thread).and_then(|e| e.runs.iter().find(|r| r["id"] == run)).map(|r| r["workspacePreparation"].clone()), state.threads.get(thread).cloned()));
        let strategy = strategy.filter(|s| s["type"] == "worktree").ok_or("The run has no worktree to prepare.")?;
        let shell = shell.ok_or("The thread does not exist.")?;
        let project_root = shell["projectId"].as_str().and_then(|p| self.engine().project_root(p)).ok_or("The project no longer exists.")?;
        let recorded = shell["worktreePath"].as_str().filter(|p| !p.is_empty()).map(str::to_string);
        let worktree = match recorded {
            Some(path) => path,
            None => {
                self.progress(thread, run, attempt, "worktree")?;
                let base = strategy["baseRef"].as_str().ok_or("The worktree has no base.")?.to_string();
                let branch = match strategy["branch"].as_str().filter(|b| !b.is_empty()) {
                    Some(branch) => branch.to_string(),
                    None => format!("t3code/{}", hex::encode(self.0.ids.random_bytes(6))),
                };
                let made = self.agent().run(&Op::WorktreeCreate { root: project_root.clone(), worktree: worktree_name(&branch, &project_root), branch: branch.clone(), base: Some(base) }).map_err(|e| e.message())?;
                let path = made["path"].as_str().ok_or("The agent API gave no path for the worktree.")?.to_string();
                let recorded = json!({"type": "thread.metadata.update", "commandId": format!("{run}:workspace:{attempt}"), "threadId": thread, "branch": made["branch"].as_str().unwrap_or(&branch), "worktreePath": path});
                self.dispatch(Family::Thread, &recorded).map_err(|f| format!("The worktree could not be recorded on the thread: {f:?}"))?;
                path
            }
        };
        self.progress(thread, run, attempt, "setup")?;
        self.await_setup(thread, &worktree)
    }

    /// Follows the setup the worktree started until it ends; a worktree whose setup never ran is ready.
    fn await_setup(&self, thread: &str, worktree: &str) -> Result<(), String> {
        let mut fold = SetupFold::new(thread, worktree, None, self.now_ms());
        for _ in 0..LOOKS {
            let batch = self.agent().setup_events(worktree, fold.last_seq(), LOOK).map_err(|e| e.message())?;
            if !batch.ran {
                return Ok(());
            }
            fold.take(&batch, self.now_ms());
            if let Some(result) = fold.result() {
                return result;
            }
        }
        Err("The setup script did not finish in time.".into())
    }

    /// A worktree whose preparation failed is removed and forgotten by the thread, so a retry makes it again: the setup script runs once, in a
    /// checkout that was made for it.
    fn discard_worktree(&self, thread: &str, run: &str, attempt: &str) {
        let Some(path) = self.engine().read(|state| state.threads.get(thread).and_then(|t| t["worktreePath"].as_str().filter(|p| !p.is_empty()).map(str::to_string))) else { return };
        if let Err(e) = self.agent().run(&Op::WorktreeRemove { root: path.clone() }) {
            self.log("run.preparation.discard.failed", &[("thread", thread.to_string()), ("path", path), ("message", e.message())]);
            return;
        }
        let forgotten = json!({"type": "thread.metadata.update", "commandId": format!("{run}:forget:{attempt}"), "threadId": thread, "branch": Value::Null, "worktreePath": Value::Null});
        if let Err(f) = self.dispatch(Family::Thread, &forgotten) {
            self.log("run.preparation.discard.failed", &[("thread", thread.to_string()), ("path", path), ("message", format!("{f:?}"))]);
        }
    }
}
