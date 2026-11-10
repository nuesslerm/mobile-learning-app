use async_trait::async_trait;
use reporting_core::application::PlanSource;
use reporting_core::domain::plans::{pricing_from_value, DispatchRun, PlanManifest, Pricing};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Plans, dispatch runs and the price table, read from the repository on every request (they change while the service runs).
pub struct PlanFiles {
    root: PathBuf,
}

impl PlanFiles {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn read_plans(root: &Path) -> Vec<PlanManifest> {
    let Ok(entries) = std::fs::read_dir(root.join("docs/plans")) else { return Vec::new() };
    let mut plans: Vec<PlanManifest> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        // A torn or foreign manifest is skipped, never fatal.
        .filter_map(|entry| PlanManifest::from_value(&read_json(&entry.path().join("manifest.json"))?))
        .collect();
    plans.sort_by(|a, b| a.slug.cmp(&b.slug));
    plans
}

fn read_runs(root: &Path) -> Vec<DispatchRun> {
    let Ok(log) = std::fs::read_to_string(root.join(".agents/tmp/opencode-dispatch-runs.jsonl")) else { return Vec::new() };
    log.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .filter_map(|record| DispatchRun::from_value(&record))
        .collect()
}

#[async_trait]
impl PlanSource for PlanFiles {
    async fn plans(&self) -> Vec<PlanManifest> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || read_plans(&root)).await.unwrap_or_default()
    }

    async fn runs(&self) -> Vec<DispatchRun> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || read_runs(&root)).await.unwrap_or_default()
    }

    async fn pricing(&self) -> Pricing {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || read_json(&root.join("libraries/rca-core/data/model-pricing.json")).map(|value| pricing_from_value(&value)).unwrap_or_default())
            .await
            .unwrap_or_default()
    }
}
