//! Orca's hosted source control rows over rca's `hosted.pr` group, which asks the executor machine's own `gh` and `glab`: the pull request rows of
//! `github.*`, the merge request rows of `gitlab.*` and the `worktree.resolvePrBase` and `worktree.resolveMrBase` start points (the request's head fetched
//! by `git.fetch` into a ref of its own). A row's `repo` selector names an rca workspace, and a request names no repository, so the executor uses the one
//! the workspace's remote names (a `prRepo` or `projectRef` of the phone names another). A row the CLIs cannot do, or whose answer needs what rca does
//! not hold (issues and work items, projects, inline comments, closing, auto-merge, rate limits, accounts), is not mapped and stays pending.
use agent_api_git::FETCH;
use agent_api_hosted::pr;
use agent_api_proto::Method as AgentMethod;
use orca_core::wire::{Failure, METHOD_NOT_FOUND, RUNTIME_ERROR};
use orca_core::{AgentApi, AgentError, Area, Ctx, Kind, Method};
use serde_json::{json, Map, Value};

mod base;
mod checks;
mod github;
mod gitlab;
mod time;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// The agent methods this crate sends: the `pr.*` and `mr.*` ones it reaches, and the fetch of a request's head.
pub const USES: &[AgentMethod] = &[pr::LIST, pr::VIEW, pr::CHECKS, pr::EDIT, pr::READY, pr::REQUEST_REVIEWERS, pr::MERGE, pr::MR_VIEW, pr::MR_EDIT, pr::MR_READY, pr::MR_SET_LABELS, pr::MR_MERGE, FETCH];

const METHODS: &[Method] = &[
    Method { name: "github.prForBranch", kind: Kind::Unary },
    Method { name: "github.prChecks", kind: Kind::Unary },
    Method { name: "github.mergePR", kind: Kind::Unary },
    Method { name: "github.updatePRTitle", kind: Kind::Unary },
    Method { name: "github.updatePR", kind: Kind::Unary },
    Method { name: "github.markPRReadyForReview", kind: Kind::Unary },
    Method { name: "github.markPRDraft", kind: Kind::Unary },
    Method { name: "github.requestPRReviewers", kind: Kind::Unary },
    Method { name: "github.removePRReviewers", kind: Kind::Unary },
    Method { name: "gitlab.mergeMR", kind: Kind::Unary },
    Method { name: "gitlab.updateMR", kind: Kind::Unary },
    Method { name: "worktree.resolvePrBase", kind: Kind::Unary },
    Method { name: "worktree.resolveMrBase", kind: Kind::Unary },
];

pub struct HostedArea;

impl Area for HostedArea {
    fn name(&self) -> &'static str {
        "hosted"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        let target = Target::of(ctx.agent, params)?;
        match method {
            "github.prForBranch" => github::pr_for_branch(&target, params),
            "github.prChecks" => github::pr_checks(&target, params),
            "github.mergePR" => github::merge(&target, params),
            "github.updatePRTitle" => github::update_title(&target, params),
            "github.updatePR" => github::update(&target, params),
            "github.markPRReadyForReview" => github::ready(&target, params, false),
            "github.markPRDraft" => github::ready(&target, params, true),
            "github.requestPRReviewers" => github::reviewers(&target, params, "add"),
            "github.removePRReviewers" => github::reviewers(&target, params, "remove"),
            "gitlab.mergeMR" => gitlab::merge(&target, params),
            "gitlab.updateMR" => gitlab::update(&target, params),
            "worktree.resolvePrBase" => base::resolve(&target, params, false),
            "worktree.resolveMrBase" => base::resolve(&target, params, true),
            other => Err(Failure::new(METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}

/// The repository a request names: the agent API and the rca workspace its `repo` selector resolved to.
pub(crate) struct Target<'a> {
    agent: &'a dyn AgentApi,
    workspace: String,
}

pub(crate) fn refusal(error: AgentError) -> Failure {
    match error {
        AgentError::Refused { code, .. } if code == "UNKNOWN_WORKSPACE" => Failure::new("selector_not_found", "selector_not_found"),
        AgentError::Refused { code, message } if code == "INVALID_PARAMS" || code == "INVALID_ARGS" => Failure::invalid(message),
        AgentError::Refused { message, .. } => Failure::new(RUNTIME_ERROR, message),
        AgentError::Failed(_) => Failure::new("runtime_unavailable", "runtime_unavailable"),
    }
}

impl<'a> Target<'a> {
    fn of(agent: &'a dyn AgentApi, params: &Value) -> Result<Target<'a>, Failure> {
        let selector = text(params, "repo").ok_or_else(|| Failure::invalid("Missing repo selector"))?;
        Ok(Target { agent, workspace: orca_map_workspace::resolve(agent, selector)?.workspace })
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

/// What an update that changes nothing is refused with, for GitHub and GitLab alike.
pub(crate) const NO_UPDATES: &str = "No updates given";

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// The request's number under `key` (`prNumber` or `iid`): a positive integer, or the host's wording for a missing one.
pub(crate) fn number(params: &Value, key: &str, what: &str) -> Result<u64, Failure> {
    params.get(key).and_then(Value::as_u64).filter(|n| *n > 0).ok_or_else(|| Failure::invalid(format!("Missing {what} number")))
}

/// What a mutation answers: the host's `{ok: true}`, or `{ok: false, error}` with the words of what refused it.
pub(crate) fn outcome(result: Result<Value, AgentError>) -> Result<Value, Failure> {
    match result {
        Ok(_) => Ok(json!({"ok": true})),
        Err(AgentError::Refused { code, message }) if code != "UNKNOWN_WORKSPACE" => Ok(json!({"ok": false, "error": message})),
        Err(other) => Err(refusal(other)),
    }
}

/// The executor's repository args of a request that names its own: `prRepo` (`owner`, `repo`, `host`) of GitHub's rows or `projectRef` (`host`, `path`)
/// of GitLab's.
pub(crate) fn repo_args(params: &Value, key: &str) -> Result<Map<String, Value>, Failure> {
    let mut args = Map::new();
    let named = match params.get(key) {
        None | Some(Value::Null) => return Ok(args),
        Some(named) => named,
    };
    let (path, host) = if key == "prRepo" {
        let (owner, repo) = (text(named, "owner").ok_or_else(|| Failure::invalid("prRepo has no owner"))?, text(named, "repo").ok_or_else(|| Failure::invalid("prRepo has no repo"))?);
        (format!("{owner}/{repo}"), text(named, "host"))
    } else {
        (text(named, "path").ok_or_else(|| Failure::invalid("projectRef has no path"))?.to_string(), text(named, "host"))
    };
    args.insert("repo".into(), json!(path));
    if let Some(host) = host.filter(|h| !matches!(*h, "github.com" | "gitlab.com")) {
        args.insert("host".into(), json!(host));
    }
    Ok(args)
}

/// `args` with the repository the request names.
pub(crate) fn with_repo(mut args: Value, params: &Value, key: &str) -> Result<Value, Failure> {
    if let Some(map) = args.as_object_mut() {
        map.extend(repo_args(params, key)?);
    }
    Ok(args)
}

/// A field the contract of rca's answer requires: refused, never read as empty.
pub(crate) fn required<'a>(answer: &'a Value, key: &str, of: &str) -> Result<&'a Value, Failure> {
    answer.get(key).filter(|v| !v.is_null()).ok_or_else(|| Failure::new(RUNTIME_ERROR, format!("rca's {of} answered no {key}")))
}
