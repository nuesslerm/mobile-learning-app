use std::time::SystemTime;

/// One gauge reading: `metric` is the stable metric name, `scope` and `name` its two labels.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub metric: String,
    pub scope: String,
    pub name: String,
    pub value: f64,
    pub at: SystemTime,
}

/// Where gauge readings go. One call is one batch, sent as one request; an `Err` is the caller's to count, never to propagate.
pub trait MetricsSink: Send + Sync {
    fn record(&self, readings: &[Reading]) -> Result<(), String>;
}
