//! `wf vm ops cancel-run`, run from agent-shell (which holds the PAT): cancels one CI run (`--run <number>`) or every CI run of `dev` that is
//! still waiting for a runner (`--queued`). The sandbox's own GitHub token cannot cancel; this is the PAT-backed way, through `wf vm exec`.
//! The PAT is used and dropped; only run numbers, ids and HTTP statuses are printed, never the token or a reply body.
use cli_http::Request;
use serde_json::{json, Value};
use std::time::Duration;

const REPO: &str = "nuesslerm/mobile-learning-app";
const PAT: &str = "VM_PROVISION_GITHUB_TOKEN";
const DEFAULT_API: &str = "https://api.github.com";
const WAITING: [&str; 4] = ["queued", "waiting", "pending", "requested"];

pub struct RunRow {
    pub id: u64,
    pub number: u64,
    pub status: String,
}

/// What GitHub is asked: the CI runs of `dev`, and one cancel.
pub trait GithubRuns {
    fn runs(&self, token: &str) -> Result<Vec<RunRow>, String>;
    fn cancel(&self, token: &str, id: u64) -> Result<u16, String>;
}

struct GithubRest {
    api: String,
}

impl GithubRest {
    fn request(&self, request: Request, token: &str) -> Result<cli_http::Response, String> {
        request
            .header("Authorization", &format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "wf")
            .timeout(Duration::from_secs(30))
            .send()
            .map_err(|_| format!("the GitHub API at {} did not answer", self.api))
    }
}

impl GithubRuns for GithubRest {
    fn runs(&self, token: &str) -> Result<Vec<RunRow>, String> {
        let reply = self.request(Request::get(&format!("{}{}", self.api, runs_path())), token)?;
        if reply.status != 200 {
            return Err(format!("GitHub refused the list of CI runs with HTTP {}", reply.status));
        }
        let body: Value = serde_json::from_slice(&reply.body).map_err(|_| "GitHub's list of CI runs is not JSON".to_string())?;
        Ok(body["workflow_runs"]
            .as_array()
            .map(|runs| runs.iter().filter_map(|r| Some(RunRow { id: r["id"].as_u64()?, number: r["run_number"].as_u64()?, status: r["status"].as_str()?.to_string() })).collect())
            .unwrap_or_default())
    }

    fn cancel(&self, token: &str, id: u64) -> Result<u16, String> {
        Ok(self.request(Request::post(&format!("{}/repos/{REPO}/actions/runs/{id}/cancel", self.api)), token)?.status)
    }
}

fn runs_path() -> String {
    format!("/repos/{REPO}/actions/workflows/deploy.yml/runs?branch=dev&per_page=100")
}

pub enum Target {
    Run(u64),
    Queued,
}

fn row(run: &RunRow, http: u16) -> Value {
    json!({ "number": run.number, "id": run.id, "statusBefore": run.status, "http": http })
}

/// Cancels what `target` names; `ok` needs every cancel accepted (202) and, for one named run, that it exists and has not finished.
pub fn cancel(github: &dyn GithubRuns, token: &str, target: &Target) -> Result<Value, String> {
    let runs = github.runs(token)?;
    let chosen: Vec<&RunRow> = match target {
        Target::Run(n) => runs.iter().filter(|r| r.number == *n || r.id == *n).collect(),
        Target::Queued => runs.iter().filter(|r| WAITING.contains(&r.status.as_str())).collect(),
    };
    if let Target::Run(n) = target {
        match chosen.first() {
            None => return Ok(json!({ "ok": false, "repo": REPO, "cancelled": [], "problem": format!("no CI run {n} among the latest 100 runs of dev") })),
            Some(run) if run.status == "completed" => return Ok(json!({ "ok": false, "repo": REPO, "cancelled": [], "problem": format!("CI run {} has already completed", run.number) })),
            Some(_) => {}
        }
    }
    let mut cancelled = Vec::new();
    for run in chosen {
        cancelled.push(row(run, github.cancel(token, run.id)?));
    }
    let ok = cancelled.iter().all(|c| c["http"] == json!(202));
    Ok(json!({ "ok": ok, "repo": REPO, "cancelled": cancelled }))
}

pub fn run(target: Target) -> Option<i32> {
    let Some(token) = secrets::load_values(&[PAT]).get(PAT).cloned().flatten().filter(|t| !t.is_empty()) else {
        wf_kit::wf_err!("wf: {PAT} is not set: run this on agent-shell, which holds it (a one-line script of `wf vm ops cancel-run ...` through `wf vm exec`)");
        return Some(1);
    };
    let github = GithubRest { api: std::env::var("WF_GITHUB_API").unwrap_or_else(|_| DEFAULT_API.to_string()) };
    match cancel(&github, &token, &target) {
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
