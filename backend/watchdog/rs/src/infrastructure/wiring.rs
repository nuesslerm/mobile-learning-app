//! The composition root: `main` fills [`Ports`] with the `docker` binary and the system clock, the flow tests with doubles,
//! and both run the same watchdog. Its events go to whichever `tracing` subscriber is current.
use super::{DockerCli, DockerCommand, TracingEvents};
use std::sync::Arc;
use watchdog_core::application::{Clock, Watchdog};

/// Everything the watchdog reaches outside its own code.
pub struct Ports {
    pub docker: Arc<dyn DockerCommand>,
    pub clock: Arc<dyn Clock>,
    pub host_label: String,
}

pub fn watchdog(ports: Ports) -> Watchdog {
    Watchdog { docker: Arc::new(DockerCli::new(ports.docker)), events: Arc::new(TracingEvents { host_label: ports.host_label }), clock: ports.clock }
}
