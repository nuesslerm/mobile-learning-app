use serde::Serialize;

pub const MINUTE_MS: i64 = 60_000;
pub const HOUR_MS: i64 = 3_600_000;

/// One reading of one metric of one subject (`scope` is `host` or `container`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sample {
    pub ts: i64,
    pub scope: String,
    pub name: String,
    pub metric: String,
    pub value: f64,
}

impl Sample {
    pub fn new(ts: i64, scope: &str, name: &str, metric: &str, value: f64) -> Self {
        Self { ts, scope: scope.into(), name: name.into(), metric: metric.into(), value }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Granularity {
    Raw,
    Minute,
    Hour,
}

/// A stored point: a raw sample or an average over a minute or an hour (the store averages when it is asked, nothing is rolled up in advance).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesPoint {
    #[serde(flatten)]
    pub sample: Sample,
    pub granularity: Granularity,
}

/// What one prune pass did: rows deleted, and the bytes in use before and after.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PruneResult {
    pub deleted_rows: u64,
    pub used_before: u64,
    pub used_after: u64,
}

/// Which points a series read wants besides its window: one scope, some metrics, and nothing before `since_ms`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeriesQuery {
    pub scope: Option<String>,
    pub metrics: Vec<String>,
    pub since_ms: Option<i64>,
}
