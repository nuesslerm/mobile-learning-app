//! The gateway's providers over HTTP, and its keys from the process environment (filled by the vault at start). The CLIs' own environments never
//! hold these: the keys are named `GATEWAY_*`, which no provider's inherited prefix covers. Not run against a real provider.
use async_trait::async_trait;
use futures_util::StreamExt;
use rca_group_gateway::{KeySource, Outbound, Provider, Upstream, UpstreamReply};
use std::time::Duration;

pub struct EnvKeys;

impl KeySource for EnvKeys {
    fn key(&self, provider: &Provider) -> Option<String> {
        std::env::var(provider.key).ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty())
    }
}

pub struct HttpUpstream {
    client: reqwest::Client,
}

impl Default for HttpUpstream {
    fn default() -> Self {
        Self::with_read_idle(READ_IDLE)
    }
}

impl HttpUpstream {
    fn with_read_idle(read_idle: Duration) -> Self {
        // No overall timeout (an answer streams as long as the model thinks); a stalled stream is cut by the read timeout. No redirects: reqwest would
        // forward `x-api-key` to another host, so a 3xx goes back to the CLI.
        let client = reqwest::Client::builder().connect_timeout(Duration::from_secs(15)).read_timeout(read_idle).redirect(reqwest::redirect::Policy::none()).build().unwrap_or_default();
        Self { client }
    }
}

const READ_IDLE: Duration = Duration::from_secs(120);

const NOT_PASSED_BACK: [&str; 4] = ["content-length", "transfer-encoding", "connection", "keep-alive"];

#[async_trait]
impl Upstream for HttpUpstream {
    async fn send(&self, request: Outbound) -> Result<UpstreamReply, String> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|e| e.to_string())?;
        let mut call = self.client.request(method, request.url()).body(request.body.clone());
        for (name, value) in &request.headers {
            call = call.header(name, value);
        }
        let answer = call.send().await.map_err(|e| e.without_url().to_string())?;
        let status = answer.status().as_u16();
        let headers = answer.headers().iter().filter(|(name, _)| !NOT_PASSED_BACK.contains(&name.as_str())).filter_map(|(name, value)| Some((name.to_string(), value.to_str().ok()?.to_string()))).collect();
        let body = answer.bytes_stream().map(|chunk| chunk.map(|bytes| bytes.to_vec()).map_err(|e| e.without_url().to_string()));
        Ok(UpstreamReply { status, headers, body: Box::pin(body) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rca_group_gateway::{Auth, Protocol};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn provider_at(port: u16) -> &'static Provider {
        let upstream: &'static str = Box::leak(format!("http://127.0.0.1:{port}").into_boxed_str());
        Box::leak(Box::new(Provider { id: "local", protocol: Protocol::Messages, upstream, auth: Auth::XApiKey, key: "GATEWAY_LOCAL", model_prefixes: &[] }))
    }

    fn call(provider: &'static Provider) -> Outbound {
        Outbound { provider, method: "POST".into(), path: "/v1/messages".into(), query: None, headers: vec![("x-api-key".into(), "real-key".into())], body: b"{}".to_vec(), credentialed: true }
    }

    async fn read_request(socket: &mut tokio::net::TcpStream) {
        let mut buf = [0u8; 4096];
        let _ = socket.read(&mut buf).await;
    }

    #[tokio::test]
    async fn given_a_provider_that_redirects_when_the_gateway_calls_then_the_redirect_goes_back_and_the_key_is_sent_nowhere_else() {
        // Given a provider that answers 302 to a second host, which records whether it was ever called
        let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target_port = target.local_addr().unwrap().port();
        let provider = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let provider_port = provider.local_addr().unwrap().port();
        let reached = tokio::spawn(async move { tokio::time::timeout(Duration::from_millis(500), target.accept()).await.is_ok() });
        tokio::spawn(async move {
            let (mut socket, _) = provider.accept().await.unwrap();
            read_request(&mut socket).await;
            let reply = format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/steal\r\nContent-Length: 0\r\n\r\n");
            socket.write_all(reply.as_bytes()).await.unwrap();
        });
        // When the gateway sends the request with the provider's key
        let answer = HttpUpstream::default().send(call(provider_at(provider_port))).await.unwrap();
        // Then the 302 is the answer, and the second host was never called
        assert_eq!(answer.status, 302);
        assert!(!reached.await.unwrap());
    }

    #[tokio::test]
    async fn given_a_provider_that_stalls_mid_stream_when_the_gateway_relays_then_the_stream_ends_with_an_error_instead_of_hanging() {
        // Given a provider that sends one chunk of a streamed answer and then goes silent
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\npart\r\n").await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        // When the gateway relays it under a short read timeout
        let upstream = HttpUpstream::with_read_idle(Duration::from_millis(200));
        let mut answer = upstream.send(call(provider_at(port))).await.unwrap();
        // Then the chunk came, and the stall ended the stream with an error
        assert_eq!(answer.body.next().await.unwrap().unwrap(), b"part".to_vec());
        let ended = tokio::time::timeout(Duration::from_secs(5), answer.body.next()).await.expect("the stall must not hold the stream");
        assert!(matches!(ended, Some(Err(_))));
    }
}
