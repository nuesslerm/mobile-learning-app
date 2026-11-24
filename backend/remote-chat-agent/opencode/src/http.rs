//! The client of `opencode serve` (the gateway of `data/opencodeGateway.ts`): JSON over HTTP with the optional Basic credential,
//! and the server-sent `/event` stream. The server is on the compose network, so there is no TLS here. `Http` is the real transport,
//! the `Opencode` port; `rca_service_core::application::opencode_client::Client` is what the features call.

use remote_chat_agent_core::application::ports::{DataStream, Reply};
use rca_service_core::application::opencode_client::FETCH_TIMEOUT;
use remote_chat_agent_core::application::ports::Opencode;
use async_trait::async_trait;
use futures_util::{stream, StreamExt};
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
        let events = response.bytes_stream().scan(SseParser::default(), |parser, chunk| {
            let items: Vec<Result<String, String>> = match chunk {
                Ok(bytes) => parser.push(&bytes).into_iter().map(Ok).collect(),
                Err(e) => vec![Err(e.to_string())],
            };
            std::future::ready(Some(stream::iter(items)))
        });
        Ok(events.flatten().boxed())
    }
}

/// Incremental server-sent-events parser: bytes in, the `data` of each completed event out. Lines are split on raw bytes, so a
/// multi-byte character cut by a chunk boundary is decoded only once its line is whole.
#[derive(Default)]
struct SseParser {
    pending: Vec<u8>,
    data: Option<String>,
    started: bool,
}

impl SseParser {
    fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.pending.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if let Some(event) = self.line(&String::from_utf8_lossy(&line)) {
                events.push(event);
            }
        }
        events
    }

    fn line(&mut self, line: &str) -> Option<String> {
        let line = if self.started { line } else { line.strip_prefix('\u{feff}').unwrap_or(line) };
        self.started = true;
        if line.is_empty() {
            return self.data.take();
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        if field == "data" {
            let value = value.strip_prefix(' ').unwrap_or(value);
            match &mut self.data {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => self.data = Some(value.to_string()),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::SseParser;

    fn feed(chunks: &[&[u8]]) -> Vec<String> {
        let mut parser = SseParser::default();
        chunks.iter().flat_map(|chunk| parser.push(chunk)).collect()
    }

    #[test]
    fn given_a_multibyte_character_split_across_chunks_when_parsed_then_it_is_intact() {
        let bytes = "data: caf\u{e9} \u{1f600}\n\n".as_bytes();
        let cut = bytes.iter().position(|b| *b == 0xC3).unwrap() + 1;
        let events = feed(&[&bytes[..cut], &bytes[cut..]]);
        assert_eq!(events, vec!["caf\u{e9} \u{1f600}".to_string()]);
        let emoji = bytes.iter().position(|b| *b == 0xF0).unwrap();
        let events = feed(&[&bytes[..emoji + 2], &bytes[emoji + 2..emoji + 3], &bytes[emoji + 3..]]);
        assert_eq!(events, vec!["caf\u{e9} \u{1f600}".to_string()]);
    }

    #[test]
    fn given_a_line_boundary_split_across_chunks_when_parsed_then_the_event_completes_on_the_blank_line() {
        assert!(feed(&[b"data: one\r"]).is_empty());
        assert_eq!(feed(&[b"data: one\r", b"\n\r", b"\n"]), vec!["one"]);
        assert_eq!(feed(&[b"da", b"ta: a\n", b"data:b\n", b"\n: note\ndata: c\n\n"]), vec!["a\nb", "c"]);
    }

    #[test]
    fn given_events_without_data_when_parsed_then_none_is_emitted() {
        assert!(feed(&[b"event: ping\n\n:keepalive\n\n"]).is_empty());
    }
}
