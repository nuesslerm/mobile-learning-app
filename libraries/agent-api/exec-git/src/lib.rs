//! The git actions of the executor contract (`docs/plans/rca-client/protocol.md`): `git_diff`, the worktree actions and the repository
//! actions of `vcs` (`git_status` .. `git_watch`, `docs/plans/rca-parity/plan.md`, section 2). A worktree lives at
//! `<worktree base>/<name>`, the base being the executor's own setting, so a request names a worktree and never a path.
use exec_proto::args::{flag, opt_string, opt_uint, string};
use exec_proto::{Action, ExecError, ExecResult, Property};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "testing")]
pub mod testing;
pub mod vcs;

pub use vcs::*;

pub const GIT_DIFF: &str = "git_diff";
pub const GIT_WORKTREE_ADD: &str = "git_worktree_add";
pub const GIT_WORKTREE_REMOVE: &str = "git_worktree_remove";
pub const GIT_WORKTREE_LIST: &str = "git_worktree_list";
pub const WORKTREE_SETUP: &str = "worktree_setup";
pub const GIT_STATUS: &str = "git_status";
pub const GIT_REFS: &str = "git_refs";
pub const GIT_LOG: &str = "git_log";
pub const GIT_COMPARE: &str = "git_compare";
pub const GIT_UPSTREAM: &str = "git_upstream";
pub const GIT_STAGE: &str = "git_stage";
pub const GIT_UNSTAGE: &str = "git_unstage";
pub const GIT_DISCARD: &str = "git_discard";
pub const GIT_COMMIT: &str = "git_commit";
pub const GIT_CHECKOUT: &str = "git_checkout";
pub const GIT_CREATE_REF: &str = "git_create_ref";
pub const GIT_FETCH: &str = "git_fetch";
pub const GIT_PULL: &str = "git_pull";
pub const GIT_PUSH: &str = "git_push";
pub const GIT_FAST_FORWARD: &str = "git_fast_forward";
pub const GIT_MERGE: &str = "git_merge";
pub const GIT_REBASE: &str = "git_rebase";
pub const GIT_CONFLICT_RESOLVE: &str = "git_conflict_resolve";
pub const GIT_BLOB: &str = "git_blob";
pub const GIT_WATCH: &str = "git_watch";
pub const GIT_INIT: &str = "git_init";
pub const GIT_CHECKPOINT: &str = "git_checkpoint";

/// The actions that stay open after their request: `git_watch` streams the status until a `cancel` names it (or `max_events` is reached).
pub const LONG: &[&str] = &[GIT_WATCH];

/// The longest a `worktree_setup` may run when the request names no timeout.
pub const SETUP_TIMEOUT_MS: u64 = 300_000;
const NAME: Property = Property { name: "name", kind: "string", about: "The worktree's directory name under the worktree base" };
const WORKTREE: Property = Property { name: "worktree", kind: "string", about: "Act on this worktree under the worktree base instead of the workspace's repository" };
const PATHS: Property = Property { name: "paths", kind: "array", about: "Paths relative to the working tree, at least one (`.` is the whole tree); read literally, never as a pathspec" };
const REMOTE: Property = Property { name: "remote", kind: "string", about: "A configured remote of the repository (default: the branch's upstream remote, else origin)" };
const OP: Property = Property { name: "op", kind: "string", about: "`start`, `abort` or `continue`" };

/// The family's actions, in the contract's order.
pub const ACTIONS: &[Action] = &[
    Action {
        name: GIT_DIFF,
        about: "The working tree's `git diff` against a revision (the index when none), limited to one path when given.",
        properties: &[
            Property { name: "path", kind: "string", about: "Path relative to the workspace root (or to the worktree); the whole tree when absent" },
            Property { name: "base", kind: "string", about: "A revision to diff against" },
            Property { name: "head", kind: "string", about: "With `base`: diff the two revisions instead of the working tree" },
            Property { name: "staged", kind: "boolean", about: "Diff the index against `base` (default HEAD) instead of the working tree" },
            Property { name: "worktree", kind: "string", about: "Diff this worktree under the worktree base instead of the workspace root" },
        ],
        required: &[],
    },
    Action {
        name: GIT_WORKTREE_ADD,
        about: "Add a worktree of the workspace's repository under the worktree base, checking out a branch or creating it from a revision.",
        properties: &[NAME, Property { name: "branch", kind: "string", about: "The branch to check out, or to create when `base` is given" }, Property { name: "base", kind: "string", about: "A revision to create `branch` from" }],
        required: &["name", "branch"],
    },
    Action {
        name: GIT_WORKTREE_REMOVE,
        about: "Remove a worktree under the worktree base; refused while it has changes unless forced.",
        properties: &[NAME, Property { name: "force", kind: "boolean", about: "Remove even with uncommitted changes" }],
        required: &["name"],
    },
    Action { name: GIT_WORKTREE_LIST, about: "List the worktrees of the workspace's repository.", properties: &[], required: &[] },
    Action {
        name: WORKTREE_SETUP,
        about: "Run setup commands in a worktree in order, stopping at the first that fails; progress is streamed as subscribe events.",
        properties: &[NAME, Property { name: "commands", kind: "array", about: "Shell commands to run, in order" }, Property { name: "timeout_ms", kind: "integer", about: "Time allowed for the whole setup (default 300000)" }],
        required: &["name", "commands"],
    },
    Action {
        name: GIT_STATUS,
        about: "The repository's status: branch, head, upstream and ahead/behind, an operation stopped on conflicts, and each changed file with its index and working tree columns.",
        properties: &[WORKTREE],
        required: &[],
    },
    Action {
        name: GIT_REFS,
        about: "The branches, remote branches and tags, with the current branch.",
        properties: &[WORKTREE, Property { name: "kind", kind: "string", about: "`branch`, `remote` or `tag` (default all)" }, Property { name: "query", kind: "string", about: "Only names containing this text" }, Property { name: "limit", kind: "integer", about: "At most this many refs (default 500)" }],
        required: &[],
    },
    Action {
        name: GIT_LOG,
        about: "Commits reachable from a revision (default HEAD), newest first, optionally of one path.",
        properties: &[WORKTREE, Property { name: "rev", kind: "string", about: "A revision or range (default HEAD)" }, Property { name: "path", kind: "string", about: "Only commits that touch this path" }, Property { name: "limit", kind: "integer", about: "At most this many commits (default 50)" }, Property { name: "skip", kind: "integer", about: "Skip this many first" }],
        required: &[],
    },
    Action {
        name: GIT_COMPARE,
        about: "How `head` differs from `base`: the merge base, commits ahead and behind, the commits of `head`, and the files it changed since the merge base.",
        properties: &[WORKTREE, Property { name: "base", kind: "string", about: "The revision to compare against" }, Property { name: "head", kind: "string", about: "The revision compared" }, Property { name: "limit", kind: "integer", about: "At most this many commits (default 50)" }],
        required: &["base", "head"],
    },
    Action { name: GIT_UPSTREAM, about: "The current branch's upstream and how far it is ahead and behind, and the configured remotes.", properties: &[WORKTREE], required: &[] },
    Action { name: GIT_STAGE, about: "Stage the paths: additions, changes and deletions.", properties: &[WORKTREE, PATHS], required: &["paths"] },
    Action { name: GIT_UNSTAGE, about: "Unstage the paths, leaving the working tree as it is.", properties: &[WORKTREE, PATHS], required: &["paths"] },
    Action {
        name: GIT_DISCARD,
        about: "Throw away changes of the paths: restore tracked files (from the index, or with scope `all` and from `source`/HEAD too) and, when asked, remove untracked ones.",
        properties: &[
            WORKTREE,
            PATHS,
            Property { name: "scope", kind: "string", about: "`worktree` (restore from the index, default) or `all` (index and working tree from the source)" },
            Property { name: "untracked", kind: "boolean", about: "Also remove untracked files and directories among the paths" },
            Property { name: "source", kind: "string", about: "Restore index and working tree from this revision (files it lacks are removed); implies scope `all`" },
        ],
        required: &["paths"],
    },
    Action {
        name: GIT_COMMIT,
        about: "Commit the index (or with `all` every tracked change) with the message; the repository's hooks run only when the executor was started with --git-hooks.",
        properties: &[WORKTREE, Property { name: "message", kind: "string", about: "The commit message" }, Property { name: "amend", kind: "boolean", about: "Replace the last commit" }, Property { name: "all", kind: "boolean", about: "Stage every tracked change first" }, Property { name: "allow_empty", kind: "boolean", about: "Commit even when nothing changed" }],
        required: &["message"],
    },
    Action {
        name: GIT_CHECKOUT,
        about: "Switch to a branch, create a local branch tracking a remote one (`track`), or detach at a revision (`detach`).",
        properties: &[WORKTREE, Property { name: "ref", kind: "string", about: "A branch, a remote branch with `track`, or any revision with `detach`" }, Property { name: "detach", kind: "boolean", about: "Detach HEAD at the revision" }, Property { name: "track", kind: "boolean", about: "Create a local branch that tracks the remote branch `ref`" }, Property { name: "force", kind: "boolean", about: "Discard local changes that would be overwritten" }],
        required: &["ref"],
    },
    Action {
        name: GIT_CREATE_REF,
        about: "Create a branch or a tag at a revision (default HEAD), optionally switching to the branch.",
        properties: &[WORKTREE, Property { name: "name", kind: "string", about: "The new name" }, Property { name: "kind", kind: "string", about: "`branch` (default) or `tag`" }, Property { name: "from", kind: "string", about: "The revision to create it at" }, Property { name: "checkout", kind: "boolean", about: "Switch to the new branch" }, Property { name: "message", kind: "string", about: "Makes a tag annotated with this message" }],
        required: &["name"],
    },
    Action { name: GIT_FETCH, about: "Fetch a configured remote of the repository, with the executor machine's own credentials.", properties: &[WORKTREE, REMOTE, Property { name: "prune", kind: "boolean", about: "Remove remote branches that are gone" }, Property { name: "refspecs", kind: "array", about: "Also fetch these into refs of the repository's own `refs/rca/` namespace (no branch list shows them): `+<source>:refs/rca/<name>`, the source a pull request's head (`refs/pull/<n>/head`, `refs/merge-requests/<n>/head`) or `refs/heads/<branch>`; the answer names each one's commit" }], required: &[] },
    Action {
        name: GIT_PULL,
        about: "Pull the current branch from a configured remote: fast-forward only by default; a conflicting merge or rebase stops with its conflicts listed.",
        properties: &[WORKTREE, REMOTE, Property { name: "branch", kind: "string", about: "The remote branch (default the upstream's)" }, Property { name: "strategy", kind: "string", about: "`ff_only` (default), `merge` or `rebase`" }],
        required: &[],
    },
    Action {
        name: GIT_PUSH,
        about: "Push a local branch (default the current one) to the same name on a configured remote; never forced except with `force_with_lease`.",
        properties: &[WORKTREE, REMOTE, Property { name: "branch", kind: "string", about: "The local branch (default the current one)" }, Property { name: "set_upstream", kind: "boolean", about: "Make the remote branch the upstream" }, Property { name: "force_with_lease", kind: "boolean", about: "Overwrite the remote branch only if it is where this repository last saw it" }],
        required: &[],
    },
    Action {
        name: GIT_FAST_FORWARD,
        about: "Fast-forward a branch (default the current one) to a revision (default its upstream); refused when it is not a fast-forward.",
        properties: &[WORKTREE, Property { name: "branch", kind: "string", about: "The branch to move (default the current one)" }, Property { name: "to", kind: "string", about: "The revision to move it to (default its upstream)" }],
        required: &[],
    },
    Action {
        name: GIT_MERGE,
        about: "Merge a revision into the current branch, or abort or continue a merge stopped on conflicts.",
        properties: &[WORKTREE, OP, Property { name: "ref", kind: "string", about: "What to merge (with op `start`)" }, Property { name: "strategy", kind: "string", about: "`default`, `ff_only`, `no_ff` or `squash`" }, Property { name: "message", kind: "string", about: "The merge commit's message" }],
        required: &["op"],
    },
    Action {
        name: GIT_REBASE,
        about: "Rebase the current branch onto a revision, or abort or continue a rebase stopped on conflicts.",
        properties: &[WORKTREE, OP, Property { name: "ref", kind: "string", about: "What to rebase onto (with op `start`)" }],
        required: &["op"],
    },
    Action {
        name: GIT_CONFLICT_RESOLVE,
        about: "Resolve one conflicted file: take our side or theirs (git's meaning of the two in the operation under way), write the given content, or mark the edited file resolved.",
        properties: &[WORKTREE, Property { name: "path", kind: "string", about: "A conflicted path" }, Property { name: "resolution", kind: "string", about: "`ours`, `theirs`, `resolved` or `content`" }, Property { name: "content", kind: "string", about: "The file's new content (with resolution `content` only)" }],
        required: &["path", "resolution"],
    },
    Action {
        name: GIT_BLOB,
        about: "The content of a file at a revision, base64, without checking anything out.",
        properties: &[WORKTREE, Property { name: "rev", kind: "string", about: "The revision" }, Property { name: "path", kind: "string", about: "The file's path in that revision" }, Property { name: "max_bytes", kind: "integer", about: "Read at most this many bytes (default 1 MiB)" }],
        required: &["rev", "path"],
    },
    Action {
        name: GIT_WATCH,
        about: "Stream the status as subscribe events (a `git.status` event at the start and whenever it changes) until cancelled or `max_events` events were sent.",
        properties: &[WORKTREE, Property { name: "interval_ms", kind: "integer", about: "How often the repository is looked at (default 1000)" }, Property { name: "max_events", kind: "integer", about: "End after this many events" }],
        required: &[],
    },
    Action {
        name: GIT_INIT,
        about: "Make a repository in a directory below the root (`git init`, which keeps an existing one as it is), optionally with first files: they are written as new files, staged and committed. A commit that fails (git has no name or email) is reported in `commit_error`, never as a failure of the action; the repository and its files exist either way.",
        properties: &[
            Property { name: "path", kind: "string", about: "The directory, relative to the workspace root, which must exist" },
            Property { name: "initial_branch", kind: "string", about: "The first branch (the machine's `init.defaultBranch`, else `main`)" },
            Property { name: "files", kind: "array", about: "First files: objects of `path` (inside the repository) and `content`, at most 32 of at most 256 KiB" },
            Property { name: "message", kind: "string", about: "The first commit's message (default `Initial commit`, used when `files` is given)" },
        ],
        required: &["path"],
    },
    Action {
        name: GIT_CHECKPOINT,
        about: "Record the working tree as it is now (tracked and untracked files, not ignored ones) as a commit under `refs/rca/checkpoints/<thread>/<turn>`, built on a private index: the repository's index, branch, HEAD and files are never touched. Restore it with `git_discard` (`scope` all, `untracked`, `source` that ref).",
        properties: &[WORKTREE, Property { name: "thread", kind: "string", about: "The thread the checkpoint belongs to: letters, digits, `.`, `_` and `-`" }, Property { name: "turn", kind: "string", about: "The turn that is about to start: letters, digits, `.`, `_` and `-`; a checkpoint of the same turn is replaced" }],
        required: &["thread", "turn"],
    },
];

/// A revision is never an option git would read: letters, digits and . _ / ~ ^ - only, not starting with -.
pub fn revision(value: &str) -> ExecResult<&str> {
    if value.is_empty() || value.starts_with('-') || !value.chars().all(|c| c.is_ascii_alphanumeric() || "._/~^-".contains(c)) {
        return Err(ExecError::invalid_args("`base` must be a revision"));
    }
    Ok(value)
}

/// The most bytes of a checkpoint's thread or turn id.
const CHECKPOINT_ID_MAX: usize = 128;

/// A thread or turn id as a single ref component: no `/`, no `..`, nothing git would read as an option or a lock file.
pub fn checkpoint_id<'a>(what: &str, value: &'a str) -> ExecResult<&'a str> {
    let bad = value.is_empty()
        || value.len() > CHECKPOINT_ID_MAX
        || value.starts_with(['-', '.'])
        || value.ends_with(['.', '/'])
        || value.ends_with(".lock")
        || value.contains("..")
        || !value.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if bad {
        return Err(ExecError::invalid_args(format!("`{what}` must be at most {CHECKPOINT_ID_MAX} letters, digits, `.`, `_` or `-`, not starting with `-` or `.`")));
    }
    Ok(value)
}

/// The ref a turn's checkpoint is kept under; `git_discard` takes it as its `source`.
pub fn checkpoint_ref(thread: &str, turn: &str) -> String {
    format!("refs/rca/checkpoints/{thread}/{turn}")
}

/// A branch name git accepts for a new or existing branch, narrowed to what cannot be read as an option or a path trick.
pub fn branch(value: &str) -> ExecResult<&str> {
    let bad = value.is_empty()
        || value.starts_with(['-', '/', '.'])
        || value.ends_with(['/', '.'])
        || value.ends_with(".lock")
        || value.contains("..")
        || value.contains("//")
        || value.contains("@{")
        || !value.chars().all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c));
    if bad {
        return Err(ExecError::invalid_args("`branch` must be a branch name"));
    }
    Ok(value)
}

/// A worktree's directory name: one plain segment, so `<base>/<name>` can never leave the base.
pub fn worktree_name(value: &str) -> ExecResult<&str> {
    if value.is_empty() || value.len() > 100 || value.starts_with(['-', '.']) || !value.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
        return Err(ExecError::invalid_args("`name` must be one plain directory name"));
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq)]
pub struct GitDiff {
    pub path: Option<String>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub staged: bool,
    pub worktree: Option<String>,
}

impl GitDiff {
    pub fn parse(args: &Value) -> ExecResult<GitDiff> {
        let base = opt_string(args, "base")?;
        if let Some(base) = &base {
            revision(base)?;
        }
        let head = opt_string(args, "head")?;
        if let Some(head) = &head {
            revision(head).map_err(|_| ExecError::invalid_args("`head` must be a revision"))?;
            if base.is_none() {
                return Err(ExecError::invalid_args("`head` needs a `base`"));
            }
        }
        let worktree = opt_string(args, "worktree")?;
        if let Some(name) = &worktree {
            worktree_name(name)?;
        }
        Ok(GitDiff { path: opt_string(args, "path")?, base, head, staged: flag(args, "staged"), worktree })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffResult {
    pub diff: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeAdd {
    pub name: String,
    pub branch: String,
    pub base: Option<String>,
}

impl WorktreeAdd {
    pub fn parse(args: &Value) -> ExecResult<WorktreeAdd> {
        let name = string(args, "name")?;
        worktree_name(&name)?;
        let branch_name = string(args, "branch")?;
        branch(&branch_name)?;
        let base = opt_string(args, "base")?;
        if let Some(base) = &base {
            revision(base)?;
        }
        Ok(WorktreeAdd { name, branch: branch_name, base })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorktreeAdded {
    pub path: String,
    pub branch: String,
    pub head: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeRemove {
    pub name: String,
    pub force: bool,
}

impl WorktreeRemove {
    pub fn parse(args: &Value) -> ExecResult<WorktreeRemove> {
        let name = string(args, "name")?;
        worktree_name(&name)?;
        Ok(WorktreeRemove { name, force: flag(args, "force") })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorktreeRemoved {
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub head: String,
    /// Absent for a detached head.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// True for the repository's own working tree.
    pub is_main: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorktreeListed {
    pub worktrees: Vec<WorktreeInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeSetup {
    pub name: String,
    pub commands: Vec<String>,
    pub timeout_ms: u64,
}

impl WorktreeSetup {
    pub fn parse(args: &Value) -> ExecResult<WorktreeSetup> {
        let name = string(args, "name")?;
        worktree_name(&name)?;
        let commands = args["commands"].as_array().ok_or_else(|| ExecError::invalid_args("`commands` must be an array of strings"))?;
        let commands = commands.iter().map(|c| c.as_str().filter(|c| !c.trim().is_empty()).map(str::to_string).ok_or_else(|| ExecError::invalid_args("`commands` must be an array of non-empty strings"))).collect::<ExecResult<Vec<_>>>()?;
        Ok(WorktreeSetup { name, commands, timeout_ms: opt_uint(args, "timeout_ms")?.unwrap_or(SETUP_TIMEOUT_MS) })
    }
}

/// One progress event of a `worktree_setup`, the `event` of a subscribe frame whose `seq` counts them from 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SetupEvent {
    #[serde(rename = "setup.started")]
    Started { steps: usize },
    #[serde(rename = "step.started")]
    StepStarted { step: usize, command: String },
    #[serde(rename = "step.output")]
    StepOutput { step: usize, stream: String, data: String },
    #[serde(rename = "step.finished")]
    StepFinished { step: usize, exit_code: i32 },
    #[serde(rename = "setup.finished")]
    Finished { ok: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetupStep {
    pub command: String,
    pub exit_code: i32,
}

/// `ok` is true when every command exited 0; the steps are those that ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetupResult {
    pub ok: bool,
    pub steps: Vec<SetupStep>,
    pub timed_out: bool,
    pub cancelled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use exec_proto::{to_value, Code};
    use serde_json::json;

    #[test]
    fn given_a_base_that_could_be_read_as_an_option_when_parsed_then_it_is_refused_and_a_revision_is_not() {
        // Given revisions git accepts and strings it would read as an option or that are no revision
        let revisions = ["HEAD", "main", "origin/dev~2", "v1.0.0", "HEAD^"];
        let refused = ["--output=/tmp/x", "-p", "", "a b", "a;b", "$(x)"];
        // When each is parsed as a base
        // Then the revisions are kept and the others are INVALID_ARGS
        for base in revisions {
            assert_eq!(GitDiff::parse(&json!({"base": base})).unwrap().base.as_deref(), Some(base));
        }
        for base in refused {
            let e = GitDiff::parse(&json!({"base": base})).unwrap_err();
            assert!(e.code == Code::InvalidArgs && e.message.contains("`base`"), "{base:?}: {e:?}");
        }
    }

    #[test]
    fn given_no_path_and_no_base_when_parsed_then_the_whole_tree_against_the_index_is_asked() {
        // Given a request with no args at all and one with a path
        // Then neither names a base, and only the second a path
        assert_eq!(GitDiff::parse(&json!({})).unwrap(), GitDiff { path: None, base: None, head: None, staged: false, worktree: None });
        assert_eq!(GitDiff::parse(&json!({"path": "a.txt"})).unwrap().path.as_deref(), Some("a.txt"));
        // And a worktree is a plain name: a path is refused naming `name`
        assert_eq!(GitDiff::parse(&json!({"worktree": "wt"})).unwrap().worktree.as_deref(), Some("wt"));
        assert!(GitDiff::parse(&json!({"worktree": "../x"})).unwrap_err().message.contains("`name`"));
        assert_eq!(to_value(&DiffResult { diff: "d".into() }).to_string(), r#"{"diff":"d"}"#);
    }
    #[test]
    fn given_names_and_branches_that_could_leave_the_base_or_be_read_as_options_when_parsed_then_each_is_refused_by_its_key() {
        // Given worktree names and branches that are paths, options or git syntax, and ones that are plain
        let plain_names = ["feature-1", "a.b_c", "T3"];
        let bad_names = ["", "../x", "a/b", "-rf", ".hidden", "a b", "a;b", "$(x)", &"n".repeat(101)];
        let plain_branches = ["main", "feature/x-1", "release_1.2"];
        let bad_branches = ["", "-b", "--orphan", "/abs", "a..b", "a//b", "x.lock", "x/", "a@{1}", ".x", "a b", "a~1", "a:b"];
        // When each is parsed as the arguments of an add
        for name in plain_names {
            assert!(WorktreeAdd::parse(&json!({"name": name, "branch": "main"})).is_ok(), "{name}");
        }
        for branch in plain_branches {
            assert!(WorktreeAdd::parse(&json!({"name": "w", "branch": branch})).is_ok(), "{branch}");
        }
        // Then the bad ones are INVALID_ARGS naming `name` or `branch`, and the same names are refused by remove and setup
        for name in bad_names {
            for e in [WorktreeAdd::parse(&json!({"name": name, "branch": "main"})).unwrap_err(), WorktreeRemove::parse(&json!({"name": name})).unwrap_err(), WorktreeSetup::parse(&json!({"name": name, "commands": []})).unwrap_err()] {
                assert!(e.code == Code::InvalidArgs && e.message.contains("`name`"), "{name:?}: {e:?}");
            }
        }
        for branch in bad_branches {
            let e = WorktreeAdd::parse(&json!({"name": "w", "branch": branch})).unwrap_err();
            assert!(e.code == Code::InvalidArgs && e.message.contains("`branch`"), "{branch:?}: {e:?}");
        }
        // And a base is checked like any revision
        assert_eq!(WorktreeAdd::parse(&json!({"name": "w", "branch": "b", "base": "origin/main"})).unwrap().base.as_deref(), Some("origin/main"));
        assert!(WorktreeAdd::parse(&json!({"name": "w", "branch": "b", "base": "--upload-pack=x"})).is_err());
    }

    #[test]
    fn given_setup_arguments_when_parsed_then_commands_must_be_non_empty_strings_and_the_timeout_defaults() {
        // Given a setup with two commands, one with a blank command, and one whose commands are no array
        let ok = WorktreeSetup::parse(&json!({"name": "w", "commands": ["pnpm i", "pnpm build"]})).unwrap();
        let blank = WorktreeSetup::parse(&json!({"name": "w", "commands": ["ls", "  "]})).unwrap_err();
        let not_array = WorktreeSetup::parse(&json!({"name": "w", "commands": "ls"})).unwrap_err();
        // Then the commands keep their order, the timeout is the default, and the others name `commands`
        assert_eq!((ok.commands, ok.timeout_ms), (vec!["pnpm i".to_string(), "pnpm build".to_string()], SETUP_TIMEOUT_MS));
        assert!(blank.message.contains("`commands`") && not_array.message.contains("`commands`"));
    }

    #[test]
    fn given_the_worktree_results_and_events_when_encoded_then_they_keep_the_contract_names() {
        // Given one of each
        let added = to_value(&WorktreeAdded { path: "/w/a".into(), branch: "b".into(), head: "abc".into() }).to_string();
        let listed = to_value(&WorktreeListed { worktrees: vec![WorktreeInfo { path: "/r".into(), head: "abc".into(), branch: None, is_main: true }] }).to_string();
        let event = to_value(&SetupEvent::StepOutput { step: 0, stream: "stdout".into(), data: "x".into() }).to_string();
        let started = to_value(&SetupEvent::Started { steps: 2 }).to_string();
        // Then the keys are the contract's: a detached head has no branch, and an event's kind is its dotted name
        assert_eq!(added, r#"{"path":"/w/a","branch":"b","head":"abc"}"#);
        assert_eq!(listed, r#"{"worktrees":[{"path":"/r","head":"abc","is_main":true}]}"#);
        assert_eq!(event, r#"{"kind":"step.output","step":0,"stream":"stdout","data":"x"}"#);
        assert_eq!(started, r#"{"kind":"setup.started","steps":2}"#);
    }
}
