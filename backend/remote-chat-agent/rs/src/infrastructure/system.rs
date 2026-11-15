//! The system behind the core's clock and id ports.

use super::ambient::{random_hex, uuid_v4};
use rca_service_core::domain::util::now_ms;
use remote_chat_agent_core::domain::ids::{Clock, IdSource};

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        now_ms()
    }
}

impl rca_service_core::application::agent_groups::ActivityClock for SystemClock {
    fn now_ms(&self) -> u64 {
        now_ms()
    }
}

impl rca_group_terminal::Clock for SystemClock {
    fn now_ms(&self) -> u64 {
        now_ms()
    }
}

pub struct RandomIds;

impl IdSource for RandomIds {
    fn uuid(&self) -> String {
        uuid_v4()
    }

    fn hex(&self, n: usize) -> String {
        random_hex(n)
    }
}
