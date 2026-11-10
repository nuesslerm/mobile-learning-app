//! A double of rca's git group: one repository held in memory, answering the methods of `USES` in the shapes of the contract (`agent-api-git`) and
//! changing as git would (a staged file leaves the working tree's column, a commit empties the index, a push publishes the branch). The chain
//! runs the real service and executor against the same calls, so a shape that drifts fails there.
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct File {
    path: String,
    index: String,
    worktree: String,
    conflicted: bool,
}

struct State {
    branch: String,
    branches: Vec<String>,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    operation: Option<String>,
    files: Vec<File>,
    subjects: Vec<String>,
    remotes: Vec<String>,
    changed: Vec<(String, String)>,
    pushed: Vec<(String, String, bool)>,
    fetched: Vec<String>,
    incoming: u32,
    rebase_stops_on: Option<String>,
    model_down: bool,
    asked_model: Vec<Vec<String>>,
}

#[derive(Clone)]
pub struct Repos(Arc<Mutex<State>>);

impl Default for Repos {
    fn default() -> Self {
        Repos(Arc::new(Mutex::new(State {
            branch: "main".into(),
            branches: vec!["main".into()],
            upstream: None,
            ahead: 0,
            behind: 0,
            operation: None,
            files: Vec::new(),
            subjects: vec!["first".into()],
            remotes: Vec::new(),
            changed: Vec::new(),
            pushed: Vec::new(),
            fetched: Vec::new(),
            incoming: 0,
            rebase_stops_on: None,
            model_down: false,
            asked_model: Vec::new(),
        })))
    }
}

fn refused(code: &str, message: impl Into<String>) -> AgentError {
    AgentError::Refused { code: code.into(), message: message.into() }
}

fn sha(n: usize) -> String {
    format!("{n:040x}")
}

impl Repos {
    /// A file in git's two status columns (`.` for none, `?` for both of an untracked file).
    pub fn file(&self, path: &str, index: &str, worktree: &str) -> &Self {
        self.0.lock().unwrap().files.push(File { path: path.into(), index: index.into(), worktree: worktree.into(), conflicted: false });
        self
    }

    /// A conflicted file, its unmerged pair in the columns, under the operation that stopped.
    pub fn conflict(&self, path: &str, index: &str, worktree: &str, operation: &str) -> &Self {
        let mut s = self.0.lock().unwrap();
        s.files.push(File { path: path.into(), index: index.into(), worktree: worktree.into(), conflicted: true });
        s.operation = Some(operation.into());
        drop(s);
        self
    }

    pub fn branch(&self, name: &str) -> &Self {
        self.0.lock().unwrap().branches.push(name.into());
        self
    }

    pub fn remote(&self, name: &str) -> &Self {
        self.0.lock().unwrap().remotes.push(name.into());
        self
    }

    pub fn track(&self, upstream: &str, ahead: u32, behind: u32) -> &Self {
        let mut s = self.0.lock().unwrap();
        (s.upstream, s.ahead, s.behind) = (Some(upstream.into()), ahead, behind);
        drop(s);
        self
    }

    /// What a fetch or pull brings in: that many commits arrive behind the branch.
    pub fn incoming(&self, commits: u32) -> &Self {
        self.0.lock().unwrap().incoming = commits;
        self
    }

    /// A file the branch changed since its merge base, as the comparison lists it.
    pub fn changed(&self, path: &str, status: &str) -> &Self {
        self.0.lock().unwrap().changed.push((path.into(), status.into()));
        self
    }

    /// A rebase onto any base stops on conflicts in this file.
    pub fn rebase_stops_on(&self, path: &str) -> &Self {
        self.0.lock().unwrap().rebase_stops_on = Some(path.into());
        self
    }

    /// The model API of the service has none configured, or cannot answer.
    pub fn model_down(&self) -> &Self {
        self.0.lock().unwrap().model_down = true;
        self
    }

    /// The staged paths the model was given each time a message was generated.
    pub fn model_asked(&self) -> Vec<Vec<String>> {
        self.0.lock().unwrap().asked_model.clone()
    }

    pub fn columns(&self, path: &str) -> Option<(String, String)> {
        self.0.lock().unwrap().files.iter().find(|f| f.path == path).map(|f| (f.index.clone(), f.worktree.clone()))
    }

    pub fn current(&self) -> String {
        self.0.lock().unwrap().branch.clone()
    }

    pub fn subjects(&self) -> Vec<String> {
        self.0.lock().unwrap().subjects.clone()
    }

    pub fn operation(&self) -> Option<String> {
        self.0.lock().unwrap().operation.clone()
    }

    /// What was pushed, as (remote, branch, set_upstream).
    pub fn pushed(&self) -> Vec<(String, String, bool)> {
        self.0.lock().unwrap().pushed.clone()
    }

    pub fn fetched(&self) -> Vec<String> {
        self.0.lock().unwrap().fetched.clone()
    }

    pub fn behind(&self) -> u32 {
        self.0.lock().unwrap().behind
    }

    /// Makes `agent` answer the methods of `USES` from this repository.
    pub fn install(&self, agent: &ScriptedAgent) {
        let on = |name: &str, f: fn(&mut State, &Value) -> Result<Value, AgentError>| {
            let me = self.clone();
            agent.reply(name, move |p| f(&mut me.0.lock().unwrap(), p));
        };
        on("git.status", |s, _| Ok(status(s)));
        on("git.upstreamStatus", |s, _| Ok(json!({"branch": s.branch, "has_upstream": s.upstream.is_some(), "upstream": s.upstream, "remote": s.upstream.as_deref().and_then(|u| u.split_once('/')).map(|(r, _)| r), "ahead": s.ahead, "behind": s.behind, "remotes": s.remotes})));
        on("git.refs", |s, _| Ok(json!({"current": s.branch, "detached": false, "refs": s.branches.iter().map(|b| json!({"name": b, "full": format!("refs/heads/{b}"), "kind": "branch", "sha": sha(1), "current": *b == s.branch})).collect::<Vec<_>>(), "truncated": false})));
        on("git.history", history);
        on("git.generateCommitMessage", |s, p| {
            if s.model_down {
                return Err(refused("MODEL_API_UNAVAILABLE", "no model is configured"));
            }
            let staged: Vec<String> = s.files.iter().filter(|f| f.index != "." && f.index != "?").map(|f| f.path.clone()).collect();
            if p["staged"] != false && staged.is_empty() {
                return Err(refused("NOTHING_TO_COMMIT", "there is no staged change to describe"));
            }
            s.asked_model.push(staged.clone());
            Ok(json!({"message": format!("feat: change {}", staged.join(", ")), "model": "double-model"}))
        });
        on("git.compare", compare);
        on("git.stage", |s, p| edit(s, p, stage));
        on("git.unstage", |s, p| edit(s, p, unstage));
        on("git.discard", |s, p| {
            let paths = paths(p)?;
            s.files.retain_mut(|f| {
                if !paths.contains(&f.path) && !paths.iter().any(|x| x == ".") {
                    return true;
                }
                f.worktree = ".".into();
                f.index != "." && f.index != "?"
            });
            Ok(json!({"restored": paths.len(), "cleaned": 0}))
        });
        on("git.commit", |s, p| {
            if !s.files.iter().any(|f| f.index != "." && f.index != "?" && !f.conflicted) {
                return Err(refused("GIT_FAILED", "nothing to commit"));
            }
            let subject = required(p, "message")?.lines().next().unwrap_or_default().to_string();
            s.files.retain_mut(|f| {
                f.index = ".".into();
                f.worktree != "."
            });
            s.subjects.push(subject.clone());
            s.ahead += u32::from(s.upstream.is_some());
            Ok(json!({"sha": sha(s.subjects.len()), "short": "abc1234", "subject": subject}))
        });
        on("git.checkout", |s, p| {
            let target = required(p, "ref")?;
            if !s.branches.iter().any(|b| b == target) {
                return Err(refused("GIT_FAILED", format!("pathspec '{target}' did not match any file(s) known to git")));
            }
            s.branch = target.into();
            Ok(json!({"branch": target, "head": sha(s.subjects.len()), "detached": false}))
        });
        on("git.fetch", |s, p| {
            let remote = remote_of(s, p)?;
            s.behind += s.incoming;
            s.fetched.push(remote.clone());
            Ok(json!({"remote": remote, "output": ""}))
        });
        on("git.pull", |s, p| {
            remote_of(s, p)?;
            if s.upstream.is_none() {
                return Err(refused("GIT_FAILED", "there is no tracking information for the current branch"));
            }
            for n in 0..s.behind {
                s.subjects.push(format!("incoming {n}"));
            }
            s.behind = 0;
            Ok(json!({"ok": true, "head": sha(s.subjects.len()), "updated": true, "conflicted": false, "conflicts": [], "output": ""}))
        });
        on("git.push", |s, p| {
            let remote = remote_of(s, p)?;
            let set = p["set_upstream"] == true;
            s.pushed.push((remote.clone(), s.branch.clone(), set));
            if set {
                s.upstream = Some(format!("{remote}/{}", s.branch));
            }
            s.ahead = 0;
            Ok(json!({"remote": remote, "branch": s.branch, "set_upstream": set, "output": ""}))
        });
        on("git.fastForward", |s, _| {
            if s.upstream.is_none() {
                return Err(refused("GIT_FAILED", "the branch has no upstream"));
            }
            for n in 0..s.behind {
                s.subjects.push(format!("incoming {n}"));
            }
            s.behind = 0;
            Ok(json!({"branch": s.branch, "from": sha(1), "head": sha(s.subjects.len()), "updated": true}))
        });
        on("git.merge", |s, p| integrate(s, p, "merge"));
        on("git.rebase", |s, p| integrate(s, p, "rebase"));
        on("git.conflictResolve", |s, p| {
            let path = required(p, "path")?;
            let Some(file) = s.files.iter_mut().find(|f| f.path == path && f.conflicted) else { return Err(refused("INVALID_ARGS", format!("`path` {path} is not a conflicted file"))) };
            if matches!(p["resolution"].as_str(), Some("ours" | "theirs")) {
                s.files.retain(|f| f.path != path);
            } else {
                (file.conflicted, file.index, file.worktree) = (false, "M".into(), ".".into());
            }
            Ok(json!({"path": path, "remaining": s.files.iter().filter(|f| f.conflicted).map(|f| &f.path).collect::<Vec<_>>()}))
        });
    }
}

fn status(s: &State) -> Value {
    let files: Vec<Value> = s.files.iter().map(|f| json!({"path": f.path, "index": f.index, "worktree": f.worktree, "conflicted": f.conflicted})).collect();
    json!({"branch": s.branch, "head": sha(s.subjects.len()), "detached": false, "upstream": s.upstream, "ahead": s.ahead, "behind": s.behind, "operation": s.operation, "clean": files.is_empty(), "truncated": false, "files": files})
}

/// A parameter the contract requires: a request without it is refused, as the service does, never read as an empty string.
fn required<'a>(p: &'a Value, key: &str) -> Result<&'a str, AgentError> {
    p[key].as_str().ok_or_else(|| refused("INVALID_PARAMS", format!("`{key}` is required")))
}

fn paths(p: &Value) -> Result<Vec<String>, AgentError> {
    p["paths"].as_array().ok_or_else(|| refused("INVALID_PARAMS", "`paths` is required"))?.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| refused("INVALID_PARAMS", "`paths` holds a non-string"))).collect()
}

fn edit(s: &mut State, p: &Value, change: fn(&mut File)) -> Result<Value, AgentError> {
    let wanted = paths(p)?;
    for f in s.files.iter_mut().filter(|f| wanted.contains(&f.path) || wanted.iter().any(|w| w == ".")) {
        change(f);
    }
    Ok(json!({"paths": wanted.len()}))
}

fn stage(f: &mut File) {
    if f.worktree == "." {
        return;
    }
    f.index = if f.index == "?" { "A".into() } else { f.worktree.clone() };
    f.worktree = ".".into();
}

fn unstage(f: &mut File) {
    let index = f.index.clone();
    match index.as_str() {
        "A" => (f.index, f.worktree) = ("?".into(), "?".into()),
        "." | "?" => {}
        other => (f.worktree, f.index) = (other.to_string(), ".".into()),
    }
}

fn remote_of(s: &State, p: &Value) -> Result<String, AgentError> {
    let remote = p["remote"].as_str().map(str::to_string).or_else(|| s.upstream.as_deref().and_then(|u| u.split_once('/')).map(|(r, _)| r.to_string())).unwrap_or_else(|| "origin".into());
    if s.remotes.contains(&remote) {
        Ok(remote)
    } else {
        Err(refused("INVALID_ARGS", format!("`remote` {remote} is not configured")))
    }
}

fn history(s: &mut State, p: &Value) -> Result<Value, AgentError> {
    let (limit, skip) = (p["limit"].as_u64().unwrap_or(50) as usize, p["skip"].as_u64().unwrap_or(0) as usize);
    let upto = match p["rev"].as_str().filter(|r| r.len() == 40) {
        Some(rev) => usize::from_str_radix(rev, 16).ok().filter(|n| (1..=s.subjects.len()).contains(n)).ok_or_else(|| refused("GIT_FAILED", format!("unknown revision {rev}")))?,
        None => s.subjects.len(),
    };
    let newest_first: Vec<Value> = s.subjects.iter().take(upto).enumerate().rev().map(|(i, subject)| json!({"sha": sha(i + 1), "short": format!("{:07x}", i + 1), "parents": if i == 0 { vec![] } else { vec![sha(i)] }, "author_name": "A U Thor", "author_email": "author@example.test", "authored_ms": 1_700_000_000_000u64 + i as u64, "subject": subject, "body": ""})).collect();
    let page: Vec<Value> = newest_first.iter().skip(skip).take(limit).cloned().collect();
    Ok(json!({"commits": page, "truncated": newest_first.len() > skip + limit}))
}

fn compare(s: &mut State, p: &Value) -> Result<Value, AgentError> {
    let base = required(p, "base")?;
    if base == "nope" {
        return Err(refused("GIT_FAILED", "unknown revision nope"));
    }
    let files: Vec<Value> = s.changed.iter().map(|(path, status)| json!({"path": path, "status": status, "additions": 2, "deletions": 1, "binary": false})).collect();
    Ok(json!({"base": base, "head": p["head"], "merge_base": sha(1), "ahead": s.ahead.max(1), "behind": s.behind, "commits": [], "files": files, "truncated": false}))
}

fn integrate(s: &mut State, p: &Value, kind: &str) -> Result<Value, AgentError> {
    let stopped = |s: &State| -> Vec<String> { s.files.iter().filter(|f| f.conflicted).map(|f| f.path.clone()).collect() };
    match p["op"].as_str() {
        Some("abort") => {
            if s.operation.as_deref() != Some(kind) {
                return Err(refused("GIT_FAILED", format!("there is no {kind} to abort")));
            }
            s.files.retain(|f| !f.conflicted);
            s.operation = None;
        }
        Some("continue") => {
            let left = stopped(s);
            if !left.is_empty() {
                return Ok(json!({"ok": false, "head": sha(s.subjects.len()), "updated": false, "conflicted": true, "conflicts": left, "operation": kind, "output": ""}));
            }
            s.operation = None;
            s.subjects.push(format!("{kind} finished"));
        }
        _ => {
            if let Some(path) = s.rebase_stops_on.clone().filter(|_| kind == "rebase") {
                s.files.push(File { path: path.clone(), index: "U".into(), worktree: "U".into(), conflicted: true });
                s.operation = Some("rebase".into());
                return Ok(json!({"ok": false, "head": sha(s.subjects.len()), "updated": false, "conflicted": true, "conflicts": [path], "operation": "rebase", "output": ""}));
            }
        }
    }
    Ok(json!({"ok": true, "head": sha(s.subjects.len()), "updated": true, "conflicted": false, "conflicts": [], "output": ""}))
}
