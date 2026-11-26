//! Orca's git rows (the ones the phone calls) over rca's git group and its `git.ai` sub-group. The phone's `worktree` selector names an rca workspace, and every row is
//! one or a few `git.*` requests to the executor that owns it (and `git.diff` reads the working tree's side through `file.stat` and `file.readChunk`):
//! nothing here runs git or touches a disk; `git.remoteCommitUrl` reads origin's URL from the hosted group's `pr.remotes`. The rows whose answer
//! needs what rca does not hold is not mapped and stays pending: `git.discoverCommitMessageModels` (one configured model, no list).
use agent_api_git::ai::{GENERATE_COMMIT_MESSAGE, GENERATE_PULL_REQUEST_FIELDS};
use agent_api_git::{BLOB, CHECKOUT, COMMIT, COMPARE, CONFLICT_RESOLVE, DISCARD, FAST_FORWARD, FETCH, HISTORY, MERGE, PULL, PUSH, REBASE, REFS, STAGE, STATUS, UNSTAGE, UPSTREAM_STATUS};
use agent_api_proto::Method as AgentMethod;
use orca_map_workspace::{FILE_READ_CHUNK, FILE_STAT};
use orca_core::wire::{Failure, METHOD_NOT_FOUND, RUNTIME_ERROR};
use orca_core::{Area, AgentApi, AgentError, Ctx, Kind, Method};
use serde_json::{json, Map, Value};

mod ai;
mod compare;
mod diff;
mod read;
mod sync;
mod write;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const GIT_STATUS: &str = "git.status";
pub const GIT_DIFF: &str = "git.diff";
pub const GIT_REMOTE_COMMIT_URL: &str = "git.remoteCommitUrl";
pub const GIT_UPSTREAM_STATUS: &str = "git.upstreamStatus";
pub const GIT_HISTORY: &str = "git.history";
pub const GIT_LOCAL_BRANCHES: &str = "git.localBranches";
pub const GIT_CHECKOUT: &str = "git.checkout";
pub const GIT_STAGE: &str = "git.stage";
pub const GIT_BULK_STAGE: &str = "git.bulkStage";
pub const GIT_UNSTAGE: &str = "git.unstage";
pub const GIT_BULK_UNSTAGE: &str = "git.bulkUnstage";
pub const GIT_DISCARD: &str = "git.discard";
pub const GIT_COMMIT: &str = "git.commit";
pub const GIT_FETCH: &str = "git.fetch";
pub const GIT_PULL: &str = "git.pull";
pub const GIT_PUSH: &str = "git.push";
pub const GIT_FAST_FORWARD: &str = "git.fastForward";
pub const GIT_ABORT_MERGE: &str = "git.abortMerge";
pub const GIT_COMPLETE_MERGE: &str = "git.completeMerge";
pub const GIT_ABORT_REBASE: &str = "git.abortRebase";
pub const GIT_REBASE_FROM_BASE: &str = "git.rebaseFromBase";
pub const GIT_RESOLVE_CONFLICT: &str = "git.resolveConflict";
pub const GIT_BRANCH_COMPARE: &str = "git.branchCompare";
pub const GIT_COMMIT_COMPARE: &str = "git.commitCompare";
pub const GIT_GENERATE_COMMIT_MESSAGE: &str = "git.generateCommitMessage";
pub const GIT_GENERATE_PULL_REQUEST_FIELDS: &str = "git.generatePullRequestFields";
pub const GIT_CANCEL_GENERATE_COMMIT_MESSAGE: &str = "git.cancelGenerateCommitMessage";
pub const GIT_CANCEL_GENERATE_PULL_REQUEST_FIELDS: &str = "git.cancelGeneratePullRequestFields";
pub const GIT_BRANCH_DIFF: &str = "git.branchDiff";
pub const GIT_COMMIT_DIFF: &str = "git.commitDiff";

/// The agent methods this crate sends: a flow test holds the list to the methods really sent.
pub const USES: &[AgentMethod] = &[STATUS, REFS, HISTORY, COMPARE, STAGE, UNSTAGE, DISCARD, COMMIT, CHECKOUT, FETCH, PULL, PUSH, FAST_FORWARD, UPSTREAM_STATUS, MERGE, REBASE, CONFLICT_RESOLVE, GENERATE_COMMIT_MESSAGE, GENERATE_PULL_REQUEST_FIELDS, BLOB, FILE_STAT, FILE_READ_CHUNK, orca_map_hosted::PR_REMOTES];

const METHODS: &[Method] = &[
    Method { name: GIT_STATUS, kind: Kind::Unary },
    Method { name: GIT_DIFF, kind: Kind::Unary },
    Method { name: GIT_REMOTE_COMMIT_URL, kind: Kind::Unary },
    Method { name: GIT_UPSTREAM_STATUS, kind: Kind::Unary },
    Method { name: GIT_HISTORY, kind: Kind::Unary },
    Method { name: GIT_LOCAL_BRANCHES, kind: Kind::Unary },
    Method { name: GIT_CHECKOUT, kind: Kind::Unary },
    Method { name: GIT_STAGE, kind: Kind::Unary },
    Method { name: GIT_BULK_STAGE, kind: Kind::Unary },
    Method { name: GIT_UNSTAGE, kind: Kind::Unary },
    Method { name: GIT_BULK_UNSTAGE, kind: Kind::Unary },
    Method { name: GIT_DISCARD, kind: Kind::Unary },
    Method { name: GIT_COMMIT, kind: Kind::Unary },
    Method { name: GIT_FETCH, kind: Kind::Unary },
    Method { name: GIT_PULL, kind: Kind::Unary },
    Method { name: GIT_PUSH, kind: Kind::Unary },
    Method { name: GIT_FAST_FORWARD, kind: Kind::Unary },
    Method { name: GIT_ABORT_MERGE, kind: Kind::Unary },
    Method { name: GIT_COMPLETE_MERGE, kind: Kind::Unary },
    Method { name: GIT_ABORT_REBASE, kind: Kind::Unary },
    Method { name: GIT_REBASE_FROM_BASE, kind: Kind::Unary },
    Method { name: GIT_RESOLVE_CONFLICT, kind: Kind::Unary },
    Method { name: GIT_BRANCH_COMPARE, kind: Kind::Unary },
    Method { name: GIT_COMMIT_COMPARE, kind: Kind::Unary },
    Method { name: GIT_GENERATE_COMMIT_MESSAGE, kind: Kind::Unary },
    Method { name: GIT_GENERATE_PULL_REQUEST_FIELDS, kind: Kind::Unary },
    Method { name: GIT_CANCEL_GENERATE_COMMIT_MESSAGE, kind: Kind::Unary },
    Method { name: GIT_CANCEL_GENERATE_PULL_REQUEST_FIELDS, kind: Kind::Unary },
    Method { name: GIT_BRANCH_DIFF, kind: Kind::Unary },
    Method { name: GIT_COMMIT_DIFF, kind: Kind::Unary },
];

pub struct GitArea;

impl Area for GitArea {
    fn name(&self) -> &'static str {
        "git"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        // the pinned host checks every param of these two rows before it resolves the selector
        let working = if method == GIT_DIFF { Some(diff::working_params(params)?) } else { None };
        if method == GIT_REMOTE_COMMIT_URL {
            read::check_commit_url(params)?;
        }
        if method == GIT_GENERATE_PULL_REQUEST_FIELDS {
            ai::check_pull_request_fields(params)?;
        }
        let repo = Repo::of(ctx.agent, method, params)?;
        if let Some(p) = working {
            return diff::working(&repo, &p, ctx.request_id);
        }
        match method {
            GIT_STATUS => read::status(&repo),
            GIT_REMOTE_COMMIT_URL => read::remote_commit_url(&repo, params),
            GIT_GENERATE_PULL_REQUEST_FIELDS => ai::generate_pull_request_fields(&repo, params),
            // a generation here is one blocking call to rca, so there is none left running to stop: the pinned host answers the same `ok` when none is
            GIT_CANCEL_GENERATE_COMMIT_MESSAGE | GIT_CANCEL_GENERATE_PULL_REQUEST_FIELDS => Ok(json!({"ok": true})),
            GIT_BRANCH_DIFF => diff::branch(&repo, params),
            GIT_COMMIT_DIFF => diff::commit(&repo, params),
            GIT_UPSTREAM_STATUS => read::upstream_status(&repo, params),
            GIT_HISTORY => read::history(&repo, params),
            GIT_LOCAL_BRANCHES => read::local_branches(&repo),
            GIT_CHECKOUT => write::checkout(&repo, params),
            GIT_STAGE => write::stage(&repo, params),
            GIT_BULK_STAGE => write::bulk_stage(&repo, params),
            GIT_UNSTAGE => write::unstage(&repo, &paths_of(params, "filePath")?),
            GIT_BULK_UNSTAGE => write::unstage(&repo, &paths_of(params, "filePaths")?),
            GIT_DISCARD => write::discard(&repo, &paths_of(params, "filePath")?),
            GIT_COMMIT => write::commit(&repo, params),
            GIT_FETCH => sync::fetch(&repo, params),
            GIT_PULL => sync::pull(&repo, params),
            GIT_PUSH => sync::push(&repo, params),
            GIT_FAST_FORWARD => sync::fast_forward(&repo, params),
            GIT_ABORT_MERGE => sync::abort(&repo, MERGE),
            GIT_ABORT_REBASE => sync::abort(&repo, REBASE),
            GIT_COMPLETE_MERGE => sync::complete_merge(&repo),
            GIT_REBASE_FROM_BASE => sync::rebase_from_base(&repo, params),
            GIT_RESOLVE_CONFLICT => write::resolve_conflict(&repo, params),
            GIT_BRANCH_COMPARE => compare::branch(&repo, params),
            GIT_COMMIT_COMPARE => compare::commit(&repo, params),
            GIT_GENERATE_COMMIT_MESSAGE => ai::generate_commit_message(&repo),
            other => Err(Failure::new(METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}

/// The repository a request names: the agent API, and the rca workspace its `worktree` selector resolved to.
pub(crate) struct Repo<'a> {
    agent: &'a dyn AgentApi,
    workspace: String,
    linked: Option<(String, String)>,
}

pub(crate) fn refusal(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { code, message } => match code.as_str() {
            "UNKNOWN_WORKSPACE" => Failure::new("selector_not_found", "selector_not_found"),
            "INVALID_PARAMS" | "INVALID_ARGS" => Failure::invalid(message),
            _ => Failure::new(RUNTIME_ERROR, message),
        },
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

impl<'a> Repo<'a> {
    fn of(agent: &'a dyn AgentApi, method: &str, params: &Value) -> Result<Repo<'a>, Failure> {
        let selector = params.get("worktree").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| Failure::invalid("Missing worktree selector"))?;
        match orca_map_workspace::git_checkout(agent, selector)? {
            orca_map_workspace::GitCheckout::Workspace(workspace) => Ok(Repo { agent, workspace, linked: None }),
            orca_map_workspace::GitCheckout::Linked { repo, head, branch } => match method {
                GIT_HISTORY | GIT_BRANCH_COMPARE | GIT_COMMIT_COMPARE | GIT_BRANCH_DIFF
                | GIT_COMMIT_DIFF | GIT_REMOTE_COMMIT_URL => {
                    if !matches!(head.len(), 40 | 64) || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(Failure::new(RUNTIME_ERROR, "Missing checkout HEAD"));
                    }
                    Ok(Repo { agent, workspace: repo, linked: Some((head, branch)) })
                }
                _ => Err(Failure::new(RUNTIME_ERROR, "This operation requires an RCA workspace for the selected checkout")),
            },
        }
    }

    /// The group's answer, with a refusal kept as the executor words it.
    pub(crate) fn try_ask(&self, method: AgentMethod, args: Value) -> Result<Value, AgentError> {
        let mut params = match args {
            Value::Object(map) => map,
            _ => Map::new(),
        };
        params.insert("workspace".into(), json!(self.workspace));
        self.agent.request(method.name(), Value::Object(params))
    }

    pub(crate) fn ask(&self, method: AgentMethod, args: Value) -> Result<Value, Failure> {
        self.try_ask(method, args).map_err(refusal)
    }
}

/// The paths a request names: the one under `filePath`, or the list under `filePaths` (which may be empty).
pub(crate) fn paths_of(params: &Value, key: &str) -> Result<Vec<String>, Failure> {
    match params.get(key) {
        Some(Value::String(p)) if !p.is_empty() => Ok(vec![p.clone()]),
        Some(Value::Array(list)) if key == "filePaths" => Ok(list.iter().filter_map(Value::as_str).filter(|p| !p.is_empty()).map(str::to_string).collect()),
        _ => Err(Failure::invalid(if key == "filePath" { "Missing file path" } else { "Missing file paths" })),
    }
}

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}
