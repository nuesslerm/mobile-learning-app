//! The clock the catalog group stamps a detection with.
use rca_service_core::application::agent_groups::CatalogClock;
use std::time::{SystemTime as Wall, UNIX_EPOCH};

pub struct SystemClock;

impl CatalogClock for SystemClock {
    fn now_ms(&self) -> u64 {
        Wall::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
    }
}
