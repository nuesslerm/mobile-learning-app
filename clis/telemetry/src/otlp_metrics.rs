//! Gauges to a Better Stack metrics source over OTLP/HTTP (JSON), one request per batch. A reading is stamped when the batch is
//! sent, a moment after the sweep that took it.

use crate::{otlp, Destination, MetricsSink, Reading};
use serde_json::{json, Value};
use std::time::SystemTime;

pub struct OtlpMetricsSink {
    destination: Destination,
    service: String,
}

impl OtlpMetricsSink {
    pub fn new(destination: Destination, service: impl Into<String>) -> Self {
        Self { destination, service: service.into() }
    }
}

impl MetricsSink for OtlpMetricsSink {
    fn record(&self, readings: &[Reading]) -> Result<(), String> {
        if readings.is_empty() {
            return Ok(());
        }
        let gauges = readings
            .iter()
            .map(|r| (r.metric.clone(), otlp::attributes([("scope", Value::from(r.scope.clone())), ("name", Value::from(r.name.clone()))]), r.value))
            .collect();
        let resource = json!({ "attributes": otlp::attributes([("service.name", Value::from(self.service.clone()))]) });
        otlp::post(&self.destination, "/v1/metrics", &otlp::gauges_body(resource, "telemetry.metrics", SystemTime::now(), gauges))
    }
}
