//! The questions the read API answers, over the store and the repository files.
use crate::application::ports::{Clock, MetricsStore, PlanSource, PortError};
use crate::domain::sample::SeriesQuery;
use crate::domain::headroom::{self, HeadroomReport};
use crate::domain::plans::{self, PlanDetail, PlanSummary, RunReport};
use crate::domain::sample::{Sample, SeriesPoint};
use crate::domain::window;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum SeriesError {
    #[error("invalid window")]
    InvalidWindow { accepted: Vec<&'static str> },
    #[error(transparent)]
    Store(#[from] PortError),
}

pub struct Reporting {
    pub store: Arc<dyn MetricsStore>,
    pub plans: Arc<dyn PlanSource>,
    pub clock: Arc<dyn Clock>,
}

impl Reporting {
    pub async fn rows(&self) -> Result<u64, PortError> {
        self.store.row_count().await
    }

    pub async fn current(&self) -> Result<Vec<Sample>, PortError> {
        self.store.latest().await
    }

    /// The window's name (the default when none was asked for) and its points.
    pub async fn series(&self, requested: Option<&str>, query: &SeriesQuery) -> Result<(String, Vec<SeriesPoint>), SeriesError> {
        let name = requested.unwrap_or(window::DEFAULT_WINDOW);
        let Some(ms) = window::window_ms(name) else { return Err(SeriesError::InvalidWindow { accepted: window::window_names() }) };
        Ok((name.to_string(), self.store.series_for(ms, self.clock.now_ms(), query).await?))
    }

    pub async fn headroom(&self) -> Result<HeadroomReport, PortError> {
        Ok(headroom::compute(&self.store.latest().await?))
    }

    pub async fn plan_summaries(&self) -> Vec<PlanSummary> {
        let (manifests, runs, pricing) = (self.plans.plans().await, self.plans.runs().await, self.plans.pricing().await);
        manifests.iter().map(|plan| plans::summarize(plan, &runs, &pricing)).collect()
    }

    pub async fn plan(&self, slug: &str) -> Option<PlanDetail> {
        let (manifests, runs, pricing) = (self.plans.plans().await, self.plans.runs().await, self.plans.pricing().await);
        manifests.iter().find(|plan| plan.slug == slug).map(|plan| plans::detail(plan, &runs, &pricing))
    }

    pub async fn run(&self, run_id: &str) -> Option<RunReport> {
        let (runs, pricing) = (self.plans.runs().await, self.plans.pricing().await);
        runs.iter().find(|run| run.run_id == run_id).map(|run| plans::run_report(run, &pricing))
    }
}
