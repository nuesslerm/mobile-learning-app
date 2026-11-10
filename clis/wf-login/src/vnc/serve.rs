//! The `tailscale` CLI side of the listener: the node's own tailnet address (so the operator never hand-sets the tailnet
//! suffix) and the `tailscale serve` route that makes the gate reachable. The container shares the sidecar's NETWORK
//! namespace only, never its filesystem, so the CLI reaches the daemon through an explicitly shared `--socket`.
use std::process::{Command, Stdio};

fn tailscale(socket: Option<&str>, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("tailscale");
    // `--socket` is a GLOBAL flag: it must precede the subcommand.
    if let Some(socket) = socket.filter(|s| !s.is_empty()) {
        command.arg(format!("--socket={socket}"));
    }
    let out = command.args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if stderr.is_empty() { format!("exit {}", out.status) } else { stderr })
    }
}

/// `https://<this node's DNS name>` from `tailscale status --json`; `None` while the node is not up.
pub fn resolve_base_url(socket: Option<&str>) -> Option<String> {
    let status: serde_json::Value = serde_json::from_str(&tailscale(socket, &["status", "--json"]).ok()?).ok()?;
    let dns = status["Self"]["DNSName"].as_str()?.trim_end_matches('.');
    (!dns.is_empty()).then(|| format!("https://{dns}"))
}

/// Binds the HTTPS route to `backend`. Loud on failure: a listener whose link leads nowhere must not look healthy.
pub fn bind_route(socket: Option<&str>, backend: &str) -> Result<(), String> {
    tailscale(socket, &["serve", "--bg", "--https=443", backend]).map(|_| ()).map_err(|e| format!("tailscale serve bind failed: {e}"))
}
