//! A double of rca's `hosted.pr` group and of the `git.fetch` that fetches a request's head: pull requests of GitHub and merge requests of GitLab in
//! memory, answering in the neutral shapes of the contract (`agent-api-hosted`) and changing as the executor's CLIs would. A request without what the
//! contract requires is refused, as the service does, never read as an empty value.
use orca_core::testing::ScriptedAgent;
use orca_core::AgentError;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
struct Request {
    provider: &'static str,
    number: u64,
    title: String,
    body: String,
    state: String,
    draft: bool,
    head_ref: String,
    base_ref: String,
    head_sha: String,
    labels: Vec<String>,
    reviewers: Vec<String>,
    checks: Vec<(String, String)>,
    mergeable: String,
    review_decision: Option<String>,
    updated_ms: u64,
    merged_with: Option<String>,
}

#[derive(Default)]
struct State {
    requests: Vec<Request>,
    /// What the remote holds: `refs/heads/<branch>`, `refs/pull/<n>/head` and `refs/merge-requests/<n>/head` to a commit.
    remote: BTreeMap<String, String>,
    /// The refs a fetch made in the repository.
    local: BTreeMap<String, String>,
    fetch_error: Option<String>,
    labels: Option<Vec<(String, String)>>,
    assignees: Option<Vec<(String, String)>>,
    view_error: Option<String>,
    comments: Vec<(u64, Value)>,
    threads: Vec<(u64, Value)>,
    reviews: Vec<(u64, Value)>,
    blobs: Vec<(String, String, Vec<u8>)>,
    remotes: Vec<(String, String)>,
    remotes_error: Option<String>,
    origin_head: Option<String>,
    signed_out: bool,
    list_error: Option<String>,
}

/// What rca's richer answers add to a comment, a review or a thread comment; every part is optional and a default adds none.
#[derive(Clone, Default)]
pub struct Extras<'a> {
    pub database_id: Option<u64>,
    pub avatar_url: Option<&'a str>,
    pub is_bot: bool,
    pub viewer_did_author: bool,
    pub reaction_subject_id: Option<&'a str>,
    /// (content, count, viewer has reacted)
    pub reactions: &'a [(&'a str, u64, bool)],
}

impl Extras<'_> {
    fn merge(&self, row: &mut Value) {
        let reactions: Vec<Value> = self.reactions.iter().map(|(c, n, v)| json!({"content": c, "count": n, "viewer_has_reacted": v})).collect();
        row["database_id"] = json!(self.database_id);
        row["avatar_url"] = json!(self.avatar_url);
        row["is_bot"] = json!(self.is_bot);
        row["viewer_did_author"] = json!(self.viewer_did_author);
        row["reaction_subject_id"] = json!(self.reaction_subject_id);
        row["reactions"] = json!(reactions);
    }
}

#[derive(Clone, Default)]
pub struct Hosting(Arc<Mutex<State>>);

fn refused(code: &str, message: impl Into<String>) -> AgentError {
    AgentError::Refused { code: code.into(), message: message.into() }
}

fn number(p: &Value) -> Result<u64, AgentError> {
    p["number"].as_u64().filter(|n| *n > 0).ok_or_else(|| refused("INVALID_ARGS", "`number` must be a positive integer"))
}

fn names(p: &Value, key: &str) -> Result<Vec<String>, AgentError> {
    p.get(key).map_or(Ok(Vec::new()), |l| l.as_array().ok_or_else(|| refused("INVALID_ARGS", format!("`{key}` must be an array")))?.iter().map(|x| x.as_str().map(str::to_string).ok_or_else(|| refused("INVALID_ARGS", format!("`{key}` holds a non-string")))).collect())
}

fn neutral(r: &Request) -> Value {
    json!({
        "number": r.number, "title": r.title, "body": r.body, "state": r.state, "draft": r.draft, "author": "octocat", "head_ref": r.head_ref, "base_ref": r.base_ref, "head_sha": r.head_sha,
        "labels": r.labels, "reviewers": r.reviewers, "mergeable": r.mergeable, "review_decision": r.review_decision, "updated_at_ms": r.updated_ms, "created_at_ms": 1_790_847_000_000u64,
        "url": format!("https://example.test/{}/{}", r.provider, r.number), "changed_files": 2, "additions": 4, "deletions": 1, "assignees": [], "merged_at_ms": null, "closed_at_ms": null,
    })
}

impl Hosting {
    fn add(&self, provider: &'static str, number: u64, title: &str, head_ref: &str, base_ref: &str) -> &Self {
        let mut s = self.0.lock().unwrap();
        let head_sha = format!("{:040x}", number * 4096 + 1);
        s.remote.insert(format!("refs/heads/{head_ref}"), head_sha.clone());
        s.remote.insert(if provider == "gitlab" { format!("refs/merge-requests/{number}/head") } else { format!("refs/pull/{number}/head") }, head_sha.clone());
        s.requests.push(Request {
            provider,
            number,
            title: title.into(),
            body: String::new(),
            state: "open".into(),
            draft: false,
            head_ref: head_ref.into(),
            base_ref: base_ref.into(),
            head_sha,
            labels: Vec::new(),
            reviewers: Vec::new(),
            checks: Vec::new(),
            mergeable: "mergeable".into(),
            review_decision: None,
            updated_ms: 1_790_936_130_000 + number,
            merged_with: None,
        });
        self
    }

    pub fn pull_request(&self, number: u64, title: &str, head_ref: &str, base_ref: &str) -> &Self {
        self.add("github", number, title, head_ref, base_ref)
    }

    pub fn merge_request(&self, number: u64, title: &str, head_ref: &str, base_ref: &str) -> &Self {
        self.add("gitlab", number, title, head_ref, base_ref)
    }

    /// The remote's branch `name` at a commit of its own.
    pub fn remote_branch(&self, name: &str, sha: &str) -> &Self {
        self.0.lock().unwrap().remote.insert(format!("refs/heads/{name}"), sha.into());
        self
    }

    fn change(&self, provider: &str, number: u64, f: impl FnOnce(&mut Request)) -> &Self {
        let mut s = self.0.lock().unwrap();
        let r = s.requests.iter_mut().find(|r| r.provider == provider && r.number == number).expect("the request exists");
        f(r);
        self
    }

    pub fn checks(&self, provider: &str, number: u64, checks: &[(&str, &str)]) -> &Self {
        self.change(provider, number, |r| r.checks = checks.iter().map(|(n, s)| (n.to_string(), s.to_string())).collect())
    }

    pub fn state(&self, provider: &str, number: u64, state: &str) -> &Self {
        self.change(provider, number, |r| r.state = state.into())
    }

    pub fn draft(&self, provider: &str, number: u64) -> &Self {
        self.change(provider, number, |r| r.draft = true)
    }

    pub fn conflicting(&self, provider: &str, number: u64) -> &Self {
        self.change(provider, number, |r| r.mergeable = "conflicting".into())
    }

    pub fn approved(&self, provider: &str, number: u64) -> &Self {
        self.change(provider, number, |r| r.review_decision = Some("approved".into()))
    }

    /// The request as the group's `view` answers it now.
    pub fn view(&self, provider: &str, number: u64) -> Value {
        let s = self.0.lock().unwrap();
        neutral(s.requests.iter().find(|r| r.provider == provider && r.number == number).expect("the request exists"))
    }

    /// The method a request was merged with, none while it is open.
    pub fn merged_with(&self, provider: &str, number: u64) -> Option<String> {
        let s = self.0.lock().unwrap();
        s.requests.iter().find(|r| r.provider == provider && r.number == number).and_then(|r| r.merged_with.clone())
    }

    /// The commit a ref of the repository names after the fetches so far.
    pub fn local_ref(&self, name: &str) -> Option<String> {
        self.0.lock().unwrap().local.get(name).cloned()
    }

    /// Makes every fetch fail with `message`.
    pub fn fetch_fails(&self, message: &str) {
        self.0.lock().unwrap().fetch_error = Some(message.into());
    }

    /// Makes every `view` fail with `message`, as the CLI does for a request it cannot find.
    pub fn view_fails(&self, message: &str) {
        self.0.lock().unwrap().view_error = Some(message.into());
    }

    /// Makes `agent` answer the methods of `USES` from these requests.
    /// The labels the repository holds as (name, color); with none set, reading them is refused as when `gh` is not signed in.
    pub fn labels(&self, labels: &[(&str, &str)]) -> &Self {
        self.0.lock().unwrap().labels = Some(labels.iter().map(|(n, c)| (n.to_string(), c.to_string())).collect());
        self
    }

    /// The accounts the repository can assign as (login, avatar url); with none set, reading them is refused as when `gh` is not signed in.
    pub fn assignees(&self, users: &[(&str, &str)]) -> &Self {
        self.0.lock().unwrap().assignees = Some(users.iter().map(|(l, a)| (l.to_string(), a.to_string())).collect());
        self
    }

    /// A conversation comment of pull request `number`, whose id is a node id as `gh pr view` prints it.
    pub fn conversation_comment(&self, number: u64, node_id: &str, author: &str, body: &str, created_ms: u64, url: &str) -> &Self {
        self.0.lock().unwrap().comments.push((number, json!({"id": node_id, "author": author, "body": body, "created_at_ms": created_ms, "url": url})));
        self
    }

    /// A review thread of pull request `number` with its comments as (database id, author, body, time).
    pub fn review_thread(&self, number: u64, id: &str, path: &str, line: Option<u64>, (resolved, outdated): (bool, bool), comments: &[(u64, &str, &str, u64)]) -> &Self {
        let rows: Vec<Value> = comments.iter().map(|(c, a, b, at)| json!({"id": c.to_string(), "author": a, "body": b, "created_at_ms": at, "url": format!("https://example.test/pull/{number}#discussion_r{c}")})).collect();
        self.0.lock().unwrap().threads.push((number, json!({"id": id, "resolved": resolved, "outdated": outdated, "path": path, "line": line, "comments": rows})));
        self
    }

    /// Gives the conversation comment last added the richer fields of rca's answer.
    pub fn conversation_comment_extras(&self, extras: &Extras) -> &Self {
        if let Some((_, row)) = self.0.lock().unwrap().comments.last_mut() {
            extras.merge(row);
        }
        self
    }

    /// Gives the comment `index` of the thread last added the richer fields of rca's answer.
    pub fn thread_comment_extras(&self, index: usize, extras: &Extras) -> &Self {
        if let Some(row) = self.0.lock().unwrap().threads.last_mut().and_then(|(_, t)| t["comments"].get_mut(index)) {
            extras.merge(row);
        }
        self
    }

    /// Gives the thread last added its start line, original lines, side and the diff hunk of its first comment.
    pub fn thread_hunk(&self, (start_line, original_line, original_start_line): (Option<u64>, Option<u64>, Option<u64>), diff_side: &str, hunk: &str) -> &Self {
        if let Some((_, t)) = self.0.lock().unwrap().threads.last_mut() {
            t["start_line"] = json!(start_line);
            t["original_line"] = json!(original_line);
            t["original_start_line"] = json!(original_start_line);
            t["diff_side"] = json!(diff_side);
            t["root_diff_hunk"] = json!(hunk);
        }
        self
    }

    /// A review of pull request `number` as (node id, author, state, body, submitted ms), with the richer fields when `extras` is given.
    pub fn review(&self, number: u64, (node_id, author, state, body): (&str, &str, &str, &str), submitted_ms: Option<u64>, extras: Option<&Extras>) -> &Self {
        let mut row = json!({"id": node_id, "author": author, "state": state, "body": body, "submitted_at_ms": submitted_ms});
        if let Some(extras) = extras {
            extras.merge(&mut row);
        }
        self.0.lock().unwrap().reviews.push((number, row));
        self
    }

    /// The file `path` as commit `rev` holds it.
    pub fn blob(&self, rev: &str, path: &str, bytes: &[u8]) -> &Self {
        self.0.lock().unwrap().blobs.push((rev.into(), path.into(), bytes.to_vec()));
        self
    }

    /// A remote of the workspace repository with its fetch URL, listed in the order added.
    pub fn remote(&self, name: &str, url: &str) -> &Self {
        self.0.lock().unwrap().remotes.push((name.into(), url.into()));
        self
    }

    /// Makes listing the remotes fail with `message`, as git does outside a repository.
    pub fn remotes_fail(&self, message: &str) {
        self.0.lock().unwrap().remotes_error = Some(message.into());
    }

    /// The remote-tracking branch origin's HEAD points at, as git records it on clone.
    pub fn origin_head(&self, branch: &str) -> &Self {
        self.0.lock().unwrap().origin_head = Some(branch.into());
        self
    }

    /// Makes the executor's CLIs report no signed-in account.
    pub fn signed_out(&self) -> &Self {
        self.0.lock().unwrap().signed_out = true;
        self
    }

    /// Makes every `list` of requests fail with `message`.
    pub fn list_fails(&self, message: &str) {
        self.0.lock().unwrap().list_error = Some(message.into());
    }

    pub fn install(&self, agent: &ScriptedAgent) {
        let me = self.clone();
        agent.reply("git.upstreamStatus", move |_| {
            let s = me.0.lock().unwrap();
            let mut up = json!({"branch": "feat/x", "has_upstream": true, "upstream": "origin/feat/x", "remote": "origin", "ahead": 0, "behind": 0, "remotes": s.remotes.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()});
            if let Some(head) = &s.origin_head {
                up["origin_head"] = json!(head);
            }
            Ok(up)
        });
        let me = self.clone();
        agent.reply("sourceControl.discover", move |p| {
            let provider = p["provider"].as_str().filter(|x| matches!(*x, "github" | "gitlab")).ok_or_else(|| refused("INVALID_PARAMS", "provider must be github or gitlab"))?;
            let signed_in = !me.0.lock().unwrap().signed_out;
            Ok(json!({"providers": [{"provider": provider, "installed": true, "signed_in": signed_in, "host": p["host"], "account": if signed_in { json!("octocat") } else { Value::Null }, "scopes": []}]}))
        });
        let me = self.clone();
        agent.reply("pr.remotes", move |_| {
            let s = me.0.lock().unwrap();
            match &s.remotes_error {
                Some(message) => Err(refused("IO_ERROR", message.clone())),
                None => Ok(json!({"remotes": s.remotes.iter().map(|(n, u)| json!({"name": n, "url": u})).collect::<Vec<_>>()})),
            }
        });
        let me = self.clone();
        agent.reply("pr.comments", move |p| {
            let (s, n) = (me.0.lock().unwrap(), number(p)?);
            Ok(json!({"comments": s.comments.iter().filter(|(x, _)| *x == n).map(|(_, c)| c.clone()).collect::<Vec<_>>()}))
        });
        let me = self.clone();
        agent.reply("pr.threads", move |p| {
            let (s, n) = (me.0.lock().unwrap(), number(p)?);
            Ok(json!({"threads": s.threads.iter().filter(|(x, _)| *x == n).map(|(_, t)| t.clone()).collect::<Vec<_>>()}))
        });
        let me = self.clone();
        agent.reply("pr.reviews", move |p| {
            let (s, n) = (me.0.lock().unwrap(), number(p)?);
            Ok(json!({"reviews": s.reviews.iter().filter(|(x, _)| *x == n).map(|(_, r)| r.clone()).collect::<Vec<_>>()}))
        });
        let me = self.clone();
        agent.reply("git.blob", move |p| {
            use base64::Engine;
            let (s, rev, path) = (me.0.lock().unwrap(), p["rev"].as_str().unwrap_or(""), p["path"].as_str().unwrap_or(""));
            let max = p["max_bytes"].as_u64().ok_or_else(|| refused("INVALID_ARGS", "`max_bytes` is required here"))? as usize;
            let (_, _, bytes) = s.blobs.iter().find(|(r, f, _)| r == rev && f == path).ok_or_else(|| refused("GIT_FAILED", "path does not exist in that revision"))?;
            let taken = &bytes[..bytes.len().min(max)];
            Ok(json!({"rev": rev, "path": path, "size": bytes.len(), "data": base64::engine::general_purpose::STANDARD.encode(taken), "truncated": taken.len() < bytes.len()}))
        });
        let me = self.clone();
        agent.reply("pr.labelCandidates", move |p| {
            let s = me.0.lock().unwrap();
            let labels = s.labels.as_ref().ok_or_else(|| refused("HOSTED_AUTH", "gh is not signed in"))?;
            Ok(json!({"labels": labels.iter().take(p["limit"].as_u64().unwrap_or(200) as usize).map(|(n, c)| json!({"name": n, "color": c, "description": ""})).collect::<Vec<_>>()}))
        });
        for method in ["pr.setState", "mr.setState"] {
            agent.reply(method, |p| match p["number"].as_u64() {
                Some(12) => Ok(json!({"number": 12, "state": p["state"]})),
                Some(99) => Err(refused("HOSTED_FAILED", "none of the git remotes configured for this repository point to a known host")),
                _ => Err(refused("HOSTED_FAILED", "HTTP 404: Not Found")),
            });
        }
        agent.reply("mr.comment", |p| {
            let body = p["body"].as_str().filter(|b| !b.trim().is_empty()).ok_or_else(|| refused("INVALID_ARGS", "`body` must not be empty"))?;
            match p["number"].as_u64() {
                Some(12) => Ok(json!({"done": true, "url": null, "id": "88", "database_id": 88, "author": "tanuki", "avatar_url": "https://gitlab.example/a.png", "is_bot": false, "body": body, "created_at": "2026-11-19T02:00:00.000Z"})),
                Some(99) => Err(refused("HOSTED_FAILED", "none of the git remotes configured for this repository points to a known GitLab host")),
                _ => Err(refused("HOSTED_FAILED", "glab: 404 Not Found")),
            }
        });
        agent.reply("pr.reply", |p| match p["comment_id"].as_u64() {
            Some(9003) => Ok(json!({"id": "PRRC_kwDOAB", "database_id": 9004, "url": "https://github.com/acme/app/pull/12#discussion_r9004", "author": "octocat", "avatar_url": "https://avatars.example/1", "is_bot": false, "body": p["body"], "created_at": "2026-11-19T02:00:00Z", "path": null, "line": null})),
            _ => Err(refused("HOSTED_FAILED", "gh: Not Found (HTTP 404)")),
        });
        agent.reply("pr.reviewComment", |p| match p["commit_id"].as_str() {
            Some("abc1234") => Ok(json!({"id": "PRRC_kwDOAC", "database_id": 9100, "thread": null, "url": "https://github.com/acme/app/pull/12#discussion_r9100", "author": "octocat", "avatar_url": "https://avatars.example/1", "is_bot": false, "body": p["body"], "created_at": "2026-11-19T03:00:00Z", "path": p["path"], "line": p["line"], "start_line": p["start_line"]})),
            Some("dead") => Err(refused("HOSTED_FAILED", "none of the git remotes configured for this repository point to a known GitHub host")),
            Some("0") => Ok(json!({"id": "PRRC_0", "database_id": 0, "thread": null, "url": null, "author": "octocat", "avatar_url": null, "is_bot": false, "body": p["body"], "created_at": null, "path": null, "line": null, "start_line": null})),
            _ => Err(refused("HOSTED_FAILED", "gh: Validation Failed (HTTP 422)")),
        });
        agent.reply("pr.resolveThread", |p| match p["thread"].as_str() {
            Some(t) if t.starts_with("PRRT_") => Ok(json!({"thread": t, "resolved": p["resolved"]})),
            _ => Err(refused("HOSTED_FAILED", "Could not resolve to a node with the global id")),
        });
        agent.reply("pr.fileViewed", |p| match p["pull_request"].as_str() {
            Some(id) if id.starts_with("PR_") => Ok(json!({"pull_request": id, "path": p["path"], "viewed": p["viewed"]})),
            _ => Err(refused("NOT_FOUND", "gh: GraphQL: Could not resolve to a node with the global id of 'x' (markFileAsViewed)")),
        });
        let me = self.clone();
        agent.reply("pr.reviewerCandidates", move |p| {
            let s = me.0.lock().unwrap();
            let users = s.assignees.as_ref().ok_or_else(|| refused("HOSTED_AUTH", "gh is not signed in"))?;
            let avatar = |a: &str| if a.is_empty() { Value::Null } else { json!(a) };
            Ok(json!({"users": users.iter().take(p["limit"].as_u64().unwrap_or(200) as usize).map(|(l, a)| json!({"login": l, "name": null, "avatarUrl": avatar(a)})).collect::<Vec<_>>()}))
        });
        for (name, provider) in [("pr.list", "github"), ("pr.view", "github"), ("pr.checks", "github"), ("pr.merge", "github"), ("pr.edit", "github"), ("pr.ready", "github"), ("pr.requestReviewers", "github"), ("mr.list", "gitlab"), ("mr.view", "gitlab"), ("mr.edit", "gitlab"), ("mr.ready", "gitlab"), ("mr.setLabels", "gitlab"), ("mr.merge", "gitlab")] {
            let me = self.clone();
            let verb = name.split_once('.').map_or(name, |(_, v)| v).to_string();
            agent.reply(name, move |p| me.answer(provider, &verb, p));
        }
        let me = self.clone();
        agent.reply("git.fetch", move |p| me.fetch(p));
    }

    fn answer(&self, provider: &'static str, verb: &str, p: &Value) -> Result<Value, AgentError> {
        let mut s = self.0.lock().unwrap();
        if verb == "list" {
            if let Some(message) = &s.list_error {
                return Err(refused("HOSTED_FAILED", message.clone()));
            }
            let head = p["head"].as_str();
            let state = p["state"].as_str().ok_or_else(|| refused("INVALID_ARGS", "`state` is required here"))?;
            let limit = p["limit"].as_u64().ok_or_else(|| refused("INVALID_ARGS", "`limit` is required here"))? as usize;
            let mut found: Vec<&Request> = s.requests.iter().filter(|r| r.provider == provider && head.is_none_or(|h| r.head_ref == h) && (state == "all" || r.state == state)).collect();
            found.sort_by_key(|r| std::cmp::Reverse(r.updated_ms));
            return Ok(json!({"prs": found.iter().take(limit).map(|r| neutral(r)).collect::<Vec<_>>(), "truncated": found.len() > limit}));
        }
        let n = number(p)?;
        if verb == "view" {
            if let Some(message) = &s.view_error {
                return Err(refused("EXEC_FAILED", message.clone()));
            }
        }
        let r = s.requests.iter_mut().find(|r| r.provider == provider && r.number == n).ok_or_else(|| refused("NOT_FOUND", format!("no pull request {n} in the repository")))?;
        match verb {
            "view" => Ok(json!({"pr": neutral(r)})),
            "checks" => {
                let count = |state: &str| r.checks.iter().filter(|c| c.1 == state).count();
                Ok(json!({"checks": r.checks.iter().map(|(name, state)| json!({"name": name, "state": state, "workflow": "CI", "url": format!("https://example.test/checks/{name}"), "started_at_ms": 1, "completed_at_ms": null})).collect::<Vec<_>>(), "pass": count("pass"), "fail": count("fail"), "pending": count("pending")}))
            }
            "merge" => {
                let method = p["method"].as_str().ok_or_else(|| refused("INVALID_ARGS", "`method` is required here"))?;
                if !["merge", "squash", "rebase"].contains(&method) {
                    return Err(refused("INVALID_ARGS", "`method` must be one of `merge`, `squash`, `rebase`"));
                }
                if provider == "gitlab" && method == "rebase" {
                    return Err(refused("INVALID_ARGS", "GitLab has no rebase merge here"));
                }
                if r.state != "open" || r.mergeable == "conflicting" {
                    return Err(refused("EXEC_FAILED", format!("pull request {n} is not mergeable: it is {} and {}", r.state, r.mergeable)));
                }
                r.state = "merged".into();
                r.merged_with = Some(method.into());
                Ok(json!({"done": true}))
            }
            "edit" => {
                let (title, body) = (p["title"].as_str(), p["body"].as_str());
                if title.is_none() && body.is_none() && p["base"].as_str().is_none() {
                    return Err(refused("INVALID_ARGS", "give at least one of `title`, `body`, `base`"));
                }
                if let Some(t) = title {
                    r.title = t.into();
                }
                if let Some(b) = body {
                    r.body = b.into();
                }
                Ok(json!({"done": true}))
            }
            "ready" => {
                r.draft = p["undo"] == true;
                Ok(json!({"done": true}))
            }
            "requestReviewers" => {
                let (add, remove) = (names(p, "add")?, names(p, "remove")?);
                if add.is_empty() && remove.is_empty() {
                    return Err(refused("INVALID_ARGS", "give at least one of `add`, `remove`"));
                }
                r.reviewers.retain(|x| !remove.contains(x));
                r.reviewers.extend(add);
                Ok(json!({"done": true}))
            }
            "setLabels" => {
                let (add, remove) = (names(p, "add")?, names(p, "remove")?);
                r.labels.retain(|x| !remove.contains(x));
                r.labels.extend(add);
                Ok(json!({"done": true}))
            }
            other => Err(refused("METHOD_NOT_FOUND", format!("the double has no {other}"))),
        }
    }

    fn fetch(&self, p: &Value) -> Result<Value, AgentError> {
        let mut s = self.0.lock().unwrap();
        if let Some(message) = s.fetch_error.clone() {
            return Err(refused("GIT_FAILED", message));
        }
        let specs = p["refspecs"].as_array().ok_or_else(|| refused("INVALID_PARAMS", "`refspecs` is required here"))?;
        let mut fetched = Vec::new();
        for spec in specs {
            let spec = spec.as_str().ok_or_else(|| refused("INVALID_ARGS", "a refspec is text"))?;
            let (source, dest) = spec.strip_prefix('+').unwrap_or(spec).split_once(':').ok_or_else(|| refused("INVALID_ARGS", format!("`{spec}` is no refspec")))?;
            let sha = s.remote.get(source).cloned().ok_or_else(|| refused("GIT_FAILED", format!("couldn't find remote ref {source}")))?;
            s.local.insert(dest.into(), sha.clone());
            fetched.push(json!({"ref": dest, "sha": sha}));
        }
        Ok(json!({"remote": "origin", "output": "", "refs": fetched}))
    }
}
