//! The one HTTP client of the CLIs: blocking `ureq`, with the proxy, redirect and timeout choices the call sites used to give `curl`.
//! A transport failure (nothing listening, a timeout, a bad certificate) is an `Err`; any HTTP status, error statuses included, is a
//! `Response`, so a caller decides what a 404 means.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use std::path::{Path, PathBuf};
use std::time::Duration;
use ureq::http;

/// The largest body a caller reads into memory.
const BODY_LIMIT: u64 = 512 * 1024 * 1024;

/// The largest body streamed to a file: it guards the disk, not memory.
const FILE_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

/// How the request reaches its host.
#[derive(Clone, Debug)]
pub enum Via {
    /// The proxy variables of the environment, as `curl` read them.
    Environment,
    /// Straight to the host, whatever the environment says.
    Direct,
    /// Through a SOCKS5 proxy that resolves the host name itself (`host:port`), as the tailnet's userspace proxy does.
    Socks5(String),
}

#[derive(Clone, Debug)]
pub struct Request {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    body_file: Option<PathBuf>,
    basic: Option<(String, String)>,
    timeout: Option<Duration>,
    connect_timeout: Option<Duration>,
    response_timeout: Option<Duration>,
    via: Via,
    follow_redirects: bool,
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

impl Request {
    pub fn new(method: &str, url: &str) -> Self {
        Self { method: method.to_ascii_uppercase(), url: url.into(), headers: Vec::new(), body: Vec::new(), body_file: None, basic: None, timeout: Some(Duration::from_secs(30)), connect_timeout: None, response_timeout: None, via: Via::Environment, follow_redirects: false }
    }

    pub fn get(url: &str) -> Self {
        Self::new("GET", url)
    }

    pub fn post(url: &str) -> Self {
        Self::new("POST", url)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// The body is this file, read from disk as it is sent and announced with the file's size, so a file larger than memory can go
    /// out. It replaces `body`; the file is opened when the request is sent.
    pub fn body_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.body_file = Some(path.into());
        self
    }

    /// A JSON body with its content type.
    pub fn json(self, body: &serde_json::Value) -> Self {
        self.header("Content-Type", "application/json").body(body.to_string())
    }

    pub fn basic_auth(mut self, user: &str, password: &str) -> Self {
        self.basic = Some((user.into(), password.into()));
        self
    }

    /// The whole exchange, connecting included, as `curl --max-time`.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Opening the connection, TLS included, apart from the whole exchange.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Waiting for the response headers once the request is sent, apart from the whole exchange.
    pub fn response_timeout(mut self, timeout: Duration) -> Self {
        self.response_timeout = Some(timeout);
        self
    }

    /// Waits as long as the service takes (a call that drives a browser).
    pub fn without_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }

    pub fn via(mut self, via: Via) -> Self {
        self.via = via;
        self
    }

    pub fn direct(self) -> Self {
        self.via(Via::Direct)
    }

    pub fn follow_redirects(mut self) -> Self {
        self.follow_redirects = true;
        self
    }

    pub fn send(self) -> Result<Response, String> {
        let mut response = self.exchange()?;
        let (status, headers) = (response.status().as_u16(), header_list(&response));
        let body = response.body_mut().with_config().limit(BODY_LIMIT).read_to_vec().map_err(|e| e.to_string())?;
        Ok(Response { status, headers, body })
    }

    /// As `send`, but a 2xx body is streamed into `dest` (up to `FILE_LIMIT`) and the file appears only once all of it arrived.
    /// Any other status comes back as `send` gives it, with no file; a 2xx comes back with an empty body.
    pub fn send_to_file(self, dest: &Path) -> Result<Response, String> {
        let mut response = self.exchange()?;
        let (status, headers) = (response.status().as_u16(), header_list(&response));
        if !(200..300).contains(&status) {
            let body = response.body_mut().with_config().limit(BODY_LIMIT).read_to_vec().map_err(|e| e.to_string())?;
            return Ok(Response { status, headers, body });
        }
        let expected = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse::<u64>().ok());
        let mut part = dest.as_os_str().to_owned();
        part.push(format!(".part-{}", std::process::id()));
        let part = PathBuf::from(part);
        let saved = std::fs::File::create(&part)
            .and_then(|mut file| std::io::copy(&mut response.body_mut().with_config().limit(FILE_LIMIT).reader(), &mut file))
            .map_err(|e| e.to_string())
            .and_then(|written| match expected {
                Some(want) if want != written => Err(format!("the body ended after {written} of {want} bytes")),
                _ => Ok(()),
            })
            .and_then(|()| std::fs::rename(&part, dest).map_err(|e| e.to_string()));
        if let Err(error) = saved {
            let _ = std::fs::remove_file(&part);
            return Err(error);
        }
        Ok(Response { status, headers, body: Vec::new() })
    }

    fn exchange(self) -> Result<http::Response<ureq::Body>, String> {
        let mut config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(if self.follow_redirects { 10 } else { 0 })
            .max_redirects_will_error(false)
            .timeout_global(self.timeout)
            .timeout_connect(self.connect_timeout)
            .timeout_recv_response(self.response_timeout)
            .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build());
        match &self.via {
            Via::Environment => {}
            Via::Direct => config = config.proxy(None),
            Via::Socks5(address) => config = config.proxy(Some(ureq::Proxy::new(&format!("socks5h://{address}")).map_err(|e| e.to_string())?)),
        }
        let agent: ureq::Agent = config.build().into();

        let mut builder = http::Request::builder().method(self.method.as_str()).uri(&self.url);
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        if let Some((user, password)) = &self.basic {
            use base64::Engine as _;
            builder = builder.header("Authorization", format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))));
        }
        let response = if let Some(path) = &self.body_file {
            let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
            agent.run(builder.body(file).map_err(|e| e.to_string())?)
        } else if self.body.is_empty() && matches!(self.method.as_str(), "GET" | "HEAD" | "DELETE") {
            agent.run(builder.body(()).map_err(|e| e.to_string())?)
        } else {
            agent.run(builder.body(self.body).map_err(|e| e.to_string())?)
        };
        response.map_err(|e| e.to_string())
    }
}

fn header_list(response: &http::Response<ureq::Body>) -> Vec<(String, String)> {
    response.headers().iter().map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned())).collect()
}

/// Whether a local service answers a GET with a 2xx inside `timeout`: the probe `curl -sf --max-time` made.
pub fn answers(url: &str, timeout: Duration) -> bool {
    Request::get(url).direct().timeout(timeout).send().is_ok_and(|r| r.ok())
}

/// The body of a GET to a local service, when it answers 2xx.
pub fn fetch_local(url: &str, timeout: Duration) -> Option<String> {
    Request::get(url).direct().timeout(timeout).send().ok().filter(Response::ok).map(|r| r.text())
}

/// Downloads `url` into `dest` (redirects followed), streamed, so a body above the read cap is fine; a failed download leaves no
/// file behind.
pub fn download(url: &str, dest: &Path, timeout: Duration) -> Result<(), String> {
    let response = Request::get(url).follow_redirects().timeout(timeout).send_to_file(dest)?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status));
    }
    Ok(())
}

/// PUTs the file at `path` to `url`, streamed from disk with its size as the length, so a file above the read cap goes out without
/// being held in memory.
pub fn put_file(url: &str, path: &Path, timeout: Duration) -> Result<Response, String> {
    Request::new("PUT", url).body_file(path).timeout(timeout).send()
}

/// What JS `encodeURIComponent` leaves alone.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

/// JS `encodeURIComponent`.
pub fn encode_uri_component(s: &str) -> String {
    utf8_percent_encode(s, URI_COMPONENT).to_string()
}

/// `URLSearchParams` encoding of one value (`application/x-www-form-urlencoded`: a space is `+`).
pub fn form_encode(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// JS `decodeURIComponent`, lossy: an escape that is not valid is kept as written and bytes that are not UTF-8 are replaced.
pub fn decode_uri_component(s: &str) -> String {
    percent_decode_str(s).decode_utf8_lossy().into_owned()
}
