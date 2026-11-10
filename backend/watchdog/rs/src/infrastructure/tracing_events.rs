use async_trait::async_trait;
use watchdog_core::application::{Event, Events};

/// Events as `tracing` records: service-kit prints them as JSON lines and ships them to BetterStack.
pub struct TracingEvents {
    pub host_label: String,
}

#[async_trait]
impl Events for TracingEvents {
    async fn emit(&self, event: Event) {
        let host_label = self.host_label.as_str();
        match event {
            Event::Started => tracing::info!(hostLabel = host_label, "watchdog.started"),
            Event::SweepFailed { error } => tracing::error!(hostLabel = host_label, error = %error, "watchdog.sweep_failed"),
            Event::Recreated { container, reason, ok, error: Some(error) } => {
                tracing::error!(hostLabel = host_label, container = %container, reason, ok, error = %error, "watchdog.recreated")
            }
            Event::Recreated { container, reason, ok, error: None } => tracing::error!(hostLabel = host_label, container = %container, reason, ok, "watchdog.recreated"),
        }
    }
}
