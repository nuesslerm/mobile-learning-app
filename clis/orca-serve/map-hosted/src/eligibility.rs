//! `hostedReview.getCreationEligibility`: whether the phone may offer to create a review of a branch, read as the host reads it. The provider comes
//! from the repository's remotes (`pr.remotes`), the default base from origin's HEAD or the conventional branches (`git.upstreamStatus`,
//! `git.refs`), the existing review from `pr.*` or `mr.*`, and the sign-in from `sourceControl.discover`. The host swallows every failure of those
//! reads into the answer; only the selectors and the schema are errors.
use crate::github::{int_nonnegative, int_positive, required_string};
use crate::{refusal, slug, text, Target};
use agent_api_git::{REFS, UPSTREAM_STATUS};
use agent_api_hosted::auth::DISCOVER;
use agent_api_hosted::pr;
use orca_core::wire::{received, Failure, RUNTIME_ERROR};
use orca_core::AgentError;
use serde_json::{json, Value};

const LINKS: [&str; 6] = ["linkedGitHubPR", "fallbackGitHubPR", "linkedGitLabMR", "linkedBitbucketPR", "linkedAzureDevOpsPR", "linkedGiteaPR"];

/// The repository default the host probes when origin's HEAD is not recorded, in its order, as (name, kind) rows of `git.refs`.
const DEFAULT_BASES: [(&str, &str); 4] = [("origin/main", "remote"), ("origin/master", "remote"), ("main", "branch"), ("master", "branch")];

fn expected(kind: &str, v: &Value) -> Failure {
    Failure::invalid(format!("Invalid input: expected {kind}, received {}", received(v)))
}

/// The schema of `hostedReview.getCreationEligibility` in its key order, answering zod's first issue; unknown keys and `wakeWslDistro` are ignored.
pub(crate) fn check_params(params: &Value) -> Result<(), Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    match params.get("worktree") {
        None => {}
        Some(Value::String(s)) if s.is_empty() => return Err(Failure::invalid("Missing worktree selector")),
        Some(Value::String(_)) => {}
        Some(other) => return Err(expected("string", other)),
    }
    required_string(params, "branch", "Missing branch")?;
    match params.get("base") {
        None | Some(Value::Null | Value::String(_)) => {}
        Some(other) => return Err(expected("string", other)),
    }
    for key in ["hasUncommittedChanges", "hasUpstream"] {
        match params.get(key) {
            None | Some(Value::Bool(_)) => {}
            Some(other) => return Err(expected("boolean", other)),
        }
    }
    for key in ["ahead", "behind"] {
        if let Some(v) = params.get(key) {
            int_nonnegative(Some(v))?;
        }
    }
    for key in LINKS {
        if let Some(v) = params.get(key).filter(|v| !v.is_null()) {
            int_positive(Some(v))?;
        }
    }
    Ok(())
}

/// `normalizeHostedReviewHeadRef`: the ref without `refs/heads/` or `refs/remotes/<remote>/`.
fn head_ref(r: &str) -> &str {
    let r = r.trim();
    let r = r.strip_prefix("refs/heads/").unwrap_or(r);
    r.strip_prefix("refs/remotes/").and_then(|rest| rest.split_once('/')).map_or(r, |(_, branch)| branch)
}

/// `normalizeHostedReviewBaseRef`: the head ref without a leading `origin/` or `upstream/`.
fn base_ref(r: &str) -> &str {
    let r = head_ref(r);
    r.strip_prefix("origin/").or_else(|| r.strip_prefix("upstream/")).unwrap_or(r)
}

/// `isSafeGitRefName` of `refs/remotes/<base>`: what git takes as a ref name, so a pattern or control byte is never probed.
fn safe_ref(base: &str) -> bool {
    let bad_char = |c: char| c.is_control() || matches!(c, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\' | '\u{7f}');
    !base.contains(bad_char)
        && !base.contains("..")
        && !base.contains("@{")
        && !base.contains("//")
        && !base.ends_with('/')
        && !base.ends_with('.')
        && base.split('/').all(|part| !part.starts_with('.') && !part.ends_with(".lock"))
}

/// The other refusals of a read the host swallows, with an unknown workspace (and an unreachable rca) still the selector's error.
fn swallowed<T>(result: Result<T, AgentError>) -> Result<Option<T>, Failure> {
    match result {
        Ok(v) => Ok(Some(v)),
        Err(AgentError::Refused { code, .. }) if code != "UNKNOWN_WORKSPACE" => Ok(None),
        Err(other) => Err(refusal(other)),
    }
}

/// `baseRefExistsOnRemote`: a remote-tracking branch of `base` under its own remote, origin, upstream or any configured remote, or exactly one
/// remote segment before it. A read that fails proves nothing missing, so it counts as present.
fn on_remote(t: &Target, candidate: &str, remotes: &[String]) -> Result<bool, Failure> {
    let base = base_ref(candidate).trim();
    if base.is_empty() || !safe_ref(base) {
        return Ok(false);
    }
    let Some(listed) = swallowed(t.try_ask(REFS, json!({"kind": "remote", "query": base})))? else { return Ok(true) };
    let segments = base.split('/').count() + 1;
    let names = ["origin", "upstream"].into_iter().chain(remotes.iter().map(String::as_str));
    let wanted: Vec<String> = names.map(|r| format!("{r}/{base}")).chain(base.contains('/').then(|| base.to_string())).collect();
    Ok(listed["refs"].as_array().into_iter().flatten().filter(|r| r["kind"] == "remote").filter_map(|r| r["name"].as_str()).any(|name| {
        wanted.iter().any(|w| w == name) || (name.ends_with(&format!("/{base}")) && name.split('/').count() == segments)
    }))
}

/// `resolveDefaultBaseRefViaExec`: the branch origin's HEAD points at, else the first of the conventional branches the repository has; a failed
/// read is none.
fn default_base(t: &Target, origin_head: Option<String>) -> Result<Option<String>, Failure> {
    if origin_head.is_some() {
        return Ok(origin_head);
    }
    let Some(listed) = swallowed(t.try_ask(REFS, json!({"query": "ma"})))? else { return Ok(None) };
    let refs = listed["refs"].as_array().cloned().unwrap_or_default();
    Ok(DEFAULT_BASES.iter().find(|(name, kind)| refs.iter().any(|r| r["name"] == *name && r["kind"] == *kind)).map(|(name, _)| name.to_string()))
}

/// The provider whose host the repository's remotes name, as the host's precedence detects it: a gitlab.com origin, then a github.com origin or
/// the remote standing in for it.
fn provider_of(remotes: &[Value]) -> &'static str {
    let url = |name: &str| remotes.iter().find(|r| r["name"] == name).and_then(|r| r["url"].as_str());
    if url("origin").is_some_and(slug::is_gitlab_com_project) {
        return "gitlab";
    }
    let github = |name: &str| url(name).and_then(slug::github_com_slug).is_some();
    if github("origin") || slug::stand_in(remotes).is_some_and(|name| github(&name)) {
        return "github";
    }
    "unsupported"
}

fn linked(params: &Value, key: &str) -> Option<u64> {
    params.get(key).and_then(Value::as_u64).filter(|n| *n > 0)
}

/// The review `number` as `{number, url}`, or none when the read fails.
fn by_number(t: &Target, view: agent_api_proto::Method, n: u64) -> Result<Option<Value>, Failure> {
    Ok(swallowed(t.try_ask(view, json!({"number": n})))?.map(|v| json!({"number": v["pr"]["number"].as_u64().unwrap_or(n), "url": v["pr"]["url"]})))
}

/// The branch's open review, else the latest one that is not, as `{number, url}`; `Err(None)` when the list cannot be read.
fn by_branch(t: &Target, list: agent_api_proto::Method, branch: &str) -> Result<Result<Option<Value>, ()>, Failure> {
    let Some(listed) = swallowed(t.try_ask(list, json!({"head": branch, "state": "all", "limit": 20})))? else { return Ok(Err(())) };
    let Some(all) = listed["prs"].as_array() else { return Err(Failure::new(RUNTIME_ERROR, format!("rca's {} answered no prs list", list.name()))) };
    Ok(Ok(all.iter().find(|p| p["state"] == "open").or_else(|| all.first()).map(|p| json!({"number": p["number"], "url": p["url"]}))))
}

/// The review of the branch the provider holds (a linked number is read directly, GitHub's fallback only when there is no link and before the
/// branch), or `Err(())` when the lookup failed, which the host keeps apart from an authoritative absence.
fn review_of(t: &Target, params: &Value, branch: &str, provider: &str) -> Result<Result<Option<Value>, ()>, Failure> {
    let (view, list, exact, fallback) = match provider {
        "github" => (pr::VIEW, pr::LIST, linked(params, "linkedGitHubPR"), linked(params, "fallbackGitHubPR").filter(|_| linked(params, "linkedGitHubPR").is_none())),
        "gitlab" => (pr::MR_VIEW, pr::MR_LIST, linked(params, "linkedGitLabMR"), None),
        _ => return Ok(Ok(None)),
    };
    if let Some(n) = exact {
        return Ok(by_number(t, view, n)?.map(Some).ok_or(()));
    }
    if let Some(n) = fallback {
        if let Some(found) = by_number(t, view, n)? {
            return Ok(Ok(Some(found)));
        }
    }
    if branch.is_empty() {
        return Ok(Ok(None));
    }
    by_branch(t, list, branch)
}

/// Whether the provider's CLI is installed and signed in on the executor's machine for the repository's host; a failed probe is not signed in.
fn signed_in(t: &Target, provider: &str) -> Result<bool, Failure> {
    let host = if provider == "gitlab" { "gitlab.com" } else { "github.com" };
    let status = swallowed(t.try_ask(DISCOVER, json!({"provider": provider, "host": host})))?;
    Ok(status.is_some_and(|s| s["providers"][0]["installed"] == true && s["providers"][0]["signed_in"] == true))
}

/// The worktree the request names, which must be a checkout of the repo: an id names its repo before `::`, and a worktree named otherwise is
/// taken as the repo's unless it resolves to another repo.
fn worktree_of<'a>(repo: &Target<'a>, selector: &str) -> Result<Target<'a>, Failure> {
    let found = orca_map_workspace::resolve(repo.agent, selector)?;
    let selector = selector.trim();
    let named = (!selector.starts_with("path:")).then(|| selector.strip_prefix("id:").unwrap_or(selector).split_once("::").map(|(r, _)| r)).flatten();
    let owned = match named {
        Some(r) => r == repo.workspace,
        None => found.workspace == repo.workspace || found.kind == "worktree",
    };
    if !owned {
        return Err(Failure::new(RUNTIME_ERROR, "Access denied: worktree does not belong to repository"));
    }
    Ok(repo.with_workspace(found.workspace))
}

/// `getHostedReviewCreationEligibility`: the provider, the branch's review and the default base, then the first rule of the host's ladder that blocks
/// creation, or whether a base is known.
pub fn eligibility(repo: &Target, params: &Value) -> Result<Value, Failure> {
    let worktree;
    let t = match text(params, "worktree") {
        Some(selector) => {
            worktree = worktree_of(repo, selector)?;
            &worktree
        }
        None => repo,
    };
    let branch = head_ref(params["branch"].as_str().unwrap_or("")).trim();
    let listed = swallowed(t.try_ask(pr::REMOTES, json!({})))?;
    let remotes = listed.as_ref().and_then(|l| l["remotes"].as_array().cloned()).unwrap_or_default();
    let names: Vec<String> = remotes.iter().filter_map(|r| r["name"].as_str().map(str::to_string)).collect();
    let provider = provider_of(&remotes);
    let candidate = params["base"].as_str().map(str::trim).filter(|b| !b.is_empty());
    let default_base_ref = match candidate {
        Some(c) if on_remote(t, c, &names)? => Some(c.to_string()),
        _ => {
            let origin_head = swallowed(t.try_ask(UPSTREAM_STATUS, json!({})))?.and_then(|u| u["origin_head"].as_str().map(str::to_string));
            default_base(t, origin_head)?.or(candidate.map(str::to_string))
        }
    };
    let base_branch = default_base_ref.as_deref().map(base_ref);
    let looked_up = if branch.is_empty() && !LINKS.iter().any(|k| linked(params, k).is_some()) {
        Ok(None)
    } else if listed.is_none() {
        Err(())
    } else {
        review_of(t, params, branch, provider)?
    };
    let lookup_failed = looked_up.is_err();
    let review = looked_up.unwrap_or(None);
    let mut answer = json!({
        "provider": provider,
        "review": review.clone().unwrap_or(Value::Null),
        "reviewLookupOutcome": if review.is_some() { "found" } else if lookup_failed { "unavailable" } else { "not_found" },
        "defaultBaseRef": default_base_ref,
        "head": if branch.is_empty() { Value::Null } else { json!(branch) },
    });
    if provider == "github" {
        answer["stackedCreationSupported"] = json!(true);
    }
    let block = |answer: &mut Value, reason: Option<&str>, next: Option<&str>| {
        answer["canCreate"] = json!(false);
        answer["blockedReason"] = json!(reason);
        answer["nextAction"] = json!(next);
    };
    let flag = |key: &str| params.get(key).and_then(Value::as_bool);
    let count = |key: &str| params.get(key).and_then(Value::as_u64).unwrap_or(0);
    if branch.is_empty() || branch == "HEAD" {
        block(&mut answer, Some("detached_head"), None);
    } else if review.is_some() {
        block(&mut answer, Some("existing_review"), Some("open_existing_review"));
    } else if provider == "unsupported" {
        block(&mut answer, Some("unsupported_provider"), None);
    } else if base_branch.is_some_and(|b| !b.is_empty() && b.to_lowercase() == branch.to_lowercase()) {
        block(&mut answer, Some("default_branch"), None);
    } else if flag("hasUncommittedChanges") == Some(true) {
        block(&mut answer, Some("dirty"), Some("commit"));
    } else if flag("hasUpstream") == Some(false) {
        block(&mut answer, Some("no_upstream"), Some("publish"));
    } else if flag("hasUpstream") != Some(true) {
        block(&mut answer, None, None);
    } else if count("behind") > 0 {
        block(&mut answer, Some("needs_sync"), Some("sync"));
    } else if !signed_in(t, provider)? {
        block(&mut answer, Some("auth_required"), Some("authenticate"));
    } else if count("ahead") > 0 {
        block(&mut answer, Some("needs_push"), Some("push"));
    } else {
        answer["canCreate"] = json!(!lookup_failed && base_branch.is_some_and(|b| !b.is_empty()));
        answer["blockedReason"] = Value::Null;
        answer["nextAction"] = Value::Null;
    }
    Ok(answer)
}
