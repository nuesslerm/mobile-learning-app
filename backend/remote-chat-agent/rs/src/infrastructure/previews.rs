//! The adapters of the preview sessions' ports: the system clock and ids from the system's randomness.
use rca_service_core::application::agent_groups::{PreviewClock, PreviewIds};
use rca_service_core::domain::util::{now_ms, random_hex};

pub struct SystemClock;

impl PreviewClock for SystemClock {
    fn now_ms(&self) -> u64 {
        now_ms()
    }
}

pub struct RandomIds;

impl PreviewIds for RandomIds {
    fn id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", random_hex(8))
    }
}
