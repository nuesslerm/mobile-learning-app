//! `vm-ops-api-rs --healthcheck`: compose's check of this container, from its own environment alone.
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(3);

/// Whether `127.0.0.1:<port>/health` answers 200, asked with the legacy raw token the container holds (compose passes no signing
/// key, and `REQUIRE_SIGNED_AUTH` is unset on the VM).
pub async fn is_healthy(port: u16, token: &str) -> bool {
    let Ok(client) = reqwest::Client::builder().no_proxy().timeout(TIMEOUT).build() else { return false };
    let Ok(response) = client.get(format!("http://127.0.0.1:{port}/health")).header("authorization", format!("Bearer {token}")).send().await else { return false };
    response.status().as_u16() == 200
}
