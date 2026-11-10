//! Implementations of the ports of `leetcode-backend-core` with real technology: the clock, a JSON file, HTTP to leetcode.com
//! and to the capture tool, and child processes.

pub mod config;
mod capture_http;
mod clock;
mod leetcode_http;
mod process_runner;
mod session_file;

pub use capture_http::HttpCaptureTool;
pub use clock::SystemClock;
pub use leetcode_http::HttpLeetcode;
pub use process_runner::ProcessRunner;
pub use session_file::FileSessionStore;

use leetcode_backend_core::application::ports::Ports;
use secrecy::SecretString;
use std::path::PathBuf;
use std::sync::Arc;

/// The real adapter behind every port, as `main` wires them.
pub fn ports(session_file: PathBuf, leetcode_origin: String, capture_base_url: String, capture_secret: SecretString) -> Ports {
    let client = http_client();
    Ports {
        store: Arc::new(FileSessionStore::new(session_file)),
        leetcode: Arc::new(HttpLeetcode::new(client.clone(), leetcode_origin)),
        capture: Arc::new(HttpCaptureTool::new(client, capture_base_url, capture_secret)),
        runner: Arc::new(ProcessRunner),
        clock: Arc::new(SystemClock),
    }
}

/// The HTTP client both outbound adapters use: redirects are never followed (a cookie must not be forwarded to another host),
/// connecting gives up after 10 seconds and a whole exchange after 60.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("a client with a fixed configuration")
}
