use async_trait::async_trait;
use reporting_core::application::Heartbeat;
use std::time::Duration;

/// A GET of the heartbeat's URL (BetterStack Uptime counts it as a ping). A failure is logged and otherwise ignored: the heartbeat
/// reports on sampling, sampling must not depend on it.
pub struct HttpHeartbeat {
    client: reqwest::Client,
    url: String,
}

impl HttpHeartbeat {
    pub fn new(url: String) -> Result<Self, reqwest::Error> {
        Ok(Self { client: reqwest::Client::builder().timeout(Duration::from_secs(10)).build()?, url })
    }
}

#[async_trait]
impl Heartbeat for HttpHeartbeat {
    async fn beat(&self) {
        match self.client.get(&self.url).send().await {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => tracing::warn!(status = response.status().as_u16(), "heartbeat.rejected"),
            Err(error) => tracing::warn!(error = %error.without_url(), "heartbeat.failed"),
        }
    }
}
