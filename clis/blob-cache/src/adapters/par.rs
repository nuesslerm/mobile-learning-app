//! `BlobStore` over OCI Object Storage pre-authenticated requests: `GET`, `PUT` and `HEAD` on `<par url><object name>`. A PAR
//! carries no identity, cannot list or delete, and one link reads where another writes.
use crate::domain::Key;
use crate::ports::{BlobStore, PutOutcome, StoreError};
use cli_http::{Request, Response, Via};
use std::path::Path;
use std::time::Duration;

const CONNECT: Duration = Duration::from_secs(3);
const RESPONSE: Duration = Duration::from_secs(10);
// A ceiling over the two above, so a body that stalls mid-transfer cannot hang a run.
const TRANSFER: Duration = Duration::from_secs(120);
// The same ceiling for an object streamed to or from a file (the pnpm store, about 0.6 GB).
const FILE_TRANSFER: Duration = Duration::from_secs(15 * 60);

/// Pauses before the retries of a request that failed in transit or was answered 408, 429 or 5xx: a blip of the store or the network must not turn a
/// whole run's cache cold (the breaker opens on the first failure that reaches it), and a store that is down still fails within a second.
const BACKOFF: [Duration; 2] = [Duration::from_millis(200), Duration::from_millis(800)];

/// What a request sends.
#[derive(Clone, Copy)]
enum Payload<'a> {
    Empty,
    Bytes(&'a [u8]),
    File(&'a Path),
}

pub struct OciParStore {
    read: String,
    write: Option<String>,
    conditional_put: bool,
    via: Via,
}

impl OciParStore {
    /// Both links end at the bucket's `/o/`; with no write link the store is read-only.
    pub fn new(read_par_url: &str, write_par_url: Option<&str>) -> OciParStore {
        let base = |url: &str| if url.ends_with('/') { url.to_string() } else { format!("{url}/") };
        OciParStore { read: base(read_par_url), write: write_par_url.map(base), conditional_put: false, via: Via::Environment }
    }

    /// Write with `if-none-match: *` instead of checking first. Only for a bucket whose PAR is known to accept it.
    pub fn conditional_put(mut self, on: bool) -> OciParStore {
        self.conditional_put = on;
        self
    }

    pub fn via(mut self, via: Via) -> OciParStore {
        self.via = via;
        self
    }

    fn request(&self, method: &str, base: &str, key: &Key, ceiling: Duration) -> Request {
        Request::new(method, &format!("{base}{key}")).via(self.via.clone()).connect_timeout(CONNECT).response_timeout(RESPONSE).timeout(ceiling)
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

    fn send(&self, method: &str, base: &str, key: &Key, payload: Payload, if_none_match: bool) -> Result<Response, StoreError> {
        self.with_retries(|| {
            let mut request = match payload {
                Payload::Empty => self.request(method, base, key, TRANSFER),
                Payload::Bytes(body) => self.request(method, base, key, TRANSFER).header("Content-Type", "application/octet-stream").body(body),
                Payload::File(path) => self.request(method, base, key, FILE_TRANSFER).header("Content-Type", "application/octet-stream").body_file(path),
            };
            if if_none_match {
                request = request.header("If-None-Match", "*");
            }
            request.send()
        })
        .map_err(|error| self.unreachable(method, key, &error))
    }

    fn unreachable(&self, method: &str, key: &Key, error: &str) -> StoreError {
        StoreError::Unreachable(format!("{method} {key}: {}", self.scrub(error)))
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
            status => Err(self.failure("PUT", key, status)),
        }
    }

    /// An error text never carries a link: the token in it is the credential.
    fn scrub(&self, text: &str) -> String {
        let mut clean = text.replace(&self.read, "<par>");
        if let Some(write) = &self.write {
            clean = clean.replace(write, "<par>");
        }
        clean
    }

    fn failure(&self, method: &str, key: &Key, status: u16) -> StoreError {
        let said = format!("{method} {key}: HTTP {status}");
        match status {
            401 | 403 => StoreError::Credential(said),
            408 | 429 | 500..=599 => StoreError::Unreachable(said),
            _ => StoreError::Rejected(said),
        }
    }
}

impl BlobStore for OciParStore {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        let response = self.send("GET", &self.read, key, Payload::Empty, false)?;
        match response.status {
            200 => Ok(Some(response.body)),
            404 => Ok(None),
            status => Err(self.failure("GET", key, status)),
        }
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        match self.send("HEAD", &self.read, key, Payload::Empty, false)?.status {
            200 => Ok(true),
            404 => Ok(false),
            status => Err(self.failure("HEAD", key, status)),
        }
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.put_payload(key, Payload::Bytes(body), if_absent)
    }

    fn get_file(&self, key: &Key, dest: &Path) -> Result<bool, StoreError> {
        let response = self.with_retries(|| self.request("GET", &self.read, key, FILE_TRANSFER).send_to_file(dest)).map_err(|error| self.unreachable("GET", key, &error))?;
        match response.status {
            200 => Ok(true),
            404 => Ok(false),
            status => {
                if (200..300).contains(&status) {
                    // saved by the stream, and not an object
                    let _ = std::fs::remove_file(dest);
                }
                Err(self.failure("GET", key, status))
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
