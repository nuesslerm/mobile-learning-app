use crate::domain::sample::Sample;
use serde::Serialize;

/// The whole Always-Free A1 tenancy (`infra/oracle-cloud/variables.tf`: 2 OCPUs, 12 GB memory, 150 GB boot volume).
pub const CPU_OCPUS: f64 = 2.0;
pub const MEMORY_BYTES: f64 = 12.0 * 1024.0 * 1024.0 * 1024.0;
pub const DISK_BYTES: f64 = 150.0 * 1024.0 * 1024.0 * 1024.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResourceReport {
    pub resource: &'static str,
    pub used: f64,
    pub ceiling: f64,
    pub remaining: f64,
    pub unit: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeadroomReport {
    pub resources: Vec<ResourceReport>,
}

fn resource(resource: &'static str, used: f64, ceiling: f64, unit: &'static str) -> ResourceReport {
    ResourceReport { resource, used, ceiling, remaining: ceiling - used, unit }
}

/// Used against the ceilings, from the latest host samples (a metric never sampled counts as unused).
pub fn compute(latest: &[Sample]) -> HeadroomReport {
    let value = |metric: &str| latest.iter().filter(|s| s.scope == "host" && s.metric == metric).map(|s| s.value).next_back().unwrap_or(0.0);
    HeadroomReport {
        resources: vec![
            resource("cpu", value("cpu") / 100.0 * CPU_OCPUS, CPU_OCPUS, "ocpu"),
            resource("memory", value("memory_used"), MEMORY_BYTES, "bytes"),
            resource("disk", value("disk_used"), DISK_BYTES, "bytes"),
        ],
    }
}
