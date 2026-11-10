use async_trait::async_trait;
use capture_core::application::ports::{Notifier, PortError};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde_json::json;

/// The Telegram notify service: `POST /notify` with the shared secret.
pub struct HttpNotifier {
    client: Client,
    base_url: String,
    secret: SecretString,
}

impl HttpNotifier {
    pub fn new(client: Client, base_url: String, secret: SecretString) -> Self {
        Self { client, base_url, secret }
    }
}

#[async_trait]
impl Notifier for HttpNotifier {
    async fn notify(&self, text: &str) -> Result<(), PortError> {
        let response = self
            .client
            .post(format!("{}/notify", self.base_url))
            .header("X-Notify-Secret", self.secret.expose_secret())
            .json(&json!({ "text": text }))
            .send()
            .await
            .map_err(|e| PortError::new(e.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(PortError::new(format!("POST /notify failed with status {}", response.status().as_u16())))
        }
    }
}
