//! `wf vm ops token-permissions`, run from agent-shell (which holds the PAT): read-only GETs that say what `VM_PROVISION_GITHUB_TOKEN`
//! may do on the repository. Only yes/no per probe, the HTTP status and the token's expiry date are printed, never the token or a body.
use cli_http::Request;
use serde_json::{json, Value};
use std::time::Duration;

const REPO: &str = "nuesslerm/mobile-learning-app";
const PAT: &str = "VM_PROVISION_GITHUB_TOKEN";
const DEFAULT_API: &str = "https://api.github.com";

pub struct Reading {
    pub status: u16,
    pub expires: Option<String>,
    pub body: Value,
}

/// What GitHub is asked: one read-only GET with the token.
pub trait GithubReads {
    fn get(&self, token: &str, path: &str) -> Result<Reading, String>;
}

struct GithubRest {
    api: String,
}

impl GithubReads for GithubRest {
    fn get(&self, token: &str, path: &str) -> Result<Reading, String> {
        let reply = Request::get(&format!("{}{path}", self.api))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "wf")
            .timeout(Duration::from_secs(30))
            .send()
            .map_err(|_| format!("the GitHub API at {} did not answer", self.api))?;
        Ok(Reading { status: reply.status, expires: reply.header("github-authentication-token-expiration").map(String::from), body: serde_json::from_slice(&reply.body).unwrap_or(Value::Null) })
    }
}

fn yes(granted: bool) -> &'static str {
    if granted { "yes" } else { "no" }
}

fn row(probe: &str, needs: &str, status: u16, granted: bool) -> Value {
    json!({ "probe": probe, "needs": needs, "granted": yes(granted), "status": status })
}

/// The report of every probe; `ok` needs a token GitHub accepts that reads the repository's contents.
pub fn report(github: &dyn GithubReads, token: &str) -> Result<Value, String> {
    let user = github.get(token, "/user")?;
    let repo = github.get(token, &format!("/repos/{REPO}"))?;
    let contents = github.get(token, &format!("/repos/{REPO}/contents/README.md"))?;
    let flag = |name: &str| repo.body["permissions"][name] == json!(true);
    let probes = vec![
        row("token-accepted", "a live token (not expired or revoked)", user.status, user.status != 401),
        row("repository-read", "Metadata: read", repo.status, repo.status == 200),
        row("contents-read", "Contents: read", contents.status, contents.status == 200),
    ];
    let granted = |name: &str| probes.iter().any(|p| p["probe"] == name && p["granted"] == "yes");
    let ok = ["token-accepted", "contents-read"].iter().all(|n| granted(n));
    Ok(json!({
        "ok": ok,
        "repo": REPO,
        "expires": user.expires,
        "repoPermissions": { "admin": yes(flag("admin")), "push": yes(flag("push")), "pull": yes(flag("pull")) },
        "probes": probes,
    }))
}

pub fn run() -> Option<i32> {
    let Some(token) = secrets::load_values(&[PAT]).get(PAT).cloned().flatten().filter(|t| !t.is_empty()) else {
        wf_kit::wf_err!("wf: {PAT} is not set: run this on agent-shell, which holds it (a one-line script of `wf vm ops token-permissions` through `wf vm exec`)");
        return Some(1);
    };
    let github = GithubRest { api: std::env::var("WF_GITHUB_API").unwrap_or_else(|_| DEFAULT_API.to_string()) };
    match report(&github, &token) {
        Ok(report) => {
            println!("{report}");
            Some(if report["ok"] == json!(true) { 0 } else { 1 })
        }
        Err(e) => {
            wf_kit::wf_err!("wf: {e}");
            Some(1)
        }
    }
}
