//! The other doubles a page meets: the upstream server a proxied request reaches, and a Chromium executable that
//! prints its DevTools line and waits.
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>)>>>;

/// Answers every request with `from upstream` and a permissive CORS header, recording each path and its headers.
pub fn upstream() -> (u16, Seen) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            let headers = request.headers().iter().map(|h| (h.field.to_string().to_ascii_lowercase(), h.value.to_string())).collect();
            log.lock().unwrap().push((request.url().to_string(), headers));
            let reply = tiny_http::Response::from_string("from upstream").with_header("Access-Control-Allow-Origin: *".parse::<tiny_http::Header>().unwrap());
            let _ = request.respond(reply);
        }
    });
    (port, seen)
}

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("cdp-flow-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An ffmpeg that writes what it is piped to its last argument. One for the whole run: the recording tests set
/// `FFMPEG_PATH` side by side, and a path one of them deleted would send the other to the real ffmpeg.
pub fn fake_ffmpeg() -> PathBuf {
    static EXE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    EXE.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("cdp-flow-ffmpeg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("ffmpeg");
        std::fs::write(&exe, "#!/bin/sh\nfor a; do out=\"$a\"; done; cat > \"$out\"\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        exe
    })
    .clone()
}

/// An executable that records its arguments and `FAKE_ENV`, then runs `body` (a shell fragment).
pub fn fake_chromium(dir: &TempDir, name: &str, body: &str) -> PathBuf {
    let exe = dir.0.join(name);
    let record = dir.0.join(format!("{name}.args"));
    std::fs::write(&exe, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" \"env=$FAKE_ENV\" > '{}'\n{body}\n", record.display())).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    exe
}
