use vm_ops_api_core::application::ports::ProcessMemory;

const BYTES_PER_MB: f64 = 1024.0 * 1024.0;

/// The resident set of this process, read from the kernel.
pub struct OsProcessMemory;

impl ProcessMemory for OsProcessMemory {
    fn rss_mb(&self) -> u64 {
        memory_stats::memory_stats().map_or(0, |usage| (usage.physical_mem as f64 / BYTES_PER_MB).round() as u64)
    }
}
