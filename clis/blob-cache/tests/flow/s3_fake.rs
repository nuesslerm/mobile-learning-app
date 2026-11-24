//! An S3 bucket on loopback: path-style `/<bucket>/<object name>`, objects in memory, every request's SigV4 signature recomputed
//! from the shared secret before it is served, and a bearer-token twin for the `dav` style of store.
use blob_cache::adapters::sigv4::{self, Credentials, Parts, UNSIGNED_PAYLOAD};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Request, Response, Server};

pub const BUCKET: &str = "bkt";
pub const ACCESS_KEY: &str = "AKIAFAKEACCESSKEY";
pub const SECRET_KEY: &str = "fake/secret+KEY0123456789abcdefghijklmnop";
pub const REGION: &str = "eu-central";
pub const BEARER: &str = "SECRET-BEARER-TOKEN";

#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub payload_hash: String,
    pub if_none_match: bool,
}

#[derive(Clone)]
pub struct S3Fake {
    pub endpoint: String,
    seen: Arc<Mutex<Vec<Seen>>>,
    fail_next: Arc<Mutex<(u32, u16)>>,
    skewed: Arc<Mutex<bool>>,
}

fn xml(code: u16, error: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    // like S3, the body echoes the access key and more, which an error text must never repeat
    let body = format!("<?xml version=\"1.0\"?><Error><Code>{error}</Code><Message>x</Message><AWSAccessKeyId>{ACCESS_KEY}</AWSAccessKeyId></Error>");
    Response::from_data(body.into_bytes()).with_status_code(code)
}

fn header(request: &Request, name: &str) -> Option<String> {
    request.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str().to_string())
}

/// `Ok(payload hash)` when the request is signed correctly with `secret`, else the S3 answer.
fn verify(request: &Request, body: &[u8], secret: &str) -> Result<String, (u16, &'static str)> {
    let denied = (403, "SignatureDoesNotMatch");
    let auth = header(request, "Authorization").ok_or((403, "AccessDenied"))?;
    let fields: HashMap<&str, &str> = auth.strip_prefix("AWS4-HMAC-SHA256 ").ok_or(denied)?.split(", ").filter_map(|f| f.split_once('=')).collect();
    let mut scope = fields.get("Credential").ok_or(denied)?.split('/');
    if scope.next() != Some(ACCESS_KEY) {
        return Err((403, "InvalidAccessKeyId"));
    }
    let (_date, region, service) = (scope.next().ok_or(denied)?, scope.next().ok_or(denied)?, scope.next().ok_or(denied)?);
    let headers: Vec<(String, String)> = fields.get("SignedHeaders").ok_or(denied)?.split(';').map(|name| (name.to_string(), header(request, name).unwrap_or_default())).collect();
    let amz_date = header(request, "x-amz-date").ok_or(denied)?;
    let payload_hash = header(request, "x-amz-content-sha256").ok_or(denied)?;
    if payload_hash != UNSIGNED_PAYLOAD && payload_hash != sigv4::sha256_hex(body) {
        return Err((400, "XAmzContentSHA256Mismatch"));
    }
    let credentials = Credentials { access_key: ACCESS_KEY, secret_key: secret, region, service };
    let (path, _) = request.url().split_once('?').unwrap_or((request.url(), ""));
    let expected = sigv4::signature(&credentials, &Parts { method: request.method().as_str(), path, query: "", headers: &headers, payload_hash: &payload_hash, amz_date: &amz_date });
    if fields.get("Signature") == Some(&expected.as_str()) {
        Ok(payload_hash)
    } else {
        Err(denied)
    }
}

impl S3Fake {
    /// Serves until the test process ends; requests signed with any secret but `SECRET_KEY` are refused.
    pub fn start() -> S3Fake {
        let server = Server::http("127.0.0.1:0").unwrap();
        let fake = S3Fake { endpoint: format!("http://127.0.0.1:{}", server.server_addr().to_ip().unwrap().port()), seen: Arc::default(), fail_next: Arc::default(), skewed: Arc::default() };
        let (seen, fail_next, skewed) = (fake.seen.clone(), fake.fail_next.clone(), fake.skewed.clone());
        std::thread::spawn(move || {
            let mut objects: HashMap<String, Vec<u8>> = HashMap::new();
            let prefix = format!("/{BUCKET}/");
            for mut request in server.incoming_requests() {
                let mut body = Vec::new();
                let _ = request.as_reader().read_to_end(&mut body);
                let method = request.method().to_string();
                let if_none_match = header(&request, "If-None-Match").as_deref() == Some("*");
                let signed = verify(&request, &body, SECRET_KEY);
                seen.lock().unwrap().push(Seen { method: method.clone(), payload_hash: header(&request, "x-amz-content-sha256").unwrap_or_default(), if_none_match });
                let failing = {
                    let mut next = fail_next.lock().unwrap();
                    (next.0 > 0).then(|| {
                        next.0 -= 1;
                        next.1
                    })
                };
                let name = request.url().strip_prefix(prefix.as_str()).map(str::to_string);
                let reply = match (failing, signed, name) {
                    (Some(code), _, _) => xml(code, "SlowDown"),
                    _ if *skewed.lock().unwrap() => xml(403, "RequestTimeTooSkewed"),
                    (_, Err((code, error)), _) => xml(code, error),
                    (_, Ok(_), None) => xml(404, "NoSuchBucket"),
                    (_, Ok(_), Some(name)) => match method.as_str() {
                        "GET" | "HEAD" => match objects.get(&name) {
                            Some(found) => Response::from_data(found.clone()).with_status_code(200),
                            None => xml(404, "NoSuchKey"),
                        },
                        "PUT" if if_none_match && objects.contains_key(&name) => xml(412, "PreconditionFailed"),
                        "PUT" => {
                            objects.insert(name, body);
                            Response::from_data(Vec::new()).with_status_code(200)
                        }
                        _ => xml(405, "MethodNotAllowed"),
                    },
                };
                let _ = request.respond(reply);
            }
        });
        fake
    }

    /// Answers the next `count` requests with `code` (a SlowDown error), then serves normally again.
    pub fn fail_next(&self, count: u32, code: u16) {
        *self.fail_next.lock().unwrap() = (count, code);
    }

    /// Answers every request as a store does when the client's clock is off.
    pub fn skew_clocks(&self) {
        *self.skewed.lock().unwrap() = true;
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// A server that wants `Authorization: Bearer <BEARER>` on every request and answers 401 otherwise; objects in memory under `/o/`.
#[derive(Clone)]
pub struct BearerFake {
    pub url: String,
    pub authorizations: Arc<Mutex<Vec<Option<String>>>>,
}

impl BearerFake {
    pub fn start() -> BearerFake {
        let server = Server::http("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/o/", server.server_addr().to_ip().unwrap().port());
        let authorizations: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
        let log = authorizations.clone();
        std::thread::spawn(move || {
            let mut objects: HashMap<String, Vec<u8>> = HashMap::new();
            for mut request in server.incoming_requests() {
                let mut body = Vec::new();
                let _ = request.as_reader().read_to_end(&mut body);
                let sent = header(&request, "Authorization");
                log.lock().unwrap().push(sent.clone());
                let name = request.url().strip_prefix("/o/").unwrap_or_default().to_string();
                let reply = match (sent.as_deref() == Some(&format!("Bearer {BEARER}")), request.method().as_str()) {
                    (false, _) => Response::from_data(Vec::new()).with_status_code(401),
                    (true, "GET" | "HEAD") => match objects.get(&name) {
                        Some(found) => Response::from_data(found.clone()).with_status_code(200),
                        None => Response::from_data(Vec::new()).with_status_code(404),
                    },
                    (true, "PUT") => {
                        objects.insert(name, body);
                        Response::from_data(Vec::new()).with_status_code(201)
                    }
                    (true, _) => Response::from_data(Vec::new()).with_status_code(405),
                };
                let _ = request.respond(reply.with_header(Header::from_bytes("X-Fake", "1").unwrap()));
            }
        });
        BearerFake { url, authorizations }
    }
}
