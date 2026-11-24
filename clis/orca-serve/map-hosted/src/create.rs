//! `hostedReview.create`: a pull request (GitHub) or merge request (GitLab) opened by rca's `pr.create` or `mr.create`, after the host's own checks
//! of the branch: the repository's provider (`pr.remotes`), the branch and whether it is clean (`git.status`), its upstream (`git.upstreamStatus`),
//! then the eligibility rules of `getCreationEligibility` with the submitted base rechecked on a remote. Every outcome but a schema or selector
//! failure is the host's `CreateHostedReviewResult` with its copy. Bitbucket, Azure DevOps and Gitea have no CLI on rca, so no remote is theirs here.
use crate::eligibility::{base_ref, evaluate, head_ref, provider_of, swallowed, worktree_of};
use crate::github::required_string;
use crate::{refusal, required, slug, text, Target};
use agent_api_git::{STATUS, UPSTREAM_STATUS};
use agent_api_hosted::pr;
use agent_api_proto::Method as AgentMethod;
use orca_map_workspace::FILE_READ;
use orca_core::wire::{received, string_field, Failure};
use orca_core::AgentError;
use serde_json::{json, Value};

/// How the executor's refusal starts when the CLI has no signed-in account (`exec_hosted::NOT_SIGNED_IN`).
const NOT_SIGNED_IN: &str = "not signed in to ";
const PROVIDERS: [&str; 5] = ["github", "gitlab", "bitbucket", "azure-devops", "gitea"];
/// The folders the host looks in for a GitHub template, in its order, each for the lower-case name and then the upper-case one.
const GITHUB_TEMPLATE_DIRS: [&str; 3] = [".github/", "", "docs/"];
const GITLAB_TEMPLATES: [&str; 4] = [".gitlab/merge_request_templates/Default.md", ".gitlab/merge_request_templates/default.md", ".gitlab/merge_request_template.md", ".gitlab/MERGE_REQUEST_TEMPLATE.md"];

/// `reviewCopy`: how the host names a provider's review in its messages.
struct Copy {
    short: &'static str,
    label: &'static str,
    name: &'static str,
    auth: &'static str,
}

fn copy(provider: &str) -> Copy {
    let pr = |name, auth| Copy { short: "PR", label: "pull request", name, auth };
    match provider {
        "gitlab" => Copy { short: "MR", label: "merge request", name: "GitLab", auth: "Run glab auth login" },
        "azure-devops" => pr("Azure DevOps", "Set ORCA_AZURE_DEVOPS_TOKEN"),
        "gitea" => pr("Gitea", "Set ORCA_GITEA_TOKEN"),
        "bitbucket" => pr("Bitbucket", "Connect Bitbucket in Settings > Integrations"),
        _ => pr("GitHub", "Run gh auth login"),
    }
}

fn outcome(code: &str, error: String, reason: Option<&str>) -> Value {
    let mut out = json!({"ok": false, "code": code, "error": error});
    if let Some(reason) = reason {
        out["reason"] = json!(reason);
    }
    out
}

fn expected(kind: &str, v: &Value) -> Failure {
    Failure::invalid(format!("Invalid input: expected {kind}, received {}", received(v)))
}

/// The schema `HostedReviewCreate` in its key order, answering zod's first issue; unknown keys are stripped.
pub(crate) fn check_params(params: &Value) -> Result<(), Failure> {
    required_string(params, "repo", "Missing repo selector")?;
    if string_field(params, "worktree")? == Some("") {
        return Err(Failure::invalid("Missing worktree selector"));
    }
    match params.get("provider") {
        Some(Value::String(_)) => {}
        None => return Err(Failure::invalid("Invalid input: expected string, received undefined")),
        Some(other) => return Err(expected("string", other)),
    }
    required_string(params, "base", "Missing base branch")?;
    string_field(params, "head")?;
    required_string(params, "title", "Missing title")?;
    string_field(params, "body")?;
    for key in ["draft", "useTemplate"] {
        match params.get(key) {
            None | Some(Value::Bool(_)) => {}
            Some(other) => return Err(expected("boolean", other)),
        }
    }
    Ok(())
}

/// The handler's refusal of a provider token this host cannot create for, answered before the repository is resolved.
pub(crate) fn gate(params: &Value) -> Option<Value> {
    let provider = params["provider"].as_str().unwrap_or("");
    (!PROVIDERS.contains(&provider)).then(|| outcome("unsupported_provider", "Creating reviews for this provider is not supported yet.".into(), Some("provider_not_supported")))
}

/// `blockedCreateResultForReason`: none for a reason the host has no copy for, which lets creation go on.
fn blocked(reason: &str, c: &Copy, base: &str) -> Option<Value> {
    let failed = |what: &str| format!("Create {} failed: {what} before creating a {}.", c.short, c.label);
    let base = base.trim();
    Some(match reason {
        "auth_required" => outcome("auth_required", format!("Create {} failed: {} is not authenticated. Next step: {} in this environment.", c.short, c.name, c.auth), Some(reason)),
        "unsupported_provider" => outcome("unsupported_provider", format!("Creating {}s requires a {} remote.", c.label, c.name), Some(reason)),
        "dirty" => outcome("validation", failed("commit or discard local changes"), Some(reason)),
        "detached_head" => outcome("validation", failed("switch to a branch"), Some(reason)),
        "default_branch" => outcome("validation", failed("choose a feature branch"), Some(reason)),
        "no_upstream" => outcome("validation", failed("publish this branch"), Some(reason)),
        "needs_push" => outcome("validation", failed("push this branch"), Some(reason)),
        "needs_sync" => outcome("validation", failed("sync this branch"), Some(reason)),
        "base_not_on_remote" => {
            let label = if base.is_empty() { String::new() } else { format!("\"{base}\" ") };
            let mut out = outcome("validation", format!("Create {} failed: the base branch {label}hasn't been pushed to the remote. Choose a pushed base or push it first.", c.short), Some(reason));
            if !base.is_empty() {
                out["base"] = json!(base);
            }
            out
        }
        _ => return None,
    })
}

/// The open request of `head` into `base`, when exactly one is listed; a failed lookup is none, as the host catches it.
fn open_match(t: &Target, list: AgentMethod, args: Value) -> Option<Value> {
    let listed = t.try_ask(list, args).ok()?;
    match listed["prs"].as_array().map(Vec::as_slice) {
        Some([only]) => {
            let number = only["number"].as_u64().filter(|n| *n > 0)?;
            let url = only["url"].as_str().filter(|u| !u.is_empty())?;
            Some(json!({"number": number, "url": url}))
        }
        _ => None,
    }
}

/// `classifyCreatePRError` and `classifyCreateMRError` over what rca said, with its own words for a missing sign-in and a timeout.
fn classify(message: &str, c: &Copy, cli: &str) -> Value {
    let lower = message.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    if any(&["not logged", "not authenticated", "authentication", &format!("{cli} auth login"), "http 401", NOT_SIGNED_IN]) {
        outcome("auth_required", format!("Create {} failed: {} is not authenticated. Next step: run {cli} auth login in this environment.", c.short, c.name), Some("auth_required"))
    } else if any(&["already exists"]) {
        existing(c, None)
    } else if any(&["timed out", "timeout", "did not finish within"]) {
        unconfirmed(c)
    } else if any(&["validation failed", "http 422"]) {
        rejected(c)
    } else {
        outcome("unknown", format!("Create {} failed: {} could not create the {}. Try again in a moment.", c.short, c.name, c.label), Some("create_failed"))
    }
}

fn existing(c: &Copy, review: Option<Value>) -> Value {
    let mut out = outcome("already_exists", format!("A {} already exists for this branch.", c.label), Some("existing_review"));
    if let Some(review) = review {
        out["existingReview"] = review;
    }
    out
}

fn unconfirmed(c: &Copy) -> Value {
    outcome("unknown_completion", format!("{} creation may have completed. Refreshing branch review state...", c.short), Some("creation_unconfirmed"))
}

fn rejected(c: &Copy) -> Value {
    outcome("validation", format!("Create {} failed: {} rejected the {}. Check the base branch and branch state, then try again.", c.short, c.name, c.label), Some("rejected"))
}

/// The provider's review template, read from the first conventional path the workspace has; none is an empty body.
fn template(t: &Target, provider: &str) -> Result<String, Failure> {
    let paths: Vec<String> = if provider == "gitlab" {
        GITLAB_TEMPLATES.iter().map(|p| p.to_string()).collect()
    } else {
        GITHUB_TEMPLATE_DIRS.iter().flat_map(|d| ["pull_request_template.md", "PULL_REQUEST_TEMPLATE.md"].map(|n| format!("{d}{n}"))).collect()
    };
    for path in &paths {
        if let Some(read) = swallowed(t.try_ask(FILE_READ, json!({"path": path})))? {
            return Ok(read["content"].as_str().unwrap_or("").to_string());
        }
    }
    Ok(String::new())
}

/// The github.com repository origin names (or the remote standing in for it), which the host creates the pull request in.
fn github_repo(remotes: &[Value]) -> Option<String> {
    let slug_of = |name: &str| remotes.iter().find(|r| r["name"] == name).and_then(|r| r["url"].as_str()).and_then(slug::github_com_slug);
    slug_of("origin").or_else(|| slug::stand_in(remotes).and_then(|n| slug_of(&n))).map(|(owner, repo)| format!("{owner}/{repo}"))
}

/// `createHostedReview` of the host, in its order: the provider of the remotes, the branch, its state and eligibility, then the provider's create.
pub fn create(repo: &Target, params: &Value) -> Result<Value, Failure> {
    let worktree;
    let t = match text(params, "worktree") {
        Some(selector) => {
            worktree = worktree_of(repo, selector)?;
            &worktree
        }
        None => repo,
    };
    let provider = params["provider"].as_str().unwrap_or("");
    let c = copy(provider);
    let listed = swallowed(t.try_ask(pr::REMOTES, json!({})))?;
    let remotes = listed.as_ref().and_then(|l| l["remotes"].as_array().cloned()).unwrap_or_default();
    if provider_of(&remotes) != provider {
        return Ok(blocked("unsupported_provider", &c, "").unwrap_or_default());
    }
    match preflight(t, params, &c, listed)? {
        Ok(branch) => open(t, params, &c, provider, &remotes, &branch),
        Err(blocked) => Ok(blocked),
    }
}

/// `validateCurrentBranchCanCreateReview`: the branch a review can be created for now, or the host's answer for why not.
fn preflight(t: &Target, params: &Value, c: &Copy, listed: Option<Value>) -> Result<Result<String, Value>, Failure> {
    let status = t.ask(STATUS, json!({}))?;
    let current = if status["detached"] == true { "HEAD" } else { head_ref(status["branch"].as_str().unwrap_or("HEAD")) };
    let requested = params["head"].as_str().map(|h| head_ref(h).trim()).filter(|h| !h.is_empty());
    if requested.is_some_and(|h| h != current) {
        return Ok(Err(outcome("validation", format!("Create {} failed: switch back to the selected branch before creating a {}.", c.short, c.label), Some("branch_changed"))));
    }
    let Some(upstream) = swallowed(t.try_ask(UPSTREAM_STATUS, json!({})))? else {
        return Ok(Err(outcome("validation", format!("Create {} failed: could not verify branch status. Refresh source control and try again.", c.short), Some("branch_status_unverified"))));
    };
    let submitted = base_ref(params["base"].as_str().unwrap_or(""));
    let branch = requested.unwrap_or(current).to_string();
    let state = json!({
        "branch": branch, "base": submitted, "hasUncommittedChanges": status["clean"] == false, "hasUpstream": upstream["has_upstream"] == true,
        "ahead": upstream["ahead"].as_u64().unwrap_or(0), "behind": upstream["behind"].as_u64().unwrap_or(0),
    });
    let origin_head = upstream["origin_head"].as_str().map(str::to_string);
    let eligibility = evaluate(t, &state, listed, Some(origin_head), true)?;
    if eligibility["reviewLookupOutcome"] == "unavailable" {
        return Ok(Err(outcome(
            "validation",
            format!("Create {} failed: Orca could not confirm whether this branch already has a {}. Retry once the {} lookup succeeds.", c.short, c.label, c.name),
            Some("review_lookup_unavailable"),
        )));
    }
    if eligibility["canCreate"] == true {
        return Ok(Ok(branch));
    }
    if eligibility["review"]["url"].as_str().is_some_and(|u| !u.is_empty()) {
        return Ok(Err(existing(c, Some(eligibility["review"].clone()))));
    }
    Ok(match eligibility["blockedReason"].as_str() {
        Some(reason) => blocked(reason, c, submitted).map_or(Ok(branch), Err),
        None => Err(outcome("validation", format!("Create {} failed: refresh source control status and try again.", c.short), Some("branch_status_unverified"))),
    })
}

/// `createGitHubPullRequest` and `createGitLabMergeRequest` over rca: the request opened, or the host's reading of why it was not.
fn open(t: &Target, params: &Value, c: &Copy, provider: &str, remotes: &[Value], branch: &str) -> Result<Value, Failure> {
    let gitlab = provider == "gitlab";
    let base = base_ref(params["base"].as_str().unwrap_or(""));
    let head = params["head"].as_str().map(head_ref).filter(|h| !h.is_empty());
    let title = params["title"].as_str().unwrap_or("").trim();
    if base.is_empty() || title.is_empty() {
        return Ok(outcome("validation", format!("Create {} failed: base branch and title are required.", c.short), None));
    }
    if head.is_some_and(|h| h.to_lowercase() == base.to_lowercase()) {
        return Ok(outcome("validation", format!("Create {} failed: choose a different base branch before creating a {}.", c.short, c.label), Some("same_base_branch")));
    }
    let given = params["body"].as_str();
    let body = if params["useTemplate"] == true && given.is_none_or(|b| b.trim().is_empty()) { template(t, provider)? } else { given.unwrap_or("").to_string() };
    let repo = (!gitlab).then(|| github_repo(remotes)).flatten();
    let with_repo = |mut args: Value| {
        if let Some(r) = &repo {
            args["repo"] = json!(r);
        }
        args
    };
    let mut args = with_repo(json!({"title": title, "head": head.unwrap_or(branch), "base": base, "body": body}));
    if params["draft"] == true {
        args["draft"] = json!(true);
    }
    let (create, list, cli) = if gitlab { (pr::MR_CREATE, pr::MR_LIST, "glab") } else { (pr::CREATE, pr::LIST, "gh") };
    let lookup = |h: &str| open_match(t, list, with_repo(json!({"head": h, "base": base, "state": "open", "limit": 2})));
    let failed = match t.try_ask(create, args) {
        Ok(created) => return Ok(json!({"ok": true, "number": required(&created, "number", create.name())?, "url": required(&created, "url", create.name())?})),
        Err(AgentError::Refused { code, message }) if code == "IO_ERROR" && message.contains("printed no address for the new request") => {
            return Ok(match head.and_then(lookup) {
                Some(found) => json!({"ok": true, "number": found["number"], "url": found["url"]}),
                None => unconfirmed(c),
            });
        }
        Err(AgentError::Refused { code, message }) if code == "UNKNOWN_WORKSPACE" || code == "INVALID_PARAMS" => return Err(refusal(AgentError::Refused { code, message })),
        Err(AgentError::Refused { code, .. }) if code == "INVALID_ARGS" => return Ok(rejected(c)),
        Err(AgentError::Refused { message, .. }) => classify(&message, c, cli),
        Err(AgentError::Failed(_)) => unconfirmed(c),
    };
    if matches!(failed["code"].as_str(), Some("already_exists" | "unknown_completion")) {
        if let Some(found) = head.and_then(lookup) {
            return Ok(existing(c, Some(found)));
        }
    }
    Ok(failed)
}
