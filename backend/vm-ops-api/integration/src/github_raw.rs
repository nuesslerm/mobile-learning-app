//! The local double of raw.githubusercontent.com that the helper cases point `provision` at (plan section 3, "Helper
//! commands"): an HTTP/1.1 server on a loopback port that answers a GET of each path it was given with that file's bytes
//! and anything else with 404, as GitHub does, and records each request's method, path and `Authorization` header.
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct GithubRaw {
    base: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    server: Option<JoinHandle<()>>,
}

impl GithubRaw {
    /// Serves `files`, each a path (`/<owner>/<repo>/<ref>/<file>`) and its bytes.
    pub fn serve(files: &[(&str, &[u8])]) -> GithubRaw {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port for the raw double");
        let base = format!("http://127.0.0.1:{}", listener.local_addr().expect("the raw double's address").port());
        let files: HashMap<String, Vec<u8>> = files.iter().map(|(path, bytes)| (path.to_string(), bytes.to_vec())).collect();
        let (requests, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
        let (log, halt) = (requests.clone(), stop.clone());
        let server = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if halt.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    answer(stream, &files, &log);
                }
            }
        });
        GithubRaw { base, requests, stop, server: Some(server) }
    }

    /// `http://127.0.0.1:<port>`, what a provision URL's `https://raw.githubusercontent.com` becomes.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Every request so far, in order: `{method, path, authorization}`, the last null when the header was not sent.
    pub fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

fn answer(mut stream: TcpStream, files: &HashMap<String, Vec<u8>>, log: &Mutex<Vec<Value>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let (mut head, mut chunk) = (Vec::new(), [0u8; 4096]);
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head).to_string();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or("").split(' ');
    let (method, path) = (request_line.next().unwrap_or(""), request_line.next().unwrap_or(""));
    let header = |name: &str| head.split("\r\n").skip(1).filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case(name));
    let authorization = header("authorization").map(|(_, v)| v.trim().to_string());
    log.lock().unwrap_or_else(|e| e.into_inner()).push(json!({ "method": method, "path": path, "authorization": authorization }));
    let (status, body) = match (method, files.get(path)) {
        ("GET", Some(bytes)) => ("200 OK", bytes.clone()),
        _ => ("404 Not Found", b"404: Not Found".to_vec()),
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes()).and_then(|()| stream.write_all(&body)).and_then(|()| stream.flush());
}

impl Drop for GithubRaw {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // A connection wakes the accept loop, which then sees the stop
        let _ = TcpStream::connect(self.base.trim_start_matches("http://"));
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}
