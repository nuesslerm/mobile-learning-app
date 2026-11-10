//! The hosted source control actions of the executor contract: pull and merge requests, runs, sign-in, clone and publish on GitHub or GitLab,
//! done by the executor machine's own `gh` and `glab` with the credentials that machine already holds (the service never holds a token).
//! The set is closed: a request names one action and typed arguments, and this crate validates every one so the executor never hands a CLI a
//! value it could read as an option, a path outside the workspace or shell syntax. Results are neutral: snake_case fields, times in
//! milliseconds, the same for both providers (`pr_*` serves GitHub pull requests and GitLab merge requests alike).
use exec_proto::args::{flag, opt_string, opt_uint, string};
use exec_proto::{Action, ExecError, ExecResult, Property};
use serde_json::Value;

#[cfg(feature = "testing")]
pub mod testing;

pub const REPO_VIEW: &str = "hosted_repo_view";
pub const REPO_LABELS: &str = "hosted_repo_labels";
pub const REPO_REVIEWERS: &str = "hosted_repo_reviewers";
pub const REPO_CLONE: &str = "hosted_repo_clone";
pub const REPO_PUBLISH: &str = "hosted_repo_publish";
pub const PR_LIST: &str = "hosted_pr_list";
pub const PR_VIEW: &str = "hosted_pr_view";
pub const PR_DIFF: &str = "hosted_pr_diff";
pub const PR_CHECKS: &str = "hosted_pr_checks";
pub const PR_COMMENTS: &str = "hosted_pr_comments";
pub const PR_REVIEWS: &str = "hosted_pr_reviews";
pub const PR_THREADS: &str = "hosted_pr_threads";
pub const PR_FILES: &str = "hosted_pr_files";
pub const PR_CREATE: &str = "hosted_pr_create";
pub const PR_EDIT: &str = "hosted_pr_edit";
pub const PR_READY: &str = "hosted_pr_ready";
pub const PR_COMMENT: &str = "hosted_pr_comment";
pub const PR_REPLY: &str = "hosted_pr_reply";
pub const PR_REVIEW: &str = "hosted_pr_review";
pub const PR_REQUEST_REVIEWERS: &str = "hosted_pr_request_reviewers";
pub const PR_LABELS: &str = "hosted_pr_labels";
pub const PR_REACT: &str = "hosted_pr_react";
pub const PR_RESOLVE_THREAD: &str = "hosted_pr_resolve_thread";
pub const PR_UPDATE_BRANCH: &str = "hosted_pr_update_branch";
pub const PR_MERGE: &str = "hosted_pr_merge";
pub const PR_STACK: &str = "hosted_pr_stack";
pub const PR_COMMENT_EDIT: &str = "hosted_pr_comment_edit";
pub const RUN_LIST: &str = "hosted_run_list";
pub const RUN_VIEW: &str = "hosted_run_view";
pub const AUTH_STATUS: &str = "hosted_auth_status";
pub const AUTH_LOGIN_START: &str = "hosted_auth_login_start";
pub const AUTH_LOGIN_WAIT: &str = "hosted_auth_login_wait";
pub const AUTH_LOGIN_CANCEL: &str = "hosted_auth_login_cancel";

/// How the executor words the two refusals a caller can act on: a machine whose CLI is missing, and one whose CLI is not signed in.
pub const NOT_INSTALLED: &str = " is not installed on the executor machine";
pub const NOT_SIGNED_IN: &str = "not signed in to ";

/// The actions that take a thread of their own: a clone and a sign-in wait block until the CLI ends or a `cancel` names the request.
pub const LONG: &[&str] = &[REPO_CLONE, AUTH_LOGIN_WAIT];

pub const LIST_DEFAULT: u64 = 30;
pub const LIST_MAX: u64 = 200;
/// What a CLI may print before the executor stops reading and says the output was cut.
pub const OUTPUT_CAP: usize = 1 << 20;
pub const TIMEOUT_DEFAULT_MS: u64 = 60_000;
pub const TIMEOUT_MAX_MS: u64 = 600_000;
pub const CLONE_TIMEOUT_DEFAULT_MS: u64 = 300_000;
pub const LOGIN_PROMPT_TIMEOUT_MS: u64 = 30_000;
pub const LOGIN_WAIT_DEFAULT_MS: u64 = 30_000;
pub const LOGIN_WAIT_MAX_MS: u64 = 900_000;
pub const BODY_MAX: usize = 64 * 1024;
pub const TITLE_MAX: usize = 256;

/// The variables a hosted CLI inherits from the executor: its own configuration directories and what reaches the network, and no token
/// variable (a machine signed in by `GH_TOKEN` signs in by `gh auth login`, whose token lands in the CLI's config directory).
pub const INHERITED_ENV: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TMPDIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "GH_CONFIG_DIR", "GLAB_CONFIG_DIR", "HTTPS_PROXY",
    "HTTP_PROXY", "ALL_PROXY", "NO_PROXY", "https_proxy", "http_proxy", "all_proxy", "no_proxy", "SSL_CERT_FILE", "SSL_CERT_DIR",
];

const PROVIDER: Property = Property { name: "provider", kind: "string", about: "`github` (gh) or `gitlab` (glab)" };
const HOST: Property = Property { name: "host", kind: "string", about: "A self-hosted instance's host name (default the provider's public host)" };
const REPO: Property = Property { name: "repo", kind: "string", about: "`owner/name` (GitLab: `group/subgroup/name`); default the repository the workspace's remote names" };
const NUMBER: Property = Property { name: "number", kind: "integer", about: "The pull request (GitHub) or merge request (GitLab) number" };
const THREAD: Property = Property { name: "thread", kind: "string", about: "A review thread's id as `hosted_pr_threads` returned it" };
const BODY: Property = Property { name: "body", kind: "string", about: "Markdown text, at most 65536 bytes" };
const LIMIT: Property = Property { name: "limit", kind: "integer", about: "At most this many items (default 30, at most 200)" };
const TIMEOUT: Property = Property { name: "timeout_ms", kind: "integer", about: "Time the CLI may run (default 60000, at most 600000)" };

macro_rules! action {
    ($name:expr, $about:expr, [$($p:expr),* $(,)?], [$($r:expr),* $(,)?]) => {
        Action { name: $name, about: $about, properties: &[PROVIDER, HOST, REPO, TIMEOUT, $($p),*], required: &["provider", $($r),*] }
    };
}

const STATE: Property = Property { name: "state", kind: "string", about: "`open` (default), `closed`, `merged` or `all`" };
const ADD: Property = Property { name: "add", kind: "array", about: "Names to add" };
const REMOVE: Property = Property { name: "remove", kind: "array", about: "Names to remove" };

/// The family's actions, in the contract's order. Every one takes `provider`, `host`, `repo` and `timeout_ms`.
pub const ACTIONS: &[Action] = &[
    action!(REPO_VIEW, "The repository's name, url, default branch, visibility and the signed-in account's permission.", [], []),
    action!(REPO_LABELS, "The labels a repository defines.", [LIMIT], []),
    action!(REPO_REVIEWERS, "The accounts that may be asked to review (the repository's collaborators or members).", [LIMIT], []),
    action!(
        REPO_CLONE,
        "Clone a repository into a directory of the workspace root or worktree base; stays open until the clone ends or is cancelled.",
        [Property { name: "dest", kind: "string", about: "Directory to clone into, relative to the root (or absolute inside the root or worktree base); must not exist or be empty" }],
        ["repo", "dest"]
    ),
    action!(
        REPO_PUBLISH,
        "Create the repository on the provider from the git repository at `dir`, make it `origin` and push the current branch.",
        [
            Property { name: "dir", kind: "string", about: "The local repository, relative to the root (default the root)" },
            Property { name: "visibility", kind: "string", about: "`private` (default) or `public`" },
            Property { name: "description", kind: "string", about: "The repository's description, at most 1024 characters" },
        ],
        ["repo"]
    ),
    action!(
        PR_LIST,
        "Pull or merge requests, most recently updated first.",
        [STATE, Property { name: "author", kind: "string", about: "Only this account's" }, Property { name: "label", kind: "string", about: "Only with this label" }, Property { name: "base", kind: "string", about: "Only into this branch" }, Property { name: "head", kind: "string", about: "Only from this branch" }, Property { name: "search", kind: "string", about: "Only matching this text" }, LIMIT],
        []
    ),
    action!(PR_VIEW, "One pull or merge request with its description, mergeability, reviewers and size.", [NUMBER], ["number"]),
    action!(PR_DIFF, "The unified diff of a pull or merge request, cut at the output cap.", [NUMBER], ["number"]),
    action!(PR_CHECKS, "The checks (GitHub) or pipeline jobs (GitLab) of a pull or merge request.", [NUMBER], ["number"]),
    action!(PR_COMMENTS, "The conversation comments of a pull or merge request.", [NUMBER], ["number"]),
    action!(PR_REVIEWS, "The reviews (GitHub) or approvals (GitLab) of a pull or merge request.", [NUMBER], ["number"]),
    action!(PR_THREADS, "The review threads of a pull or merge request, each with its comments and whether it is resolved.", [NUMBER], ["number"]),
    action!(PR_FILES, "The files a pull or merge request changes, with their line counts.", [NUMBER], ["number"]),
    action!(
        PR_CREATE,
        "Open a pull or merge request from a pushed branch.",
        [
            Property { name: "title", kind: "string", about: "The title, one line of at most 256 characters" },
            BODY,
            Property { name: "head", kind: "string", about: "The branch with the changes" },
            Property { name: "base", kind: "string", about: "The branch to merge into (default the repository's)" },
            Property { name: "draft", kind: "boolean", about: "Open it as a draft" },
            Property { name: "labels", kind: "array", about: "Labels to set" },
            Property { name: "reviewers", kind: "array", about: "Accounts to ask for review" },
        ],
        ["title", "head"]
    ),
    action!(
        PR_EDIT,
        "Change a pull or merge request's title, description or base branch.",
        [NUMBER, Property { name: "title", kind: "string", about: "The new title" }, BODY, Property { name: "base", kind: "string", about: "The new base branch" }],
        ["number"]
    ),
    action!(PR_READY, "Mark a draft ready for review, or with `undo` make it a draft again.", [NUMBER, Property { name: "undo", kind: "boolean", about: "Make it a draft" }], ["number"]),
    action!(PR_COMMENT, "Add a conversation comment.", [NUMBER, BODY], ["number", "body"]),
    action!(PR_REPLY, "Reply in a review thread.", [NUMBER, THREAD, BODY], ["number", "thread", "body"]),
    action!(
        PR_REVIEW,
        "Submit a review: approve, request changes (GitHub only) or comment.",
        [NUMBER, Property { name: "verdict", kind: "string", about: "`approve`, `request_changes` or `comment`" }, BODY],
        ["number", "verdict"]
    ),
    action!(PR_REQUEST_REVIEWERS, "Ask accounts for review, or withdraw the request.", [NUMBER, ADD, REMOVE], ["number"]),
    action!(PR_LABELS, "Add and remove labels.", [NUMBER, ADD, REMOVE], ["number"]),
    action!(
        PR_REACT,
        "React to the pull or merge request or to one of its comments.",
        [
            NUMBER,
            Property { name: "target", kind: "string", about: "`pr` (default), `comment` (a conversation comment) or `review_comment`" },
            Property { name: "comment_id", kind: "integer", about: "The comment's id, with a target other than `pr`" },
            Property { name: "content", kind: "string", about: "`+1`, `-1`, `laugh`, `confused`, `heart`, `hooray`, `rocket` or `eyes`" },
            Property { name: "undo", kind: "boolean", about: "Take the signed-in account's reaction back instead of adding one" },
        ],
        ["number", "content"]
    ),
    action!(PR_RESOLVE_THREAD, "Resolve a review thread, or with `resolved: false` reopen it.", [NUMBER, THREAD, Property { name: "resolved", kind: "boolean", about: "Default true" }], ["number", "thread"]),
    action!(PR_UPDATE_BRANCH, "Bring the pull or merge request's branch up to date with its base.", [NUMBER, Property { name: "rebase", kind: "boolean", about: "Rebase instead of merging the base in" }], ["number"]),
    action!(
        PR_MERGE,
        "Merge a pull or merge request.",
        [
            NUMBER,
            Property { name: "method", kind: "string", about: "`merge` (default), `squash` or `rebase`" },
            Property { name: "auto", kind: "boolean", about: "Merge when the checks pass" },
            Property { name: "delete_branch", kind: "boolean", about: "Delete the source branch afterwards" },
            Property { name: "match_head_commit", kind: "string", about: "Merge only if the head is this commit (7-64 hex characters)" },
        ],
        ["number"]
    ),
    action!(PR_STACK, "The native stack a pull request is in (GitHub), its layers bottom to top; none when it is in no stack or the provider has no stacks.", [NUMBER], ["number"]),
    action!(
        PR_COMMENT_EDIT,
        "Replace the text of a comment: a conversation comment (`issue`, by the id `hosted_pr_comments` returned) or a review comment (`review`, by its numeric id).",
        [NUMBER, Property { name: "comment", kind: "string", about: "The comment's id" }, Property { name: "kind", kind: "string", about: "`issue` (default) or `review`" }, BODY],
        ["number", "comment", "body"]
    ),
    action!(RUN_LIST, "Recent workflow runs (GitHub) or pipelines (GitLab).", [Property { name: "branch", kind: "string", about: "Only runs of this branch" }, LIMIT], []),
    action!(RUN_VIEW, "One run with its jobs.", [Property { name: "run", kind: "integer", about: "The run's id" }], ["run"]),
    action!(AUTH_STATUS, "Whether the CLI is installed and signed in on the executor machine, as which account and with which scopes (never the token).", [], []),
    action!(
        AUTH_LOGIN_START,
        "Start the CLI's web sign-in and return the one-time code and the address to open; the sign-in finishes when the person approves it there.",
        [],
        []
    ),
    action!(AUTH_LOGIN_WAIT, "Wait for a started sign-in to finish (or for `timeout_ms`); a still pending sign-in can be waited for again.", [Property { name: "flow", kind: "string", about: "The id `hosted_auth_login_start` returned" }, Property { name: "timeout_ms", kind: "integer", about: "Time to wait (default 30000, at most 900000)" }], ["flow"]),
    action!(AUTH_LOGIN_CANCEL, "Stop a started sign-in.", [Property { name: "flow", kind: "string", about: "The id `hosted_auth_login_start` returned" }], ["flow"]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Github,
    Gitlab,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Github => "github",
            Provider::Gitlab => "gitlab",
        }
    }

    /// The CLI that serves the provider.
    pub fn program(self) -> &'static str {
        match self {
            Provider::Github => "gh",
            Provider::Gitlab => "glab",
        }
    }

    pub fn default_host(self) -> &'static str {
        match self {
            Provider::Github => "github.com",
            Provider::Gitlab => "gitlab.com",
        }
    }
}

fn bad(message: impl Into<String>) -> ExecError {
    ExecError::invalid_args(message)
}

fn control(c: char) -> bool {
    c.is_control()
}

/// `owner/name` for GitHub, two or more segments for GitLab. A segment is letters, digits, `_`, `-`, `.`, never `.`, `..` or a leading `-`.
pub fn slug(provider: Provider, value: &str) -> ExecResult<()> {
    let segments: Vec<&str> = value.split('/').collect();
    let count_ok = match provider {
        Provider::Github => segments.len() == 2,
        Provider::Gitlab => (2..=20).contains(&segments.len()),
    };
    let segment_ok = |s: &&str| !s.is_empty() && s.len() <= 100 && *s != "." && *s != ".." && !s.starts_with('-') && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'));
    if count_ok && segments.iter().all(segment_ok) {
        Ok(())
    } else {
        Err(bad(format!("`repo` must be {} of letters, digits, `_`, `-`, `.`", if provider == Provider::Github { "`owner/name`" } else { "`group/name` (subgroups allowed)" })))
    }
}

/// A host name with an optional port.
pub fn host(value: &str) -> ExecResult<()> {
    let (name, port) = match value.split_once(':') {
        Some((n, p)) => (n, Some(p)),
        None => (value, None),
    };
    let name_ok = !name.is_empty() && name.len() <= 253 && !name.starts_with(['-', '.']) && !name.ends_with(['-', '.']) && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'));
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.bytes().all(|b| b.is_ascii_digit()));
    if name_ok && port_ok {
        Ok(())
    } else {
        Err(bad("`host` must be a host name, with an optional port"))
    }
}

/// A branch or tag name as git accepts it, narrowed to letters, digits and `._/-+@=` and with no leading `-`.
pub fn ref_name(key: &str, value: &str) -> ExecResult<()> {
    let ok = !value.is_empty()
        && value.len() <= 255
        && !value.starts_with(['-', '/'])
        && !value.ends_with(['/', '.'])
        && !value.ends_with(".lock")
        && !value.contains("..")
        && !value.contains("//")
        && !value.contains("@{")
        && value != "@"
        && value.bytes().all(|b| b.is_ascii_alphanumeric() || b"._/-+@=".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(bad(format!("`{key}` must be a branch name")))
    }
}

/// A label: printable, no comma (the CLIs join labels with one), no leading `-`, at most 50 characters.
pub fn label(value: &str) -> ExecResult<()> {
    if value.is_empty() || value.chars().count() > 50 || value.starts_with('-') || value.trim() != value || value.contains(',') || value.chars().any(control) {
        return Err(bad("a label must be 1-50 printable characters with no comma and no leading `-`"));
    }
    Ok(())
}

/// An account name, or `org/team`.
pub fn login(value: &str) -> ExecResult<()> {
    let ok = !value.is_empty() && value.len() <= 80 && !value.starts_with(['-', '/', '.']) && value.matches('/').count() <= 1 && !value.ends_with('/') && value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/'));
    if ok {
        Ok(())
    } else {
        Err(bad("an account must be letters, digits, `_`, `-`, `.` (or `org/team`)"))
    }
}

/// A review thread or flow id: an opaque token of letters, digits, `_`, `-`, `=`.
pub fn token(key: &str, value: &str) -> ExecResult<()> {
    if value.is_empty() || value.len() > 128 || value.starts_with('-') || !value.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'=')) {
        return Err(bad(format!("`{key}` must be an id of letters, digits, `_`, `-`, `=`")));
    }
    Ok(())
}

fn one_line(key: &str, value: &str, max: usize) -> ExecResult<()> {
    if value.trim().is_empty() || value.chars().count() > max || value.chars().any(control) {
        return Err(bad(format!("`{key}` must be one line of 1-{max} characters")));
    }
    Ok(())
}

fn body(key: &str, value: &str) -> ExecResult<()> {
    if value.len() > BODY_MAX || value.contains('\0') {
        return Err(bad(format!("`{key}` must be text of at most {BODY_MAX} bytes")));
    }
    Ok(())
}

/// A path as `dest` and `dir` name it before the executor confines it: no NUL, no leading `-`.
fn path(key: &str, value: &str) -> ExecResult<()> {
    if value.is_empty() || value.contains('\0') || value.starts_with('-') || value.len() > 4096 {
        return Err(bad(format!("`{key}` must be a path of the workspace")));
    }
    Ok(())
}

fn number(args: &Value) -> ExecResult<u64> {
    match opt_uint(args, "number")? {
        Some(n) if (1..=i32::MAX as u64).contains(&n) => Ok(n),
        _ => Err(bad("`number` must be a positive integer")),
    }
}

fn limit(args: &Value) -> ExecResult<u64> {
    match opt_uint(args, "limit")? {
        None => Ok(LIST_DEFAULT),
        Some(n) if (1..=LIST_MAX).contains(&n) => Ok(n),
        Some(_) => Err(bad(format!("`limit` must be 1-{LIST_MAX}"))),
    }
}

fn choice<T: Copy>(args: &Value, key: &str, default: T, table: &[(&str, T)]) -> ExecResult<T> {
    match opt_string(args, key)? {
        None => Ok(default),
        Some(v) => table.iter().find(|(name, _)| *name == v).map(|(_, t)| *t).ok_or_else(|| bad(format!("`{key}` must be one of {}", table.iter().map(|(n, _)| format!("`{n}`")).collect::<Vec<_>>().join(", ")))),
    }
}

fn names(args: &Value, key: &str, check: fn(&str) -> ExecResult<()>) -> ExecResult<Vec<String>> {
    let items = match &args[key] {
        Value::Null => return Ok(Vec::new()),
        Value::Array(items) if items.len() <= 50 => items,
        _ => return Err(bad(format!("`{key}` must be an array of at most 50 names"))),
    };
    items
        .iter()
        .map(|v| {
            let s = v.as_str().ok_or_else(|| bad(format!("`{key}` must hold strings")))?;
            check(s).map_err(|e| bad(format!("`{key}`: {}", e.message)))?;
            Ok(s.to_string())
        })
        .collect()
}

fn text_arg(args: &Value, key: &str, check: impl Fn(&str) -> ExecResult<()>) -> ExecResult<Option<String>> {
    let value = opt_string(args, key)?;
    if let Some(v) = &value {
        check(v).map_err(|e| if e.message.contains(&format!("`{key}`")) { e } else { bad(format!("`{key}`: {}", e.message)) })?;
    }
    Ok(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Closed,
    Merged,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReactTarget {
    Pr,
    Comment,
    ReviewComment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentKind {
    Issue,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostedCall {
    RepoView,
    RepoLabels { limit: u64 },
    RepoReviewers { limit: u64 },
    RepoClone { dest: String },
    RepoPublish { dir: String, visibility: Visibility, description: Option<String> },
    PrList { state: PrState, author: Option<String>, label: Option<String>, base: Option<String>, head: Option<String>, search: Option<String>, limit: u64 },
    PrView { number: u64 },
    PrDiff { number: u64 },
    PrChecks { number: u64 },
    PrComments { number: u64 },
    PrReviews { number: u64 },
    PrThreads { number: u64 },
    PrFiles { number: u64 },
    PrCreate { title: String, body: Option<String>, head: String, base: Option<String>, draft: bool, labels: Vec<String>, reviewers: Vec<String> },
    PrEdit { number: u64, title: Option<String>, body: Option<String>, base: Option<String> },
    PrReady { number: u64, undo: bool },
    PrComment { number: u64, body: String },
    PrReply { number: u64, thread: String, body: String },
    PrReview { number: u64, verdict: Verdict, body: Option<String> },
    PrRequestReviewers { number: u64, add: Vec<String>, remove: Vec<String> },
    PrLabels { number: u64, add: Vec<String>, remove: Vec<String> },
    PrReact { number: u64, target: ReactTarget, comment_id: Option<u64>, content: String, undo: bool },
    PrResolveThread { number: u64, thread: String, resolved: bool },
    PrUpdateBranch { number: u64, rebase: bool },
    PrMerge { number: u64, method: MergeMethod, auto: bool, delete_branch: bool, match_head_commit: Option<String> },
    PrStack { number: u64 },
    PrCommentEdit { number: u64, comment: String, kind: CommentKind, body: String },
    RunList { branch: Option<String>, limit: u64 },
    RunView { run: u64 },
    AuthStatus,
    LoginStart,
    LoginWait { flow: String, wait_ms: u64 },
    LoginCancel { flow: String },
}

impl HostedCall {
    /// The wire name of the action this call is.
    pub fn action(&self) -> &'static str {
        match self {
            HostedCall::RepoView => REPO_VIEW,
            HostedCall::RepoLabels { .. } => REPO_LABELS,
            HostedCall::RepoReviewers { .. } => REPO_REVIEWERS,
            HostedCall::RepoClone { .. } => REPO_CLONE,
            HostedCall::RepoPublish { .. } => REPO_PUBLISH,
            HostedCall::PrList { .. } => PR_LIST,
            HostedCall::PrView { .. } => PR_VIEW,
            HostedCall::PrDiff { .. } => PR_DIFF,
            HostedCall::PrChecks { .. } => PR_CHECKS,
            HostedCall::PrComments { .. } => PR_COMMENTS,
            HostedCall::PrReviews { .. } => PR_REVIEWS,
            HostedCall::PrThreads { .. } => PR_THREADS,
            HostedCall::PrFiles { .. } => PR_FILES,
            HostedCall::PrCreate { .. } => PR_CREATE,
            HostedCall::PrEdit { .. } => PR_EDIT,
            HostedCall::PrReady { .. } => PR_READY,
            HostedCall::PrComment { .. } => PR_COMMENT,
            HostedCall::PrReply { .. } => PR_REPLY,
            HostedCall::PrReview { .. } => PR_REVIEW,
            HostedCall::PrRequestReviewers { .. } => PR_REQUEST_REVIEWERS,
            HostedCall::PrLabels { .. } => PR_LABELS,
            HostedCall::PrReact { .. } => PR_REACT,
            HostedCall::PrResolveThread { .. } => PR_RESOLVE_THREAD,
            HostedCall::PrUpdateBranch { .. } => PR_UPDATE_BRANCH,
            HostedCall::PrMerge { .. } => PR_MERGE,
            HostedCall::PrStack { .. } => PR_STACK,
            HostedCall::PrCommentEdit { .. } => PR_COMMENT_EDIT,
            HostedCall::RunList { .. } => RUN_LIST,
            HostedCall::RunView { .. } => RUN_VIEW,
            HostedCall::AuthStatus => AUTH_STATUS,
            HostedCall::LoginStart => AUTH_LOGIN_START,
            HostedCall::LoginWait { .. } => AUTH_LOGIN_WAIT,
            HostedCall::LoginCancel { .. } => AUTH_LOGIN_CANCEL,
        }
    }
}

/// One validated request of the family.
#[derive(Debug, Clone, PartialEq)]
pub struct HostedOp {
    pub provider: Provider,
    pub host: Option<String>,
    pub repo: Option<String>,
    pub timeout_ms: u64,
    pub call: HostedCall,
}

const REACTIONS: [&str; 8] = ["+1", "-1", "laugh", "confused", "heart", "hooray", "rocket", "eyes"];

impl HostedOp {
    /// `None` when `tool` is not one of this family's actions.
    pub fn parse(tool: &str, args: &Value) -> Option<ExecResult<HostedOp>> {
        if !ACTIONS.iter().any(|a| a.name == tool) {
            return None;
        }
        Some(Self::read(tool, args))
    }

    fn read(tool: &str, a: &Value) -> ExecResult<HostedOp> {
        let provider = match string(a, "provider").ok().as_deref() {
            Some("github") => Provider::Github,
            Some("gitlab") => Provider::Gitlab,
            _ => return Err(bad("`provider` must be `github` or `gitlab`")),
        };
        let host_arg = text_arg(a, "host", host)?;
        let repo = text_arg(a, "repo", |r| slug(provider, r))?;
        let need_repo = |repo: &Option<String>| repo.is_some().then_some(()).ok_or_else(|| bad("`repo` is required"));
        let timeout_default = if tool == REPO_CLONE { CLONE_TIMEOUT_DEFAULT_MS } else { TIMEOUT_DEFAULT_MS };
        let timeout_ms = match opt_uint(a, "timeout_ms")? {
            None => timeout_default,
            Some(t) if (1..=TIMEOUT_MAX_MS).contains(&t) => t,
            Some(_) => return Err(bad(format!("`timeout_ms` must be 1-{TIMEOUT_MAX_MS}"))),
        };
        let title_of = |key: &str| text_arg(a, key, |t| one_line("title", t, TITLE_MAX));
        let body_of = |key: &str| text_arg(a, key, |t| body(key, t));
        let branch_of = |key: &str| text_arg(a, key, |t| ref_name(key, t));
        let required_body = || -> ExecResult<String> {
            let text = string(a, "body")?;
            body("body", &text)?;
            if text.trim().is_empty() {
                return Err(bad("`body` must not be empty"));
            }
            Ok(text)
        };
        let thread_of = || -> ExecResult<String> {
            let t = string(a, "thread")?;
            token("thread", &t)?;
            Ok(t)
        };
        let flow_of = || -> ExecResult<String> {
            let t = string(a, "flow")?;
            token("flow", &t)?;
            Ok(t)
        };
        let call = match tool {
            REPO_VIEW => HostedCall::RepoView,
            REPO_LABELS => HostedCall::RepoLabels { limit: limit(a)? },
            REPO_REVIEWERS => HostedCall::RepoReviewers { limit: limit(a)? },
            REPO_CLONE => {
                need_repo(&repo)?;
                let dest = string(a, "dest")?;
                path("dest", &dest)?;
                HostedCall::RepoClone { dest }
            }
            REPO_PUBLISH => {
                need_repo(&repo)?;
                let dir = opt_string(a, "dir")?.unwrap_or_else(|| ".".into());
                path("dir", &dir)?;
                let description = text_arg(a, "description", |d| one_line("description", d, 1024))?;
                HostedCall::RepoPublish { dir, visibility: choice(a, "visibility", Visibility::Private, &[("private", Visibility::Private), ("public", Visibility::Public)])?, description }
            }
            PR_LIST => HostedCall::PrList {
                state: choice(a, "state", PrState::Open, &[("open", PrState::Open), ("closed", PrState::Closed), ("merged", PrState::Merged), ("all", PrState::All)])?,
                author: text_arg(a, "author", login)?,
                label: text_arg(a, "label", label)?,
                base: branch_of("base")?,
                head: branch_of("head")?,
                search: text_arg(a, "search", |s| one_line("search", s, 256))?,
                limit: limit(a)?,
            },
            PR_VIEW => HostedCall::PrView { number: number(a)? },
            PR_DIFF => HostedCall::PrDiff { number: number(a)? },
            PR_CHECKS => HostedCall::PrChecks { number: number(a)? },
            PR_COMMENTS => HostedCall::PrComments { number: number(a)? },
            PR_REVIEWS => HostedCall::PrReviews { number: number(a)? },
            PR_THREADS => HostedCall::PrThreads { number: number(a)? },
            PR_FILES => HostedCall::PrFiles { number: number(a)? },
            PR_CREATE => {
                let title = string(a, "title")?;
                one_line("title", &title, TITLE_MAX)?;
                let head = string(a, "head")?;
                ref_name("head", &head)?;
                HostedCall::PrCreate { title, body: body_of("body")?, head, base: branch_of("base")?, draft: flag(a, "draft"), labels: names(a, "labels", label)?, reviewers: names(a, "reviewers", login)? }
            }
            PR_EDIT => {
                let (title, body, base) = (title_of("title")?, body_of("body")?, branch_of("base")?);
                if title.is_none() && body.is_none() && base.is_none() {
                    return Err(bad("give at least one of `title`, `body`, `base`"));
                }
                HostedCall::PrEdit { number: number(a)?, title, body, base }
            }
            PR_READY => HostedCall::PrReady { number: number(a)?, undo: flag(a, "undo") },
            PR_COMMENT => HostedCall::PrComment { number: number(a)?, body: required_body()? },
            PR_REPLY => HostedCall::PrReply { number: number(a)?, thread: thread_of()?, body: required_body()? },
            PR_REVIEW => {
                let verdict = choice(a, "verdict", None, &[("approve", Some(Verdict::Approve)), ("request_changes", Some(Verdict::RequestChanges)), ("comment", Some(Verdict::Comment))])?.ok_or_else(|| bad("`verdict` is required: approve, request_changes or comment"))?;
                let text = body_of("body")?.filter(|b| !b.trim().is_empty());
                if verdict != Verdict::Approve && text.is_none() {
                    return Err(bad("`body` is required with this verdict"));
                }
                HostedCall::PrReview { number: number(a)?, verdict, body: text }
            }
            PR_REQUEST_REVIEWERS | PR_LABELS => {
                let check: fn(&str) -> ExecResult<()> = if tool == PR_LABELS { label } else { login };
                let (add, remove) = (names(a, "add", check)?, names(a, "remove", check)?);
                if add.is_empty() && remove.is_empty() {
                    return Err(bad("give at least one name in `add` or `remove`"));
                }
                let number = number(a)?;
                if tool == PR_LABELS {
                    HostedCall::PrLabels { number, add, remove }
                } else {
                    HostedCall::PrRequestReviewers { number, add, remove }
                }
            }
            PR_REACT => {
                let target = choice(a, "target", ReactTarget::Pr, &[("pr", ReactTarget::Pr), ("comment", ReactTarget::Comment), ("review_comment", ReactTarget::ReviewComment)])?;
                let comment_id = opt_uint(a, "comment_id")?;
                if (target == ReactTarget::Pr) == comment_id.is_some() {
                    return Err(bad("`comment_id` is given with a `target` of `comment` or `review_comment`, and only then"));
                }
                let content = string(a, "content")?;
                if !REACTIONS.contains(&content.as_str()) {
                    return Err(bad(format!("`content` must be one of {}", REACTIONS.join(", "))));
                }
                HostedCall::PrReact { number: number(a)?, target, comment_id, content, undo: flag(a, "undo") }
            }
            PR_RESOLVE_THREAD => HostedCall::PrResolveThread { number: number(a)?, thread: thread_of()?, resolved: a.get("resolved").and_then(Value::as_bool).unwrap_or(true) },
            PR_UPDATE_BRANCH => HostedCall::PrUpdateBranch { number: number(a)?, rebase: flag(a, "rebase") },
            PR_MERGE => {
                let sha = text_arg(a, "match_head_commit", |s| if (7..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit()) { Ok(()) } else { Err(bad("`match_head_commit` must be 7-64 hex characters")) })?;
                HostedCall::PrMerge { number: number(a)?, method: choice(a, "method", MergeMethod::Merge, &[("merge", MergeMethod::Merge), ("squash", MergeMethod::Squash), ("rebase", MergeMethod::Rebase)])?, auto: flag(a, "auto"), delete_branch: flag(a, "delete_branch"), match_head_commit: sha }
            }
            PR_STACK => HostedCall::PrStack { number: number(a)? },
            PR_COMMENT_EDIT => {
                let comment = string(a, "comment")?;
                token("comment", &comment)?;
                let kind = choice(a, "kind", CommentKind::Issue, &[("issue", CommentKind::Issue), ("review", CommentKind::Review)])?;
                HostedCall::PrCommentEdit { number: number(a)?, comment, kind, body: required_body()? }
            }
            RUN_LIST => HostedCall::RunList { branch: branch_of("branch")?, limit: limit(a)? },
            RUN_VIEW => match opt_uint(a, "run")? {
                Some(run) if run > 0 => HostedCall::RunView { run },
                _ => return Err(bad("`run` must be a positive integer")),
            },
            AUTH_STATUS => HostedCall::AuthStatus,
            AUTH_LOGIN_START => HostedCall::LoginStart,
            AUTH_LOGIN_WAIT => {
                let wait_ms = match opt_uint(a, "timeout_ms")? {
                    None => LOGIN_WAIT_DEFAULT_MS,
                    Some(t) if (1..=LOGIN_WAIT_MAX_MS).contains(&t) => t,
                    Some(_) => return Err(bad(format!("`timeout_ms` must be 1-{LOGIN_WAIT_MAX_MS}"))),
                };
                HostedCall::LoginWait { flow: flow_of()?, wait_ms }
            }
            AUTH_LOGIN_CANCEL => HostedCall::LoginCancel { flow: flow_of()? },
            _ => return Err(bad(format!("{tool} is not a hosted action"))),
        };
        Ok(HostedOp { provider, host: host_arg, repo, timeout_ms, call })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Arguments that satisfy an action, derived from its own table: every required property gets a value of its kind.
    fn minimal(action: &Action) -> Value {
        let mut args = serde_json::Map::new();
        for name in action.required {
            let property = action.properties.iter().find(|p| p.name == *name);
            let value = match (*name, property.map(|p| p.kind)) {
                ("provider", _) => json!("github"),
                ("repo", _) => json!("octo/hello"),
                ("verdict", _) => json!("approve"),
                ("content", _) => json!("+1"),
                ("dest", _) => json!("clones/hello"),
                (_, Some("integer")) => json!(7),
                (_, Some("array")) => json!(["x"]),
                (_, Some("boolean")) => json!(true),
                _ => json!("a"),
            };
            args.insert((*name).to_string(), value);
        }
        // The actions that need one of several optional arguments
        match action.name {
            PR_EDIT => args.insert("title".into(), json!("t")),
            PR_LABELS | PR_REQUEST_REVIEWERS => args.insert("add".into(), json!(["x"])),
            _ => None,
        };
        Value::Object(args)
    }

    #[test]
    fn given_every_action_of_the_table_when_asked_with_its_required_arguments_then_it_parses_and_a_missing_provider_is_refused() {
        for action in ACTIONS {
            // Given the action's required arguments
            let mut args = minimal(action);
            // When it is read, Then it is an operation, and without a provider a refusal naming the key
            let op = HostedOp::parse(action.name, &args).unwrap_or_else(|| panic!("{} is unknown", action.name));
            assert!(op.is_ok(), "{}: {op:?}", action.name);
            args.as_object_mut().unwrap().remove("provider");
            let refused = HostedOp::parse(action.name, &args).unwrap().unwrap_err();
            assert!(refused.message.contains("`provider`"), "{}: {refused:?}", action.name);
        }
        // And a tool that is not an action of the family is none
        assert!(HostedOp::parse("git_status", &json!({})).is_none());
        assert!(HostedOp::parse("hosted_api", &json!({"provider": "github"})).is_none());
    }

    #[test]
    fn given_text_that_a_cli_could_read_as_an_option_or_a_shell_when_used_as_a_name_then_every_argument_that_carries_it_is_refused() {
        let hostile = ["-x", "--web", "--repo=evil/x", "a b", "a;b", "a$(id)", "a`id`", "a|b", "a&b", "a>b", "a\nb", "a\rb", "a\0b", "../x", "a/../b", "", "a,b", "~x", "a:b", "a\\b"];
        for text in hostile {
            // Given hostile text in each argument that names something, and the minimal request for the action that takes it
            let cases: Vec<(&str, &str, Value)> = vec![
                (PR_LIST, "repo", json!(text)),
                (PR_LIST, "host", json!(text)),
                (PR_LIST, "author", json!(text)),
                (PR_LIST, "base", json!(text)),
                (PR_LIST, "head", json!(text)),
                (PR_CREATE, "head", json!(text)),
                (PR_CREATE, "title", json!(text)),
                (PR_LABELS, "add", json!([text])),
                (PR_LABELS, "remove", json!([text])),
                (PR_REQUEST_REVIEWERS, "add", json!([text])),
                (PR_REPLY, "thread", json!(text)),
                (RUN_LIST, "branch", json!(text)),
                (AUTH_LOGIN_WAIT, "flow", json!(text)),
                (PR_MERGE, "match_head_commit", json!(text)),
                (REPO_CLONE, "dest", json!(text)),
                (PR_COMMENT_EDIT, "comment", json!(text)),
            ];
            for (tool, key, value) in cases {
                let action = ACTIONS.iter().find(|a| a.name == tool).unwrap();
                let mut args = minimal(action);
                if matches!(key, "add" | "remove") {
                    args["add"] = json!(["ok"]);
                }
                args[key] = value;
                // When the request is read
                let outcome = HostedOp::parse(tool, &args).unwrap();
                // Then it is refused, except where the text is harmless in that argument (a title, a label or a path may hold spaces and shell characters, which are never a shell's)
                let harmless = match (tool, key) {
                    (PR_CREATE, "title") => !text.is_empty() && !text.chars().any(char::is_control),
                (PR_LABELS, "add" | "remove") => !text.is_empty() && !text.starts_with('-') && !text.chars().any(char::is_control) && !text.contains(','),
                    (REPO_CLONE, "dest") => !text.is_empty() && !text.starts_with('-') && !text.contains('\0'),
                    _ => false,
                };
                assert_eq!(outcome.is_ok(), harmless, "{tool} {key} {text:?}: {outcome:?}");
            }
        }
    }

    #[test]
    fn given_names_that_are_valid_when_used_then_each_kind_accepts_what_its_provider_names() {
        // Given the names the providers use
        assert!(slug(Provider::Github, "octo/.github").is_ok());
        assert!(slug(Provider::Gitlab, "group/sub/sub2/proj.x").is_ok());
        assert!(host("git.example.com:8443").is_ok());
        assert!(ref_name("head", "feature/PR-12_a.b").is_ok());
        assert!(label("good first issue").is_ok());
        assert!(login("octo-cat").is_ok() && login("org/team-a").is_ok());
        // Then the shapes of the other provider are refused
        assert!(slug(Provider::Github, "a/b/c").is_err());
        assert!(slug(Provider::Gitlab, "alone").is_err());
        assert!(login("a/b/c").is_err());
    }

    #[test]
    fn given_arguments_out_of_range_or_inconsistent_when_read_then_each_is_refused_with_its_key() {
        let github = |tool: &str, mut args: Value| {
            args["provider"] = json!("github");
            HostedOp::parse(tool, &args).unwrap().unwrap_err().message
        };
        assert!(github(PR_VIEW, json!({"number": 0})).contains("`number`"));
        assert!(github(PR_VIEW, json!({"number": 99999999999u64})).contains("`number`"));
        assert!(github(PR_VIEW, json!({"number": "7"})).contains("`number`"));
        assert!(github(PR_LIST, json!({"limit": 201})).contains("`limit`"));
        assert!(github(PR_LIST, json!({"state": "draft"})).contains("`state`"));
        assert!(github(PR_LIST, json!({"timeout_ms": 600_001})).contains("`timeout_ms`"));
        assert!(github(PR_REVIEW, json!({"number": 1, "verdict": "comment"})).contains("`body`"));
        assert!(github(PR_REVIEW, json!({"number": 1, "verdict": "merge"})).contains("`verdict`"));
        assert!(github(PR_REACT, json!({"number": 1, "content": "+1", "comment_id": 5})).contains("`comment_id`"));
        assert!(github(PR_REACT, json!({"number": 1, "content": "thumbsup"})).contains("`content`"));
        assert!(github(PR_LABELS, json!({"number": 1})).contains("`add`"));
        assert!(github(PR_EDIT, json!({"number": 1})).contains("at least one"));
        assert!(github(PR_COMMENT, json!({"number": 1, "body": "x".repeat(BODY_MAX + 1)})).contains("`body`"));
        assert!(github(REPO_CLONE, json!({"dest": "d"})).contains("`repo`"));
        assert!(github(PR_COMMENT_EDIT, json!({"number": 1, "comment": "IC_1", "kind": "inline", "body": "x"})).contains("`kind`"));
        assert!(github(PR_COMMENT_EDIT, json!({"number": 1, "comment": "IC_1", "body": " "})).contains("`body`"));
        assert!(github(PR_VIEW, json!({"number": 1, "repo": "no-slash"})).contains("`repo`"));
    }
}
