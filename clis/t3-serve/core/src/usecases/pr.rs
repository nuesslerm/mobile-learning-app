//! The pull request rows of M11 (`t3-pr`): what the phone reads and does on a pull or merge request, asked of the agent API's `hosted.pr` group
//! (the executor's own `gh` and `glab`, with the machine's own sign-in: t3 serve holds no token and runs no CLI). The agent API answers in
//! neutral snake_case with times in milliseconds; this module is the mapping to T3's wire, and what the CLIs cannot say stays unsaid
//! (a field with no source is absent or empty, never invented).
use super::history::EnvironmentRequest;
use crate::domain::command::Command;
use super::queries::{field, QueryFailure};
use super::service::{Refusal, Service};
use crate::ports::{AgentError, Op};
use serde_json::{json, Map, Value};
use t3_contracts::time::iso;

const UNAVAILABLE: &str = "PullRequestUnavailableError";
const OPERATION: &str = "PullRequestOperationError";
const DEFAULT_HOSTS: [&str; 2] = ["github.com", "gitlab.com"];
/// The files viewed marks the server keeps (the host has no store the CLIs reach), by pull request.
const VIEWED_SCOPE: &str = "t3";

pub(super) const PULL_REQUEST_COMMANDS: [&str; 3] = ["thread.pull-request.link", "thread.pull-request.unlink", "thread.pull-request.watch"];

/// The provider a host belongs to: GitLab's names say so, every other host is GitHub's (Enterprise included).
pub(super) fn provider_of(host: Option<&str>) -> &'static str {
    if host.is_some_and(|h| h.contains("gitlab")) {
        "gitlab"
    } else {
        "github"
    }
}

fn host_of(provider: &str, host: Option<&str>) -> String {
    host.map_or_else(|| if provider == "gitlab" { "gitlab.com" } else { "github.com" }.to_string(), str::to_string)
}

/// The failure of a request of the hosted group as the RPC declares it: the executor's missing CLI and missing sign-in are the unavailable
/// error with the reasons T3 names them, everything else is an operation error.
pub(super) fn failure(operation: &str, provider: &str, e: &AgentError) -> QueryFailure {
    let message = e.message();
    let detail = if message.trim().is_empty() { "the executor refused without saying why".to_string() } else { message };
    let code = if let AgentError::Refused { code, .. } = e { code.as_str() } else { "" };
    let unavailable = |reason: &str| QueryFailure::Error(json!({"_tag": UNAVAILABLE, "reason": reason, "provider": provider, "requirement": detail}));
    match code {
        "CLI_NOT_INSTALLED" => unavailable("cli-missing"),
        "PROVIDER_NOT_SIGNED_IN" => unavailable("cli-unauthenticated"),
        "NOT_FOUND" => QueryFailure::Error(json!({"_tag": OPERATION, "operation": operation, "detail": detail, "reason": "not-found"})),
        _ => QueryFailure::Error(json!({"_tag": OPERATION, "operation": operation, "detail": detail})),
    }
}

fn refuse(operation: &str, detail: &str) -> QueryFailure {
    QueryFailure::Error(json!({"_tag": OPERATION, "operation": operation, "detail": detail}))
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn actor(login: Option<&str>) -> Value {
    login.map_or(Value::Null, |l| json!({"login": l, "name": null, "avatarUrl": null}))
}

fn labels(pr: &Value) -> Vec<Value> {
    pr["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|name| json!({"name": name, "color": null})).collect()
}

fn when(ms: &Value) -> String {
    iso(ms.as_i64().unwrap_or(0))
}

fn when_or_null(ms: &Value) -> Value {
    ms.as_i64().map_or(Value::Null, |m| json!(iso(m)))
}

/// `changes-requested` for the neutral `changes_requested`.
fn dashed(value: &Value) -> Option<String> {
    value.as_str().map(|s| s.replace('_', "-"))
}

fn check_status(state: &str) -> &'static str {
    match state {
        "pass" => "success",
        "fail" => "failure",
        "cancel" => "cancelled",
        "skipped" => "skipped",
        _ => "pending",
    }
}

fn check_rows(checks: &Value) -> Vec<Value> {
    checks["checks"].as_array().into_iter().flatten().filter_map(|c| Some(json!({"name": text(c, "name")?, "status": check_status(c["state"].as_str().unwrap_or("")), "description": text(c, "workflow"), "url": text(c, "url")}))).collect()
}

/// A line count of the paths a change touches, split by what the files are, judged from their paths.
pub fn breakdown(files: &[Value]) -> Value {
    let mut parts: Vec<(&str, u64, u64)> = ["code", "tests", "mocks", "generated", "docs"].iter().map(|k| (*k, 0, 0)).collect();
    for file in files {
        let path = file["path"].as_str().unwrap_or_default().to_lowercase();
        let kind = if path.contains("mock") || path.contains("fixture") {
            "mocks"
        } else if path.contains("test") || path.contains("spec") {
            "tests"
        } else if path.ends_with(".lock") || path.contains("lock.json") || path.contains(".generated.") || path.contains("/dist/") || path.contains("generated/") {
            "generated"
        } else if path.ends_with(".md") || path.starts_with("docs/") {
            "docs"
        } else {
            "code"
        };
        if let Some(part) = parts.iter_mut().find(|(k, _, _)| *k == kind) {
            part.1 += file["additions"].as_u64().unwrap_or(0);
            part.2 += file["deletions"].as_u64().unwrap_or(0);
        }
    }
    Value::Object(parts.into_iter().map(|(k, a, d)| (k.to_string(), json!({"additions": a, "deletions": d}))).collect())
}

/// What the phone may offer for a provider: only what the executor's actions do.
fn capabilities(provider: &str) -> (Value, Value) {
    let github = provider == "github";
    let verdicts = if github { json!(["comment", "approve", "request-changes"]) } else { json!(["comment", "approve"]) };
    let methods = if github { json!(["merge", "squash", "rebase"]) } else { json!(["merge", "squash"]) };
    let actions = json!(["merge", "ready", "draft", "update-branch", "enable-auto-merge"]);
    let caps = json!({
        "diff": true, "comment": true, "actions": actions, "mergeMethods": methods, "updateMethods": ["merge", "rebase"], "search": true, "reactions": true, "labels": true,
        "review": {"inlineComment": false, "reply": true, "resolve": true, "verdicts": verdicts},
        "reviewers": {"request": true, "listCandidates": true}, "edit": {"changeRequest": true, "comment": true},
    });
    let permissions = json!({"actions": actions, "comment": true, "resolve": true, "verdicts": verdicts, "requestReviewers": true, "updateMethods": ["merge", "rebase"], "labels": true, "editChangeRequest": true});
    (caps, permissions)
}

fn reaction_content(content: &str) -> Option<&'static str> {
    Some(match content {
        "thumbs-up" => "+1",
        "thumbs-down" => "-1",
        "laugh" => "laugh",
        "hooray" => "hooray",
        "confused" => "confused",
        "heart" => "heart",
        "rocket" => "rocket",
        "eyes" => "eyes",
        _ => return None,
    })
}

/// A project of the phone's list and where its files are.
#[derive(Clone)]
pub(super) struct Project {
    pub id: String,
    pub title: String,
    pub root: String,
}

/// A pull request or a repository of a project, as a request of the hosted group names it.
#[derive(Clone)]
pub(super) struct Target {
    pub project: Project,
    pub provider: &'static str,
    pub host: Option<String>,
    pub repo: Option<String>,
}

impl Target {
    /// The params every request of the group takes: the provider, a self-hosted host and the repository (the project's remote when none).
    fn params(&self, extra: Value) -> Value {
        let mut params = json!({"provider": self.provider});
        if let Some(host) = self.host.as_deref().filter(|h| !DEFAULT_HOSTS.contains(h)) {
            params["host"] = json!(host);
        }
        if let Some(repo) = &self.repo {
            params["repo"] = json!(repo);
        }
        if let (Some(into), Some(from)) = (params.as_object_mut(), extra.as_object()) {
            into.extend(from.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())));
        }
        params
    }
}

impl Service {
    pub(super) fn projects_named(&self, ids: &[String]) -> Vec<Project> {
        self.engine().read(|state| {
            state
                .projects
                .iter()
                .filter(|(id, _)| ids.is_empty() || ids.contains(id))
                .filter_map(|(id, p)| Some(Project { id: id.clone(), title: p["title"].as_str().unwrap_or(id).to_string(), root: p["workspaceRoot"].as_str()?.to_string() }))
                .collect()
        })
    }

    fn project(&self, operation: &str, id: &str) -> Result<Project, QueryFailure> {
        self.projects_named(&[id.to_string()]).into_iter().next().ok_or_else(|| QueryFailure::Error(json!({"_tag": OPERATION, "operation": operation, "detail": format!("project {id} was not found"), "reason": "not-found"})))
    }

    /// The pull request a `PullRequestRef` names: its project's directory, its provider and repository.
    pub(super) fn target(&self, operation: &str, payload: &Value) -> Result<Target, QueryFailure> {
        let project = self.project(operation, field(payload, "projectId")?)?;
        let host = text(payload, "host").map(str::to_string);
        Ok(Target { provider: provider_of(host.as_deref()), host, repo: text(payload, "repository").map(str::to_string), project })
    }

    pub(super) fn number(payload: &Value) -> Result<u64, QueryFailure> {
        payload.get("number").and_then(Value::as_u64).filter(|n| *n > 0).ok_or_else(|| QueryFailure::Defect("Missing key\n  at [\"number\"]".to_string()))
    }

    pub(super) fn ask_hosted(&self, operation: &str, target: &Target, method: &'static str, extra: Value) -> Result<Value, QueryFailure> {
        self.agent().run(&Op::Hosted { method, root: target.project.root.clone(), params: target.params(extra) }).map_err(|e| failure(operation, target.provider, &e))
    }

    /// The pull request's own read and its number as the next request's params.
    pub(super) fn view(&self, operation: &str, target: &Target, number: u64) -> Result<Value, QueryFailure> {
        Ok(self.ask_hosted(operation, target, "pr.view", json!({"number": number}))?["pr"].clone())
    }

    fn account_of(&self, target: &Target) -> Option<String> {
        let status = self.agent().run(&Op::Hosted { method: "sourceControl.discover", root: target.project.root.clone(), params: target.params(json!({})) }).ok()?;
        status["providers"][0]["account"].as_str().map(str::to_string)
    }

    fn summary_of(&self, target: &Target, pr: &Value, number: u64) -> Result<Value, QueryFailure> {
        let url = text(pr, "url").ok_or_else(|| refuse("summary", "the host gave a pull request without a url"))?;
        let mut summary = Map::new();
        let repository = target.repo.clone().unwrap_or_default();
        summary.extend([
            ("provider".to_string(), json!(target.provider)),
            ("projectId".into(), json!(target.project.id)),
            ("repository".into(), json!(repository)),
            ("number".into(), json!(number)),
            ("title".into(), json!(text(pr, "title").unwrap_or("(untitled)"))),
            ("url".into(), json!(url)),
            ("state".into(), json!(text(pr, "state").unwrap_or("open"))),
            ("isDraft".into(), json!(pr["draft"].as_bool().unwrap_or(false))),
            ("headBranch".into(), json!(text(pr, "head_ref").unwrap_or_default())),
            ("baseBranch".into(), json!(text(pr, "base_ref").unwrap_or_default())),
            ("closedAt".into(), when_or_null(&pr["closed_at_ms"])),
            ("mergedAt".into(), when_or_null(&pr["merged_at_ms"])),
            ("updatedAt".into(), json!(when(&pr["updated_at_ms"]))),
            ("author".into(), actor(text(pr, "author"))),
            ("mergeability".into(), json!(text(pr, "mergeable").unwrap_or("unknown"))),
        ]);
        for (key, from) in [("additions", "additions"), ("deletions", "deletions"), ("changedFiles", "changed_files")] {
            if let Some(n) = pr[from].as_u64() {
                summary.insert(key.into(), json!(n));
            }
        }
        if let Some(decision) = dashed(&pr["review_decision"]) {
            summary.insert("reviewDecision".into(), json!(decision));
        }
        Ok(Value::Object(summary))
    }

    /// `pullRequests.list`: each project's pull requests, newest update first.
    pub fn pr_list(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let ids: Vec<String> = payload["projectIds"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).chain(text(payload, "projectId").map(str::to_string)).collect();
        let host = text(payload, "host").map(str::to_string);
        let provider = provider_of(host.as_deref());
        let filters = &payload["filters"];
        let state = text(payload, "state").unwrap_or("open");
        let query = json!({
            "state": state, "limit": payload["limit"].as_u64().map(|l| l.min(200)), "search": text(payload, "query"), "author": text(filters, "author"),
            "label": filters["labels"][0][0].as_str(),
        });
        let (mut entries, mut errors, mut viewers) = (Vec::new(), Vec::new(), Map::new());
        let projects = self.projects_named(&ids);
        let configured = projects.len() as u64;
        for project in projects {
            let target = Target { provider, host: host.clone(), repo: None, project };
            let fail = |e: QueryFailure, target: &Target| match e {
                QueryFailure::Error(v) => json!({"projectId": target.project.id, "projectTitle": target.project.title, "message": v["detail"].as_str().or(v["requirement"].as_str()).unwrap_or("the host refused")}),
                QueryFailure::Defect(m) => json!({"projectId": target.project.id, "projectTitle": target.project.title, "message": m}),
            };
            let repo = match self.ask_hosted("list", &target, "sourceControl.lookupRepository", json!({})) {
                Ok(answer) => text(&answer["repo"], "slug").unwrap_or_default().to_string(),
                Err(e) => {
                    errors.push(fail(e, &target));
                    continue;
                }
            };
            let target = Target { repo: Some(repo.clone()), ..target };
            let viewer = self.account_of(&target);
            match self.ask_hosted("list", &target, "pr.list", query.clone()) {
                Ok(listed) => {
                    for pr in listed["prs"].as_array().into_iter().flatten() {
                        let mut entry = match self.summary_of(&target, pr, pr["number"].as_u64().unwrap_or(1)) {
                            Ok(entry) => entry,
                            Err(e) => {
                                errors.push(fail(e, &target));
                                continue;
                            }
                        };
                        let requested = viewer.as_deref().is_some_and(|account| pr["reviewers"].as_array().into_iter().flatten().any(|r| r.as_str().is_some_and(|r| r.eq_ignore_ascii_case(account))));
                        let map = entry.as_object_mut().expect("a summary is an object");
                        map.retain(|k, _| !["closedAt", "mergedAt", "changedFiles", "additions", "deletions", "reviewDecision"].contains(&k.as_str()));
                        map.extend([
                            ("host".to_string(), json!(host_of(provider, host.as_deref()))),
                            ("projectTitle".into(), json!(target.project.title)),
                            ("createdAt".into(), json!(when(&pr["created_at_ms"]))),
                            ("additions".into(), json!(0)),
                            ("deletions".into(), json!(0)),
                            ("viewerReviewRequested".into(), json!(requested)),
                            ("labels".into(), json!(labels(pr))),
                        ]);
                        if let Some(decision) = dashed(&pr["review_decision"]) {
                            map.insert("reviewDecision".into(), json!(decision));
                        }
                        entries.push(entry);
                    }
                }
                Err(e) => errors.push(fail(e, &target)),
            }
            if let (false, Some(account)) = (viewers.contains_key(&host_of(provider, host.as_deref())), viewer) {
                viewers.insert(host_of(provider, host.as_deref()), json!(account));
            }
        }
        let providers = if configured == 0 { vec![] } else { vec![json!({"host": host_of(provider, host.as_deref()), "kind": provider, "searchesOnHost": true, "projectCount": configured, "configured": errors.len() as u64 != configured, "detail": null})] };
        Ok(json!({"viewers": viewers, "providers": providers, "entries": entries, "errors": errors, "truncated": false, "nextCursors": {}}))
    }

    /// `pullRequests.listStats`: the line counts of each pull request, from its files.
    pub fn pr_list_stats(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let mut stats = Vec::new();
        for reference in payload["refs"].as_array().into_iter().flatten() {
            let target = self.target("listStats", reference)?;
            let files = self.ask_hosted("listStats", &target, "pr.files", json!({"number": Self::number(reference)?}))?;
            let sum = |key: &str| files["files"].as_array().into_iter().flatten().map(|f| f[key].as_u64().unwrap_or(0)).sum::<u64>();
            stats.push(json!({"projectId": target.project.id, "repository": target.repo.clone().unwrap_or_default(), "number": Self::number(reference)?, "additions": sum("additions"), "deletions": sum("deletions")}));
        }
        Ok(json!({"stats": stats}))
    }

    /// `pullRequests.filterOptions`: the labels the repositories define, or the accounts that can be asked to review in them.
    pub fn pr_filter_options(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let ids: Vec<String> = payload["projectIds"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).chain(text(payload, "projectId").map(str::to_string)).collect();
        let host = text(payload, "host").map(str::to_string);
        let (mut found_labels, mut authors): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
        let query = text(payload, "query").map(str::to_lowercase);
        let wanted = |name: &str| query.as_ref().is_none_or(|q| name.to_lowercase().contains(q));
        for project in self.projects_named(&ids) {
            let target = Target { provider: provider_of(host.as_deref()), host: host.clone(), repo: None, project };
            let repo = self.ask_hosted("filterOptions", &target, "sourceControl.lookupRepository", json!({}))?;
            let target = Target { repo: text(&repo["repo"], "slug").map(str::to_string), ..target };
            if text(payload, "kind") == Some("authors") {
                let users = self.ask_hosted("filterOptions", &target, "pr.reviewerCandidates", json!({}))?;
                authors.extend(users["users"].as_array().into_iter().flatten().filter_map(|u| text(u, "login")).filter(|l| wanted(l)).map(|l| actor(Some(l))));
            } else {
                let defined = self.ask_hosted("filterOptions", &target, "pr.labelCandidates", json!({}))?;
                found_labels.extend(defined["labels"].as_array().into_iter().flatten().filter_map(|l| text(l, "name").filter(|n| wanted(n)).map(|n| json!({"name": n, "color": l["color"]}))));
            }
        }
        found_labels.dedup_by(|a, b| a["name"] == b["name"]);
        authors.dedup_by(|a, b| a["login"] == b["login"]);
        Ok(json!({"labels": found_labels, "authors": authors, "truncated": false}))
    }

    /// `pullRequests.summary`
    pub fn pr_summary(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("summary", payload)?;
        let number = Self::number(payload)?;
        let pr = self.view("summary", &target, number)?;
        self.summary_of(&target, &pr, number)
    }

    /// `pullRequests.preview`
    pub fn pr_preview(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("preview", payload)?;
        let number = Self::number(payload)?;
        let pr = self.view("preview", &target, number)?;
        Ok(json!({"projectId": target.project.id, "repository": target.repo.clone().unwrap_or_default(), "number": number, "title": text(&pr, "title").unwrap_or("(untitled)"), "url": text(&pr, "url").ok_or_else(|| refuse("preview", "the host gave a pull request without a url"))?, "author": actor(text(&pr, "author")), "state": text(&pr, "state").unwrap_or("open"), "isDraft": pr["draft"].as_bool().unwrap_or(false), "createdAt": when(&pr["created_at_ms"])}))
    }

    /// `pullRequests.routing`: the account the machine is signed in as for the project's host.
    pub fn pr_routing(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("routing", payload)?;
        let identity = self.identity("routing", &target)?;
        Ok(json!({"accountId": identity["accountId"], "host": identity["host"], "provider": identity["provider"], "viewer": identity["viewer"], "projectTitle": target.project.title, "workspaceRoot": target.project.root}))
    }

    /// `pullRequests.routingIdentity`: the same for a host, with no project.
    pub fn pr_routing_identity(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let host = field(payload, "host")?;
        let target = Target { provider: provider_of(Some(host)), host: Some(host.to_string()), repo: None, project: Project { id: String::new(), title: String::new(), root: String::new() } };
        self.identity("routingIdentity", &target)
    }

    fn identity(&self, operation: &str, target: &Target) -> Result<Value, QueryFailure> {
        let status = self.ask_hosted(operation, target, "sourceControl.discover", json!({}))?;
        let provider = &status["providers"][0];
        match (provider["signed_in"].as_bool(), text(provider, "account")) {
            (Some(true), Some(account)) => Ok(json!({"accountId": account, "host": provider["host"], "provider": target.provider, "viewer": account})),
            _ => Err(failure(operation, target.provider, &AgentError::Refused { code: "PROVIDER_NOT_SIGNED_IN".into(), message: format!("the executor machine is not signed in to {}", host_of(target.provider, target.host.as_deref())) })),
        }
    }

    /// `pullRequests.detail`: the pull request with its checks and what the host lets the viewer do.
    pub fn pr_detail(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("detail", payload)?;
        let number = Self::number(payload)?;
        let pr = self.view("detail", &target, number)?;
        let checks = check_rows(&self.ask_hosted("detail", &target, "pr.checks", json!({"number": number}))?);
        let (capabilities, permissions) = capabilities(target.provider);
        let mut detail = self.summary_of(&target, &pr, number)?;
        let map = detail.as_object_mut().expect("a summary is an object");
        map.retain(|k, _| !["reviewDecision", "mergeability"].contains(&k.as_str()));
        let merge = target.provider == "github";
        map.extend([
            ("capabilities".to_string(), capabilities),
            ("viewerPermissions".into(), permissions),
            ("projectTitle".into(), json!(target.project.title)),
            ("workspaceRoot".into(), json!(target.project.root)),
            ("body".into(), json!(pr["body"].as_str().unwrap_or_default())),
            ("mergeability".into(), json!(text(&pr, "mergeable").unwrap_or("unknown"))),
            ("createdAt".into(), json!(when(&pr["created_at_ms"]))),
            ("reviewers".into(), json!(pr["reviewers"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|l| actor(Some(l))).collect::<Vec<_>>())),
            ("labels".into(), json!(labels(&pr))),
            ("checks".into(), json!(checks)),
            ("mergeCapabilities".into(), json!({"merge": true, "squash": true, "rebase": merge})),
        ]);
        for (key, from) in [("additions", "additions"), ("deletions", "deletions"), ("changedFiles", "changed_files")] {
            map.insert(key.into(), json!(pr[from].as_u64().unwrap_or(0)));
        }
        if let Some(sha) = text(&pr, "head_sha") {
            map.insert("headSha".into(), json!(sha));
        }
        Ok(detail)
    }

    /// `pullRequests.checks`
    pub fn pr_checks(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("checks", payload)?;
        let number = Self::number(payload)?;
        let pr = self.view("checks", &target, number)?;
        let checks = self.ask_hosted("checks", &target, "pr.checks", json!({"number": number}))?;
        Ok(json!({"state": text(&pr, "state").unwrap_or("open"), "checks": check_rows(&checks)}))
    }

    /// `pullRequests.activity`: the conversation, the reviews and the review threads. The CLIs list no commits, so none are given.
    pub fn pr_activity(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("activity", payload)?;
        let number = Self::number(payload)?;
        let ask = |method: &'static str| self.ask_hosted("activity", &target, method, json!({"number": number}));
        let (comments, reviews, threads) = (ask("pr.comments")?, ask("pr.reviews")?, ask("pr.threads")?);
        let mut rows: Vec<Value> = comments["comments"].as_array().into_iter().flatten().map(|c| json!({"id": c["id"], "kind": "issue-comment", "author": actor(text(c, "author")), "body": c["body"], "createdAt": when(&c["created_at_ms"]), "url": c["url"], "path": null, "reviewState": null})).collect();
        rows.extend(reviews["reviews"].as_array().into_iter().flatten().map(|r| json!({"id": r["id"], "kind": "review", "author": actor(text(r, "author")), "body": r["body"], "createdAt": when(&r["submitted_at_ms"]), "url": null, "path": null, "reviewState": r["state"]})));
        let thread_rows: Vec<Value> = threads["threads"].as_array().into_iter().flatten().filter_map(thread_row).collect();
        Ok(json!({"comments": rows, "commentCount": rows.len(), "commentsTruncated": false, "reviewThreads": thread_rows, "commits": []}))
    }

    /// `pullRequests.threadComments`: the comments of one review thread.
    pub fn pr_thread_comments(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("threadComments", payload)?;
        let (number, thread) = (Self::number(payload)?, field(payload, "threadId")?);
        let threads = self.ask_hosted("threadComments", &target, "pr.threads", json!({"number": number}))?;
        let found = threads["threads"].as_array().into_iter().flatten().find(|t| t["id"] == thread).and_then(thread_row);
        match found {
            Some(row) => Ok(json!({"comments": row["comments"], "nextCursor": null})),
            None => Err(QueryFailure::Error(json!({"_tag": OPERATION, "operation": "threadComments", "detail": format!("thread {thread} was not found"), "reason": "not-found"}))),
        }
    }

    /// `pullRequests.changeBreakdown`
    pub fn pr_change_breakdown(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("changeBreakdown", payload)?;
        let files = self.ask_hosted("changeBreakdown", &target, "pr.files", json!({"number": Self::number(payload)?}))?;
        Ok(breakdown(files["files"].as_array().map_or(&[], Vec::as_slice)))
    }

    /// `pullRequests.labelCandidates`: the labels the repository defines and which the pull request has.
    pub fn pr_label_candidates(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("labelCandidates", payload)?;
        let pr = self.view("labelCandidates", &target, Self::number(payload)?)?;
        let applied: Vec<&str> = pr["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let defined = self.ask_hosted("labelCandidates", &target, "pr.labelCandidates", json!({}))?;
        let candidates: Vec<Value> = defined["labels"].as_array().into_iter().flatten().filter_map(|l| Some(json!({"name": text(l, "name")?, "color": l["color"], "description": l["description"], "isApplied": applied.contains(&text(l, "name")?)}))).collect();
        Ok(json!({"candidates": candidates, "truncated": false}))
    }

    /// `pullRequests.reviewerCandidates`
    pub fn pr_reviewer_candidates(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("reviewerCandidates", payload)?;
        let pr = self.view("reviewerCandidates", &target, Self::number(payload)?)?;
        let requested: Vec<&str> = pr["reviewers"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let users = self.ask_hosted("reviewerCandidates", &target, "pr.reviewerCandidates", json!({}))?;
        let candidates: Vec<Value> = users["users"].as_array().into_iter().flatten().filter_map(|u| {
            let login = text(u, "login")?;
            Some(json!({"login": login, "name": u["name"], "avatarUrl": null, "id": login, "kind": "user", "isRequested": requested.contains(&login)}))
        }).collect();
        Ok(json!({"candidates": candidates, "truncated": false}))
    }

    /// One action of the hosted group for the pull request `payload` names, answered with no value.
    fn act(&self, operation: &str, payload: &Value, method: &'static str, extra: Value) -> Result<(), QueryFailure> {
        let target = self.target(operation, payload)?;
        let mut params = json!({"number": Self::number(payload)?});
        if let (Some(into), Some(from)) = (params.as_object_mut(), extra.as_object()) {
            into.extend(from.clone());
        }
        self.ask_hosted(operation, &target, method, params).map(drop)
    }

    pub fn pr_comment(&self, payload: &Value) -> Result<(), QueryFailure> {
        self.act("comment", payload, "pr.comment", json!({"body": field(payload, "body")?}))
    }

    pub fn pr_reply(&self, payload: &Value) -> Result<(), QueryFailure> {
        self.act("replyToThread", payload, "pr.reply", json!({"thread": field(payload, "threadId")?, "body": field(payload, "body")?}))
    }

    pub fn pr_set_thread_resolution(&self, payload: &Value) -> Result<(), QueryFailure> {
        self.act("setThreadResolution", payload, "pr.resolveThread", json!({"thread": field(payload, "threadId")?, "resolved": payload["resolved"].as_bool().unwrap_or(true)}))
    }

    pub fn pr_set_reaction(&self, payload: &Value) -> Result<(), QueryFailure> {
        let content = reaction_content(field(payload, "content")?).ok_or_else(|| QueryFailure::Defect("Expected a reaction content\n  at [\"content\"]".into()))?;
        let subject = match text(payload, "subjectId") {
            Some(id) => json!({"target": "comment", "comment_id": id.parse::<u64>().map_err(|_| refuse("setReaction", "a reaction names a comment by its numeric id"))?}),
            None => json!({"target": "pr"}),
        };
        let mut extra = subject;
        extra["content"] = json!(content);
        extra["undo"] = if payload["reacted"] == json!(false) { json!(true) } else { Value::Null };
        self.act("setReaction", payload, "pr.react", extra)
    }

    /// `pullRequests.updateComment`: a conversation comment or a review comment given new text; the comment's id and the text go to the executor,
    /// which sends the text to the host on stdin.
    pub fn pr_update_comment(&self, payload: &Value) -> Result<(), QueryFailure> {
        let kind = match field(payload, "kind")? {
            "review-comment" => "review",
            _ => "issue",
        };
        self.act("updateComment", payload, "pr.editComment", json!({"comment": field(payload, "commentId")?, "kind": kind, "body": field(payload, "body")?}))
    }

    /// `pullRequests.stack`: the native stack the pull request is in, its layers bottom to top; null when the host says it is in none.
    pub fn pr_stack(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("stack", payload)?;
        let answer = self.ask_hosted("stack", &target, "pr.stack", json!({"number": Self::number(payload)?}))?;
        let stack = &answer["stack"];
        if stack.is_null() {
            return Ok(Value::Null);
        }
        let layers: Vec<Value> = stack["layers"].as_array().into_iter().flatten().map(|l| {
            let mut layer = json!({"number": l["number"], "headBranch": l["head_ref"], "state": l["state"]});
            for (key, from) in [("title", "title"), ("headSha", "head_sha")] {
                if let Some(v) = text(l, from) {
                    layer[key] = json!(v);
                }
            }
            if let Some(draft) = l["draft"].as_bool() {
                layer["isDraft"] = json!(draft);
            }
            layer
        }).collect();
        Ok(json!({"id": stack["id"], "number": stack["number"], "url": stack["url"], "base": stack["base"], "layers": layers}))
    }

    pub fn pr_request_reviewers(&self, payload: &Value) -> Result<(), QueryFailure> {
        let ids: Vec<&str> = payload["reviewers"].as_array().into_iter().flatten().filter_map(|r| r["id"].as_str()).collect();
        let key = if payload["requested"].as_bool().unwrap_or(true) { "add" } else { "remove" };
        self.act("requestReviewers", payload, "pr.requestReviewers", json!({ key: ids }))
    }

    pub fn pr_set_labels(&self, payload: &Value) -> Result<(), QueryFailure> {
        let names: Vec<&str> = payload["labels"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let key = if payload["applied"].as_bool().unwrap_or(true) { "add" } else { "remove" };
        self.act("setLabels", payload, "pr.setLabels", json!({ key: names }))
    }

    pub fn pr_submit_review(&self, payload: &Value) -> Result<(), QueryFailure> {
        if payload["comments"].as_array().is_some_and(|c| !c.is_empty()) {
            return Err(refuse("submitReview", "inline review comments are not supported by the host's CLI; send the review without them"));
        }
        let verdict = match field(payload, "verdict")? {
            "approve" => "approve",
            "request-changes" => "request_changes",
            _ => "comment",
        };
        self.act("submitReview", payload, "pr.review", json!({"verdict": verdict, "body": payload["body"].as_str().filter(|b| !b.is_empty())}))
    }

    pub fn pr_update(&self, payload: &Value) -> Result<(), QueryFailure> {
        let (title, body) = (text(payload, "title"), payload.get("body").and_then(Value::as_str));
        if title.is_none() && body.is_none() {
            return Err(refuse("update", "only the title and the description can be changed through the host's CLI"));
        }
        self.act("update", payload, "pr.edit", json!({"title": title, "body": body}))
    }

    /// `pullRequests.runAction`: merge, ready, draft, update-branch and auto-merge; the rest are not things the CLIs do.
    pub fn pr_run_action(&self, payload: &Value) -> Result<(), QueryFailure> {
        let method = text(payload, "mergeMethod");
        match field(payload, "action")? {
            "merge" => self.act("runAction", payload, "pr.merge", json!({"method": method})),
            "enable-auto-merge" => self.act("runAction", payload, "pr.merge", json!({"method": method, "auto": true})),
            "ready" => self.act("runAction", payload, "pr.ready", json!({})),
            "draft" => self.act("runAction", payload, "pr.ready", json!({"undo": true})),
            "update-branch" => self.act("runAction", payload, "pr.updateBranch", json!({"rebase": text(payload, "updateMethod") == Some("rebase")})),
            other => Err(refuse("runAction", &format!("{other} is not something the host's CLI does"))),
        }
    }

    /// The diff of a pull request, as the HTTP route and nothing else asks for it: the executor cuts it at its output cap and says so.
    pub fn pr_diff(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let target = self.target("diff", payload)?;
        let diff = self.ask_hosted("diff", &target, "pr.diff", json!({"number": Self::number(payload)?}))?;
        Ok(json!({"patch": diff["diff"].as_str().unwrap_or_default(), "truncated": diff["truncated"].as_bool().unwrap_or(false), "nextCursor": null}))
    }

    /// `POST /api/pull-requests/diff`: the same read as an HTTP route, behind the environment gate (protocol, bearer, scope) with the JSON error the route declares.
    pub fn pull_request_diff_http(&self, request: &EnvironmentRequest, body: &[u8]) -> Result<Value, Refusal> {
        self.environment_gate("pullRequests.diff", request)?;
        let payload: Value = serde_json::from_slice(body).map_err(|_| Refusal { status: 400, body: json!({"_tag": "BadRequest", "message": "the body is a JSON pull request reference"}) })?;
        self.pr_diff(&payload).map_err(|failure| match failure {
            QueryFailure::Defect(message) => Refusal { status: 400, body: json!({"_tag": "BadRequest", "message": message}) },
            QueryFailure::Error(error) => Refusal { status: if error["_tag"] == UNAVAILABLE { 503 } else { 502 }, body: error },
        })
    }

    fn thread_links(&self, thread: &str) -> Result<Vec<Value>, String> {
        let stored = self.agent().run(&Op::StateGet { scope: VIEWED_SCOPE.into(), key: format!("thread.pullRequests:{thread}") }).map_err(|e| e.message())?;
        Ok(stored["value"].as_array().cloned().unwrap_or_default())
    }

    fn set_state(&self, key: String, value: Value) -> Result<(), String> {
        self.agent().run(&Op::StateSet { scope: VIEWED_SCOPE.into(), key, value }).map(drop).map_err(|e| e.message())
    }

    /// `thread.pull-request.link`, `.unlink` and `.watch`: the links of a thread kept in the agent API's `state` (per thread, and per pull
    /// request the threads that link it), so they outlive this server's rows and `pullRequests.linkedThreads` can read them back.
    pub(super) fn pull_request_command(&self, cmd: &Command) -> Result<(), String> {
        let thread = cmd.require("threadId")?;
        let (host, repository) = (cmd.require("host")?, cmd.require("repository")?);
        let number = cmd.body.get("number").and_then(Value::as_u64).filter(|n| *n > 0).ok_or_else(|| format!("{} needs number", cmd.kind))?;
        let same = |e: &Value| e["host"] == host && e["repository"] == repository && e["number"] == number;
        let mut links = self.thread_links(thread)?;
        let entry = |source: &Value, url: &Value, watching: bool| json!({"host": host, "repository": repository, "number": number, "url": url, "source": source, "watching": watching});
        match cmd.kind.as_str() {
            "thread.pull-request.link" => {
                let (url, source) = (cmd.require("url")?, cmd.require("source")?);
                let watching = links.iter().find(|e| same(e)).is_some_and(|e| e["watching"] == json!(true));
                links.retain(|e| !same(e));
                links.push(entry(&json!(source), &json!(url), watching));
            }
            "thread.pull-request.unlink" => links.retain(|e| !same(e)),
            _ => {
                let watching = cmd.body.get("watching").and_then(Value::as_bool).ok_or("thread.pull-request.watch needs watching")?;
                match links.iter_mut().find(|e| same(e)) {
                    Some(found) => found["watching"] = json!(watching),
                    None => {
                        let link = cmd.body.get("link").filter(|l| l.is_object()).ok_or("thread.pull-request.watch names a pull request this thread has not linked and gives no link for it")?;
                        links.push(entry(&link["source"], &link["url"], watching));
                    }
                }
            }
        }
        let linked = links.iter().any(same);
        self.set_state(format!("thread.pullRequests:{thread}"), json!(links))?;
        let key = format!("pr.threads:{repository}:{number}");
        let mut threads: Vec<Value> = self.agent().run(&Op::StateGet { scope: VIEWED_SCOPE.into(), key: key.clone() }).map_err(|e| e.message())?["value"].as_array().cloned().unwrap_or_default();
        threads.retain(|t| t != thread);
        if linked {
            threads.push(json!(thread));
        }
        self.set_state(key, json!(threads))
    }

    /// `pullRequests.linkedThreads`: the threads that link the pull request, as the agent API's `state` records them.
    pub fn pr_linked_threads(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let key = format!("pr.threads:{}:{}", field(payload, "repository")?, Self::number(payload)?);
        let stored = self.agent().run(&Op::StateGet { scope: VIEWED_SCOPE.into(), key }).map_err(|e| failure("linkedThreads", "github", &e))?;
        let rows: Vec<Value> = stored["value"].as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(|id| {
            self.engine().read(|state| {
                let shell = state.threads.get(id)?;
                Some(json!({"id": id, "projectId": shell["projectId"], "title": shell["title"], "archivedAt": shell["archivedAt"]}))
            })
        }).collect();
        Ok(json!({"threads": rows}))
    }

    fn viewed_key(&self, payload: &Value) -> Result<String, QueryFailure> {
        Ok(format!("pr.viewed:{}:{}:{}", field(payload, "projectId")?, text(payload, "repository").unwrap_or_default(), Self::number(payload)?))
    }

    fn viewed_files(&self, payload: &Value) -> Result<Map<String, Value>, QueryFailure> {
        let stored = self.agent().run(&Op::StateGet { scope: VIEWED_SCOPE.into(), key: self.viewed_key(payload)? }).map_err(|e| failure("filesViewed", "github", &e))?;
        Ok(stored["value"].as_object().cloned().unwrap_or_default())
    }

    /// `pullRequests.filesViewed`: the marks this server keeps in the agent API's `state` (the CLIs reach no host store of them).
    pub fn pr_files_viewed(&self, payload: &Value) -> Result<Value, QueryFailure> {
        let files: Vec<Value> = self.viewed_files(payload)?.into_iter().map(|(path, state)| json!({"path": path, "state": state})).collect();
        Ok(json!({"files": files, "truncated": false}))
    }

    pub fn pr_set_files_viewed(&self, payload: &Value) -> Result<(), QueryFailure> {
        let mut marks = self.viewed_files(payload)?;
        for press in payload["files"].as_array().into_iter().flatten() {
            let Some(path) = press["path"].as_str() else { continue };
            if press["viewed"].as_bool().unwrap_or(false) {
                marks.insert(path.to_string(), json!("viewed"));
            } else {
                marks.remove(path);
            }
        }
        self.agent().run(&Op::StateSet { scope: VIEWED_SCOPE.into(), key: self.viewed_key(payload)?, value: Value::Object(marks) }).map(drop).map_err(|e| failure("setFilesViewed", "github", &e))
    }
}

/// A review thread of the neutral shape as T3's: a thread with no file has no place in a diff, so it has no row.
fn thread_row(t: &Value) -> Option<Value> {
    let comments: Vec<Value> = t["comments"].as_array().into_iter().flatten().map(|c| json!({"id": c["id"], "author": actor(text(c, "author")), "body": c["body"], "createdAt": when(&c["created_at_ms"]), "url": c["url"]})).collect();
    Some(json!({"id": text(t, "id")?, "path": text(t, "path")?, "line": t["line"].as_u64().filter(|l| *l > 0), "side": "right", "isResolved": t["resolved"].as_bool().unwrap_or(false), "isOutdated": t["outdated"].as_bool().unwrap_or(false), "comments": comments, "commentCount": comments.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_the_files_of_a_change_when_split_then_each_line_count_lands_in_the_kind_its_path_says_and_the_parts_add_up() {
        // Given files of every kind
        let files: Vec<Value> = [("src/a.rs", 5, 1), ("src/a.test.ts", 3, 0), ("mocks/m.ts", 2, 2), ("Cargo.lock", 9, 9), ("docs/x.md", 1, 0)].iter().map(|(p, a, d)| json!({"path": p, "additions": a, "deletions": d})).collect();
        // When split
        let split = breakdown(&files);
        // Then each part has its own and the totals are the files'
        assert_eq!((split["code"]["additions"].as_u64(), split["tests"]["additions"].as_u64(), split["mocks"]["deletions"].as_u64(), split["generated"]["additions"].as_u64(), split["docs"]["additions"].as_u64()), (Some(5), Some(3), Some(2), Some(9), Some(1)));
        let total: u64 = ["code", "tests", "mocks", "generated", "docs"].iter().map(|k| split[k]["additions"].as_u64().unwrap() + split[k]["deletions"].as_u64().unwrap()).sum();
        assert_eq!(total, files.iter().map(|f| f["additions"].as_u64().unwrap() + f["deletions"].as_u64().unwrap()).sum::<u64>());
    }

    #[test]
    fn given_the_refusals_of_the_agent_api_when_mapped_then_a_missing_cli_and_a_missing_sign_in_are_the_unavailable_error_and_the_rest_an_operation_error() {
        let refused = |code: &str| failure("view", "github", &AgentError::Refused { code: code.into(), message: format!("{code} said so") });
        let tag = |f: QueryFailure| match f {
            QueryFailure::Error(v) => (v["_tag"].as_str().unwrap().to_string(), v["reason"].as_str().map(str::to_string)),
            QueryFailure::Defect(m) => panic!("{m}"),
        };
        assert_eq!(tag(refused("CLI_NOT_INSTALLED")), (UNAVAILABLE.to_string(), Some("cli-missing".to_string())));
        assert_eq!(tag(refused("PROVIDER_NOT_SIGNED_IN")), (UNAVAILABLE.to_string(), Some("cli-unauthenticated".to_string())));
        assert_eq!(tag(refused("NOT_FOUND")), (OPERATION.to_string(), Some("not-found".to_string())));
        assert_eq!(tag(refused("IO")), (OPERATION.to_string(), None));
    }

    #[test]
    fn given_a_host_when_its_provider_and_params_are_made_then_gitlab_names_say_gitlab_and_a_public_host_is_not_sent() {
        assert_eq!((provider_of(None), provider_of(Some("github.example.com")), provider_of(Some("gitlab.example.com"))), ("github", "github", "gitlab"));
        let target = |host: Option<&str>| Target { project: Project { id: "p".into(), title: "t".into(), root: "/r".into() }, provider: provider_of(host), host: host.map(str::to_string), repo: Some("o/n".into()) };
        assert_eq!(target(Some("github.com")).params(json!({"number": 3, "none": null})), json!({"provider": "github", "repo": "o/n", "number": 3}));
        assert_eq!(target(Some("gitlab.example.com")).params(json!({}))["host"], "gitlab.example.com");
    }
}
