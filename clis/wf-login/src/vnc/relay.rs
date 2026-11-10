//! Hands a captured credential FILE back to agent-shell: watch for the tool's credential path and, once the login is valid, POST it
//! (base64) to vm-ops-api's `/agent-shell/credentials`, which writes it into the real agent-shell container with the right
//! ownership (a shared volume does not work: the two containers' users have different uids).
use super::descriptor::{env_or_vault, http_request, Descriptor};
use super::engine::LINK_TTL_MS;
use super::log;
use base64::Engine as _;
use std::time::{Duration, Instant};

const DEFAULT_BASE_URL: &str = "http://vm-ops-api-main:8080";
const POLL_INTERVAL: Duration = Duration::from_secs(2);

pub fn expand_tilde(p: &str) -> std::path::PathBuf {
    let home = || std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => p.into(),
    }
}

/// Starts watching on its own thread; a tool whose credential is not a file (leetcode's cookies) has nothing to watch.
/// The window matches the link's own TTL, so a login finished late is still relayed rather than stranded on the desktop.
pub fn watch_and_relay(descriptor: &Descriptor, user: &str) {
    let Some(raw_path) = descriptor.valid_path.clone().or_else(|| descriptor.credential_paths.first().cloned()) else { return };
    let tool = descriptor.id.clone();
    let Some(token) = env_or_vault("VM_OPS_API_AUTH_TOKEN") else {
        log::error("agent_shell_relay.missing_token", &[("tool", tool)]);
        return;
    };
    let base = std::env::var("VNC_AUTH_VM_OPS_API_URL").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    let path = expand_tilde(&raw_path);
    let (descriptor, user) = (descriptor.clone(), user.to_string());
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_millis(LINK_TTL_MS as u64);
        while Instant::now() < deadline {
            // The tool's own validity check decides, not the file's mere existence: a gcloud command run here creates an empty
            // credentials.db, and relaying that would overwrite agent-shell's good login with nothing.
            if path.exists() && crate::auth::is_authenticated(&descriptor, Some(&user)) {
                match std::fs::read(&path) {
                    Ok(content) => relay(&base, &token, &tool, &content),
                    Err(e) => log::error("agent_shell_relay.read_failed", &[("tool", tool.clone()), ("err", e.to_string())]),
                }
                return;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

fn relay(base: &str, token: &str, tool: &str, content: &[u8]) {
    let body = serde_json::json!({ "tool": tool, "content": base64::engine::general_purpose::STANDARD.encode(content) }).to_string();
    let headers = vec![("content-type".to_string(), "application/json".to_string()), ("authorization".to_string(), format!("Bearer {token}"))];
    match http_request("POST", &format!("{}/agent-shell/credentials", base.trim_end_matches('/')), &headers, Some(&body), 30) {
        Ok((status, _)) if (200..300).contains(&status) => log::info("agent_shell_relay.succeeded", &[("tool", tool.to_string())]),
        Ok((status, _)) => log::error("agent_shell_relay.rejected", &[("tool", tool.to_string()), ("status", status.to_string())]),
        Err(e) => log::error("agent_shell_relay.request_failed", &[("tool", tool.to_string()), ("err", e)]),
    }
}
