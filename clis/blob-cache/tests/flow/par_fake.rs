//! The bucket as a PAR URL sees it, on loopback: `/p/<token>/n/<namespace>/b/<bucket>/o/<object name>`, objects in memory.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tiny_http::{Response, Server};

pub const TOKEN: &str = "SECRET-PAR-TOKEN";

#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub if_none_match: bool,
}

#[derive(Clone)]
pub struct ParFake {
    pub url: String,
    status: Arc<Mutex<Option<u16>>>,
    body: Arc<Mutex<Option<String>>>,
    seen: Arc<Mutex<Vec<Seen>>>,
    /// How many of the next requests answer `.1`, before serving normally again.
    fail_next: Arc<Mutex<(u32, u16)>>,
}

impl ParFake {
    /// Serves until the test process ends.
    pub fn start() -> ParFake {
        let server = Server::http("127.0.0.1:0").unwrap();
        let prefix = format!("/p/{TOKEN}/n/ns/b/build-cache/o/");
        let url = format!("http://127.0.0.1:{}{prefix}", server.server_addr().to_ip().unwrap().port());
        let fake = ParFake { url, status: Arc::default(), body: Arc::default(), seen: Arc::default(), fail_next: Arc::default() };
        let (status, body_forced, seen, fail_next) = (fake.status.clone(), fake.body.clone(), fake.seen.clone(), fake.fail_next.clone());
        std::thread::spawn(move || {
            let mut objects: HashMap<String, Vec<u8>> = HashMap::new();
            for mut request in server.incoming_requests() {
                let method = request.method().to_string();
                let if_none_match = request.headers().iter().any(|h| h.field.equiv("If-None-Match") && h.value.as_str() == "*");
                seen.lock().unwrap().push(Seen { method: method.clone(), if_none_match });
                let mut body = Vec::new();
                let _ = request.as_reader().read_to_end(&mut body);
                let mut forced = *status.lock().unwrap();
                {
                    let mut next = fail_next.lock().unwrap();
                    if next.0 > 0 {
                        next.0 -= 1;
                        forced = Some(next.1);
                    }
                }
                let forced_body = body_forced.lock().unwrap().clone();
                let name = request.url().strip_prefix(prefix.as_str()).map(str::to_string);
                let reply = match (forced, name) {
                    (Some(code), _) if forced_body.is_some() => Response::from_data(forced_body.unwrap_or_default().into_bytes()).with_status_code(code),
                    (Some(400), _) => Response::from_data(br#"{"code":"StorageQuotaExceeded","message":"The tenancy's Object Storage quota is exceeded"}"#.to_vec()).with_status_code(400),
                    (Some(code), _) => Response::from_data(Vec::new()).with_status_code(code),
                    (None, None) => Response::from_data(Vec::new()).with_status_code(404),
                    (None, Some(name)) => match method.as_str() {
                        "GET" | "HEAD" => match objects.get(&name) {
                            Some(found) => Response::from_data(found.clone()).with_status_code(200),
                            None => Response::from_data(Vec::new()).with_status_code(404),
                        },
                        "PUT" if if_none_match && objects.contains_key(&name) => Response::from_data(Vec::new()).with_status_code(412),
                        "PUT" => {
                            objects.insert(name, body);
                            Response::from_data(Vec::new()).with_status_code(200)
                        }
                        _ => Response::from_data(Vec::new()).with_status_code(405),
                    },
                };
                let _ = request.respond(reply);
            }
        });
        fake
    }

    /// Answers every request with `code` from now on, or serves normally again with `None`.
    pub fn answer_all_with(&self, code: Option<u16>) {
        *self.body.lock().unwrap() = None;
        *self.status.lock().unwrap() = code;
    }

    /// `answer_all_with(Some(code))` with `body` as the answer's body (400 otherwise answers with the quota body).
    pub fn answer_all_with_body(&self, code: u16, body: &str) {
        *self.body.lock().unwrap() = Some(body.to_string());
        *self.status.lock().unwrap() = Some(code);
    }

    /// Answers the next `count` requests with `code`, then serves normally again.
    pub fn fail_next(&self, count: u32, code: u16) {
        *self.fail_next.lock().unwrap() = (count, code);
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

/// A PAR URL that nothing listens on.
pub fn closed_url() -> String {
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    format!("http://127.0.0.1:{port}/p/{TOKEN}/n/ns/b/build-cache/o/")
}
