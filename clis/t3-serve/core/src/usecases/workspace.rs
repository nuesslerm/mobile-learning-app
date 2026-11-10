//! The M6 rows: the files of a project written, searched, browsed and measured, the diffs of a thread, and the worktrees of a project, each asked
//! of the agent API's `files.edit`, `workspace` and `workspace.worktrees` groups. A thread's directory is its worktree when it has one, else its
//! project's root.
use super::queries::{failed_with, field, QueryFailure};
use super::service::Service;
use crate::ports::{AgentError, Op, SetupBatch, SetupEvent};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;
use t3_contracts::time::iso;
use t3_contracts::wire;

/// The most paths one `filesystem.getMetadata` asks about.
const METADATA_BATCH: usize = 64;
/// The most lines of a stage's output a setup snapshot carries, and the most characters of one.
const TAIL_LINES: usize = 10;
const TAIL_WIDTH: usize = 400;
const DETAIL_WIDTH: usize = 200;
const ERROR_WIDTH: usize = 1000;

/// How long one look at a setup waits for it to finish before the snapshot so far is sent.
const SETUP_WAIT: Duration = Duration::from_secs(1);

/// A rest between two looks at a setup that said nothing new, for an agent API that does not wait itself.
const IDLE: Duration = Duration::from_millis(20);

const IMAGE_EXTENSIONS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico"];

fn extension(path: &str) -> Option<String> {
    path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

fn mime_type(path: &str) -> Option<&'static str> {
    Some(match extension(path)?.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "html" => "text/html",
        "md" => "text/markdown",
        "txt" => "text/plain",
        _ => return None,
    })
}

fn clip(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

/// A directory name for a worktree: the branch as one plain segment, made unique to the project by a short hash of its directory.
pub(super) fn worktree_name(branch: &str, project_dir: &str) -> String {
    let plain: String = branch.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '-' }).collect();
    let plain = plain.trim_start_matches(['-', '.']);
    let suffix = hex::encode(&Sha256::digest(project_dir.as_bytes())[..3]);
    format!("{}-{suffix}", clip(plain, 90))
}

impl Service {
    /// The directory a thread's files and diffs are in: its worktree, else its project's root, else the executor's own.
    pub(super) fn thread_dir(&self, thread: &str) -> String {
        self.engine()
            .read(|state| state.threads.get(thread).cloned())
            .map(|row| match row["worktreePath"].as_str().filter(|p| !p.is_empty()) {
                Some(path) => path.to_string(),
                None => row["projectId"].as_str().and_then(|p| self.engine().project_root(p)).unwrap_or_default(),
            })
            .unwrap_or_default()
    }

    /// `projects.writeFile`: the file written under the project's directory, its parents made.
    pub fn write_file(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (root, path) = (field(payload, "cwd")?, field(payload, "relativePath")?);
        let content = payload.get("contents").and_then(Value::as_str).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"contents\"]".into()))?;
        let op = Op::FileWrite { root: root.into(), path: path.into(), content: content.into() };
        self.agent().run(&op).map_err(|e| failed_with("ProjectWriteFileError", &e, json!({"cwd": root, "relativePath": path})))?;
        Ok(json!({"relativePath": path}))
    }

    /// `filesystem.browse`: the directories of the directory the partial path is in whose names start with what follows its last slash.
    pub fn browse(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let partial = field(payload, "partialPath")?;
        let cwd = payload.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty());
        let refuse = |failure: &str, message: String| QueryFailure::Error(json!({"_tag": "FilesystemBrowseError", "partialPath": partial, "cwd": cwd, "failure": failure, "message": message}));
        let bytes = partial.as_bytes();
        if bytes.len() >= 3 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/') {
            return Err(refuse("windows_path_unsupported", format!("Windows paths are not supported: {partial}")));
        }
        let absolute = partial.starts_with('/') || partial.starts_with('~');
        if !absolute && cwd.is_none() {
            return Err(refuse("current_project_required", "A relative path needs the project it is relative to.".into()));
        }
        let (dir, prefix) = match partial.rsplit_once('/') {
            Some(("", prefix)) => ("/", prefix),
            Some((dir, prefix)) => (dir, prefix),
            None => (".", partial),
        };
        let root = if absolute { "" } else { cwd.unwrap_or_default() };
        let listed = self.agent().run(&Op::FileBrowse { root: root.into(), path: dir.into(), show_hidden: prefix.starts_with('.') }).map_err(|e| refuse("read_directory_failed", e.message()))?;
        let wanted = prefix.to_lowercase();
        let entries: Vec<Value> = listed["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["kind"] == "dir")
            .filter_map(|e| Some((e["name"].as_str()?, e["path"].as_str()?)))
            .filter(|(name, _)| name.to_lowercase().starts_with(&wanted))
            .map(|(name, path)| json!({"name": name, "fullPath": path}))
            .collect();
        Ok(json!({"parentPath": listed["path"], "entries": entries}))
    }

    /// `filesystem.getMetadata`: the kind and size of each path, in order, null for one that is missing or cannot be read.
    pub fn get_metadata(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let paths = payload.get("paths").and_then(Value::as_array).filter(|p| !p.is_empty() && p.len() <= METADATA_BATCH).ok_or_else(|| QueryFailure::Defect(format!("Expected between 1 and {METADATA_BATCH} paths at [\"paths\"]")))?;
        let paths: Vec<&str> = paths.iter().map(|p| p.as_str().filter(|p| !p.trim().is_empty()).ok_or_else(|| QueryFailure::Defect("Expected a non-empty path at [\"paths\"]".into()))).collect::<Result<_, _>>()?;
        let entries: Vec<Value> = paths
            .into_iter()
            .map(|path| match self.agent().run(&Op::FileStat { root: String::new(), path: path.into() }) {
                Ok(stat) if stat["kind"] == "file" => {
                    let mut entry = json!({"kind": "file", "byteLength": stat["size"]});
                    if let Some(mime) = mime_type(path) {
                        entry["mimeType"] = json!(mime);
                    }
                    entry
                }
                Ok(stat) => json!({"kind": if stat["kind"] == "dir" { "directory" } else { "other" }}),
                Err(_) => Value::Null,
            })
            .collect();
        Ok(json!({"entries": entries}))
    }

    /// `projects.searchEntries`: the entries whose path contains the query, best match first; with no query, the project's top level.
    pub fn search_entries(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        let query = payload.get("query").and_then(Value::as_str).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"query\"]".into()))?.trim();
        let limit = payload.get("limit").and_then(Value::as_u64).filter(|l| *l > 0).ok_or_else(|| QueryFailure::Defect("Expected a positive integer at [\"limit\"]".into()))? as usize;
        let kind = payload.get("kind").and_then(Value::as_str);
        let images = payload.get("imageOnly").and_then(Value::as_bool).unwrap_or(false);
        let refuse = |e: AgentError| failed_with("ProjectSearchEntriesError", &e, json!({"cwd": root, "queryLength": query.chars().count(), "limit": limit}));
        let (found, truncated): (Vec<(String, bool)>, bool) = if query.is_empty() {
            let top = self.agent().list_dir(root, "").map_err(refuse)?;
            (top.into_iter().map(|e| (e.name, e.directory)).collect(), false)
        } else {
            let result = self.agent().run(&Op::FileSearch { root: root.into(), query: query.into(), limit: u32::try_from(limit).unwrap_or(u32::MAX) }).map_err(refuse)?;
            let rows = result["entries"].as_array().into_iter().flatten().filter_map(|e| Some((e["path"].as_str()?.to_string(), e["kind"] == "dir"))).collect();
            (rows, result["truncated"].as_bool().unwrap_or(false))
        };
        let wanted = |(path, directory): &(String, bool)| {
            let right_kind = match kind {
                Some("directory") => *directory,
                Some("file") => !*directory,
                _ => true,
            };
            right_kind && (!images || (!*directory && extension(path).is_some_and(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))))
        };
        let mut entries: Vec<Value> = found.into_iter().filter(wanted).map(|(path, directory)| json!({"path": path, "kind": if directory { "directory" } else { "file" }})).collect();
        let cut = entries.len() > limit;
        entries.truncate(limit);
        Ok(json!({"entries": entries, "truncated": truncated || cut}))
    }

    /// The turns of a thread that are over: the count its diffs are taken at.
    fn finished_turns(&self, thread: &str) -> u64 {
        self.engine().read(|state| state.entities.get(thread).map_or(0, |e| e.runs.iter().filter(|r| r["status"] != "running").count() as u64))
    }

    /// `orchestration.getTurnDiff` and `orchestration.getFullThreadDiff`: what changed in the thread's directory since the revision it is at.
    /// The agent API keeps no base per turn, so only the diff from the thread's start to its last finished turn is one it can give.
    pub fn thread_diff(&self, payload: &Value, whole: bool) -> Result<Value, QueryFailure> {
        let tag = if whole { "OrchestrationGetFullThreadDiffError" } else { "OrchestrationGetTurnDiffError" };
        let refuse = |message: String| QueryFailure::Error(json!({"_tag": tag, "message": message}));
        let count = |key: &str| payload.get(key).and_then(Value::as_u64).ok_or_else(|| QueryFailure::Defect(format!("Expected a non-negative integer at [\"{key}\"]")));
        let thread = field(payload, "threadId")?;
        let to = count("toTurnCount")?;
        let from = if whole { 0 } else { count("fromTurnCount")? };
        if from > to {
            return Err(QueryFailure::Defect("fromTurnCount must be less than or equal to toTurnCount".into()));
        }
        if self.engine().read(|s| !s.threads.contains_key(thread)) {
            return Err(refuse(format!("Thread {thread} was not found.")));
        }
        if payload.get("ignoreWhitespace").and_then(Value::as_bool) == Some(true) {
            return Err(refuse("The agent API cannot ignore whitespace in a diff.".into()));
        }
        let finished = self.finished_turns(thread);
        if from != 0 || to != finished {
            return Err(refuse(format!("Only the diff of turns 0 to {finished} (the thread's whole work) can be given: no revision is recorded for the start of each turn.")));
        }
        let root = self.thread_dir(thread);
        let op = if whole { Op::ThreadDiff { root } } else { Op::TurnDiff { root, base: "HEAD".into() } };
        let diff = self.agent().run(&op).map_err(|e| refuse(e.message()))?;
        Ok(json!({"threadId": thread, "fromTurnCount": from, "toTurnCount": to, "diff": diff["diff"].as_str().unwrap_or_default()}))
    }

    /// `vcs.createWorktree`: the branch checked out in a worktree of its own (a new branch from `refName` when `newRefName` is given), which the
    /// project's directory is the repository of. The worktree's path is the executor's.
    pub fn create_worktree(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        let ref_name = field(payload, "refName")?;
        let named = |key: &str| payload.get(key).and_then(Value::as_str).filter(|s| !s.is_empty());
        let (branch, base) = match named("newRefName") {
            Some(new) => (new, Some(named("baseRefName").unwrap_or(ref_name).to_string())),
            None => (ref_name, None),
        };
        let op = Op::WorktreeCreate { root: root.into(), worktree: worktree_name(branch, root), branch: branch.into(), base };
        let made = self.agent().run(&op).map_err(|e| QueryFailure::Error(json!({"_tag": "GitCommandError", "operation": "createWorktree", "command": "git worktree add", "cwd": root, "detail": e.message()})))?;
        Ok(json!({"worktree": {"path": made["path"], "refName": made["branch"]}}))
    }

    /// `worktreeSetup.cancel`: whether a setup running in the thread's worktree was stopped.
    pub fn cancel_setup(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let thread = field(payload, "threadId")?;
        let root = self.thread_dir(thread);
        let in_worktree = self.engine().read(|s| s.threads.get(thread).is_some_and(|r| r["worktreePath"].as_str().is_some_and(|p| !p.is_empty())));
        if !in_worktree {
            return Ok(json!({"cancelled": false}));
        }
        match self.agent().run(&Op::WorktreeSetupCancel { root }) {
            Ok(done) => Ok(json!({"cancelled": done["cancelled"].as_bool().unwrap_or(false)})),
            Err(e) => Err(QueryFailure::Error(wire::host_error(&e.message()))),
        }
    }

    /// The worktree a thread's setup runs in, none when the thread is not in one.
    fn setup_worktree(&self, thread: &str) -> Option<String> {
        self.engine().read(|s| s.threads.get(thread).and_then(|r| r["worktreePath"].as_str().filter(|p| !p.is_empty()).map(str::to_string)))
    }

    /// `subscribeWorktreeSetup`: the setup of the thread's worktree as the agent API has it so far, with its first snapshot; none (a null frame)
    /// when the thread is in no worktree or no setup ran in it.
    pub fn open_setup(&self, thread: &str) -> Option<(SetupFold, Value)> {
        let worktree = self.setup_worktree(thread)?;
        let branch = self.engine().read(|s| s.threads.get(thread).and_then(|r| r["branch"].as_str().map(str::to_string)));
        let batch = match self.agent().setup_events(&worktree, 0, SETUP_WAIT) {
            Ok(batch) if batch.ran => batch,
            Ok(_) => return None,
            Err(e) => {
                self.log("worktree.setup.unreadable", &[("thread", thread.to_string()), ("message", e.message())]);
                return None;
            }
        };
        let mut fold = SetupFold::new(thread, &worktree, branch, self.now_ms());
        fold.take(&batch, self.now_ms());
        let first = fold.snapshot();
        Some((fold, first))
    }

    /// Follows the setup on a thread of its own, sending a snapshot to `to` (with the id of the stream) for each change, until it finishes, the
    /// agent API stops answering or `stop` is set.
    pub fn follow_setup(&self, thread: &str, mut fold: SetupFold, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let (svc, thread) = (self.clone(), thread.to_string());
        let Some(worktree) = self.setup_worktree(&thread) else { return };
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) && !fold.finished() {
                match svc.agent().setup_events(&worktree, fold.last_seq(), SETUP_WAIT) {
                    Ok(batch) => {
                        if fold.take(&batch, svc.now_ms()) {
                            if to.send((id.clone(), fold.snapshot())).is_err() {
                                return;
                            }
                        } else {
                            std::thread::sleep(IDLE);
                        }
                    }
                    Err(e) => {
                        svc.log("worktree.setup.follow.stopped", &[("thread", thread.clone()), ("message", e.message())]);
                        return;
                    }
                }
            }
        });
    }
}

/// A thread's worktree setup folded from the agent API's events into the snapshot T3 shows: the checkout (done: the worktree exists) and the
/// setup commands as one stage, whose detail is the command now running and whose tail is its latest output.
pub struct SetupFold {
    thread: String,
    worktree: String,
    branch: Option<String>,
    started: i64,
    ended: Option<i64>,
    steps: usize,
    finished_steps: usize,
    command: Option<String>,
    failed_step: Option<(usize, i32)>,
    tail: Vec<String>,
    outcome: Option<bool>,
    last_seq: u64,
}

impl SetupFold {
    pub fn new(thread: &str, worktree: &str, branch: Option<String>, now: i64) -> SetupFold {
        SetupFold { thread: thread.into(), worktree: worktree.into(), branch, started: now, ended: None, steps: 0, finished_steps: 0, command: None, failed_step: None, tail: vec![], outcome: None, last_seq: 0 }
    }

    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    pub fn finished(&self) -> bool {
        self.outcome.is_some()
    }

    /// Takes the events of a batch; whether any was new.
    pub fn take(&mut self, batch: &SetupBatch, now: i64) -> bool {
        let mut changed = false;
        let seen = self.last_seq;
        for SetupEvent { seq, kind, data } in batch.events.iter().filter(|e| e.seq > seen) {
            changed = true;
            self.last_seq = *seq;
            match kind.as_str() {
                "setup.started" => self.steps = data["steps"].as_u64().unwrap_or(0) as usize,
                "step.started" => {
                    self.command = data["command"].as_str().map(str::to_string);
                    self.tail.clear();
                }
                "step.output" => {
                    let lines = data["data"].as_str().unwrap_or_default().lines().map(|l| clip(l, TAIL_WIDTH));
                    self.tail.extend(lines);
                    let cut = self.tail.len().saturating_sub(TAIL_LINES);
                    self.tail.drain(..cut);
                }
                "step.finished" => {
                    let code = data["exit_code"].as_i64().unwrap_or(0) as i32;
                    self.finished_steps += 1;
                    if code != 0 {
                        self.failed_step = Some((data["step"].as_u64().unwrap_or(0) as usize, code));
                    }
                    self.command = None;
                }
                "setup.finished" => {
                    self.outcome = Some(data["ok"].as_bool().unwrap_or(false));
                    self.ended = Some(now);
                }
                _ => {}
            }
        }
        changed
    }

    /// A setup that ended without every command finishing, and with none failing, was cut off: cancelled, or out of time.
    fn phase(&self) -> &'static str {
        match self.outcome {
            None => "running",
            Some(true) => "done",
            Some(false) if self.failed_step.is_none() && self.finished_steps < self.steps => "cancelled",
            Some(false) => "failed",
        }
    }

    pub fn snapshot(&self) -> Value {
        let phase = self.phase();
        let stage_status = match phase {
            "running" => "running",
            "done" if self.steps == 0 => "skipped",
            "done" => "done",
            "cancelled" => "skipped",
            _ => "failed",
        };
        let percent = (self.steps > 0).then(|| (self.finished_steps * 100 / self.steps) as u64);
        let error = match (phase, self.failed_step) {
            ("failed", Some((step, code))) => Some(clip(&format!("Setup command {} exited with code {code}.", step + 1), ERROR_WIDTH)),
            ("cancelled", _) => Some("The setup was cancelled.".to_string()),
            _ => None,
        };
        let mut script = Map::new();
        script.insert("id".into(), json!("setup-script"));
        script.insert("status".into(), json!(stage_status));
        script.insert("startedAt".into(), json!(iso(self.started)));
        script.insert("endedAt".into(), self.ended.map_or(Value::Null, |ms| json!(iso(ms))));
        script.insert("percent".into(), percent.map_or(Value::Null, |p| json!(p)));
        script.insert("detail".into(), self.command.as_deref().map_or(Value::Null, |c| json!(clip(c, DETAIL_WIDTH))));
        script.insert("tail".into(), json!(self.tail));
        let checkout = json!({"id": "checkout", "status": "done", "startedAt": iso(self.started), "endedAt": iso(self.started), "percent": 100, "detail": null, "tail": []});
        json!({
            "threadId": self.thread,
            "phase": phase,
            "startedAt": iso(self.started),
            "endedAt": self.ended.map_or(Value::Null, |ms| json!(iso(ms))),
            "branch": self.branch,
            "baseRef": null,
            "worktreePath": self.worktree,
            "setupScript": null,
            "stages": [checkout, Value::Object(script)],
            "error": error,
            "sequence": self.last_seq,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(seq: u64, kind: &str, data: Value) -> SetupEvent {
        SetupEvent { seq, kind: kind.into(), data }
    }

    #[test]
    fn given_the_setup_events_of_two_commands_when_folded_then_the_snapshot_follows_the_commands_and_ends_done() {
        // Given the events of a setup of two commands, the first still running
        let mut fold = SetupFold::new("t1", "/base/wt", Some("feature/x".into()), 1_000);
        let first = SetupBatch { ran: true, events: vec![event(1, "setup.started", json!({"steps": 2})), event(2, "step.started", json!({"step": 0, "command": "pnpm i"})), event(3, "step.output", json!({"step": 0, "stream": "stdout", "data": "a\nb\n"}))] };
        // When they are folded
        assert!(fold.take(&first, 2_000));
        // Then the snapshot is running at the command and shows its output
        let running = fold.snapshot();
        assert_eq!((running["phase"].clone(), running["sequence"].clone(), running["stages"][1]["detail"].clone(), running["stages"][1]["tail"].clone()), (json!("running"), json!(3), json!("pnpm i"), json!(["a", "b"])));
        // And when the rest arrives the setup is done with every command counted, and the events already folded change nothing
        let rest = SetupBatch { ran: true, events: first.events.iter().cloned().chain([event(4, "step.finished", json!({"step": 0, "exit_code": 0})), event(5, "step.started", json!({"step": 1, "command": "x"})), event(6, "step.finished", json!({"step": 1, "exit_code": 0})), event(7, "setup.finished", json!({"ok": true}))]).collect() };
        assert!(fold.take(&rest, 9_000));
        assert!(!fold.take(&rest, 10_000));
        let done = fold.snapshot();
        assert_eq!((done["phase"].clone(), done["stages"][1]["status"].clone(), done["stages"][1]["percent"].clone(), done["error"].clone(), done["endedAt"].clone()), (json!("done"), json!("done"), json!(100), Value::Null, json!(iso(9_000))));
    }

    #[test]
    fn given_a_setup_that_ended_by_a_failing_command_or_was_cut_off_when_folded_then_it_is_failed_or_cancelled() {
        // Given a command that exited 3, and a setup whose command never finished
        let mut failing = SetupFold::new("t1", "/w", None, 0);
        failing.take(&SetupBatch { ran: true, events: vec![event(1, "setup.started", json!({"steps": 2})), event(2, "step.started", json!({"step": 0, "command": "x"})), event(3, "step.finished", json!({"step": 0, "exit_code": 3})), event(4, "setup.finished", json!({"ok": false}))] }, 5);
        let mut cut = SetupFold::new("t1", "/w", None, 0);
        cut.take(&SetupBatch { ran: true, events: vec![event(1, "setup.started", json!({"steps": 1})), event(2, "step.started", json!({"step": 0, "command": "sleep 9"})), event(3, "setup.finished", json!({"ok": false}))] }, 5);
        // Then the first is failed saying which command and code, the second cancelled
        assert_eq!((failing.snapshot()["phase"].clone(), failing.snapshot()["error"].clone()), (json!("failed"), json!("Setup command 1 exited with code 3.")));
        assert_eq!(cut.snapshot()["phase"], "cancelled");
    }

    #[test]
    fn given_branches_and_project_directories_when_named_as_worktrees_then_the_name_is_one_plain_segment_unique_to_the_project() {
        // Given a branch with slashes and one that starts with a dot, in two projects
        let a = worktree_name("feature/login page", "/p/a");
        // Then the name is plain characters only, the two projects differ and one project always gets the same name
        assert!(a.starts_with("feature-login-page-") && a.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)), "{a}");
        assert_ne!(a, worktree_name("feature/login page", "/p/b"));
        assert_eq!(a, worktree_name("feature/login page", "/p/a"));
        assert!(!worktree_name(".hidden", "/p/a").starts_with('.'));
    }
}
