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
    view_error: Option<String>,
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
    pub fn install(&self, agent: &ScriptedAgent) {
        for (name, provider) in [("pr.list", "github"), ("pr.view", "github"), ("pr.checks", "github"), ("pr.merge", "github"), ("pr.edit", "github"), ("pr.ready", "github"), ("pr.requestReviewers", "github"), ("mr.view", "gitlab"), ("mr.edit", "gitlab"), ("mr.ready", "gitlab"), ("mr.setLabels", "gitlab"), ("mr.merge", "gitlab")] {
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
