//! `wf ship ci-runner`, run by hand from agent-shell (which holds the PAT): mints the runner's one-hour registration token and redeploys
//! the runner with it as `RUNNER_TOKEN`, plus a one-hour Tailscale key of the runner's own tag as `TS_CI_RUNNER_AUTH_KEY`. The PAT
//! mints and is dropped; neither it nor the key is stored, written or printed.
use cli_http::Request;
use serde_json::Value;
use std::time::Duration;

const REPO: &str = "nuesslerm/mobile-learning-app";
const PAT: &str = "VM_PROVISION_GITHUB_TOKEN";
const DEFAULT_API: &str = "https://api.github.com";

/// What GitHub is asked: a registration token for a new runner of this repository.
pub trait Registrations {
    fn mint(&self, pat: &str) -> Result<String, String>;
}

struct GithubRest {
    api: String,
}

impl Registrations for GithubRest {
    fn mint(&self, pat: &str) -> Result<String, String> {
        let url = format!("{}/repos/{REPO}/actions/runners/registration-token", self.api);
        let reply = Request::post(&url)
            .header("Authorization", &format!("Bearer {pat}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "wf")
            .timeout(Duration::from_secs(30))
            .send()
            .map_err(|_| format!("the GitHub API at {} did not answer", self.api))?;
        if reply.status != 201 {
            return Err(format!("GitHub refused the registration token with HTTP {} (the PAT needs Administration: write on {REPO}): {}", reply.status, reply.text().chars().take(200).collect::<String>()));
        }
        let body: Value = serde_json::from_slice(&reply.body).map_err(|_| "GitHub's registration-token reply is not JSON".to_string())?;
        body["token"].as_str().filter(|t| !t.is_empty()).map(String::from).ok_or_else(|| "GitHub's reply carries no token".to_string())
    }
}

/// The `.env` lines the redeploy appends to the runner's synced env.
fn env_lines(token: &str, node_key: Option<&str>) -> String {
    let mut lines = format!("RUNNER_TOKEN={token}\n");
    if let Some(key) = node_key {
        lines.push_str(&format!("TS_CI_RUNNER_AUTH_KEY={key}\n"));
    }
    lines
}

fn ship(github: &dyn Registrations, pat: &str, git_ref: &str, tag: &str, wipe_state: bool) -> Result<(), String> {
    let token = github.mint(pat)?;
    eprintln!("minted a one-hour runner registration token; redeploying ci-runner with it as RUNNER_TOKEN");
    // Agent-shell holds the PAT but not the Tailscale credentials: a key minted where those are readable can be handed in through the environment.
    let node_key = match std::env::var("TS_CI_RUNNER_AUTH_KEY").ok().filter(|k| !k.is_empty()) {
        Some(key) => {
            eprintln!("sending the Tailscale key given in TS_CI_RUNNER_AUTH_KEY");
            Some(key)
        }
        None => match crate::tailscale::mint_runner_auth_key() {
            Ok(key) => {
                eprintln!("minted a one-hour tag:ci-self-hosted Tailscale key; sending it as TS_CI_RUNNER_AUTH_KEY");
                Some(key)
            }
            Err(why) => {
                println!("the runner keeps joining with the shared key (no tag:ci-self-hosted key minted): {}", why.lines().next().unwrap_or_default());
                None
            }
        },
    };
    let reply = crate::deploy_run::redeploy("ci-runner", git_ref, tag, wipe_state, false, Some(&env_lines(&token, node_key.as_deref())))?;
    println!("{}", serde_json::to_string_pretty(&reply).map_err(|e| e.to_string())?);
    Ok(())
}

pub fn run(git_ref: &str, tag: Option<&str>, wipe_state: bool) -> Result<Option<i32>, String> {
    let pat = secrets::load_values(&[PAT]).get(PAT).cloned().flatten().filter(|p| !p.is_empty()).ok_or_else(|| format!("{PAT} is not set: `wf ship ci-runner` runs from agent-shell, which holds it"))?;
    let github = GithubRest { api: std::env::var("WF_GITHUB_API").unwrap_or_else(|_| DEFAULT_API.to_string()) };
    ship(&github, &pat, git_ref, tag.filter(|t| !t.is_empty()).unwrap_or("latest"), wipe_state).map(|_| Some(0))
}
