//! The doubles of the flow tests, in-process servers on loopback. `MemBucket` is the one private bucket as a pre-authenticated link
//! sees it (objects in memory, every request needs the link), plus the two operator calls the `oci` stub makes. `LibsProxy` is the
//! tailnet read proxy in front of it, serving index and crates the way backend/libs-proxy does. `CargoRegistryFake` is a registry
//! with Cargo's publish API.
use crate::domain::index;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server};

pub const TOKEN: &str = "SECRET-PAR-TOKEN";

type Objects = Arc<Mutex<BTreeMap<String, (Vec<u8>, i64)>>>;

fn serve(server: Server, mut handle: impl FnMut(Request) + Send + 'static) {
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            handle(request);
        }
    });
}

fn reply(request: Request, status: u16, body: Vec<u8>) {
    let _ = request.respond(Response::from_data(body).with_status_code(status));
}

fn query(url: &str, name: &str) -> String {
    url.split_once('?').map(|(_, q)| q).unwrap_or("").split('&').find_map(|pair| pair.strip_prefix(&format!("{name}="))).unwrap_or("").to_string()
}

#[derive(Clone)]
pub struct MemBucket {
    port: u16,
    objects: Objects,
}

const BUCKET: &str = "build-cache";

impl MemBucket {
    pub fn start() -> MemBucket {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let objects: Objects = Arc::default();
        let store = objects.clone();
        let prefix = format!("/p/{TOKEN}/n/ns/b/{BUCKET}/o/");
        serve(server, move |mut request| {
            let url = request.url().to_string();
            let mut body = Vec::new();
            let _ = request.as_reader().read_to_end(&mut body);
            let mut objects = store.lock().unwrap();
            if let Some(name) = url.strip_prefix(&prefix) {
                let name = name.to_string();
                return match request.method() {
                    Method::Put => {
                        objects.insert(name, (body, wf_kit::time::now_ms()));
                        reply(request, 200, Vec::new());
                    }
                    _ => match objects.get(&name) {
                        Some((bytes, _)) => reply(request, 200, bytes.clone()),
                        None => reply(request, 404, Vec::new()),
                    },
                };
            }
            if url.starts_with("/admin/list") {
                let wanted = query(&url, "prefix");
                let rows: Vec<serde_json::Value> = objects
                    .iter()
                    .filter(|(name, _)| name.starts_with(&wanted))
                    .map(|(name, (bytes, created))| serde_json::json!({ "name": name, "size": bytes.len(), "time-created": wf_kit::time::iso(*created) }))
                    .collect();
                return reply(request, 200, serde_json::json!({ "data": rows }).to_string().into_bytes());
            }
            if url.starts_with("/admin/delete") {
                objects.remove(&query(&url, "name"));
                return reply(request, 200, Vec::new());
            }
            reply(request, 404, Vec::new());
        });
        MemBucket { port, objects }
    }

    /// The link for `LIBS_READ_PAR_URL` and `LIBS_WRITE_PAR_URL` (ends at the `libs/` prefix).
    pub fn par_url(&self) -> String {
        format!("http://127.0.0.1:{}/p/{TOKEN}/n/ns/b/{BUCKET}/o/libs/", self.port)
    }

    pub fn put(&self, name: &str, body: &[u8]) {
        self.objects.lock().unwrap().insert(name.to_string(), (body.to_vec(), wf_kit::time::now_ms()));
    }

    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        self.objects.lock().unwrap().get(name).map(|(b, _)| b.clone())
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.get(name).map(|b| String::from_utf8_lossy(&b).into_owned())
    }

    pub fn names(&self) -> Vec<String> {
        self.objects.lock().unwrap().keys().cloned().collect()
    }

    /// Makes an object look `days` old to the listing.
    pub fn age(&self, name: &str, days: i64) {
        if let Some(entry) = self.objects.lock().unwrap().get_mut(name) {
            entry.1 = wf_kit::time::now_ms() - days * 86_400_000;
        }
    }

    /// An `oci` that answers `os ns get`, `os object list` and `os object delete` from this bucket, to put first on PATH.
    pub fn oci_stub(&self, bin_dir: &Path) {
        let script = format!(
            "#!/bin/bash\nbase=http://127.0.0.1:{}\n\n\
             opt() {{ local want=$1; shift; while [ $# -gt 0 ]; do [ \"$1\" = \"$want\" ] && echo \"$2\" && return; shift; done; }}\n\
             case \"$1 $2 $3\" in\n\
             \"os ns get\") echo '{{\"data\":\"ns\"}}';;\n\
             \"os object list\") curl -s --noproxy '*' \"$base/admin/list?prefix=$(opt --prefix \"$@\")\";;\n\
             \"os object delete\") curl -s --noproxy '*' -X DELETE \"$base/admin/delete?name=$(opt --object-name \"$@\")\";;\n\
             *) echo \"oci stub: $*\" >&2; exit 1;;\nesac\n",
            self.port
        );
        let path = bin_dir.join("oci");
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
}

/// The read proxy of the private bucket: `GET /index/<sparse path>` and `GET /dl/<crate>/<version>.crate` read `libs/index/...` and
/// `libs/crates/...` of the bucket, `GET /index/config.json` is generated here (not read), anything else is 404 and a write verb 405.
/// It holds no link: a reader of it never sees one.
#[derive(Clone)]
pub struct LibsProxy {
    port: u16,
    served: Arc<Mutex<Vec<String>>>,
}

impl LibsProxy {
    pub fn start(bucket: &MemBucket) -> LibsProxy {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let served: Arc<Mutex<Vec<String>>> = Arc::default();
        let (log, bucket) = (served.clone(), bucket.clone());
        serve(server, move |request| {
            let url = request.url().to_string();
            if request.method() != &Method::Get {
                return reply(request, 405, Vec::new());
            }
            log.lock().unwrap().push(url.clone());
            let found = if url == "/index/config.json" {
                Some(format!("{{\"dl\":\"http://127.0.0.1:{port}/dl/{{crate}}/{{version}}.crate\"}}\n").into_bytes())
            } else if let Some(path) = url.strip_prefix("/index/") {
                bucket.get(&format!("libs/index/{path}"))
            } else if let Some(path) = url.strip_prefix("/dl/") {
                bucket.get(&format!("libs/crates/{path}"))
            } else {
                None
            };
            match found {
                Some(bytes) => reply(request, 200, bytes),
                None => reply(request, 404, Vec::new()),
            }
        });
        LibsProxy { port, served }
    }

    /// The registry's `indexUrl` in `libraries.json`, and what Cargo is given after `sparse+`.
    pub fn index_url(&self) -> String {
        format!("http://127.0.0.1:{}/index/", self.port)
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The request paths answered so far, in order.
    pub fn requests(&self) -> Vec<String> {
        self.served.lock().unwrap().clone()
    }

    /// A `GET` of `path` as a reader would send it: `(status, body)`.
    pub fn fetch(&self, path: &str) -> (u16, Vec<u8>) {
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        write!(stream, "GET {path} HTTP/1.0\r\nHost: proxy\r\n\r\n").unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let status = String::from_utf8_lossy(&raw[..split]).split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, raw[split + 4..].to_vec())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    pub name: String,
    pub vers: String,
    pub token: Option<String>,
}

/// A registry with Cargo's publish API (`PUT /api/v1/crates/new`), serving its own sparse index and downloads. It records what it
/// was sent and the `Authorization` header, so a test can say the right credential arrived.
#[derive(Clone)]
pub struct CargoRegistryFake {
    port: u16,
    received: Arc<Mutex<Vec<Received>>>,
}

impl CargoRegistryFake {
    pub fn start() -> CargoRegistryFake {
        let server = Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let received: Arc<Mutex<Vec<Received>>> = Arc::default();
        let log = received.clone();
        let (mut indexes, mut crates): (BTreeMap<String, String>, BTreeMap<String, Vec<u8>>) = Default::default();
        serve(server, move |mut request| {
            let url = request.url().to_string();
            let mut body = Vec::new();
            let _ = request.as_reader().read_to_end(&mut body);
            if request.method() == &Method::Put && url == "/api/v1/crates/new" {
                let token = request.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.to_string());
                let json_len = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
                let meta: serde_json::Value = serde_json::from_slice(&body[4..4 + json_len]).unwrap();
                let rest = &body[4 + json_len..];
                let crate_len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
                let bytes = rest[4..4 + crate_len].to_vec();
                let (name, vers) = (meta["name"].as_str().unwrap().to_string(), meta["vers"].as_str().unwrap().to_string());
                let line = serde_json::json!({ "name": name, "vers": vers, "deps": [], "cksum": hex::encode(Sha256::digest(&bytes)), "features": {}, "yanked": false });
                indexes.entry(index::path(&name)).or_default().push_str(&format!("{line}\n"));
                crates.insert(format!("{name}/{vers}"), bytes);
                log.lock().unwrap().push(Received { name, vers, token });
                return reply(request, 200, br#"{"warnings":{"invalid_categories":[],"invalid_badges":[],"other":[]}}"#.to_vec());
            }
            if url == "/config.json" {
                let config = serde_json::json!({ "dl": format!("http://127.0.0.1:{port}/dl/{{crate}}/{{version}}.crate"), "api": format!("http://127.0.0.1:{port}") });
                let mut response = Response::from_data(config.to_string().into_bytes());
                response.add_header(Header::from_bytes("Content-Type", "application/json").unwrap());
                let _ = request.respond(response);
                return;
            }
            if let Some(name) = url.strip_prefix("/dl/") {
                let key = name.trim_end_matches(".crate");
                return match crates.get(key) {
                    Some(bytes) => reply(request, 200, bytes.clone()),
                    None => reply(request, 404, Vec::new()),
                };
            }
            match indexes.get(url.trim_start_matches('/')) {
                Some(text) => reply(request, 200, text.clone().into_bytes()),
                None => reply(request, 404, Vec::new()),
            }
        });
        CargoRegistryFake { port, received }
    }

    pub fn index_url(&self) -> String {
        format!("sparse+http://127.0.0.1:{}/", self.port)
    }

    pub fn received(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }
}
