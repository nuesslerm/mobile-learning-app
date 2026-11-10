//! Where a CLI on this host reaches a thread's tool server (`McpEndpoint`): the service's own `/mcp/<thread>` on its loopback port. The port is
//! the configured one until the listener says which it bound (a flow binds port 0).
use crate::ports::McpAddress;
use rca_group_agent::McpEndpoint;
use std::sync::atomic::{AtomicU16, Ordering};

pub struct LoopbackMcp {
    configured: u16,
    bound: AtomicU16,
}

impl LoopbackMcp {
    pub fn new(configured: u16) -> LoopbackMcp {
        LoopbackMcp { configured, bound: AtomicU16::new(0) }
    }

    /// Records the port the listener bound.
    pub fn listening_on(&self, port: u16) {
        self.bound.store(port, Ordering::SeqCst);
    }

    pub fn url_of(&self, thread: &str) -> String {
        let port = match self.bound.load(Ordering::SeqCst) {
            0 => self.configured,
            bound => bound,
        };
        let thread = percent_encoding::utf8_percent_encode(thread, percent_encoding::NON_ALPHANUMERIC);
        format!("http://127.0.0.1:{port}/mcp/{thread}")
    }
}

impl McpEndpoint for LoopbackMcp {
    fn url(&self, thread: &str) -> String {
        self.url_of(thread)
    }
}

impl McpAddress for LoopbackMcp {
    fn listening_on(&self, port: u16) {
        LoopbackMcp::listening_on(self, port);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_service_that_has_not_bound_when_a_url_is_made_then_it_names_the_configured_port_and_after_binding_the_bound_one_with_the_thread_escaped() {
        // Given an address configured for port 8787
        let mcp = LoopbackMcp::new(8787);
        // When a url is made before and after the listener bound port 41000
        let before = mcp.url_of("t 1");
        mcp.listening_on(41_000);
        // Then the first names 8787, the second 41000, and a thread's id is escaped as a path segment
        assert_eq!(before, "http://127.0.0.1:8787/mcp/t%201");
        assert_eq!(McpEndpoint::url(&mcp, "a/b"), "http://127.0.0.1:41000/mcp/a%2Fb");
    }
}
