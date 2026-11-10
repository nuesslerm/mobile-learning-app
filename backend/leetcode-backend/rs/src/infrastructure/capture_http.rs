use leetcode_backend_core::application::ports::CaptureTool;
use secrecy::{ExposeSecret, SecretString};
use tracing::Instrument;

/// Asks the capture tool to start a login. Fire and forget: the request runs on its own task, inside the span of the request that
/// needed it, and a failure is logged, never returned.
pub struct HttpCaptureTool {
    client: reqwest::Client,
    base_url: String,
    secret: SecretString,
}

impl HttpCaptureTool {
    pub fn new(client: reqwest::Client, base_url: impl Into<String>, secret: SecretString) -> Self {
        Self { client, base_url: base_url.into(), secret }
    }
}

impl CaptureTool for HttpCaptureTool {
    fn request_login(&self) {
        let request = self.client.post(format!("{}/trigger", self.base_url)).header("X-Capture-Trigger-Secret", self.secret.expose_secret());
        tokio::spawn(
            async move {
                let failure = match request.send().await {
                    Ok(response) if response.status().is_success() => return,
                    Ok(response) => format!("POST /trigger failed with status {}", response.status().as_u16()),
                    Err(error) => format!("fetch failed: {error}"),
                };
                tracing::error!(error = %failure, "leetcode.notifyCaptureNeeded.failed");
            }
            .in_current_span(),
        );
    }
}
