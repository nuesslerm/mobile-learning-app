//! Gauges to a Better Stack metrics source over OTLP/HTTP (JSON), one request per batch. A reading is stamped when the batch is
//! collected, a moment after the sweep that took it.

use crate::destination::why;
use crate::{Destination, MetricsSink, Reading};
use opentelemetry::metrics::MeterProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{MetricExporter, Protocol, WithExportConfig as _, WithHttpConfig as _};
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::Resource;
use std::collections::HashMap;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

pub struct OtlpMetricsSink {
    destination: Destination,
    service: String,
}

impl OtlpMetricsSink {
    pub fn new(destination: Destination, service: impl Into<String>) -> Self {
        Self { destination, service: service.into() }
    }

    fn export(&self, readings: &[Reading]) -> Result<(), String> {
        let exporter = MetricExporter::builder()
            .with_http()
            .with_protocol(Protocol::HttpJson)
            .with_endpoint(self.destination.endpoint("/v1/metrics"))
            .with_timeout(TIMEOUT)
            .with_headers(HashMap::from([("Authorization".to_string(), format!("Bearer {}", self.destination.token()))]))
            .build()
            .map_err(|e| e.to_string())?;
        let provider = SdkMeterProvider::builder()
            .with_periodic_exporter(exporter)
            .with_resource(Resource::builder_empty().with_service_name(self.service.clone()).build())
            .build();
        let meter = provider.meter("telemetry.metrics");
        for reading in readings {
            let attributes = [KeyValue::new("scope", reading.scope.clone()), KeyValue::new("name", reading.name.clone())];
            meter.f64_gauge(reading.metric.clone()).build().record(reading.value, &attributes);
        }
        // Shutdown collects and exports once more and answers with that export's result; a flush as well would send the batch twice.
        provider.shutdown().map_err(|e| why(&e.to_string()))
    }
}

impl MetricsSink for OtlpMetricsSink {
    /// The blocking HTTP client may not be created or dropped on a Tokio worker, so the export runs on a thread of its own.
    fn record(&self, readings: &[Reading]) -> Result<(), String> {
        if readings.is_empty() {
            return Ok(());
        }
        std::thread::scope(|scope| scope.spawn(|| self.export(readings)).join()).unwrap_or_else(|_| Err("the exporter panicked".to_string()))
    }
}
