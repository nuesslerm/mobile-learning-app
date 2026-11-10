//! What a Docker stats document says about a container's CPU and memory.
use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CpuUsage {
    #[serde(default)]
    pub total_usage: f64,
    #[serde(default)]
    pub percpu_usage: Option<Vec<f64>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CpuStats {
    #[serde(default)]
    pub cpu_usage: CpuUsage,
    #[serde(default)]
    pub system_cpu_usage: Option<f64>,
    #[serde(default)]
    pub online_cpus: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemoryStats {
    #[serde(default)]
    pub usage: Option<f64>,
    #[serde(default)]
    pub limit: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContainerStats {
    #[serde(default)]
    pub cpu_stats: CpuStats,
    #[serde(default)]
    pub precpu_stats: CpuStats,
    #[serde(default)]
    pub memory_stats: MemoryStats,
    #[serde(default)]
    pub blkio_stats: BlkioStats,
}

/// The container's disk traffic as the engine reports it: cumulative bytes per operation (`read`, `write`, in any case).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlkioStats {
    #[serde(default, deserialize_with = "null_as_empty")]
    pub io_service_bytes_recursive: Vec<BlkioEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlkioEntry {
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    pub value: f64,
}

/// The engine sends `null` for a list it has nothing to put in.
fn null_as_empty<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<BlkioEntry>, D::Error> {
    Ok(Option::<Vec<BlkioEntry>>::deserialize(deserializer)?.unwrap_or_default())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ContainerSummary {
    pub id: String,
    #[serde(default)]
    pub names: Vec<String>,
    /// Docker's status line, such as `Up 3 minutes (unhealthy)`.
    #[serde(default)]
    pub status: String,
}

impl ContainerSummary {
    /// 1 when the container's healthcheck passes, 0 when it fails, none while it starts or when the container has no healthcheck.
    pub fn health(&self) -> Option<f64> {
        if self.status.contains("(unhealthy)") {
            Some(0.0)
        } else if self.status.contains("(healthy)") {
            Some(1.0)
        } else {
            None
        }
    }

    /// The first name without its leading slash, or the id.
    pub fn display_name(&self) -> String {
        self.names.first().map(|name| name.strip_prefix('/').unwrap_or(name).to_string()).unwrap_or_else(|| self.id.clone())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DiskContainer {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub names: Option<Vec<String>>,
    #[serde(default)]
    pub size_rw: Option<f64>,
    #[serde(default)]
    pub size_root_fs: Option<f64>,
}

impl DiskContainer {
    /// The first name without its leading slash, else the id, else empty (such an entry is skipped).
    pub fn display_name(&self) -> String {
        match self.names.as_ref().and_then(|names| names.first()) {
            Some(name) => name.strip_prefix('/').unwrap_or(name).to_string(),
            None => self.id.clone().unwrap_or_default(),
        }
    }
}

/// CPU use in percent of one CPU over the window between the two stats readings; 0 when nothing advanced.
pub fn cpu_percent(stats: &ContainerStats) -> f64 {
    let cpu_delta = stats.cpu_stats.cpu_usage.total_usage - stats.precpu_stats.cpu_usage.total_usage;
    let system_delta = stats.cpu_stats.system_cpu_usage.unwrap_or(0.0) - stats.precpu_stats.system_cpu_usage.unwrap_or(0.0);
    if cpu_delta <= 0.0 || system_delta <= 0.0 {
        return 0.0;
    }
    let online = stats
        .cpu_stats
        .online_cpus
        .or_else(|| stats.cpu_stats.cpu_usage.percpu_usage.as_ref().map(|cpus| cpus.len() as f64))
        .unwrap_or(1.0);
    cpu_delta / system_delta * online * 100.0
}

/// Bytes the container has read from and written to disk since it started.
pub fn disk_bytes(stats: &ContainerStats) -> (f64, f64) {
    let sum = |op: &str| stats.blkio_stats.io_service_bytes_recursive.iter().filter(|e| e.op.eq_ignore_ascii_case(op)).map(|e| e.value).sum();
    (sum("read"), sum("write"))
}

/// Memory in use and its limit, in bytes.
pub fn memory_usage(stats: &ContainerStats) -> (f64, f64) {
    (stats.memory_stats.usage.unwrap_or(0.0), stats.memory_stats.limit.unwrap_or(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_blkio_stats_in_either_case_or_null_when_summed_then_reads_and_writes_are_totalled() {
        let stats: ContainerStats = serde_json::from_str(r#"{"blkio_stats":{"io_service_bytes_recursive":[{"major":8,"minor":0,"op":"read","value":100},{"op":"Write","value":40},{"op":"write","value":2},{"op":"Total","value":142}]}}"#).unwrap();
        assert_eq!(disk_bytes(&stats), (100.0, 42.0));
        let none: ContainerStats = serde_json::from_str(r#"{"blkio_stats":{"io_service_bytes_recursive":null}}"#).unwrap();
        assert_eq!(disk_bytes(&none), (0.0, 0.0));
        assert_eq!(disk_bytes(&ContainerStats::default()), (0.0, 0.0));
    }
}
