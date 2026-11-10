//! Implementations of the ports of `leetcode-auth-cookie-capture-core` with real technology: HTTP to the backend and Telegram, the
//! browser daemon of `wf`, the virtual display's child processes and the system clock.

mod backend_http;
mod browser_daemon;
pub mod config;
mod display_stack;
mod notifier_http;
mod system_clock;

pub use backend_http::HttpBackend;
pub use browser_daemon::WfBrowser;
pub use display_stack::XvfbStacks;
pub use notifier_http::HttpNotifier;
pub use system_clock::SystemClock;

use capture_core::application::ports::Ports;
use secrecy::SecretString;
use std::sync::Arc;
use std::time::Duration;

/// One client for the backend and Telegram; a call that outlasts 30 s is a failure, not a wait.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("a default client builds")
}

/// The real adapter behind every port, as `main` wires them.
pub fn ports(backend_base_url: String, admin_secret: SecretString, notify_base_url: String, notify_secret: SecretString, wf_bin: Option<&str>) -> Ports {
    let client = http_client();
    Ports {
        backend: Arc::new(HttpBackend::new(client.clone(), backend_base_url, admin_secret)),
        notifier: Arc::new(HttpNotifier::new(client.clone(), notify_base_url, notify_secret)),
        browsers: Arc::new(WfBrowser::new(client, wf_bin)),
        displays: Arc::new(XvfbStacks::on_path()),
        clock: Arc::new(SystemClock),
    }
}
