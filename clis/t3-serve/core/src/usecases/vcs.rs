//! The M9 rows: the repository of a project's directory, read and changed through the agent API's `git` group (the executor's own `git`, with
//! its own credentials): the status and its stream, the refs, switching and creating one, pulling, the diffs a review shows, and the stacked
//! actions (stage, commit, push) with their progress. The executor says which files changed; the insertions and deletions the app shows are
//! counted here from the diff the executor gives, and nothing here runs git.
use super::queries::{field, QueryFailure};
use super::service::Service;
use crate::ports::{AgentError, Follow, Op};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use t3_contracts::time::iso;

/// The most refs one `git.refs` is asked for, and the page size when the app names none.
const REFS_ASK: u32 = 2000;
const REFS_PAGE: usize = 100;
const REFS_PAGE_MAX: usize = 200;
/// The most characters of one diff a review source carries.
const DIFF_CAP: usize = 1_000_000;
const COMMIT_LOG: u32 = 1;
const DEFAULT_REFS: [&str; 2] = ["main", "master"];

fn command_error(operation: &str, cwd: &str, detail: &str) -> Value {
    let mut error = json!({"_tag": "GitCommandError", "operation": operation, "command": "git", "cwd": cwd, "detail": detail});
    let reason = if detail.contains("not a git repository") {
        Some("not_a_repository")
    } else if detail.contains("non-fast-forward") || detail.contains("fetch first") {
        Some("push_rejected_non_fast_forward")
    } else if detail.contains("rejected") {
        Some("push_rejected_by_remote")
    } else if detail.contains("Authentication failed") || detail.contains("could not read Username") {
        Some("authentication_failed")
    } else if detail.contains("already exists") {
        Some("branch_already_exists")
    } else {
        None
    };
    if let (Some(reason), Some(map)) = (reason, error.as_object_mut()) {
        map.insert("reason".into(), json!(reason));
    }
    error
}

fn manager_error(operation: &str, cwd: &str, detail: &str) -> Value {
    json!({"_tag": "GitManagerError", "operation": operation, "cwd": cwd, "detail": detail})
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Insertions and deletions of each path a unified diff changes, counted from its hunks.
pub fn numstat(diff: &str) -> Vec<(String, u64, u64)> {
    let mut rows: Vec<(String, u64, u64)> = Vec::new();
    let mut in_hunk = false;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = rest.rsplit_once(" b/").map_or(rest, |(_, new)| new).to_string();
            rows.push((path, 0, 0));
            in_hunk = false;
        } else if line.starts_with("@@") {
            in_hunk = true;
        } else if in_hunk {
            if let Some(last) = rows.last_mut() {
                if line.starts_with('+') {
                    last.1 += 1;
                } else if line.starts_with('-') {
                    last.2 += 1;
                }
            }
        }
    }
    rows
}

fn hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))[..16].to_string()
}

fn capped(diff: &str) -> (String, bool) {
    if diff.len() <= DIFF_CAP {
        return (diff.to_string(), false);
    }
    let mut end = DIFF_CAP;
    while !diff.is_char_boundary(end) {
        end -= 1;
    }
    (diff[..end].to_string(), true)
}

/// The two halves of `VcsStatusResult` the executor's status makes: the local state and the remote one.
fn halves(root: &str, status: &Value, has_remote: bool, counts: &HashMap<String, (u64, u64)>) -> (Value, Value) {
    let branch = text_of(status, "branch");
    let files: Vec<Value> = status["files"].as_array().into_iter().flatten().filter_map(|f| f["path"].as_str()).map(|path| {
        let (insertions, deletions) = counts.get(path).copied().unwrap_or((0, 0));
        json!({"path": path, "insertions": insertions, "deletions": deletions})
    }).collect();
    let (insertions, deletions) = files.iter().fold((0, 0), |(i, d), f| (i + f["insertions"].as_u64().unwrap_or(0), d + f["deletions"].as_u64().unwrap_or(0)));
    let mut working_tree = Map::new();
    if root.starts_with('/') {
        working_tree.insert("rootPath".into(), json!(root));
    }
    working_tree.extend([("files".to_string(), json!(files)), ("insertions".into(), json!(insertions)), ("deletions".into(), json!(deletions))]);
    let local = json!({
        "isRepo": true,
        "hasPrimaryRemote": has_remote,
        "isDefaultRef": branch.is_some_and(|b| DEFAULT_REFS.contains(&b)),
        "refName": branch,
        "hasWorkingTreeChanges": !files.is_empty(),
        "workingTree": Value::Object(working_tree),
    });
    let remote = json!({"hasUpstream": text_of(status, "upstream").is_some(), "aheadCount": status["ahead"].as_u64().unwrap_or(0), "behindCount": status["behind"].as_u64().unwrap_or(0), "pr": null});
    (local, remote)
}

fn not_a_repo(e: &AgentError) -> bool {
    e.message().contains("not a git repository")
}

/// One source of a diff preview: what it is called and the two revisions (and the path) its diff is asked for.
struct Source<'a> {
    id: &'a str,
    kind: &'a str,
    title: &'a str,
    base: Option<&'a str>,
    head: Option<&'a str>,
    path: Option<&'a str>,
    subject: Option<String>,
}

/// What the status stream last told the app, so a frame is sent only for a half that changed.
pub struct StatusFold {
    root: String,
    has_remote: bool,
    local: Value,
    remote: Value,
}

impl StatusFold {
    pub fn root(&self) -> &str {
        &self.root
    }
}

impl Service {
    fn git_counts(&self, root: &str) -> HashMap<String, (u64, u64)> {
        let diff = self.agent().run(&Op::GitDiff { root: root.into(), base: Some("HEAD".into()), head: None, path: None });
        diff.ok().map(|d| numstat(d["diff"].as_str().unwrap_or_default()).into_iter().map(|(p, i, d)| (p, (i, d))).collect()).unwrap_or_default()
    }

    /// The status, the upstream and the change counts of the repository at `root`, as the two halves of `VcsStatusResult`. A directory that is no
    /// repository is a status that says so.
    fn git_halves(&self, root: &str) -> Result<(Value, Value, bool), AgentError> {
        let status = match self.agent().run(&Op::GitStatus { root: root.into() }) {
            Ok(status) => status,
            Err(e) if not_a_repo(&e) => {
                let local = json!({"isRepo": false, "hasPrimaryRemote": false, "isDefaultRef": false, "refName": null, "hasWorkingTreeChanges": false, "workingTree": {"files": [], "insertions": 0, "deletions": 0}});
                return Ok((local, json!({"hasUpstream": false, "aheadCount": 0, "behindCount": 0, "pr": null}), false));
            }
            Err(e) => return Err(e),
        };
        let has_remote = self.agent().run(&Op::GitUpstream { root: root.into() }).map(|u| u["remotes"].as_array().is_some_and(|r| !r.is_empty())).unwrap_or(false);
        let (local, remote) = halves(root, &status, has_remote, &self.git_counts(root));
        Ok((local, remote, has_remote))
    }

    /// `vcs.refreshStatus`: the status of the repository.
    pub fn vcs_refresh_status(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        let (mut local, remote, _) = self.git_halves(root).map_err(|e| QueryFailure::Error(manager_error("refreshStatus", root, &e.message())))?;
        if let (Some(into), Some(from)) = (local.as_object_mut(), remote.as_object()) {
            into.extend(from.clone());
        }
        Ok(local)
    }

    /// `subscribeVcsStatus`: the status as a snapshot, and then what the executor's status stream says changed.
    pub fn open_vcs_status(&self, payload: &Value) -> Result<(StatusFold, Value), QueryFailure> {
        let root = field(payload, "cwd")?;
        let (local, remote, has_remote) = self.git_halves(root).map_err(|e| QueryFailure::Error(manager_error("subscribeStatus", root, &e.message())))?;
        let first = json!({"_tag": "snapshot", "local": local, "remote": remote});
        Ok((StatusFold { root: root.into(), has_remote, local, remote }, first))
    }

    /// Follows the status on a thread of its own, sending a frame for each half that changed until the app's stream is gone or `stop` is set.
    pub fn follow_vcs_status(&self, mut fold: StatusFold, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let root = fold.root.clone();
            let result = svc.agent().git_follow(&root, &mut |status| {
                if stop.load(Ordering::SeqCst) {
                    return Follow::Stop;
                }
                let Some(status) = status else { return Follow::Continue };
                let (local, remote) = halves(&root, &status, fold.has_remote, &svc.git_counts(&root));
                let mut frames = Vec::new();
                if local != fold.local {
                    frames.push(json!({"_tag": "localUpdated", "local": local}));
                    fold.local = local;
                }
                if remote != fold.remote {
                    frames.push(json!({"_tag": "remoteUpdated", "remote": remote}));
                    fold.remote = remote;
                }
                if frames.into_iter().any(|frame| to.send((id.clone(), frame)).is_err()) {
                    return Follow::Stop;
                }
                Follow::Continue
            });
            if let Err(e) = result {
                svc.log("vcs.status.stopped", &[("message", e.message())]);
            }
        });
    }

    /// `vcs.listRefs`: the branches (and remote branches) a page at a time.
    pub fn vcs_list_refs(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        let fail = |e: AgentError| QueryFailure::Error(command_error("listRefs", root, &e.message()));
        let kind = match text_of(payload, "refKind") {
            Some("local") => Some("branch"),
            Some("remote") => Some("remote"),
            _ => None,
        };
        let listed = match self.agent().run(&Op::GitRefs { root: root.into(), kind, limit: REFS_ASK }) {
            Ok(listed) => listed,
            Err(e) if not_a_repo(&e) => return Ok(json!({"refs": [], "isRepo": false, "hasPrimaryRemote": false, "nextCursor": null, "totalCount": 0})),
            Err(e) => return Err(fail(e)),
        };
        let query = text_of(payload, "query").map(str::to_lowercase);
        let rows: Vec<Value> = listed["refs"].as_array().into_iter().flatten().filter(|r| r["kind"] != "tag").filter(|r| query.as_ref().is_none_or(|q| r["name"].as_str().is_some_and(|n| n.to_lowercase().contains(q)))).cloned().collect();
        let (cursor, limit) = (payload.get("cursor").and_then(Value::as_u64).unwrap_or(0) as usize, payload.get("limit").and_then(Value::as_u64).map_or(REFS_PAGE, |l| l as usize).min(REFS_PAGE_MAX));
        let has_remote = self.agent().run(&Op::GitUpstream { root: root.into() }).map(|u| u["remotes"].as_array().is_some_and(|r| !r.is_empty())).unwrap_or(false);
        let page: Vec<Value> = rows.iter().skip(cursor).take(limit).map(|r| {
            let name = r["name"].as_str().unwrap_or_default();
            let remote = r["kind"] == "remote";
            let mut shown = json!({"name": name, "current": r["current"].as_bool().unwrap_or(false), "isDefault": DEFAULT_REFS.contains(&name), "worktreePath": null});
            if let Some(map) = shown.as_object_mut() {
                if remote {
                    map.insert("isRemote".into(), json!(true));
                    if let Some((remote_name, _)) = name.split_once('/') {
                        map.insert("remoteName".into(), json!(remote_name));
                    }
                }
                if let Some(upstream) = text_of(r, "upstream") {
                    map.insert("upstreamRef".into(), json!(format!("refs/remotes/{upstream}")));
                }
            }
            shown
        }).collect();
        let next = cursor + page.len();
        Ok(json!({"refs": page, "isRepo": true, "hasPrimaryRemote": has_remote, "nextCursor": if next < rows.len() { json!(next) } else { Value::Null }, "totalCount": rows.len()}))
    }

    /// `vcs.switchRef`: a branch checked out, or for a remote branch a local one made to track it.
    pub fn vcs_switch_ref(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (root, name) = (field(payload, "cwd")?, field(payload, "refName")?);
        let fail = |e: AgentError| QueryFailure::Error(command_error("switchRef", root, &e.message()));
        let remote = self.agent().run(&Op::GitRefs { root: root.into(), kind: Some("remote"), limit: REFS_ASK }).map(|r| r["refs"].as_array().is_some_and(|refs| refs.iter().any(|x| x["name"] == name))).unwrap_or(false);
        let local = self.agent().run(&Op::GitRefs { root: root.into(), kind: Some("branch"), limit: REFS_ASK }).map(|r| r["refs"].as_array().is_some_and(|refs| refs.iter().any(|x| x["name"] == name))).unwrap_or(false);
        let moved = self.agent().run(&Op::GitCheckout { root: root.into(), target: name.into(), track: remote && !local }).map_err(fail)?;
        Ok(json!({"refName": text_of(&moved, "branch")}))
    }

    /// `vcs.createRef`: a branch created at the head, switched to when asked.
    pub fn vcs_create_ref(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let (root, name) = (field(payload, "cwd")?, field(payload, "refName")?);
        let checkout = payload.get("switchRef").and_then(Value::as_bool).unwrap_or(false);
        self.agent().run(&Op::GitCreateRef { root: root.into(), name: name.into(), from: None, checkout }).map_err(|e| QueryFailure::Error(command_error("createRef", root, &e.message())))?;
        Ok(json!({"refName": name}))
    }

    /// `vcs.pull`: the current branch fast-forwarded from its upstream; a pull that would not fast-forward is the refusal.
    pub fn vcs_pull(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        let fail = |detail: &str| QueryFailure::Error(command_error("pull", root, detail));
        let before = self.agent().run(&Op::GitUpstream { root: root.into() }).map_err(|e| fail(&e.message()))?;
        let Some(branch) = text_of(&before, "branch") else { return Err(fail("not on a branch (detached HEAD)")) };
        let pulled = self.agent().run(&Op::GitPull { root: root.into() }).map_err(|e| fail(&e.message()))?;
        if pulled["ok"] == false {
            return Err(fail(&format!("the pull stopped on conflicts: {}", pulled["conflicts"].as_array().into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))));
        }
        let status = if pulled["updated"] == true { "pulled" } else { "skipped_up_to_date" };
        Ok(json!({"status": status, "refName": branch, "upstreamRef": text_of(&before, "upstream")}))
    }

    fn diff_source(&self, root: &str, src: Source) -> Result<Value, AgentError> {
        let Source { id, kind, title, base, head, path, subject } = src;
        let op = Op::GitDiff { root: root.into(), base: base.map(str::to_string), head: head.map(str::to_string), path: path.map(str::to_string) };
        let raw = self.agent().run(&op)?;
        let (diff, truncated) = capped(raw["diff"].as_str().unwrap_or_default());
        let files: Vec<Value> = numstat(&diff).into_iter().map(|(path, additions, deletions)| json!({"path": path, "previousPath": null, "additions": additions, "deletions": deletions})).collect();
        let mut source = json!({"id": id, "kind": kind, "title": title, "baseRef": if kind == "working-tree" { Value::Null } else { json!(base) }, "headRef": if kind == "working-tree" { Value::Null } else { json!(head) }, "diff": diff, "diffHash": hash(&diff), "truncated": truncated, "files": files});
        if let (Some(subject), Some(map)) = (subject, source.as_object_mut()) {
            map.insert("commitSubject".into(), json!(subject));
        }
        Ok(source)
    }

    /// `review.getDiffPreview`: the working tree against the head, the branch against `baseRef`, and one commit, each as a source.
    pub fn review_diff_preview(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        let fail = |e: AgentError| QueryFailure::Error(command_error("reviewDiffPreview", root, &e.message()));
        let file = payload.get("file").filter(|f| f.is_object());
        let only = file.and_then(|f| text_of(f, "sourceKind"));
        let path = file.and_then(|f| text_of(f, "path"));
        let wants = |kind: &str| only.is_none_or(|o| o == kind);
        let mut sources = Vec::new();
        if wants("working-tree") {
            sources.push(self.diff_source(root, Source { id: "working-tree", kind: "working-tree", title: "Working tree", base: Some("HEAD"), head: None, path, subject: None }).map_err(fail)?);
        }
        if let (Some(base), true) = (text_of(payload, "baseRef"), wants("branch-range")) {
            sources.push(self.diff_source(root, Source { id: "branch-range", kind: "branch-range", title: "Branch changes", base: Some(base), head: Some("HEAD"), path, subject: None }).map_err(fail)?);
        }
        if let (Some(sha), true) = (text_of(payload, "commitSha"), wants("commit")) {
            let subject = self.agent().run(&Op::GitHistory { root: root.into(), rev: Some(sha.into()), limit: COMMIT_LOG }).ok().and_then(|h| h["commits"][0]["subject"].as_str().map(str::to_string));
            let parent = format!("{sha}^");
            let id = format!("commit:{sha}");
            let title = subject.clone().unwrap_or_else(|| sha.to_string());
            sources.push(self.diff_source(root, Source { id: &id, kind: "commit", title: &title, base: Some(&parent), head: Some(sha), path, subject }).map_err(fail)?);
        }
        Ok(json!({"cwd": root, "generatedAt": iso(self.now_ms()), "sources": sources}))
    }

    /// `review.getDiffFileContents`: a changed file as it was and as it is, from the revisions the source names (the working tree's own file
    /// for the new side of the working tree).
    pub fn review_diff_file_contents(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let root = field(payload, "cwd")?;
        let (kind, change) = (field(payload, "sourceKind")?, field(payload, "changeType")?);
        let (old_path, new_path) = (field(payload, "oldPath")?, field(payload, "newPath")?);
        let (base, head) = (text_of(payload, "baseRef"), text_of(payload, "headRef"));
        let fail = |e: AgentError| QueryFailure::Error(command_error("reviewDiffFileContents", root, &e.message()));
        let blob = |rev: &str, path: &str| -> Result<String, QueryFailure> {
            match self.agent().run(&Op::GitBlob { root: root.into(), rev: rev.into(), path: path.into() }) {
                Ok(blob) => Ok(blob["text"].as_str().unwrap_or_default().to_string()),
                Err(AgentError::Refused { code, .. }) if code == "NOT_FOUND" => Ok(String::new()),
                Err(e) => Err(fail(e)),
            }
        };
        let (old_rev, new_rev) = match kind {
            "branch-range" => (base.unwrap_or("HEAD").to_string(), head.unwrap_or("HEAD").to_string()),
            "commit" => (format!("{}^", head.unwrap_or("HEAD")), head.unwrap_or("HEAD").to_string()),
            _ => ("HEAD".to_string(), String::new()),
        };
        let old = if change == "new" { String::new() } else { blob(&old_rev, old_path)? };
        let new = if change == "deleted" {
            String::new()
        } else if kind == "working-tree" {
            self.agent().read_file(root, new_path).unwrap_or_default()
        } else {
            blob(&new_rev, new_path)?
        };
        Ok(json!({"oldContents": old, "newContents": new}))
    }

    /// `git.runStackedAction`: the steps of the action in order on a thread of its own, each reported as a progress frame, the stream ending
    /// with `action_finished` or `action_failed`.
    pub fn run_stacked_action(&self, payload: &Value, id: Value, to: Sender<(Value, Value)>, stop: Arc<AtomicBool>) -> Result<(), QueryFailure> {
        let (action_id, root, action) = (field(payload, "actionId")?.to_string(), field(payload, "cwd")?.to_string(), field(payload, "action")?.to_string());
        if !["commit", "push", "create_pr", "commit_push", "commit_push_pr"].contains(&action.as_str()) {
            return Err(QueryFailure::Defect("Expected a stacked action\n  at [\"action\"]".into()));
        }
        let message = text_of(payload, "commitMessage").map(str::to_string);
        let paths: Vec<String> = payload.get("filePaths").and_then(Value::as_array).map(|p| p.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let feature_branch = payload.get("featureBranch").and_then(Value::as_bool).unwrap_or(false);
        let svc = self.clone();
        std::thread::spawn(move || {
            let run = Stacked { svc, id, to, stop, action_id, root, action, message, paths, feature_branch };
            run.go();
        });
        Ok(())
    }
}

struct Stacked {
    svc: Service,
    id: Value,
    to: Sender<(Value, Value)>,
    stop: Arc<AtomicBool>,
    action_id: String,
    root: String,
    action: String,
    message: Option<String>,
    paths: Vec<String>,
    feature_branch: bool,
}

impl Stacked {
    fn frame(&self, kind: &str, extra: Value) -> bool {
        let mut frame = json!({"actionId": self.action_id, "cwd": self.root, "action": self.action, "kind": kind});
        if let (Some(into), Some(from)) = (frame.as_object_mut(), extra.as_object()) {
            into.extend(from.clone());
        }
        !self.stop.load(Ordering::SeqCst) && self.to.send((self.id.clone(), frame)).is_ok()
    }

    fn agent(&self, op: &Op) -> Result<Value, AgentError> {
        self.svc.agent().run(op)
    }

    fn fail(&self, phase: Option<&str>, message: &str) {
        self.frame("action_failed", json!({"phase": phase, "message": if message.trim().is_empty() { "the action failed" } else { message.trim() }}));
    }

    fn go(&self) {
        let wants = |step: &str| match step {
            "commit" => self.action.starts_with("commit"),
            "push" => self.action.contains("push"),
            _ => self.action.ends_with("pr"),
        };
        let mut phases: Vec<&str> = Vec::new();
        if self.feature_branch && wants("commit") {
            phases.push("branch");
        }
        phases.extend(["commit", "push", "pr"].into_iter().filter(|p| wants(p)));
        if !self.frame("action_started", json!({"phases": phases})) {
            return;
        }
        let end = |this: &Self| {
            let _ = this.to.send((this.id.clone(), t3_contracts::rpc::end_marker()));
        };
        let mut result = json!({
            "action": self.action,
            "branch": {"status": "skipped_not_requested"},
            "commit": {"status": "skipped_not_requested"},
            "push": {"status": "skipped_not_requested"},
            "pr": {"status": "skipped_not_requested"},
        });
        if phases.contains(&"branch") {
            self.frame("phase_started", json!({"phase": "branch", "label": "Creating a branch"}));
            let name = branch_name(self.message.as_deref().unwrap_or("changes"));
            if let Err(e) = self.agent(&Op::GitCreateRef { root: self.root.clone(), name: name.clone(), from: None, checkout: true }) {
                self.fail(Some("branch"), &e.message());
                return end(self);
            }
            result["branch"] = json!({"status": "created", "name": name});
        }
        if wants("commit") {
            self.frame("phase_started", json!({"phase": "commit", "label": "Committing"}));
            let Some(message) = self.message.clone() else {
                self.fail(Some("commit"), "a commit message is required: this server does not write one");
                return end(self);
            };
            let paths = if self.paths.is_empty() { vec![".".to_string()] } else { self.paths.clone() };
            if let Err(e) = self.agent(&Op::GitStage { root: self.root.clone(), paths }) {
                self.fail(Some("commit"), &e.message());
                return end(self);
            }
            let clean = self.agent(&Op::GitStatus { root: self.root.clone() }).map(|s| s["files"].as_array().is_none_or(|f| f.iter().all(|x| x["index"] == "." || x["index"] == "?"))).unwrap_or(false);
            if clean {
                result["commit"] = json!({"status": "skipped_no_changes"});
            } else {
                match self.agent(&Op::GitCommit { root: self.root.clone(), message: message.clone() }) {
                    Ok(done) => result["commit"] = json!({"status": "created", "commitSha": done["sha"], "subject": done["subject"]}),
                    Err(e) => {
                        self.fail(Some("commit"), &e.message());
                        return end(self);
                    }
                }
            }
        }
        if wants("push") {
            self.frame("phase_started", json!({"phase": "push", "label": "Pushing"}));
            let before = self.agent(&Op::GitUpstream { root: self.root.clone() }).unwrap_or_default();
            let has_upstream = before["has_upstream"] == true;
            let up_to_date = has_upstream && before["ahead"] == 0;
            if up_to_date {
                result["push"] = json!({"status": "skipped_up_to_date", "branch": before["branch"]});
            } else {
                match self.agent(&Op::GitPush { root: self.root.clone(), set_upstream: !has_upstream }) {
                    Ok(done) => result["push"] = json!({"status": "pushed", "branch": done["branch"], "upstreamBranch": format!("{}/{}", done["remote"].as_str().unwrap_or("origin"), done["branch"].as_str().unwrap_or_default()), "setUpstream": !has_upstream}),
                    Err(e) => {
                        self.fail(Some("push"), &e.message());
                        return end(self);
                    }
                }
            }
        }
        if wants("pr") {
            self.frame("phase_started", json!({"phase": "pr", "label": "Opening a pull request"}));
            self.fail(Some("pr"), "pull requests are not served by this server yet");
            return end(self);
        }
        let toast = json!({"title": toast_title(&result), "cta": {"kind": "none"}});
        result["toast"] = toast;
        self.frame("action_finished", json!({"result": result}));
        end(self);
    }
}

fn toast_title(result: &Value) -> String {
    let (commit, push) = (result["commit"]["status"].as_str().unwrap_or_default(), result["push"]["status"].as_str().unwrap_or_default());
    match (commit, push) {
        ("created", "pushed") => "Committed and pushed",
        ("created", _) => "Committed",
        (_, "pushed") => "Pushed",
        _ => "Nothing to do",
    }
    .to_string()
}

/// A branch name from a commit message: `t3code/` and its words, lowercase, `-` between.
fn branch_name(message: &str) -> String {
    let words: Vec<String> = message.lines().next().unwrap_or_default().split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).take(6).map(str::to_lowercase).collect();
    format!("t3code/{}", if words.is_empty() { "changes".to_string() } else { words.join("-") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_unified_diff_of_two_files_when_counted_then_each_path_has_its_own_insertions_and_deletions_and_header_lines_are_not_counted() {
        // Given a diff that changes one file by two lines and removes a line of another whose text starts with dashes
        let diff = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n one\n+two\n+three\ndiff --git a/dir/b b.md b/dir/b b.md\n--- a/dir/b b.md\n+++ b/dir/b b.md\n@@ -1,2 +1 @@\n--- not a header\n keep\n";
        // When counted, Then a.txt has two insertions and the other one deletion, and its name keeps the space
        assert_eq!(numstat(diff), [("a.txt".to_string(), 2, 0), ("dir/b b.md".to_string(), 0, 1)]);
    }

    #[test]
    fn given_commit_messages_when_made_into_branch_names_then_they_are_lowercase_words_under_the_prefix_and_an_empty_one_is_changes() {
        assert_eq!(branch_name("Fix the Login page!\n\nbody"), "t3code/fix-the-login-page");
        assert_eq!(branch_name("***"), "t3code/changes");
    }

    #[test]
    fn given_git_failures_when_turned_into_errors_then_the_reason_the_app_tells_apart_is_named_and_other_text_is_only_detail() {
        assert_eq!(command_error("pull", "/r", "fatal: not a git repository")["reason"], json!("not_a_repository"));
        assert_eq!(command_error("push", "/r", "! [rejected] main -> main (fetch first)")["reason"], json!("push_rejected_non_fast_forward"));
        assert!(command_error("push", "/r", "disk full").get("reason").is_none());
    }
}
