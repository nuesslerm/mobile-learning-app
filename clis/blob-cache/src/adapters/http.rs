//! `BlobStore` over plain HTTP object requests: `GET`, `PUT` and `HEAD` on `<base><object name>`, with the credential an `Auth`
//! strategy supplies: none (a PAR carries it in the URL), a bearer token, or an S3 SigV4 signature.
use super::sigv4::{self, Credentials, Parts, UNSIGNED_PAYLOAD};
use crate::domain::Key;
use crate::ports::{BlobStore, PutOutcome, StoreError};
use cli_http::{Request, Response, Via};
use std::path::Path;
use std::time::{Duration, SystemTime};

const CONNECT: Duration = Duration::from_secs(3);
const RESPONSE: Duration = Duration::from_secs(10);
// A ceiling over the two above, so a body that stalls mid-transfer cannot hang a run.
const TRANSFER: Duration = Duration::from_secs(120);
// The same ceiling for an object streamed to or from a file (the pnpm store, about 0.6 GB).
const FILE_TRANSFER: Duration = Duration::from_secs(15 * 60);

/// Pauses before the retries of a request that failed in transit or was answered 408, 429 or 5xx: a blip of the store or the network must not turn a
/// whole run's cache cold (the breaker opens on the first failure that reaches it), and a store that is down still fails within a second.
const BACKOFF: [Duration; 2] = [Duration::from_millis(200), Duration::from_millis(800)];

/// Error codes of an S3 error body that an error text may name; the body itself is never quoted, it can echo the access key.
const S3_CODES: [&str; 9] =
    ["SignatureDoesNotMatch", "RequestTimeTooSkewed", "InvalidAccessKeyId", "AccessDenied", "NoSuchBucket", "NoSuchKey", "SlowDown", "PreconditionFailed", "StorageQuotaExceeded"];

/// How a request proves who sends it.
#[derive(Clone)]
pub enum Auth {
    /// Nothing is added: the URL is the credential (an OCI PAR).
    None,
    /// `Authorization: Bearer <token>` on every request.
    Bearer(String),
    /// AWS Signature Version 4 on every request; `service` is `s3`.
    SigV4 { access_key: String, secret_key: String, region: String, service: String },
}

impl Auth {
    pub fn s3(access_key: &str, secret_key: &str, region: &str) -> Auth {
        Auth::SigV4 { access_key: access_key.into(), secret_key: secret_key.into(), region: region.into(), service: "s3".into() }
    }

    fn secrets(&self) -> Vec<&str> {
        match self {
            Auth::None => vec![],
            Auth::Bearer(token) => vec![token],
            Auth::SigV4 { access_key, secret_key, .. } => vec![access_key, secret_key],
        }
    }
}

/// Where one side (read or write) of the store lives and how it authenticates.
#[derive(Clone)]
pub struct Side {
    base: String,
    auth: Auth,
}

/// A bucket of an S3-compatible service.
pub struct S3Location<'a> {
    /// `https://host[:port]` of the service, with no bucket in it.
    pub endpoint: &'a str,
    pub bucket: &'a str,
    /// Keys live under this prefix; may be empty.
    pub prefix: &'a str,
    /// `https://bucket.host/key` instead of the default path style `https://host/bucket/key`.
    pub virtual_host: bool,
}

impl Side {
    /// `base` ends at the place the object name is appended to (a slash is added when missing).
    pub fn new(base: &str, auth: Auth) -> Side {
        Side { base: if base.ends_with('/') { base.to_string() } else { format!("{base}/") }, auth }
    }

    pub fn s3(location: &S3Location, auth: Auth) -> Side {
        let endpoint = location.endpoint.trim_end_matches('/');
        let prefix = location.prefix.trim_matches('/');
        let prefix = if prefix.is_empty() { String::new() } else { format!("{}/", sigv4::encode_path(prefix)) };
        let bucket = sigv4::encode_path(location.bucket);
        let base = match (location.virtual_host, endpoint.split_once("://")) {
            (true, Some((scheme, host))) => format!("{scheme}://{bucket}.{host}/{prefix}"),
            _ => format!("{endpoint}/{bucket}/{prefix}"),
        };
        Side::new(&base, auth)
    }
}

/// What a request sends.
#[derive(Clone, Copy)]
enum Payload<'a> {
    Empty,
    Bytes(&'a [u8]),
    File(&'a Path),
}

pub struct HttpStore {
    read: Side,
    write: Option<Side>,
    conditional_put: bool,
    via: Via,
}

impl HttpStore {
    /// With no write side the store is read-only.
    pub fn new(read: Side, write: Option<Side>) -> HttpStore {
        HttpStore { read, write, conditional_put: false, via: Via::Environment }
    }

    /// Write with `if-none-match: *` instead of checking first. Only for a store known to accept it.
    pub fn conditional_put(mut self, on: bool) -> HttpStore {
        self.conditional_put = on;
        self
    }

    pub fn via(mut self, via: Via) -> HttpStore {
        self.via = via;
        self
    }

    fn payload_hash(side: &Side, payload: Payload) -> Result<String, String> {
        if !matches!(side.auth, Auth::SigV4 { .. }) || side.base.starts_with("https://") {
            return Ok(UNSIGNED_PAYLOAD.to_string());
        }
        match payload {
            Payload::Empty => Ok(sigv4::sha256_hex(b"")),
            Payload::Bytes(body) => Ok(sigv4::sha256_hex(body)),
            Payload::File(path) => sigv4::sha256_file_hex(path).map_err(|e| format!("{}: {e}", path.display())),
        }
    }

    fn request(&self, method: &str, side: &Side, key: &Key, ceiling: Duration, payload_hash: &str) -> Request {
        let url = format!("{}{key}", side.base);
        let mut request = Request::new(method, &url).via(self.via.clone()).connect_timeout(CONNECT).response_timeout(RESPONSE).timeout(ceiling);
        match &side.auth {
            Auth::None => {}
            Auth::Bearer(token) => request = request.header("Authorization", &format!("Bearer {token}")),
            Auth::SigV4 { access_key, secret_key, region, service } => {
                let after_scheme = url.split_once("://").map_or(url.as_str(), |(_, rest)| rest);
                let (host, path) = after_scheme.split_once('/').map_or((after_scheme, "/".to_string()), |(host, path)| (host, format!("/{path}")));
                let amz_date = sigv4::amz_date(SystemTime::now());
                let headers = vec![("host".to_string(), host.to_string()), ("x-amz-content-sha256".to_string(), payload_hash.to_string()), ("x-amz-date".to_string(), amz_date.clone())];
                let credentials = Credentials { access_key, secret_key, region, service };
                let authorization = sigv4::authorization(&credentials, &Parts { method, path: &path, query: "", headers: &headers, payload_hash, amz_date: &amz_date });
                request = request.header("x-amz-date", &amz_date).header("x-amz-content-sha256", payload_hash).header("Authorization", &authorization);
            }
        }
        request
    }

    /// Asks again after each pause of `BACKOFF` while the attempt failed in transit or was answered with a status worth another try.
    fn with_retries(&self, mut attempt: impl FnMut() -> Result<Response, String>) -> Result<Response, String> {
        let mut retried = 0;
        loop {
            let result = attempt();
            let again = match &result {
                Err(_) => true,
                Ok(response) => matches!(response.status, 408 | 429 | 500..=599),
            };
            if !again || retried >= BACKOFF.len() {
                return result;
            }
            std::thread::sleep(BACKOFF[retried]);
            retried += 1;
        }
    }

    fn send(&self, method: &str, side: &Side, key: &Key, payload: Payload, if_none_match: bool) -> Result<Response, StoreError> {
        let hash = Self::payload_hash(side, payload).map_err(|error| self.unreachable(side, method, key, &error))?;
        self.with_retries(|| {
            let mut request = match payload {
                Payload::Empty => self.request(method, side, key, TRANSFER, &hash),
                Payload::Bytes(body) => self.request(method, side, key, TRANSFER, &hash).header("Content-Type", "application/octet-stream").body(body),
                Payload::File(path) => self.request(method, side, key, FILE_TRANSFER, &hash).header("Content-Type", "application/octet-stream").body_file(path),
            };
            if if_none_match {
                request = request.header("If-None-Match", "*");
            }
            request.send()
        })
        .map_err(|error| self.unreachable(side, method, key, &error))
    }

    fn unreachable(&self, side: &Side, method: &str, key: &Key, error: &str) -> StoreError {
        StoreError::Unreachable(format!("{method} {key}: {}", self.scrub(side, error)))
    }

    fn put_payload(&self, key: &Key, payload: Payload, if_absent: bool) -> Result<PutOutcome, StoreError> {
        let Some(write) = &self.write else {
            return Err(StoreError::Rejected(format!("PUT {key}: no write link is configured")));
        };
        if if_absent && !self.conditional_put && self.exists(key)? {
            return Ok(PutOutcome::Exists);
        }
        let response = self.send("PUT", write, key, payload, if_absent && self.conditional_put)?;
        match response.status {
            200 | 201 | 204 => Ok(PutOutcome::Stored),
            412 if if_absent => Ok(PutOutcome::Exists),
            400 if String::from_utf8_lossy(&response.body).contains("StorageQuotaExceeded") => Err(StoreError::Rejected(format!("PUT {key}: HTTP 400 StorageQuotaExceeded"))),
            _ => Err(self.failure("PUT", key, &response)),
        }
    }

    /// An error text never carries a link or a credential: the token, the secret key and the access key are scrubbed with the URLs.
    fn scrub(&self, side: &Side, text: &str) -> String {
        let mut clean = text.to_string();
        for known in std::iter::once(&self.read).chain(&self.write) {
            let placeholder = if matches!(known.auth, Auth::None) { "<par>" } else { "<store>" };
            clean = clean.replace(&known.base, placeholder);
            for secret in known.auth.secrets().into_iter().filter(|s| !s.is_empty()) {
                clean = clean.replace(secret, "<redacted>");
            }
        }
        for secret in side.auth.secrets().into_iter().filter(|s| !s.is_empty()) {
            clean = clean.replace(secret, "<redacted>");
        }
        clean
    }

    fn failure(&self, method: &str, key: &Key, response: &Response) -> StoreError {
        let status = response.status;
        let body = String::from_utf8_lossy(&response.body);
        let code = S3_CODES.iter().find(|code| body.contains(&format!("<Code>{code}</Code>"))).copied();
        let mut said = format!("{method} {key}: HTTP {status}");
        if let Some(code) = code {
            said.push_str(&format!(" {code}"));
        }
        match (status, code) {
            (_, Some("RequestTimeTooSkewed")) => StoreError::Credential(format!("{said} (this host's clock differs from the store's by more than 15 minutes; fix the clock)")),
            (401 | 403, _) | (_, Some("SignatureDoesNotMatch" | "InvalidAccessKeyId")) => StoreError::Credential(said),
            (408 | 429 | 500..=599, _) => StoreError::Unreachable(said),
            _ => StoreError::Rejected(said),
        }
    }
}

impl BlobStore for HttpStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        let response = self.send("GET", &self.read, key, Payload::Empty, false)?;
        match response.status {
            200 => Ok(Some(response.body)),
            // a missing bucket is a misconfiguration, never a cold cache
            404 if String::from_utf8_lossy(&response.body).contains("<Code>NoSuchBucket</Code>") => Err(self.failure("GET", key, &response)),
            404 => Ok(None),
            _ => Err(self.failure("GET", key, &response)),
        }
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        let response = self.send("HEAD", &self.read, key, Payload::Empty, false)?;
        match response.status {
            200 => Ok(true),
            404 => Ok(false),
            _ => Err(self.failure("HEAD", key, &response)),
        }
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.put_payload(key, Payload::Bytes(body), if_absent)
    }

    fn get_file(&self, key: &Key, dest: &Path) -> Result<bool, StoreError> {
        let side = &self.read;
        let hash = Self::payload_hash(side, Payload::Empty).map_err(|error| self.unreachable(side, "GET", key, &error))?;
        let response = self.with_retries(|| self.request("GET", side, key, FILE_TRANSFER, &hash).send_to_file(dest)).map_err(|error| self.unreachable(side, "GET", key, &error))?;
        match response.status {
            200 => Ok(true),
            404 if !String::from_utf8_lossy(&response.body).contains("<Code>NoSuchBucket</Code>") => Ok(false),
            status => {
                if (200..300).contains(&status) {
                    // saved by the stream, and not an object
                    let _ = std::fs::remove_file(dest);
                }
                Err(self.failure("GET", key, &response))
            }
        }
    }

    fn put_file(&self, key: &Key, source: &Path, if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.put_payload(key, Payload::File(source), if_absent)
    }

    fn requests_per_put(&self, if_absent: bool) -> u64 {
        if if_absent && !self.conditional_put {
            2
        } else {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s3(endpoint: &str, prefix: &str, virtual_host: bool) -> Side {
        Side::s3(&S3Location { endpoint, bucket: "bkt", prefix, virtual_host }, Auth::s3("AK", "SK", "r"))
    }

    #[test]
    fn given_an_s3_location_when_addressed_then_path_style_is_the_default_and_virtual_host_moves_the_bucket_into_the_host() {
        assert_eq!(s3("https://s3.example.com", "a/b", false).base, "https://s3.example.com/bkt/a/b/");
        assert_eq!(s3("https://s3.example.com/", "", false).base, "https://s3.example.com/bkt/");
        assert_eq!(s3("https://s3.example.com:9000", "/p/", true).base, "https://bkt.s3.example.com:9000/p/");
    }

    #[test]
    fn given_a_sigv4_side_when_the_endpoint_is_https_or_http_then_the_payload_is_unsigned_or_hashed() {
        let body = Payload::Bytes(b"abc");
        assert_eq!(HttpStore::payload_hash(&s3("https://h", "", false), body).unwrap(), UNSIGNED_PAYLOAD);
        assert_eq!(HttpStore::payload_hash(&s3("http://h", "", false), body).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn given_an_error_text_when_scrubbed_then_urls_tokens_and_keys_are_gone() {
        let side = Side::new("https://h/o/", Auth::Bearer("tok-123".into()));
        let store = HttpStore::new(side.clone(), None);
        let clean = store.scrub(&side, "GET https://h/o/x failed: sent tok-123");
        assert!(!clean.contains("tok-123") && !clean.contains("https://h/o/"), "{clean}");
        let par = Side::new("https://h/p/SECRET/o/", Auth::None);
        assert_eq!(HttpStore::new(par.clone(), None).scrub(&par, "bad https://h/p/SECRET/o/k"), "bad <par>k");
    }
}
