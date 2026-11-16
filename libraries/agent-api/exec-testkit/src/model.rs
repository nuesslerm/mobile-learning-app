//! The stand-in for the model API: the program `fake-model`, an OpenAI-compatible chat-completions server on a free loopback port that
//! answers from a script, so an opencode pointed at it (`OpencodeServer::start_scripted`) needs no network and no key. Built once per
//! process (or the one `EXEC_TESTKIT_FAKE_MODEL` names) and stopped when dropped.
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Names a built `fake-model` so a suite builds it once for everyone.
pub const ENV_FAKE_MODEL: &str = "EXEC_TESTKIT_FAKE_MODEL";

fn binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| match std::env::var_os(ENV_FAKE_MODEL) {
        Some(path) => {
            let path = PathBuf::from(path);
            assert!(path.is_file(), "{ENV_FAKE_MODEL}={} is no file", path.display());
            path
        }
        None => crate::cli::build("fake-model"),
    })
}

pub struct FakeModel {
    child: Child,
    addr: SocketAddr,
}

impl FakeModel {
    /// Returns once the server answers.
    pub fn start() -> FakeModel {
        let port = TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).expect("a free port").port();
        let child = Command::new(binary()).env("PORT", port.to_string()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("fake-model starts");
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let model = FakeModel { child, addr };
        let deadline = Instant::now() + Duration::from_secs(10);
        while crate::service::http(addr, "GET", "/v1/models", &[], "").is_err() {
            assert!(Instant::now() < deadline, "fake-model did not answer on {addr}");
            std::thread::sleep(Duration::from_millis(50));
        }
        model
    }

    /// `http://127.0.0.1:<port>/v1`, the `baseURL` of the provider.
    pub fn base_url(&self) -> String {
        format!("http://{}/v1", self.addr)
    }

    /// The chat-completions request bodies it has answered, oldest first.
    pub fn requests(&self) -> Vec<serde_json::Value> {
        let (_, body) = crate::service::http(self.addr, "GET", "/v1/requests", &[], "").expect("fake-model answers");
        body.as_array().cloned().unwrap_or_default()
    }
}

impl Drop for FakeModel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
