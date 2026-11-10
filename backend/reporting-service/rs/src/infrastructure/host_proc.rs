use async_trait::async_trait;
use nix::sys::statvfs::statvfs;
use reporting_core::application::{HostReaders, PortError};

/// `/proc` and the root filesystem of the machine the container runs on (nothing here runs lxcfs, so `/proc` is the host's).
pub struct ProcHost;

#[async_trait]
impl HostReaders for ProcHost {
    async fn read_stat(&self) -> Result<String, PortError> {
        tokio::fs::read_to_string("/proc/stat").await.map_err(PortError::new)
    }

    async fn read_meminfo(&self) -> Result<String, PortError> {
        tokio::fs::read_to_string("/proc/meminfo").await.map_err(PortError::new)
    }

    async fn read_loadavg(&self) -> Result<String, PortError> {
        tokio::fs::read_to_string("/proc/loadavg").await.map_err(PortError::new)
    }

    async fn read_pressure(&self, resource: &str) -> Result<String, PortError> {
        tokio::fs::read_to_string(format!("/proc/pressure/{resource}")).await.map_err(PortError::new)
    }

    async fn root_fs_kb(&self) -> Result<(f64, f64), PortError> {
        let stat = statvfs("/").map_err(PortError::new)?;
        let unit_kb = stat.fragment_size() as f64 / 1024.0;
        Ok(((stat.blocks() as f64 * unit_kb).round(), (stat.blocks_available() as f64 * unit_kb).round()))
    }
}
