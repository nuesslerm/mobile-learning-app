use async_trait::async_trait;
use reporting_core::application::{PortError, RunnerObservation, RunnerSource};
use serde_json::Value;
use std::time::Duration;

/// One runner of one repository, read through the GitHub REST API with a read-only token. Queued runs stand for queued jobs.
pub struct GithubRunner {
    client: reqwest::Client,
    api_base: String,
    repo: String,
    runner: String,
    token: String,
}

impl GithubRunner {
    pub fn new(api_base: impl Into<String>, repo: String, runner: String, token: String) -> Result<Self, reqwest::Error> {
        Ok(Self { client: reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?, api_base: api_base.into(), repo, runner, token })
    }

    async fn get(&self, path: &str) -> Result<Value, PortError> {
        let response = self
            .client
            .get(format!("{}/repos/{}/actions/{path}", self.api_base, self.repo))
            .bearer_auth(&self.token)
            .header("User-Agent", "reporting-service")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .map_err(|e| PortError::new(e.without_url()))?;
        if !response.status().is_success() {
            return Err(PortError::new(format!("GitHub answered {}", response.status())));
        }
        response.json().await.map_err(|e| PortError::new(e.without_url()))
    }
}

#[async_trait]
impl RunnerSource for GithubRunner {
    fn name(&self) -> String {
        self.runner.clone()
    }

    async fn observe(&self) -> Result<RunnerObservation, PortError> {
        let runners = self.get("runners?per_page=100").await?;
        let queued = self.get("runs?status=queued&per_page=1").await?;
        let ours = runners["runners"].as_array().into_iter().flatten().find(|r| r["name"] == self.runner.as_str());
        Ok(RunnerObservation {
            listener_up: ours.is_some_and(|r| r["status"] == "online"),
            busy: ours.is_some_and(|r| r["busy"] == true),
            queued_jobs: queued["total_count"].as_u64().unwrap_or(0),
        })
    }
}
