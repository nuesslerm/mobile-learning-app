use async_trait::async_trait;
use capture_core::application::ports::{Backend, PortError, Registration};
use reqwest::{Client, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

/// The leetcode-backend's login-capture and session routes, with the admin secret.
pub struct HttpBackend {
    client: Client,
    base_url: String,
    admin_secret: SecretString,
}

impl HttpBackend {
    pub fn new(client: Client, base_url: String, admin_secret: SecretString) -> Self {
        Self { client, base_url, admin_secret }
    }
}

#[async_trait]
impl Backend for HttpBackend {
    async fn register(&self) -> Result<Registration, PortError> {
        let response = self
            .client
            .post(format!("{}/api/leetcode/login-session", self.base_url))
            .header("X-Admin-Secret", self.admin_secret.expose_secret())
            .send()
            .await
            .map_err(|e| PortError::new(e.to_string()))?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|e| PortError::new(e.to_string()))?;
        let expires_at = body.get("expiresAt").and_then(Value::as_u64).unwrap_or(0);
        match status {
            StatusCode::OK => Ok(Registration { started: true, expires_at }),
            StatusCode::CONFLICT => Ok(Registration { started: false, expires_at }),
            other => Err(PortError::new(format!("POST /login-session failed with unexpected status {}", other.as_u16()))),
        }
    }

    async fn submit_cookie(&self, cookie: &str) -> Result<(), PortError> {
        let response = self
            .client
            .post(format!("{}/api/leetcode/session", self.base_url))
            .header("X-Admin-Secret", self.admin_secret.expose_secret())
            .json(&json!({ "cookie": cookie }))
            .send()
            .await
            .map_err(|e| PortError::new(e.to_string()))?;
        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status().as_u16();
        let text = response.text().await.unwrap_or_default();
        Err(PortError::new(format!("POST /session failed ({status}): {text}")))
    }
}
