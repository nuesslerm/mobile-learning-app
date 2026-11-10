//! The client of `opencode serve` (the gateway of `data/opencodeGateway.ts`): JSON over HTTP with the optional Basic credential,
//! and the server-sent `/event` stream. The server is on the compose network, so there is no TLS here. `Http` is the real transport,
//! the `Opencode` port; `rca_service_core::application::opencode_client::Client` is what the features call.

use remote_chat_agent_core::application::ports::{DataStream, Reply};
use rca_service_core::application::opencode_client::FETCH_TIMEOUT;
use remote_chat_agent_core::application::ports::Opencode;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::Value;
use std::time::Duration;

/// opencode over HTTP.
pub struct Http {
    http: reqwest::Client,
    base: String,
    credential: Option<(String, String)>,
}

impl Http {
    /// The server URL, with the Basic credential the settings carry when a password is set.
    pub fn from_settings(settings: &rca_service_core::application::settings::Settings) -> Self {
        let credential = settings.opencode_server_password.clone().map(|password| (settings.opencode_server_username.clone(), password));
        Self::new(&settings.opencode_server_url, credential)
    }

    pub fn new(url: &str, credential: Option<(String, String)>) -> Self {
        let http = reqwest::Client::builder().connect_timeout(FETCH_TIMEOUT).build().expect("a client with a fixed configuration");
        Self { http, base: url.trim_end_matches('/').to_string(), credential }
    }

    fn builder(&self, method: Method, path: &str, body: Option<&Value>) -> reqwest::RequestBuilder {
        let mut request = self.http.request(method, format!("{}{path}", self.base)).header(reqwest::header::ACCEPT, "application/json, text/event-stream");
        if let Some((user, password)) = &self.credential {
            request = request.basic_auth(user, Some(password));
        }
        match body {
            Some(body) => request.json(body),
            None => request,
        }
    }
}

#[async_trait]
impl Opencode for Http {
    async fn request(&self, method: &str, path: &str, body: Option<&Value>, timeout: Duration) -> Result<Reply, String> {
        let method = Method::from_bytes(method.as_bytes()).unwrap_or(Method::GET);
        let response = self.builder(method, path, body).timeout(timeout).send().await.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(|e| e.to_string())?.to_vec();
        Ok(Reply { status, body })
    }

    async fn open_stream(&self, path: &str) -> Result<DataStream, String> {
        let response = self.builder(Method::GET, path, None).send().await.map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("opencode {path} responded with {}", response.status().as_u16()));
        }
        Ok(response.bytes_stream().eventsource().map(|event| event.map(|event| event.data).map_err(|e| e.to_string())).boxed())
    }
}
