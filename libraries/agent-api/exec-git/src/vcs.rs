//! The repository actions of the git family (`git_status` .. `git_watch`): what a request may ask, validated here so the executor never hands
//! git a value it could read as an option, a magic pathspec or a path outside the tree. Every action works on the workspace's repository, or
//! on a worktree under the worktree base when the request names one. Times are milliseconds, fields are snake_case.
use crate::{branch, revision, worktree_name};
use exec_proto::args::{flag, opt_string, opt_uint, string};
use exec_proto::{ExecError, ExecResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LOG_DEFAULT: usize = 50;
pub const LOG_MAX: usize = 500;
pub const REFS_DEFAULT: usize = 500;
pub const REFS_MAX: usize = 5000;
/// The most files a status lists; `truncated` says there were more.
pub const STATUS_FILES_CAP: usize = 5000;
pub const PATHS_MAX: usize = 1000;
pub const BLOB_DEFAULT: u64 = 1 << 20;
pub const BLOB_MAX: u64 = 8 << 20;
pub const WATCH_DEFAULT_MS: u64 = 1000;
pub const WATCH_MIN_MS: u64 = 100;
pub const WATCH_MAX_MS: u64 = 60_000;
/// The most first files `git_init` writes, and the most bytes of one.
pub const INIT_FILES_MAX: usize = 32;
pub const INIT_FILE_MAX: usize = 256 << 10;
/// The message of the first commit of `git_init` when the request names none.
pub const INIT_MESSAGE: &str = "Initial commit";

/// A path of the tree as a request names it: relative, no `..`, nothing inside `.git`; `.` is the whole tree. Returns it normalised.
pub fn tree_path(value: &str) -> ExecResult<String> {
    let bad = || ExecError::invalid_args("`path` must be a path inside the repository's working tree");
    if value.is_empty() || value.contains('\0') {
        return Err(bad());
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => return Err(bad()),
            p if p.eq_ignore_ascii_case(".git") => return Err(bad()),
            p => parts.push(p),
        }
    }
    if value.starts_with('/') {
        return Err(bad());
    }
    Ok(if parts.is_empty() { ".".to_string() } else { parts.join("/") })
}

fn paths(args: &Value) -> ExecResult<Vec<String>> {
    let Some(list) = args.get("paths").filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or_else(|| ExecError::invalid_args("`paths` must be an array of strings"))?;
    if list.len() > PATHS_MAX {
        return Err(ExecError::invalid_args(format!("`paths` holds at most {PATHS_MAX} paths")));
    }
    list.iter().map(|p| p.as_str().ok_or_else(|| ExecError::invalid_args("`paths` must be an array of strings")).and_then(tree_path)).collect()
}

fn rev(args: &Value, key: &str) -> ExecResult<Option<String>> {
    let value = opt_string(args, key)?;
    if let Some(v) = &value {
        revision(v).map_err(|_| ExecError::invalid_args(format!("`{key}` must be a revision")))?;
    }
    Ok(value)
}

fn required_rev(args: &Value, key: &str) -> ExecResult<String> {
    let value = string(args, key)?;
    revision(&value).map_err(|_| ExecError::invalid_args(format!("`{key}` must be a revision")))?;
    Ok(value)
}

fn branch_arg(args: &Value, key: &str) -> ExecResult<Option<String>> {
    let value = opt_string(args, key)?;
    if let Some(v) = &value {
        branch(v).map_err(|_| ExecError::invalid_args(format!("`{key}` must be a branch name")))?;
    }
    Ok(value)
}

fn remote_arg(args: &Value) -> ExecResult<Option<String>> {
    let value = opt_string(args, "remote")?;
    if let Some(v) = &value {
        if v.is_empty() || v.starts_with('-') || !v.chars().all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c)) {
            return Err(ExecError::invalid_args("`remote` must be the name of a configured remote"));
        }
    }
    Ok(value)
}

/// What a fetch may bring in beyond the remote's branches: one of a pull request's head refs or a branch of the remote, into a ref of the
/// repository's own `refs/rca/` namespace, so it never moves a branch, a tag or the checkout.
#[derive(Debug, Clone, PartialEq)]
pub struct Refspec {
    pub source: String,
    pub destination: String,
}

impl Refspec {
    /// The argument `git fetch` is given: a forced update of the destination.
    pub fn arg(&self) -> String {
        format!("+{}:{}", self.source, self.destination)
    }
}

fn refspec(value: &str) -> ExecResult<Refspec> {
    let bad = || ExecError::invalid_args("`refspecs` must be `+<source>:refs/rca/<name>`, the source a pull request's head or `refs/heads/<branch>`");
    let (source, destination) = value.strip_prefix('+').unwrap_or(value).split_once(':').ok_or_else(bad)?;
    let number = |n: &str| !n.is_empty() && n.len() <= 10 && n.bytes().all(|b| b.is_ascii_digit());
    let source_ok = match source.strip_prefix("refs/pull/").or_else(|| source.strip_prefix("refs/merge-requests/")) {
        Some(rest) => rest.strip_suffix("/head").is_some_and(number),
        None => source.strip_prefix("refs/heads/").is_some_and(|b| crate::branch(b).is_ok()),
    };
    let destination_ok = destination.strip_prefix("refs/rca/").is_some_and(|rest| {
        let parts: Vec<&str> = rest.split('/').collect();
        parts.len() <= 8 && parts.iter().all(|p| !p.is_empty() && *p != "." && *p != ".." && !p.ends_with(".lock") && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')))
    });
    if source_ok && destination_ok {
        Ok(Refspec { source: source.to_string(), destination: destination.to_string() })
    } else {
        Err(bad())
    }
}

fn refspecs(args: &Value) -> ExecResult<Vec<Refspec>> {
    let Some(list) = args.get("refspecs").filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let list = list.as_array().filter(|l| l.len() <= 8).ok_or_else(|| ExecError::invalid_args("`refspecs` must be an array of at most 8 strings"))?;
    list.iter().map(|v| v.as_str().ok_or_else(|| ExecError::invalid_args("`refspecs` must be an array of strings")).and_then(refspec)).collect()
}

fn limit(args: &Value, default: usize, max: usize) -> ExecResult<usize> {
    let n = opt_uint(args, "limit")?.map_or(default, |n| n as usize);
    if n == 0 || n > max {
        return Err(ExecError::invalid_args(format!("`limit` must be between 1 and {max}")));
    }
    Ok(n)
}

fn choice<T: Copy>(args: &Value, key: &str, default: T, names: &[(&str, T)]) -> ExecResult<T> {
    match opt_string(args, key)? {
        None => Ok(default),
        Some(v) => names.iter().find(|(n, _)| *n == v).map(|(_, t)| *t).ok_or_else(|| ExecError::invalid_args(format!("`{key}` must be one of {}", names.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")))),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Branch,
    Remote,
    Tag,
}

impl RefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RefKind::Branch => "branch",
            RefKind::Remote => "remote",
            RefKind::Tag => "tag",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardScope {
    /// Restore the working tree from the index.
    Worktree,
    /// Restore index and working tree.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullStrategy {
    FfOnly,
    Merge,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    Default,
    FfOnly,
    NoFf,
    Squash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Start,
    Abort,
    Continue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Ours,
    Theirs,
    /// The file already holds the resolution: mark it resolved.
    Resolved,
    /// Write `content`, then mark it resolved.
    Content,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GitCall {
    Status,
    Refs { kind: Option<RefKind>, query: Option<String>, limit: usize },
    Log { rev: Option<String>, path: Option<String>, limit: usize, skip: usize },
    Compare { base: String, head: String, limit: usize },
    Upstream,
    Stage { paths: Vec<String> },
    Unstage { paths: Vec<String> },
    Discard { paths: Vec<String>, scope: DiscardScope, untracked: bool, source: Option<String> },
    Commit { message: String, amend: bool, all: bool, allow_empty: bool },
    Checkout { target: String, detach: bool, track: bool, force: bool },
    CreateRef { name: String, kind: RefKind, from: Option<String>, checkout: bool, message: Option<String> },
    Fetch { remote: Option<String>, prune: bool, refspecs: Vec<Refspec> },
    Pull { remote: Option<String>, branch: Option<String>, strategy: PullStrategy },
    Push { remote: Option<String>, branch: Option<String>, set_upstream: bool, force_with_lease: bool },
    FastForward { branch: Option<String>, to: Option<String> },
    Merge { step: Step, target: Option<String>, strategy: MergeStrategy, message: Option<String> },
    Rebase { step: Step, onto: Option<String> },
    ConflictResolve { path: String, resolution: Resolution, content: Option<String> },
    Blob { rev: String, path: String, max_bytes: u64 },
    Watch { interval_ms: u64, max_events: Option<u64> },
    Init { path: String, initial_branch: Option<String>, files: Vec<InitFile>, message: String },
    Checkpoint { thread: String, turn: String },
}

/// A first file of `git_init`: `path` is inside the new repository.
#[derive(Debug, Clone, PartialEq)]
pub struct InitFile {
    pub path: String,
    pub content: String,
}

fn init_files(args: &Value) -> ExecResult<Vec<InitFile>> {
    let Some(list) = args.get("files").filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let bad = || ExecError::invalid_args("`files` must be an array of objects with a `path` and a `content`");
    let list = list.as_array().ok_or_else(bad)?;
    if list.len() > INIT_FILES_MAX {
        return Err(ExecError::invalid_args(format!("`files` holds at most {INIT_FILES_MAX} files")));
    }
    let mut files: Vec<InitFile> = Vec::new();
    for item in list {
        let path = tree_path(item.get("path").and_then(Value::as_str).ok_or_else(bad)?)?;
        let content = item.get("content").and_then(Value::as_str).ok_or_else(bad)?;
        if path == "." || content.len() > INIT_FILE_MAX {
            return Err(ExecError::invalid_args(format!("each of `files` must name a file of at most {INIT_FILE_MAX} bytes")));
        }
        if files.iter().any(|f| f.path == path) {
            return Err(ExecError::invalid_args(format!("`files` names {path} twice")));
        }
        files.push(InitFile { path, content: content.to_string() });
    }
    Ok(files)
}

/// One validated request of the repository actions.
#[derive(Debug, Clone, PartialEq)]
pub struct GitOp {
    pub worktree: Option<String>,
    pub call: GitCall,
}

impl GitOp {
    /// `None` when `tool` is not one of the repository actions.
    pub fn parse(tool: &str, args: &Value) -> Option<ExecResult<GitOp>> {
        let make: fn(&Value) -> ExecResult<GitCall> = match tool {
            crate::GIT_STATUS => |_| Ok(GitCall::Status),
            crate::GIT_REFS => |a| {
                let kind = choice(a, "kind", None, &[("branch", Some(RefKind::Branch)), ("remote", Some(RefKind::Remote)), ("tag", Some(RefKind::Tag))])?;
                Ok(GitCall::Refs { kind, query: opt_string(a, "query")?, limit: limit(a, REFS_DEFAULT, REFS_MAX)? })
            },
            crate::GIT_LOG => |a| {
                let path = opt_string(a, "path")?.map(|p| tree_path(&p)).transpose()?;
                Ok(GitCall::Log { rev: rev(a, "rev")?, path, limit: limit(a, LOG_DEFAULT, LOG_MAX)?, skip: opt_uint(a, "skip")?.unwrap_or(0) as usize })
            },
            crate::GIT_COMPARE => |a| Ok(GitCall::Compare { base: required_rev(a, "base")?, head: required_rev(a, "head")?, limit: limit(a, LOG_DEFAULT, LOG_MAX)? }),
            crate::GIT_UPSTREAM => |_| Ok(GitCall::Upstream),
            crate::GIT_STAGE => |a| Ok(GitCall::Stage { paths: some_paths(a)? }),
            crate::GIT_UNSTAGE => |a| Ok(GitCall::Unstage { paths: some_paths(a)? }),
            crate::GIT_DISCARD => |a| {
                let paths = some_paths(a)?;
                Ok(GitCall::Discard { paths, scope: choice(a, "scope", DiscardScope::Worktree, &[("worktree", DiscardScope::Worktree), ("all", DiscardScope::All)])?, untracked: flag(a, "untracked"), source: rev(a, "source")? })
            },
            crate::GIT_COMMIT => |a| {
                let message = string(a, "message")?;
                if message.trim().is_empty() || message.contains('\0') {
                    return Err(ExecError::invalid_args("`message` must not be empty"));
                }
                Ok(GitCall::Commit { message, amend: flag(a, "amend"), all: flag(a, "all"), allow_empty: flag(a, "allow_empty") })
            },
            crate::GIT_CHECKOUT => |a| {
                let (detach, track) = (flag(a, "detach"), flag(a, "track"));
                if detach && track {
                    return Err(ExecError::invalid_args("`detach` and `track` exclude each other"));
                }
                Ok(GitCall::Checkout { target: required_rev(a, "ref")?, detach, track, force: flag(a, "force") })
            },
            crate::GIT_CREATE_REF => |a| {
                let name = string(a, "name")?;
                branch(&name).map_err(|_| ExecError::invalid_args("`name` must be a branch or tag name"))?;
                let kind = choice(a, "kind", RefKind::Branch, &[("branch", RefKind::Branch), ("tag", RefKind::Tag)])?;
                let message = opt_string(a, "message")?;
                if message.as_deref().is_some_and(|m| m.trim().is_empty() || m.contains('\0')) || (message.is_some() && kind != RefKind::Tag) {
                    return Err(ExecError::invalid_args("`message` is the annotation of a tag and must not be empty"));
                }
                Ok(GitCall::CreateRef { name, kind, from: rev(a, "from")?, checkout: flag(a, "checkout"), message })
            },
            crate::GIT_FETCH => |a| Ok(GitCall::Fetch { remote: remote_arg(a)?, prune: flag(a, "prune"), refspecs: refspecs(a)? }),
            crate::GIT_PULL => |a| Ok(GitCall::Pull { remote: remote_arg(a)?, branch: branch_arg(a, "branch")?, strategy: choice(a, "strategy", PullStrategy::FfOnly, &[("ff_only", PullStrategy::FfOnly), ("merge", PullStrategy::Merge), ("rebase", PullStrategy::Rebase)])? }),
            crate::GIT_PUSH => |a| Ok(GitCall::Push { remote: remote_arg(a)?, branch: branch_arg(a, "branch")?, set_upstream: flag(a, "set_upstream"), force_with_lease: flag(a, "force_with_lease") }),
            crate::GIT_FAST_FORWARD => |a| Ok(GitCall::FastForward { branch: branch_arg(a, "branch")?, to: rev(a, "to")? }),
            crate::GIT_MERGE => |a| {
                let step = step(a)?;
                let target = rev(a, "ref")?;
                if step == Step::Start && target.is_none() {
                    return Err(ExecError::invalid_args("`ref` is required to start a merge"));
                }
                let strategy = choice(a, "strategy", MergeStrategy::Default, &[("default", MergeStrategy::Default), ("ff_only", MergeStrategy::FfOnly), ("no_ff", MergeStrategy::NoFf), ("squash", MergeStrategy::Squash)])?;
                let message = opt_string(a, "message")?.filter(|m| !m.trim().is_empty() && !m.contains('\0'));
                Ok(GitCall::Merge { step, target, strategy, message })
            },
            crate::GIT_REBASE => |a| {
                let step = step(a)?;
                let onto = rev(a, "ref")?;
                if step == Step::Start && onto.is_none() {
                    return Err(ExecError::invalid_args("`ref` is required to start a rebase"));
                }
                Ok(GitCall::Rebase { step, onto })
            },
            crate::GIT_CONFLICT_RESOLVE => |a| {
                let path = tree_path(&string(a, "path")?)?;
                let resolution = choice(a, "resolution", None, &[("ours", Some(Resolution::Ours)), ("theirs", Some(Resolution::Theirs)), ("resolved", Some(Resolution::Resolved)), ("content", Some(Resolution::Content))])?.ok_or_else(|| ExecError::invalid_args("`resolution` is required: ours, theirs, resolved or content"))?;
                let content = match a.get("content") {
                    Some(Value::String(c)) => Some(c.clone()),
                    None | Some(Value::Null) => None,
                    Some(_) => return Err(ExecError::invalid_args("`content` must be a string")),
                };
                if (resolution == Resolution::Content) != content.is_some() {
                    return Err(ExecError::invalid_args("`content` is given with resolution `content` and only then"));
                }
                Ok(GitCall::ConflictResolve { path, resolution, content })
            },
            crate::GIT_BLOB => |a| {
                let max_bytes = opt_uint(a, "max_bytes")?.unwrap_or(BLOB_DEFAULT);
                if max_bytes == 0 || max_bytes > BLOB_MAX {
                    return Err(ExecError::invalid_args(format!("`max_bytes` must be between 1 and {BLOB_MAX}")));
                }
                let path = tree_path(&string(a, "path")?)?;
                if path == "." {
                    return Err(ExecError::invalid_args("`path` must name a file"));
                }
                Ok(GitCall::Blob { rev: required_rev(a, "rev")?, path, max_bytes })
            },
            crate::GIT_WATCH => |a| {
                let interval_ms = opt_uint(a, "interval_ms")?.unwrap_or(WATCH_DEFAULT_MS);
                if !(WATCH_MIN_MS..=WATCH_MAX_MS).contains(&interval_ms) {
                    return Err(ExecError::invalid_args(format!("`interval_ms` must be between {WATCH_MIN_MS} and {WATCH_MAX_MS}")));
                }
                let max_events = opt_uint(a, "max_events")?;
                if max_events == Some(0) {
                    return Err(ExecError::invalid_args("`max_events` must be at least 1"));
                }
                Ok(GitCall::Watch { interval_ms, max_events })
            },
            crate::GIT_INIT => |a| {
                if a.get("worktree").is_some_and(|w| !w.is_null()) {
                    return Err(ExecError::invalid_args("`worktree` is no argument of git_init: it makes a repository at `path`"));
                }
                let path = string(a, "path")?;
                if path.is_empty() || path.contains('\0') {
                    return Err(ExecError::invalid_args("`path` must name a directory"));
                }
                let message = opt_string(a, "message")?.unwrap_or_else(|| INIT_MESSAGE.to_string());
                if message.trim().is_empty() || message.contains('\0') {
                    return Err(ExecError::invalid_args("`message` must not be empty"));
                }
                Ok(GitCall::Init { path, initial_branch: branch_arg(a, "initial_branch")?, files: init_files(a)?, message })
            },
            crate::GIT_CHECKPOINT => |a| {
                let (thread, turn) = (string(a, "thread")?, string(a, "turn")?);
                crate::checkpoint_id("thread", &thread)?;
                crate::checkpoint_id("turn", &turn)?;
                Ok(GitCall::Checkpoint { thread, turn })
            },
            _ => return None,
        };
        Some(parse_target(args).and_then(|worktree| make(args).map(|call| GitOp { worktree, call })))
    }

    pub fn action(&self) -> &'static str {
        match self.call {
            GitCall::Status => crate::GIT_STATUS,
            GitCall::Refs { .. } => crate::GIT_REFS,
            GitCall::Log { .. } => crate::GIT_LOG,
            GitCall::Compare { .. } => crate::GIT_COMPARE,
            GitCall::Upstream => crate::GIT_UPSTREAM,
            GitCall::Stage { .. } => crate::GIT_STAGE,
            GitCall::Unstage { .. } => crate::GIT_UNSTAGE,
            GitCall::Discard { .. } => crate::GIT_DISCARD,
            GitCall::Commit { .. } => crate::GIT_COMMIT,
            GitCall::Checkout { .. } => crate::GIT_CHECKOUT,
            GitCall::CreateRef { .. } => crate::GIT_CREATE_REF,
            GitCall::Fetch { .. } => crate::GIT_FETCH,
            GitCall::Pull { .. } => crate::GIT_PULL,
            GitCall::Push { .. } => crate::GIT_PUSH,
            GitCall::FastForward { .. } => crate::GIT_FAST_FORWARD,
            GitCall::Merge { .. } => crate::GIT_MERGE,
            GitCall::Rebase { .. } => crate::GIT_REBASE,
            GitCall::ConflictResolve { .. } => crate::GIT_CONFLICT_RESOLVE,
            GitCall::Blob { .. } => crate::GIT_BLOB,
            GitCall::Watch { .. } => crate::GIT_WATCH,
            GitCall::Init { .. } => crate::GIT_INIT,
            GitCall::Checkpoint { .. } => crate::GIT_CHECKPOINT,
        }
    }
}

fn parse_target(args: &Value) -> ExecResult<Option<String>> {
    let worktree = opt_string(args, "worktree")?;
    if let Some(name) = &worktree {
        worktree_name(name)?;
    }
    Ok(worktree)
}

/// At least one path; `.` is the whole tree, so no request means "everything" by omission.
fn some_paths(args: &Value) -> ExecResult<Vec<String>> {
    let paths = paths(args)?;
    if paths.is_empty() {
        return Err(ExecError::invalid_args("`paths` must name at least one path (`.` is the whole tree)"));
    }
    Ok(paths)
}

fn step(args: &Value) -> ExecResult<Step> {
    choice(args, "op", None, &[("start", Some(Step::Start)), ("abort", Some(Step::Abort)), ("continue", Some(Step::Continue))])?.ok_or_else(|| ExecError::invalid_args("`op` is required: start, abort or continue"))
}

/// One entry of a status. `index` and `worktree` are git's two-letter status columns (`M`, `A`, `D`, `R`, `C`, `T`, `U`, `.` for none; both `?` for an
/// untracked file); a conflict has `conflicted` set and the unmerged pair in the columns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orig_path: Option<String>,
    pub index: String,
    pub worktree: String,
    pub conflicted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitStatus {
    /// Absent for a detached head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Absent before the first commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    pub detached: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// `merge`, `rebase`, `cherry-pick` or `revert` while one is stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub files: Vec<StatusFile>,
    pub truncated: bool,
    pub clean: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefInfo {
    /// The short name (`main`, `origin/main`, `v1`).
    pub name: String,
    pub full: String,
    pub kind: String,
    pub sha: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefsResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    pub detached: bool,
    pub refs: Vec<RefInfo>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitInfo {
    pub sha: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author_name: String,
    pub author_email: String,
    pub authored_ms: i64,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogResult {
    pub commits: Vec<CommitInfo>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orig_path: Option<String>,
    /// `added`, `modified`, `deleted`, `renamed`, `copied` or `type_changed`.
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u32>,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompareResult {
    pub base: String,
    pub head: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_base: Option<String>,
    /// Commits in `head` that `base` lacks, and the other way round.
    pub ahead: u32,
    pub behind: u32,
    pub commits: Vec<CommitInfo>,
    /// The files `head` changed since the merge base (against `base` itself when the two share no history).
    pub files: Vec<DiffFile>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpstreamStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub has_upstream: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// The names of the repository's configured remotes.
    pub remotes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathsDone {
    pub paths: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscardDone {
    /// Tracked files restored or removed, and untracked paths cleaned.
    pub restored: usize,
    pub cleaned: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitDone {
    pub sha: String,
    pub short: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeadMoved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub head: String,
    pub detached: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RefCreated {
    pub name: String,
    pub kind: String,
    pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fetched {
    pub remote: String,
    pub output: String,
    /// The commit each requested destination holds after the fetch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<FetchedRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FetchedRef {
    #[serde(rename = "ref")]
    pub name: String,
    pub sha: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pushed {
    pub remote: String,
    pub branch: String,
    pub set_upstream: bool,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FastForwarded {
    pub branch: String,
    pub from: String,
    pub head: String,
    pub updated: bool,
}

/// What a pull, merge, rebase (or their abort and continue) left: `ok` is false when it stopped on conflicts, which are then `conflicts` and
/// `operation` says what waits for `git_conflict_resolve` and a continue or abort. Any other failure is an error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Integrated {
    pub ok: bool,
    pub head: String,
    pub updated: bool,
    pub conflicted: bool,
    pub conflicts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Resolved {
    pub path: String,
    /// The paths that still conflict.
    pub remaining: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlobResult {
    pub rev: String,
    pub path: String,
    pub size: u64,
    /// The bytes, base64 (the first `max_bytes` of them).
    pub data: String,
    pub truncated: bool,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watched {
    pub events: u64,
    pub cancelled: bool,
}

/// The result of `git_init`: the repository's directory as the request named it, its first branch, and the first commit when there was one.
/// `commit_error` says why files were written but not committed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitDone {
    pub path: String,
    pub branch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_error: Option<String>,
}

/// The result of `git_checkpoint`: the ref the checkpoint is kept under, its commit, and the commit HEAD was at (none on an unborn branch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Checkpointed {
    #[serde(rename = "ref")]
    pub name: String,
    pub commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
}

/// An event of `git_watch`: the status, whenever it differs from the last one sent (and once at the start). `seq` of the frame counts them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum GitEvent {
    #[serde(rename = "git.status")]
    Status { status: GitStatus },
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_proto::Code;
    use serde_json::json;

    fn parse(tool: &str, args: Value) -> ExecResult<GitOp> {
        GitOp::parse(tool, &args).expect("a repository action")
    }

    #[test]
    fn given_every_name_of_the_family_when_parsed_then_the_action_it_names_comes_back_and_a_diff_or_pty_name_is_none() {
        // Given the family's actions with the arguments each requires
        for action in crate::ACTIONS.iter().filter(|a| GitOp::parse(a.name, &json!({})).is_some()) {
            let mut args = serde_json::Map::new();
            for key in action.required {
                args.insert(key.to_string(), match *key { "op" => json!("abort"), "resolution" => json!("resolved"), "paths" => json!(["a"]), _ => json!("v") });
            }
            // When parsed, Then the op names the same action
            assert_eq!(parse(action.name, Value::Object(args)).unwrap().action(), action.name);
        }
        assert!(GitOp::parse(crate::GIT_DIFF, &json!({})).is_none());
        assert!(GitOp::parse("pty_open", &json!({})).is_none());
    }

    #[test]
    fn given_the_arguments_of_a_repository_init_when_parsed_then_the_files_are_checked_the_message_defaults_and_a_worktree_is_refused() {
        // Given an init that names only its directory, and one with first files, a branch and a message
        let bare = parse(crate::GIT_INIT, json!({"path": "p"})).unwrap();
        let full = parse(crate::GIT_INIT, json!({"path": "p/q", "initial_branch": "trunk", "message": "first", "files": [{"path": "./README.md", "content": "hi"}, {"path": "assets/icon.svg", "content": "<svg/>"}]})).unwrap();
        // Then the bare one has no files and the default message, and the full one keeps its files with their paths normalised
        assert_eq!(bare.call, GitCall::Init { path: "p".into(), initial_branch: None, files: vec![], message: INIT_MESSAGE.into() });
        let GitCall::Init { files, initial_branch, message, .. } = full.call else { panic!("an init") };
        assert_eq!((files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), initial_branch.as_deref(), message.as_str()), (vec!["README.md", "assets/icon.svg"], Some("trunk"), "first"));
        // And each argument that could make git write elsewhere or read as an option is refused with the key that holds it
        let refused = |args: Value| parse(crate::GIT_INIT, args).unwrap_err();
        for (args, key) in [
            (json!({"path": "p", "files": [{"path": "../x", "content": ""}]}), "`path`"),
            (json!({"path": "p", "files": [{"path": ".git/hooks/pre-commit", "content": ""}]}), "`path`"),
            (json!({"path": "p", "files": [{"path": "a", "content": ""}, {"path": "./a", "content": ""}]}), "twice"),
            (json!({"path": "p", "files": [{"path": "a"}]}), "`files`"),
            (json!({"path": "p", "files": [{"path": "a", "content": "x".repeat(INIT_FILE_MAX + 1)}]}), "at most"),
            (json!({"path": "p", "files": vec![json!({"path": "a", "content": ""}); INIT_FILES_MAX + 1]}), "at most"),
            (json!({"path": "p", "initial_branch": "-x"}), "`initial_branch`"),
            (json!({"path": "p", "message": "  "}), "`message`"),
            (json!({"path": "p", "worktree": "wt"}), "`worktree`"),
            (json!({"path": ""}), "`path`"),
        ] {
            let e = refused(args.clone());
            assert!(e.code == Code::InvalidArgs && e.message.contains(key), "{args}: {e:?}");
        }
        // And the result leaves a commit that did not happen out
        assert_eq!(serde_json::to_string(&InitDone { path: "p".into(), branch: "main".into(), commit: None, commit_error: Some("no identity".into()) }).unwrap(), r#"{"path":"p","branch":"main","commit_error":"no identity"}"#);
    }

    #[test]
    fn given_paths_that_climb_out_name_git_internals_or_are_absolute_when_parsed_then_each_is_refused_and_plain_ones_are_normalised() {
        // Given paths of a tree and ones that leave it
        let plain = [("a.txt", "a.txt"), ("./a/./b.txt", "a/b.txt"), ("dir//f", "dir/f"), (".", "."), ("-rf", "-rf"), (":(top)x", ":(top)x"), ("a b/c", "a b/c")];
        let bad = ["", "..", "../x", "a/../../x", "/etc/passwd", ".git/config", "a/.git/hooks/x", ".GIT/x", "a\0b"];
        // When each is staged
        for (given, want) in plain {
            let op = parse(crate::GIT_STAGE, json!({"paths": [given]})).unwrap();
            // Then the plain ones are kept (a magic-looking one is a literal name: the executor reads every pathspec literally)
            assert_eq!(op.call, GitCall::Stage { paths: vec![want.to_string()] });
        }
        for given in bad {
            let e = parse(crate::GIT_STAGE, json!({"paths": [given]})).unwrap_err();
            assert!(e.code == Code::InvalidArgs && e.message.contains("`path`"), "{given:?}: {e:?}");
        }
    }

    #[test]
    fn given_stage_unstage_or_discard_arguments_when_parsed_then_at_least_one_path_is_required_and_the_whole_tree_is_a_dot() {
        // Given no paths, an empty list, and a list
        // Then none of the three actions means "everything" by omission, and a dot is how the tree is named
        for tool in [crate::GIT_STAGE, crate::GIT_UNSTAGE, crate::GIT_DISCARD] {
            assert!(parse(tool, json!({})).unwrap_err().message.contains("`paths`"), "{tool}");
            assert!(parse(tool, json!({"paths": []})).unwrap_err().message.contains("`paths`"), "{tool}");
        }
        assert_eq!(parse(crate::GIT_STAGE, json!({"paths": ["."]})).unwrap().call, GitCall::Stage { paths: vec![".".into()] });
        assert_eq!(parse(crate::GIT_UNSTAGE, json!({"paths": ["a", "b/c"]})).unwrap().call, GitCall::Unstage { paths: vec!["a".into(), "b/c".into()] });
    }

    #[test]
    fn given_refspecs_when_parsed_then_only_a_pull_requests_head_or_a_branch_into_the_rca_namespace_is_accepted() {
        // Given the refspecs a pull request read needs, and ones that would move a branch, a tag or run a helper
        let fetch = |spec: &str| parse(crate::GIT_FETCH, json!({"refspecs": [spec]}));
        for good in ["+refs/pull/12/head:refs/rca/pr/12/head", "refs/merge-requests/4/head:refs/rca/pr/4/head", "+refs/heads/main:refs/rca/pr/12/base", "+refs/heads/feature/x-1:refs/rca/a.b_c"] {
            // When parsed, Then each is a forced update of its destination
            let GitCall::Fetch { refspecs, .. } = fetch(good).unwrap().call else { panic!("{good} is no fetch") };
            assert_eq!(refspecs[0].arg(), format!("+{}", good.trim_start_matches('+')));
        }
        for bad in ["+refs/heads/main:refs/heads/main", "+refs/pull/12/head:refs/tags/t", "+refs/pull/x/head:refs/rca/a", "+refs/pull//head:refs/rca/a", "+refs/pull/12/head:refs/rca/../x", "+refs/pull/12/head:refs/rca/a:b", "+refs/pull/12/head:refs/rca/", "+refs/pull/12/head:refs/rca/a.lock", "+refs/tags/v1:refs/rca/t", "--upload-pack=x", "+:refs/rca/a", "+refs/heads/-x:refs/rca/a", "refs/pull/12/head", ""] {
            // Then each is refused naming `refspecs`
            let e = fetch(bad).unwrap_err();
            assert!(e.code == Code::InvalidArgs && e.message.contains("`refspecs`"), "{bad:?}: {e:?}");
        }
        assert!(parse(crate::GIT_FETCH, json!({"refspecs": vec!["+refs/pull/1/head:refs/rca/a"; 9]})).unwrap_err().message.contains("at most 8"));
        assert!(matches!(parse(crate::GIT_FETCH, json!({})).unwrap().call, GitCall::Fetch { refspecs, .. } if refspecs.is_empty()));
    }

    #[test]
    fn given_revisions_and_names_that_git_could_read_as_options_when_parsed_then_the_key_that_holds_them_is_named() {
        // Given one request per action that takes a revision, a branch or a remote, each carrying an option as its value
        let cases = [
            (crate::GIT_LOG, "rev"),
            (crate::GIT_COMPARE, "base"),
            (crate::GIT_COMPARE, "head"),
            (crate::GIT_DISCARD, "source"),
            (crate::GIT_CHECKOUT, "ref"),
            (crate::GIT_CREATE_REF, "from"),
            (crate::GIT_CREATE_REF, "name"),
            (crate::GIT_PULL, "branch"),
            (crate::GIT_PULL, "remote"),
            (crate::GIT_PUSH, "branch"),
            (crate::GIT_PUSH, "remote"),
            (crate::GIT_FETCH, "remote"),
            (crate::GIT_FETCH, "refspecs"),
            (crate::GIT_FAST_FORWARD, "to"),
            (crate::GIT_FAST_FORWARD, "branch"),
            (crate::GIT_MERGE, "ref"),
            (crate::GIT_REBASE, "ref"),
            (crate::GIT_BLOB, "rev"),
        ];
        for (tool, key) in cases {
            for value in ["--upload-pack=x", "-b", "a b", "$(x)", "a;b"] {
                // When it is parsed
                let mut args = json!({"paths": ["a"], "base": "main", "head": "main", "ref": "main", "name": "n", "rev": "HEAD", "path": "a", "op": "start"});
                args[key] = json!(value);
                // Then it is INVALID_ARGS naming that key, whatever else is valid
                let e = parse(tool, args).unwrap_err();
                assert!(e.code == Code::InvalidArgs && e.message.contains(&format!("`{key}`")), "{tool} {key}={value:?}: {e:?}");
            }
        }
    }

    #[test]
    fn given_the_arguments_of_a_merge_or_rebase_when_parsed_then_abort_and_continue_need_no_ref_and_start_does() {
        // Given each step of each
        for tool in [crate::GIT_MERGE, crate::GIT_REBASE] {
            // When parsed, Then a start without a ref is refused, abort and continue are not, and a missing or unknown step names `op`
            assert!(parse(tool, json!({"op": "start"})).unwrap_err().message.contains("`ref`"));
            assert!(parse(tool, json!({})).unwrap_err().message.contains("`op`"));
            assert!(matches!(parse(tool, json!({"op": "abort"})).unwrap().call, GitCall::Merge { step: Step::Abort, .. } | GitCall::Rebase { step: Step::Abort, .. }));
            assert!(parse(tool, json!({"op": "continue"})).is_ok());
            assert!(parse(tool, json!({"op": "skip"})).unwrap_err().message.contains("`op`"));
        }
    }

    #[test]
    fn given_conflict_resolution_arguments_when_parsed_then_content_goes_with_the_content_resolution_only() {
        // Given each resolution
        let ok = |a: Value| parse(crate::GIT_CONFLICT_RESOLVE, a);
        // Then content pairs only with `content`, and the default marks the file resolved
        assert!(ok(json!({"path": "a", "resolution": "content", "content": "x"})).is_ok());
        assert!(ok(json!({"path": "a", "resolution": "content"})).is_err());
        assert!(ok(json!({"path": "a", "resolution": "ours", "content": "x"})).is_err());
        assert!(matches!(ok(json!({"path": "a", "resolution": "resolved"})).unwrap().call, GitCall::ConflictResolve { resolution: Resolution::Resolved, .. }));
        assert!(ok(json!({"path": "a"})).unwrap_err().message.contains("`resolution`"));
        assert!(ok(json!({"path": "a", "resolution": "both"})).unwrap_err().message.contains("`resolution`"));
    }

    #[test]
    fn given_limits_intervals_and_a_worktree_when_parsed_then_out_of_range_values_and_paths_for_names_are_refused() {
        // Given requests with limits and intervals at and past their bounds
        assert!(parse(crate::GIT_LOG, json!({"limit": LOG_MAX})).is_ok());
        for bad in [0, LOG_MAX + 1] {
            assert!(parse(crate::GIT_LOG, json!({"limit": bad})).unwrap_err().message.contains("`limit`"));
        }
        assert!(parse(crate::GIT_WATCH, json!({"interval_ms": WATCH_MIN_MS - 1})).is_err());
        assert!(parse(crate::GIT_WATCH, json!({"max_events": 0})).is_err());
        assert!(parse(crate::GIT_BLOB, json!({"rev": "HEAD", "path": "."})).is_err());
        assert!(parse(crate::GIT_BLOB, json!({"rev": "HEAD", "path": "a", "max_bytes": BLOB_MAX + 1})).is_err());
        // And a worktree is a plain name, for every action
        assert_eq!(parse(crate::GIT_STATUS, json!({"worktree": "wt"})).unwrap().worktree.as_deref(), Some("wt"));
        assert!(parse(crate::GIT_STATUS, json!({"worktree": "../x"})).unwrap_err().message.contains("`name`"));
        // And a tag annotation goes with a tag only
        assert!(parse(crate::GIT_CREATE_REF, json!({"name": "v1", "kind": "tag", "message": "m"})).is_ok());
        assert!(parse(crate::GIT_CREATE_REF, json!({"name": "b", "message": "m"})).is_err());
    }

    #[test]
    fn given_a_status_and_an_event_when_encoded_then_absent_values_are_left_out_and_the_event_kind_is_its_dotted_name() {
        // Given a clean status of a detached head and the event that carries it
        let status = GitStatus { branch: None, head: Some("abc".into()), detached: true, upstream: None, ahead: 0, behind: 0, operation: None, files: vec![], truncated: false, clean: true };
        let event = exec_proto::to_value(&GitEvent::Status { status: status.clone() }).to_string();
        // Then the keys are the contract's
        assert_eq!(event, r#"{"kind":"git.status","status":{"head":"abc","detached":true,"ahead":0,"behind":0,"files":[],"truncated":false,"clean":true}}"#);
    }

    #[test]
    fn given_the_ids_of_a_checkpoint_when_parsed_then_only_one_ref_component_each_is_taken_and_the_ref_is_built_from_them() {
        // Given a thread and a turn that are plain ids, and ones that could leave the ref, be read as an option or collide with a lock file
        let plain = parse(crate::GIT_CHECKPOINT, json!({"thread": "thread-1.a", "turn": "u_2"})).unwrap();
        // Then the call keeps them and the ref is under the checkpoint namespace
        assert_eq!(plain.call, GitCall::Checkpoint { thread: "thread-1.a".into(), turn: "u_2".into() });
        assert_eq!(crate::checkpoint_ref("thread-1.a", "u_2"), "refs/rca/checkpoints/thread-1.a/u_2");
        for bad in ["", "a/b", "..", "a..b", "-x", ".hidden", "x.lock", "a b", "a\u{0}b", &"x".repeat(129)] {
            for key in ["thread", "turn"] {
                let mut args = json!({"thread": "t", "turn": "u"});
                args[key] = json!(bad);
                assert_eq!(parse(crate::GIT_CHECKPOINT, args).unwrap_err().code, Code::InvalidArgs, "{key} {bad:?}");
            }
        }
    }
}
