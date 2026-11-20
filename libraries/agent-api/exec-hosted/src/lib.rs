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
pub const REPO_REMOTES: &str = "hosted_repo_remotes";
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
pub const PR_FILE_VIEWED: &str = "hosted_pr_file_viewed";
pub const PR_STATE: &str = "hosted_pr_state";
pub const PR_UPDATE_BRANCH: &str = "hosted_pr_update_branch";
pub const PR_MERGE: &str = "hosted_pr_merge";
pub const PR_STACK: &str = "hosted_pr_stack";
pub const PR_COMMENT_EDIT: &str = "hosted_pr_comment_edit";
pub const PR_REVIEW_COMMENT: &str = "hosted_pr_review_comment";
pub const ISSUE_LIST: &str = "hosted_issue_list";
pub const ISSUE_VIEW: &str = "hosted_issue_view";
pub const ISSUE_COMMENTS: &str = "hosted_issue_comments";
pub const ISSUE_COMMENT: &str = "hosted_issue_comment";
pub const ISSUE_EDIT: &str = "hosted_issue_edit";
pub const ISSUE_STATE: &str = "hosted_issue_state";
pub const ISSUE_CREATE: &str = "hosted_issue_create";
pub const ISSUE_SEARCH: &str = "hosted_issue_search";
pub const ISSUE_COUNT: &str = "hosted_issue_count";
pub const ISSUE_TODOS: &str = "hosted_issue_todos";
pub const RUN_LIST: &str = "hosted_run_list";
pub const RUN_VIEW: &str = "hosted_run_view";
pub const RUN_RERUN: &str = "hosted_run_rerun";
pub const PR_CHECK_DETAILS: &str = "hosted_pr_check_details";
pub const PR_AUTO_MERGE: &str = "hosted_pr_auto_merge";
pub const PROJECT_LIST: &str = "hosted_project_list";
pub const PROJECT_RESOLVE: &str = "hosted_project_resolve";
pub const PROJECT_VIEWS: &str = "hosted_project_views";
pub const PROJECT_FIELD_SET: &str = "hosted_project_field_set";
pub const PROJECT_FIELD_CLEAR: &str = "hosted_project_field_clear";
pub const COMMENT_ADD: &str = "hosted_comment_add";
pub const COMMENT_EDIT: &str = "hosted_comment_edit";
pub const COMMENT_DELETE: &str = "hosted_comment_delete";
pub const REPO_ISSUE_TYPES: &str = "hosted_repo_issue_types";
pub const ISSUE_TYPE_SET: &str = "hosted_issue_type_set";
pub const ISSUE_UPDATE: &str = "hosted_issue_update";
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
const ISSUE_NUMBER: Property = Property { name: "number", kind: "integer", about: "The issue number" };
const BODY: Property = Property { name: "body", kind: "string", about: "Markdown text, at most 65536 bytes" };
const LIMIT: Property = Property { name: "limit", kind: "integer", about: "At most this many items (default 30, at most 200)" };
const TIMEOUT: Property = Property { name: "timeout_ms", kind: "integer", about: "Time the CLI may run (default 60000, at most 600000)" };

macro_rules! action {
    ($name:expr, $about:expr, [$($p:expr),* $(,)?], [$($r:expr),* $(,)?]) => {
        Action { name: $name, about: $about, properties: &[PROVIDER, HOST, REPO, TIMEOUT, $($p),*], required: &["provider", $($r),*] }
    };
}

const WORK_KIND: Property = Property { name: "kind", kind: "string", about: "`all` (default: issues and pull or merge requests), `issue` or `pr`" };
const WORK_STATE: Property = Property { name: "state", kind: "string", about: "`open` (default), `closed`, `merged` (only with kind `pr`) or `all`" };
const WORK_AUTHOR: Property = Property { name: "author", kind: "string", about: "Only this account's" };
const WORK_LABEL: Property = Property { name: "label", kind: "string", about: "Only with this label" };
const WORK_TEXT: Property = Property { name: "search", kind: "string", about: "Only matching this text" };
const PAGE: Property = Property { name: "page", kind: "integer", about: "The page of `limit` items to read (default 1, at most 100)" };

const STATE: Property = Property { name: "state", kind: "string", about: "`open` (default), `closed`, `merged` or `all`" };
const OWNER: Property = Property { name: "owner", kind: "string", about: "The account or organization that owns the project" };
const OWNER_TYPE: Property = Property { name: "owner_type", kind: "string", about: "`organization` or `user`" };
const PROJECT_NUMBER: Property = Property { name: "project_number", kind: "integer", about: "The project's number under its owner" };
const PROJECT_ID: Property = Property { name: "project", kind: "string", about: "The project's node id" };
const ITEM_ID: Property = Property { name: "item", kind: "string", about: "The project item's node id" };
const FIELD_ID: Property = Property { name: "field", kind: "string", about: "The project field's node id" };
const COMMENT_ID: Property = Property { name: "comment_id", kind: "integer", about: "The issue comment's numeric id" };
const ADD: Property = Property { name: "add", kind: "array", about: "Names to add" };
const REMOVE: Property = Property { name: "remove", kind: "array", about: "Names to remove" };

/// The family's actions, in the contract's order. Every one takes `provider`, `host`, `repo` and `timeout_ms`.
pub const ACTIONS: &[Action] = &[
    action!(REPO_VIEW, "The repository's name, url, default branch, visibility and the signed-in account's permission.", [], []),
    action!(REPO_REMOTES, "The workspace repository's remotes and their fetch URLs, as git lists them, read locally with no network call; none when it has no remote.", [], []),
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
    action!(PR_COMMENTS, "The conversation comments of a pull or merge request. On GitHub each comment keeps `id` as the node id and also answers database_id, avatar_url, is_bot, viewer_did_author, reaction_subject_id and reactions ([{content, count, viewer_has_reacted}]).", [NUMBER], ["number"]),
    action!(PR_REVIEWS, "The reviews (GitHub) or approvals (GitLab) of a pull or merge request. On GitHub each review keeps `id` as the node id and also answers database_id, avatar_url, is_bot, viewer_did_author, reaction_subject_id, reactions and submitted_at_ms.", [NUMBER], ["number"]),
    action!(PR_THREADS, "The review threads of a pull or merge request, each with its comments and whether it is resolved. On GitHub each thread also answers start_line, original_line, original_start_line, diff_side and root_diff_hunk, and each thread comment keeps `id` as the node id and also answers database_id, avatar_url, is_bot, viewer_did_author, reaction_subject_id and reactions.", [NUMBER], ["number"]),
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
    action!(PR_COMMENT, "Add a conversation comment; GitLab answers the new note's id, database_id, author, avatar_url, is_bot, body and created_at.", [NUMBER, BODY], ["number", "body"]),
    action!(PR_REPLY, "Reply in a review thread, named by `thread` or (GitHub) by the numeric id of a comment in it, `comment_id`, which answers the new comment's id, database_id, url, author, avatar_url, is_bot, body, created_at, path and line.", [NUMBER, THREAD, BODY, Property { name: "comment_id", kind: "integer", about: "GitHub: the numeric id of a review comment of the thread, instead of `thread`" }], ["number", "body"]),
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
    action!(PR_STATE, "Close a pull or merge request, or reopen a closed one; answers its number and new state.", [NUMBER, Property { name: "state", kind: "string", about: "`closed` or `open`" }], ["number", "state"]),
    action!(PR_RESOLVE_THREAD, "Resolve a review thread, or with `resolved: false` reopen it. GitHub names a thread by its node id alone, so `number` is needed on GitLab only.", [NUMBER, THREAD, Property { name: "resolved", kind: "boolean", about: "Default true" }], ["thread"]),
    action!(
        PR_FILE_VIEWED,
        "Mark a file of a pull request viewed for the signed-in account, or with `viewed: false` unmark it (GitHub; GitLab keeps this mark in the browser).",
        [Property { name: "pull_request", kind: "string", about: "The pull request's node id" }, Property { name: "path", kind: "string", about: "The file's path in the pull request" }, Property { name: "viewed", kind: "boolean", about: "Default true" }],
        ["pull_request", "path"]
    ),
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
    action!(
        PR_REVIEW_COMMENT,
        "Comment on a line (or, GitHub, a range ending at `line`) of a file of the request's head, on the new side; answers the new comment's id, database_id, thread (GitLab), url, author, avatar_url, is_bot, body, created_at, path, line and start_line.",
        [
            NUMBER,
            BODY,
            Property { name: "commit_id", kind: "string", about: "The head commit the line belongs to (GitHub `commit_id`, GitLab `head_sha`)" },
            Property { name: "path", kind: "string", about: "The file's path in the repository" },
            Property { name: "line", kind: "integer", about: "The line of the new file" },
            Property { name: "start_line", kind: "integer", about: "GitHub: the first line of a range ending at `line`" },
            Property { name: "base_sha", kind: "string", about: "GitLab: the merge request's base_sha (diff_refs)" },
            Property { name: "start_sha", kind: "string", about: "GitLab: the merge request's start_sha (diff_refs)" },
            Property { name: "old_path", kind: "string", about: "GitLab: the file's old path (default `path`)" },
        ],
        ["number", "body", "commit_id", "path", "line"]
    ),
    action!(
        ISSUE_LIST,
        "Issues, most recently updated first.",
        [Property { name: "state", kind: "string", about: "`open` (default), `closed` or `all`" }, Property { name: "author", kind: "string", about: "Only this account's" }, Property { name: "label", kind: "string", about: "Only with this label" }, Property { name: "search", kind: "string", about: "Only matching this text" }, LIMIT],
        []
    ),
    action!(ISSUE_VIEW, "One issue with its description, labels and assignees.", [ISSUE_NUMBER], ["number"]),
    action!(ISSUE_COMMENTS, "The comments of an issue, oldest first.", [ISSUE_NUMBER], ["number"]),
    action!(ISSUE_COMMENT, "Add a comment to an issue.", [ISSUE_NUMBER, BODY], ["number", "body"]),
    action!(ISSUE_EDIT, "Change an issue's title or description.", [ISSUE_NUMBER, Property { name: "title", kind: "string", about: "The new title" }, BODY], ["number"]),
    action!(ISSUE_STATE, "Close an issue, or reopen a closed one; answers its number and new state.", [ISSUE_NUMBER, Property { name: "state", kind: "string", about: "`closed` or `open`" }], ["number", "state"]),
    action!(
        ISSUE_CREATE,
        "Open an issue; answers its number and url.",
        [Property { name: "title", kind: "string", about: "The title, one line" }, BODY, Property { name: "labels", kind: "array", about: "Labels to put on it" }],
        ["title"]
    ),
    action!(
        ISSUE_SEARCH,
        "Issues and pull or merge requests of the repository that match a search, most recently updated first, with the total the provider counts.",
        [WORK_KIND, WORK_STATE, WORK_AUTHOR, WORK_LABEL, WORK_TEXT, LIMIT, PAGE],
        []
    ),
    action!(ISSUE_COUNT, "How many issues and pull or merge requests of the repository match a search.", [WORK_KIND, WORK_STATE, WORK_AUTHOR, WORK_LABEL, WORK_TEXT], []),
    action!(
        ISSUE_TODOS,
        "What the signed-in account is asked to look at: GitLab's pending to-dos, GitHub's unread notifications. Account-wide, never one repository's.",
        [LIMIT],
        []
    ),
    action!(RUN_LIST, "Recent workflow runs (GitHub) or pipelines (GitLab).", [Property { name: "branch", kind: "string", about: "Only runs of this branch" }, LIMIT], []),
    action!(RUN_VIEW, "One run with its jobs.", [Property { name: "run", kind: "integer", about: "The run's id" }], ["run"]),
    action!(
        RUN_RERUN,
        "Start a run again: GitHub reruns the whole workflow run or, with `failed_only`, its failed jobs; GitLab retries the pipeline's failed jobs (it has no rerun of the passed ones).",
        [Property { name: "run", kind: "integer", about: "The run's id" }, Property { name: "failed_only", kind: "boolean", about: "Only the failed jobs (GitHub; GitLab always retries only those)" }],
        ["run"]
    ),
    action!(
        PR_CHECK_DETAILS,
        "One check in depth: its output, annotations and the jobs of its run with their steps and the tail of a failed job's log. GitHub reads a check run (`check`) and the jobs of a workflow run (`run`); GitLab reads a job (`check`) and, with `run`, the jobs of its pipeline. One of the two is required.",
        [Property { name: "check", kind: "integer", about: "GitHub: a check run's id (an Actions job's id is its check run's id); GitLab: a job's id" }, Property { name: "run", kind: "integer", about: "GitHub: the workflow run's id; GitLab: the pipeline's id" }],
        []
    ),
    action!(
        PR_AUTO_MERGE,
        "Turn auto-merge on or off for a pull or merge request: GitHub asks for it through the enable mutation, which refuses a pull request that is already clean (it never merges at once); GitLab sets or cancels merge when the pipeline succeeds, which merges at once when the pipeline has already passed.",
        [NUMBER, Property { name: "enabled", kind: "boolean", about: "`true` to turn it on, `false` to turn it off" }, Property { name: "method", kind: "string", about: "`merge` (default), `squash` or `rebase` (GitHub; GitLab merges as the project is set to)" }],
        ["number", "enabled"]
    ),
    action!(PROJECT_LIST, "The GitHub projects the signed-in account can open: its own and those of its organizations (the first 40 and 20 per organization), with the login it is signed in as. Needs the `project` scope.", [], []),
    action!(
        PROJECT_RESOLVE,
        "Find a GitHub project (or, with no `project_number`, only the owner) under an organization or a user; answers its node id and title.",
        [OWNER, OWNER_TYPE, PROJECT_NUMBER],
        ["owner", "owner_type"]
    ),
    action!(PROJECT_VIEWS, "The saved views of a GitHub project: id, number, name and layout.", [OWNER, OWNER_TYPE, PROJECT_NUMBER], ["owner", "owner_type", "project_number"]),
    action!(
        PROJECT_FIELD_SET,
        "Set one field of a project item: `kind` names how `value` reads (`text`, `date` as YYYY-MM-DD, `single_select` and `iteration` by option or iteration id), `number_value` is the value of `number`.",
        [
            PROJECT_ID,
            ITEM_ID,
            FIELD_ID,
            Property { name: "kind", kind: "string", about: "`text`, `number`, `date`, `single_select` or `iteration`" },
            Property { name: "value", kind: "string", about: "The text, date, option id or iteration id" },
            Property { name: "number_value", kind: "number", about: "The number, with `kind` `number`" },
        ],
        ["project", "item", "field", "kind"]
    ),
    action!(PROJECT_FIELD_CLEAR, "Clear one field of a project item.", [PROJECT_ID, ITEM_ID, FIELD_ID], ["project", "item", "field"]),
    action!(
        COMMENT_ADD,
        "Add a comment to an issue or pull request of `repo` through the REST API; answers the new comment's id, author, avatar_url, body, created_at, url and whether the author is a bot.",
        [ISSUE_NUMBER, BODY],
        ["repo", "number", "body"]
    ),
    action!(COMMENT_EDIT, "Replace the text of an issue or pull request conversation comment of `repo`, by its numeric id.", [COMMENT_ID, BODY], ["repo", "comment_id", "body"]),
    action!(COMMENT_DELETE, "Delete an issue or pull request conversation comment of `repo`, by its numeric id.", [COMMENT_ID], ["repo", "comment_id"]),
    action!(REPO_ISSUE_TYPES, "The issue types a repository's organization defines: id, name, color and description; none when the repository has no issue types.", [], ["repo"]),
    action!(
        ISSUE_TYPE_SET,
        "Set an issue's type by id, or clear it when `issue_type` is absent.",
        [ISSUE_NUMBER, Property { name: "issue_type", kind: "string", about: "The issue type's node id; absent clears the type" }],
        ["repo", "number"]
    ),
    action!(
        ISSUE_UPDATE,
        "Change an issue as one request: its state (with a reason, or the issue it duplicates), title and description, and the labels and assignees to add or remove. The steps run in that order and stop at the first that fails.",
        [
            ISSUE_NUMBER,
            Property { name: "state", kind: "string", about: "`closed` or `open`" },
            Property { name: "state_reason", kind: "string", about: "With `closed`: `completed`, `not_planned` or `duplicate`" },
            Property { name: "duplicate_of", kind: "integer", about: "The issue this one duplicates, with `state_reason` `duplicate`, and only then" },
            Property { name: "title", kind: "string", about: "The new title" },
            BODY,
            Property { name: "add_labels", kind: "array", about: "Labels to add" },
            Property { name: "remove_labels", kind: "array", about: "Labels to remove" },
            Property { name: "add_assignees", kind: "array", about: "Accounts to assign" },
            Property { name: "remove_assignees", kind: "array", about: "Accounts to unassign" },
        ],
        ["repo", "number"]
    ),
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

/// A label that goes into a quoted search word: no quote or backslash, which would end the quote early.
pub fn work_label(value: &str) -> ExecResult<()> {
    label(value)?;
    if value.contains(['"', '\\']) {
        return Err(bad("a label to search for must not hold a quote or a backslash"));
    }
    Ok(())
}

/// Free search text: one line, and no qualifier that moves the search off the workspace repository.
pub fn search_text(value: &str) -> ExecResult<()> {
    one_line("search", value, 256)?;
    let scoped = value.split_whitespace().any(|w| {
        let w = w.trim_start_matches('-').to_ascii_lowercase();
        ["repo:", "org:", "user:", "owner:"].iter().any(|q| w.starts_with(q))
    });
    if scoped {
        return Err(bad("`search` must not hold a repo:, org:, user: or owner: qualifier"));
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

/// A GitHub account or organization name: letters, digits and single hyphens, at most 39 characters, no leading or trailing hyphen.
pub fn owner_name(value: &str) -> ExecResult<()> {
    let ok = !value.is_empty() && value.len() <= 39 && !value.starts_with('-') && !value.ends_with('-') && !value.contains("--") && value.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if ok {
        Ok(())
    } else {
        Err(bad("`owner` must be an account name of letters, digits and single `-`"))
    }
}

/// A calendar date as GraphQL's `Date`: `YYYY-MM-DD`.
pub fn iso_date(value: &str) -> ExecResult<()> {
    let b = value.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    let ok = b.len() == 10 && b[4] == b'-' && b[7] == b'-' && digits(0..4) && digits(5..7) && digits(8..10) && (1..=12).contains(&value[5..7].parse::<u8>().unwrap_or(0)) && (1..=31).contains(&value[8..10].parse::<u8>().unwrap_or(0));
    if ok {
        Ok(())
    } else {
        Err(bad("`value` must be a date as YYYY-MM-DD"))
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

/// A commit as GitHub and GitLab name it: 1-64 hex characters (the host passes on any it was given, so no minimum length).
fn sha(key: &str, value: &str) -> ExecResult<()> {
    if value.is_empty() || value.len() > 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad(format!("`{key}` must be 1-64 hex characters")));
    }
    Ok(())
}

/// A file of the repository as a review comment names it; it travels in a JSON body, so spaces and shell characters are harmless.
fn file_path(key: &str, value: &str) -> ExecResult<()> {
    if value.is_empty() || value.len() > 4096 || value.starts_with(['-', '/']) || value.chars().any(control) {
        return Err(bad(format!("`{key}` must be a file path of the repository")));
    }
    Ok(())
}

fn positive(args: &Value, key: &str) -> ExecResult<Option<u64>> {
    match opt_uint(args, key)? {
        Some(n) if !(1..=i32::MAX as u64).contains(&n) => Err(bad(format!("`{key}` must be a positive integer"))),
        n => Ok(n),
    }
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
pub enum WorkKind {
    Issue,
    Pr,
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
pub enum OwnerType {
    Organization,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateReason {
    Completed,
    NotPlanned,
    Duplicate,
}

/// The value of a project field and how GraphQL types it.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Text(String),
    Number(f64),
    Date(String),
    SingleSelect(String),
    Iteration(String),
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
    RepoRemotes,
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
    PrReply { number: u64, thread: Option<String>, comment_id: Option<u64>, body: String },
    PrReview { number: u64, verdict: Verdict, body: Option<String> },
    PrRequestReviewers { number: u64, add: Vec<String>, remove: Vec<String> },
    PrLabels { number: u64, add: Vec<String>, remove: Vec<String> },
    PrReact { number: u64, target: ReactTarget, comment_id: Option<u64>, content: String, undo: bool },
    PrResolveThread { number: Option<u64>, thread: String, resolved: bool },
    PrFileViewed { pull_request: String, path: String, viewed: bool },
    PrState { number: u64, closed: bool },
    PrUpdateBranch { number: u64, rebase: bool },
    PrMerge { number: u64, method: MergeMethod, auto: bool, delete_branch: bool, match_head_commit: Option<String> },
    PrStack { number: u64 },
    PrCommentEdit { number: u64, comment: String, kind: CommentKind, body: String },
    PrReviewComment { number: u64, body: String, commit_id: String, path: String, line: u64, start_line: Option<u64>, base_sha: Option<String>, start_sha: Option<String>, old_path: Option<String> },
    IssueList { state: PrState, author: Option<String>, label: Option<String>, search: Option<String>, limit: u64 },
    IssueView { number: u64 },
    IssueComments { number: u64 },
    IssueComment { number: u64, body: String },
    IssueEdit { number: u64, title: Option<String>, body: Option<String> },
    IssueState { number: u64, closed: bool },
    IssueCreate { title: String, body: Option<String>, labels: Vec<String> },
    IssueSearch { kind: WorkKind, state: PrState, author: Option<String>, label: Option<String>, search: Option<String>, limit: u64, page: u64 },
    IssueCount { kind: WorkKind, state: PrState, author: Option<String>, label: Option<String>, search: Option<String> },
    IssueTodos { limit: u64 },
    RunList { branch: Option<String>, limit: u64 },
    RunView { run: u64 },
    RunRerun { run: u64, failed_only: bool },
    PrCheckDetails { check: Option<u64>, run: Option<u64> },
    PrAutoMerge { number: u64, enabled: bool, method: MergeMethod },
    ProjectList,
    ProjectResolve { owner: String, owner_type: OwnerType, number: Option<u64> },
    ProjectViews { owner: String, owner_type: OwnerType, number: u64 },
    ProjectFieldSet { project: String, item: String, field: String, value: FieldValue },
    ProjectFieldClear { project: String, item: String, field: String },
    CommentAdd { number: u64, body: String },
    CommentEdit { comment_id: u64, body: String },
    CommentDelete { comment_id: u64 },
    RepoIssueTypes,
    IssueTypeSet { number: u64, issue_type: Option<String> },
    IssueUpdate(Box<IssueUpdate>),
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
            HostedCall::RepoRemotes => REPO_REMOTES,
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
            HostedCall::PrFileViewed { .. } => PR_FILE_VIEWED,
            HostedCall::PrState { .. } => PR_STATE,
            HostedCall::PrUpdateBranch { .. } => PR_UPDATE_BRANCH,
            HostedCall::PrMerge { .. } => PR_MERGE,
            HostedCall::PrStack { .. } => PR_STACK,
            HostedCall::PrCommentEdit { .. } => PR_COMMENT_EDIT,
            HostedCall::PrReviewComment { .. } => PR_REVIEW_COMMENT,
            HostedCall::IssueList { .. } => ISSUE_LIST,
            HostedCall::IssueView { .. } => ISSUE_VIEW,
            HostedCall::IssueComments { .. } => ISSUE_COMMENTS,
            HostedCall::IssueComment { .. } => ISSUE_COMMENT,
            HostedCall::IssueEdit { .. } => ISSUE_EDIT,
            HostedCall::IssueState { .. } => ISSUE_STATE,
            HostedCall::IssueCreate { .. } => ISSUE_CREATE,
            HostedCall::IssueSearch { .. } => ISSUE_SEARCH,
            HostedCall::IssueCount { .. } => ISSUE_COUNT,
            HostedCall::IssueTodos { .. } => ISSUE_TODOS,
            HostedCall::RunList { .. } => RUN_LIST,
            HostedCall::RunView { .. } => RUN_VIEW,
            HostedCall::RunRerun { .. } => RUN_RERUN,
            HostedCall::PrCheckDetails { .. } => PR_CHECK_DETAILS,
            HostedCall::PrAutoMerge { .. } => PR_AUTO_MERGE,
            HostedCall::ProjectList => PROJECT_LIST,
            HostedCall::ProjectResolve { .. } => PROJECT_RESOLVE,
            HostedCall::ProjectViews { .. } => PROJECT_VIEWS,
            HostedCall::ProjectFieldSet { .. } => PROJECT_FIELD_SET,
            HostedCall::ProjectFieldClear { .. } => PROJECT_FIELD_CLEAR,
            HostedCall::CommentAdd { .. } => COMMENT_ADD,
            HostedCall::CommentEdit { .. } => COMMENT_EDIT,
            HostedCall::CommentDelete { .. } => COMMENT_DELETE,
            HostedCall::RepoIssueTypes => REPO_ISSUE_TYPES,
            HostedCall::IssueTypeSet { .. } => ISSUE_TYPE_SET,
            HostedCall::IssueUpdate(_) => ISSUE_UPDATE,
            HostedCall::AuthStatus => AUTH_STATUS,
            HostedCall::LoginStart => AUTH_LOGIN_START,
            HostedCall::LoginWait { .. } => AUTH_LOGIN_WAIT,
            HostedCall::LoginCancel { .. } => AUTH_LOGIN_CANCEL,
        }
    }
}

/// What `hosted_issue_update` changes; every part is optional, at least one is given.
#[derive(Debug, Clone, PartialEq)]
pub struct IssueUpdate {
    pub number: u64,
    /// `Some(true)` closes the issue, `Some(false)` reopens it.
    pub closed: Option<bool>,
    pub reason: Option<StateReason>,
    pub duplicate_of: Option<u64>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub add_labels: Vec<String>,
    pub remove_labels: Vec<String>,
    pub add_assignees: Vec<String>,
    pub remove_assignees: Vec<String>,
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
        let github_only = matches!(tool, PROJECT_LIST | PROJECT_RESOLVE | PROJECT_VIEWS | PROJECT_FIELD_SET | PROJECT_FIELD_CLEAR | COMMENT_ADD | COMMENT_EDIT | COMMENT_DELETE | REPO_ISSUE_TYPES | ISSUE_TYPE_SET | ISSUE_UPDATE);
        if github_only && provider != Provider::Github {
            return Err(bad(format!("{tool} is GitHub's: `provider` must be `github`")));
        }
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
            REPO_REMOTES => HostedCall::RepoRemotes,
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
                search: text_arg(a, "search", search_text)?,
                limit: limit(a)?,
            },
            ISSUE_LIST => HostedCall::IssueList {
                state: choice(a, "state", PrState::Open, &[("open", PrState::Open), ("closed", PrState::Closed), ("all", PrState::All)])?,
                author: text_arg(a, "author", login)?,
                label: text_arg(a, "label", label)?,
                search: text_arg(a, "search", search_text)?,
                limit: limit(a)?,
            },
            ISSUE_VIEW => HostedCall::IssueView { number: number(a)? },
            ISSUE_COMMENTS => HostedCall::IssueComments { number: number(a)? },
            ISSUE_COMMENT => HostedCall::IssueComment { number: number(a)?, body: required_body()? },
            ISSUE_EDIT => {
                let (title, body) = (title_of("title")?, body_of("body")?);
                if title.is_none() && body.is_none() {
                    return Err(bad("give at least one of `title`, `body`"));
                }
                HostedCall::IssueEdit { number: number(a)?, title, body }
            }
            ISSUE_STATE => {
                let closed = choice(a, "state", None, &[("closed", Some(true)), ("open", Some(false))])?.ok_or_else(|| bad("`state` is required: closed or open"))?;
                HostedCall::IssueState { number: number(a)?, closed }
            }
            ISSUE_CREATE => {
                let title = string(a, "title")?;
                one_line("title", &title, TITLE_MAX)?;
                HostedCall::IssueCreate { title, body: body_of("body")?, labels: names(a, "labels", label)? }
            }
            ISSUE_SEARCH | ISSUE_COUNT => {
                let kind = choice(a, "kind", WorkKind::All, &[("all", WorkKind::All), ("issue", WorkKind::Issue), ("pr", WorkKind::Pr)])?;
                let state = choice(a, "state", PrState::Open, &[("open", PrState::Open), ("closed", PrState::Closed), ("merged", PrState::Merged), ("all", PrState::All)])?;
                if state == PrState::Merged && kind != WorkKind::Pr {
                    return Err(bad("`state` `merged` applies to `kind` `pr` only"));
                }
                let (author, label, search) = (text_arg(a, "author", login)?, text_arg(a, "label", work_label)?, text_arg(a, "search", search_text)?);
                if tool == ISSUE_COUNT {
                    HostedCall::IssueCount { kind, state, author, label, search }
                } else {
                    let page = match opt_uint(a, "page")? {
                        None => 1,
                        Some(n) if (1..=100).contains(&n) => n,
                        Some(_) => return Err(bad("`page` must be 1-100")),
                    };
                    HostedCall::IssueSearch { kind, state, author, label, search, limit: limit(a)?, page }
                }
            }
            ISSUE_TODOS => HostedCall::IssueTodos { limit: limit(a)? },
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
            PR_REPLY => {
                let comment_id = opt_uint(a, "comment_id")?.map(|c| if (1..=i64::MAX as u64).contains(&c) { Ok(c) } else { Err(bad("`comment_id` must be a positive integer")) }).transpose()?;
                let thread = if a.get("thread").is_some_and(|t| !t.is_null()) { Some(thread_of()?) } else { None };
                match (provider, &thread, comment_id) {
                    (_, Some(_), Some(_)) | (_, None, None) => return Err(bad("give one of `thread` and `comment_id`")),
                    (Provider::Gitlab, None, Some(_)) => return Err(bad("`comment_id` names a GitHub review comment; GitLab replies by `thread`")),
                    _ => {}
                }
                HostedCall::PrReply { number: number(a)?, thread, comment_id, body: required_body()? }
            }
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
            PR_STATE => {
                let closed = choice(a, "state", None, &[("closed", Some(true)), ("open", Some(false))])?.ok_or_else(|| bad("`state` is required: closed or open"))?;
                HostedCall::PrState { number: number(a)?, closed }
            }
            PR_RESOLVE_THREAD => {
                let number = match (provider, a.get("number").filter(|n| !n.is_null())) {
                    (Provider::Github, None) => None,
                    _ => Some(number(a)?),
                };
                HostedCall::PrResolveThread { number, thread: thread_of()?, resolved: a.get("resolved").and_then(Value::as_bool).unwrap_or(true) }
            }
            PR_FILE_VIEWED => {
                let pull_request = string(a, "pull_request")?;
                token("pull_request", &pull_request)?;
                let file = string(a, "path")?;
                file_path("path", &file)?;
                HostedCall::PrFileViewed { pull_request, path: file, viewed: a.get("viewed").and_then(Value::as_bool).unwrap_or(true) }
            }
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
            PR_REVIEW_COMMENT => {
                let commit_id = string(a, "commit_id")?;
                sha("commit_id", &commit_id)?;
                let path = string(a, "path")?;
                file_path("path", &path)?;
                let line = positive(a, "line")?.ok_or_else(|| bad("`line` must be a positive integer"))?;
                let start_line = positive(a, "start_line")?;
                let (base_sha, start_sha) = (text_arg(a, "base_sha", |s| sha("base_sha", s))?, text_arg(a, "start_sha", |s| sha("start_sha", s))?);
                let old_path = text_arg(a, "old_path", |p| file_path("old_path", p))?;
                if provider == Provider::Gitlab {
                    if start_line.is_some() {
                        return Err(bad("`start_line` is GitHub's; GitLab comments on one line"));
                    }
                    if base_sha.is_none() || start_sha.is_none() {
                        return Err(bad("`base_sha` and `start_sha` are required on GitLab"));
                    }
                }
                HostedCall::PrReviewComment { number: number(a)?, body: required_body()?, commit_id, path, line, start_line, base_sha, start_sha, old_path }
            }
            RUN_LIST => HostedCall::RunList { branch: branch_of("branch")?, limit: limit(a)? },
            RUN_VIEW => match opt_uint(a, "run")? {
                Some(run) if run > 0 => HostedCall::RunView { run },
                _ => return Err(bad("`run` must be a positive integer")),
            },
            RUN_RERUN => match opt_uint(a, "run")? {
                Some(run) if run > 0 => HostedCall::RunRerun { run, failed_only: flag(a, "failed_only") },
                _ => return Err(bad("`run` must be a positive integer")),
            },
            PR_CHECK_DETAILS => {
                let (check, run) = (positive(a, "check")?, positive(a, "run")?);
                if check.is_none() && run.is_none() {
                    return Err(bad("give `check` or `run`"));
                }
                HostedCall::PrCheckDetails { check, run }
            }
            PR_AUTO_MERGE => {
                let enabled = a.get("enabled").and_then(Value::as_bool).ok_or_else(|| bad("`enabled` must be true or false"))?;
                let method = choice(a, "method", MergeMethod::Merge, &[("merge", MergeMethod::Merge), ("squash", MergeMethod::Squash), ("rebase", MergeMethod::Rebase)])?;
                HostedCall::PrAutoMerge { number: number(a)?, enabled, method }
            }
            PROJECT_LIST => HostedCall::ProjectList,
            PROJECT_RESOLVE | PROJECT_VIEWS => {
                let owner = string(a, "owner")?;
                owner_name(&owner)?;
                let owner_type = choice(a, "owner_type", None, &[("organization", Some(OwnerType::Organization)), ("user", Some(OwnerType::User))])?.ok_or_else(|| bad("`owner_type` must be `organization` or `user`"))?;
                let number = positive(a, "project_number")?;
                match (tool, number) {
                    (PROJECT_VIEWS, Some(number)) => HostedCall::ProjectViews { owner, owner_type, number },
                    (PROJECT_VIEWS, None) => return Err(bad("`project_number` must be a positive integer")),
                    (_, number) => HostedCall::ProjectResolve { owner, owner_type, number },
                }
            }
            PROJECT_FIELD_SET | PROJECT_FIELD_CLEAR => {
                let (project, item, field) = (string(a, "project")?, string(a, "item")?, string(a, "field")?);
                token("project", &project)?;
                token("item", &item)?;
                token("field", &field)?;
                if tool == PROJECT_FIELD_CLEAR {
                    HostedCall::ProjectFieldClear { project, item, field }
                } else {
                    if !a["number_value"].is_null() && !a["number_value"].as_f64().is_some_and(f64::is_finite) {
                        return Err(bad("`number_value` must be a number"));
                    }
                    let kind = choice(a, "kind", None, &[("text", Some(0)), ("number", Some(1)), ("date", Some(2)), ("single_select", Some(3)), ("iteration", Some(4))])?.ok_or_else(|| bad("`kind` must be one of `text`, `number`, `date`, `single_select`, `iteration`"))?;
                    let value = if kind == 1 {
                        let n = a["number_value"].as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad("`number_value` must be a number"))?;
                        FieldValue::Number(n)
                    } else {
                        let v = string(a, "value")?;
                        match kind {
                            0 => {
                                body("value", &v)?;
                                FieldValue::Text(v)
                            }
                            2 => {
                                iso_date(&v)?;
                                FieldValue::Date(v)
                            }
                            3 => {
                                token("value", &v)?;
                                FieldValue::SingleSelect(v)
                            }
                            _ => {
                                token("value", &v)?;
                                FieldValue::Iteration(v)
                            }
                        }
                    };
                    HostedCall::ProjectFieldSet { project, item, field, value }
                }
            }
            COMMENT_ADD => {
                need_repo(&repo)?;
                HostedCall::CommentAdd { number: number(a)?, body: required_body()? }
            }
            COMMENT_EDIT | COMMENT_DELETE => {
                need_repo(&repo)?;
                let comment_id = positive(a, "comment_id")?.ok_or_else(|| bad("`comment_id` must be a positive integer"))?;
                if tool == COMMENT_EDIT {
                    HostedCall::CommentEdit { comment_id, body: required_body()? }
                } else {
                    HostedCall::CommentDelete { comment_id }
                }
            }
            REPO_ISSUE_TYPES => {
                need_repo(&repo)?;
                HostedCall::RepoIssueTypes
            }
            ISSUE_TYPE_SET => {
                need_repo(&repo)?;
                let issue_type = text_arg(a, "issue_type", |t| token("issue_type", t))?;
                HostedCall::IssueTypeSet { number: number(a)?, issue_type }
            }
            ISSUE_UPDATE => {
                need_repo(&repo)?;
                let closed = choice(a, "state", None, &[("closed", Some(true)), ("open", Some(false))])?;
                let reason = choice(a, "state_reason", None, &[("completed", Some(StateReason::Completed)), ("not_planned", Some(StateReason::NotPlanned)), ("duplicate", Some(StateReason::Duplicate))])?;
                let duplicate_of = positive(a, "duplicate_of")?;
                if reason.is_some() && closed != Some(true) {
                    return Err(bad("`state_reason` applies with `state` `closed` only"));
                }
                match (reason, duplicate_of) {
                    (Some(StateReason::Duplicate), None) => return Err(bad("`duplicate_of` is required with `state_reason` `duplicate`")),
                    (Some(StateReason::Duplicate), Some(_)) | (_, None) => {}
                    (_, Some(_)) => return Err(bad("`duplicate_of` is valid only with `state_reason` `duplicate`")),
                }
                let update = IssueUpdate {
                    number: number(a)?,
                    closed,
                    reason,
                    duplicate_of,
                    title: title_of("title")?,
                    body: body_of("body")?,
                    add_labels: names(a, "add_labels", label)?,
                    remove_labels: names(a, "remove_labels", label)?,
                    add_assignees: names(a, "add_assignees", login)?,
                    remove_assignees: names(a, "remove_assignees", login)?,
                };
                let nothing = update.closed.is_none() && update.title.is_none() && update.body.is_none() && [&update.add_labels, &update.remove_labels, &update.add_assignees, &update.remove_assignees].iter().all(|l| l.is_empty());
                if nothing {
                    return Err(bad("give at least one of `state`, `title`, `body`, `add_labels`, `remove_labels`, `add_assignees`, `remove_assignees`"));
                }
                HostedCall::IssueUpdate(Box::new(update))
            }
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

#[cfg(any(test, feature = "testing"))]
/// The fewest arguments that make `action` a valid request: its required ones, and one of the optional ones an action needs one of.
pub fn minimal_args(action: &Action) -> Value {
    let mut args = serde_json::Map::new();
    for name in action.required {
        let property = action.properties.iter().find(|p| p.name == *name);
        let value = match (*name, property.map(|p| p.kind)) {
            ("provider", _) => serde_json::json!("github"),
            ("repo", _) => serde_json::json!("octo/hello"),
            ("verdict", _) => serde_json::json!("approve"),
            ("owner_type", _) => serde_json::json!("organization"),
            ("kind", _) => serde_json::json!("text"),
            ("content", _) => serde_json::json!("+1"),
            ("dest", _) => serde_json::json!("clones/hello"),
            (_, Some("integer")) => serde_json::json!(7),
            (_, Some("array")) => serde_json::json!(["x"]),
            (_, Some("boolean")) => serde_json::json!(true),
            _ => serde_json::json!("a"),
        };
        args.insert((*name).to_string(), value);
    }
    // The actions that need one of several optional arguments
    match action.name {
        PR_EDIT | ISSUE_EDIT => args.insert("title".into(), serde_json::json!("t")),
        PR_LABELS | PR_REQUEST_REVIEWERS => args.insert("add".into(), serde_json::json!(["x"])),
        // a reply names its thread or (GitHub) a comment of it, one of the two
        PR_REPLY => args.insert("thread".into(), serde_json::json!("PRRT_1")),
        PR_STATE | ISSUE_STATE => args.insert("state".into(), serde_json::json!("closed")),
        PR_CHECK_DETAILS => args.insert("check".into(), serde_json::json!(7)),
        PROJECT_FIELD_SET => args.insert("value".into(), serde_json::json!("v")),
        ISSUE_UPDATE => args.insert("title".into(), serde_json::json!("t")),
        _ => None,
    };
    Value::Object(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Arguments that satisfy an action, derived from its own table: every required property gets a value of its kind.
    fn minimal(action: &Action) -> Value {
        crate::minimal_args(action)
    }

    #[test]
    fn given_a_search_or_label_that_would_leave_the_repository_or_break_the_quote_when_asked_then_it_is_refused() {
        // Given searches and labels a caller might send
        // When they are checked
        // Then a scope qualifier, a quote and a backslash are refused, plain words pass
        assert!(search_text("flaky login").is_ok());
        for bad_search in ["repo:victim/private flaky", "-repo:a/b x", "x ORG:acme", "user:bob"] {
            assert!(search_text(bad_search).is_err(), "{bad_search}");
        }
        assert!(work_label("bug").is_ok());
        for bad_label in ["x\\", "a\"b"] {
            assert!(work_label(bad_label).is_err(), "{bad_label}");
        }
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
                (PR_REVIEW_COMMENT, "commit_id", json!(text)),
                (PR_REVIEW_COMMENT, "path", json!(text)),
                (PR_FILE_VIEWED, "pull_request", json!(text)),
                (PR_FILE_VIEWED, "path", json!(text)),
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
                    (PR_REVIEW_COMMENT | PR_FILE_VIEWED, "path") => !text.is_empty() && !text.starts_with(['-', '/']) && !text.chars().any(char::is_control),
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
        assert!(github(PR_REVIEW_COMMENT, json!({"number": 1, "body": "x", "commit_id": "abc", "path": "a", "line": 0})).contains("`line`"));
        assert!(github(PR_REVIEW_COMMENT, json!({"number": 1, "body": "x", "commit_id": "xyz!", "path": "a", "line": 3})).contains("`commit_id`"));
        assert!(github(PR_REVIEW_COMMENT, json!({"number": 1, "body": "x", "commit_id": "abc", "path": "a", "line": 3, "start_line": -2})).contains("`start_line`"));
        assert!(github(PR_FILE_VIEWED, json!({"pull_request": "PR_1", "path": ""})).contains("`path`"));
        assert!(github(PR_FILE_VIEWED, json!({"pull_request": "PR 1", "path": "a"})).contains("`pull_request`"));
        assert!(github(PR_CHECK_DETAILS, json!({})).contains("`check` or `run`"));
        assert!(github(PR_CHECK_DETAILS, json!({"check": 0})).contains("`check`"));
        assert!(github(PR_CHECK_DETAILS, json!({"check": 3, "run": "9"})).contains("`run`"));
        assert!(github(RUN_RERUN, json!({"run": 0})).contains("`run`"));
        assert!(github(PR_AUTO_MERGE, json!({"number": 1})).contains("`enabled`"));
        assert!(github(PR_AUTO_MERGE, json!({"number": 1, "enabled": "yes"})).contains("`enabled`"));
        assert!(github(PR_AUTO_MERGE, json!({"number": 1, "enabled": true, "method": "octopus"})).contains("`method`"));
        let gitlab = HostedOp::parse(PR_REVIEW_COMMENT, &json!({"provider": "gitlab", "number": 1, "body": "x", "commit_id": "abc", "path": "a", "line": 3, "base_sha": "ba5e", "start_sha": "5ca1", "start_line": 2})).unwrap().unwrap_err();
        assert!(gitlab.message.contains("`start_line`"));
    }

    #[test]
    fn given_the_project_and_issue_update_actions_when_asked_then_each_value_is_typed_by_its_kind_and_a_contradiction_or_another_provider_is_refused_with_its_key() {
        let parse = |tool: &str, args: Value| HostedOp::parse(tool, &args).unwrap();
        let base = json!({"provider": "github", "project": "PVT_1", "item": "PVTI_1", "field": "PVTF_1"});
        let set = |extra: Value| {
            let mut a = base.clone();
            a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            parse(PROJECT_FIELD_SET, a)
        };
        // Given a value of each kind, Then it is read as that kind
        assert_eq!(set(json!({"kind": "number", "number_value": 2.5})).unwrap().call, HostedCall::ProjectFieldSet { project: "PVT_1".into(), item: "PVTI_1".into(), field: "PVTF_1".into(), value: FieldValue::Number(2.5) });
        assert!(matches!(set(json!({"kind": "single_select", "value": "f75ad846"})).unwrap().call, HostedCall::ProjectFieldSet { value: FieldValue::SingleSelect(_), .. }));
        assert!(matches!(set(json!({"kind": "date", "value": "2026-12-01"})).unwrap().call, HostedCall::ProjectFieldSet { value: FieldValue::Date(_), .. }));
        // And a number that is no number, a date that is none, a kind that is none and a missing value are refused naming their key
        for (extra, key) in [(json!({"kind": "number", "number_value": "3"}), "number_value"), (json!({"kind": "date", "value": "2026-13-40"}), "value"), (json!({"kind": "label", "value": "x"}), "kind"), (json!({"kind": "text"}), "value")] {
            assert!(set(extra).unwrap_err().message.contains(&format!("`{key}`")), "{key}");
        }
        // And the actions are GitHub's
        let gitlab = parse(PROJECT_LIST, json!({"provider": "gitlab"})).unwrap_err();
        assert!(gitlab.message.contains("GitHub's"), "{gitlab:?}");
        // And an issue update closes only with a reason it can state: a reason needs `closed`, a duplicate needs its target and only a duplicate has one
        let update = |extra: Value| {
            let mut a = json!({"provider": "github", "repo": "octo/hello", "number": 7});
            a.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            parse(ISSUE_UPDATE, a)
        };
        assert!(update(json!({"state": "open", "state_reason": "completed"})).unwrap_err().message.contains("`state_reason`"));
        assert!(update(json!({"state": "closed", "state_reason": "duplicate"})).unwrap_err().message.contains("`duplicate_of`"));
        assert!(update(json!({"state": "closed", "state_reason": "completed", "duplicate_of": 3})).unwrap_err().message.contains("`duplicate_of`"));
        assert!(update(json!({"state": "closed", "state_reason": "duplicate", "duplicate_of": 3})).is_ok());
        assert!(update(json!({})).unwrap_err().message.contains("at least one"));
    }
}
