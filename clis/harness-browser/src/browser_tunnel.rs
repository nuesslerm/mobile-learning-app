//! `wf browser tunnel [services...]`: publishes the deployed VM services on localhost ports (remote port + 10000) by
//! forwarding raw TCP through tailscaled's userspace SOCKS5 listener, so tests, curl and a browser all see ordinary
//! `http://localhost:<port>` URLs. Foreground on purpose: a backgrounded tunnel outlives its task and then serves a stale
//! mapping after a VM recreate. Ctrl-C is the teardown.
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

struct Mapping {
    name: &'static str,
    host: &'static str,
    remote_port: u16,
    local_port: u16,
}

const SERVICES: [Mapping; 4] = [
    Mapping { name: "vm-ops-api", host: "vm-ops-api-main", remote_port: 8080, local_port: 18080 },
    Mapping { name: "remote-chat-agent", host: "remote-chat-agent-prod", remote_port: 8787, local_port: 18787 },
    Mapping { name: "leetcode-backend", host: "leetcode-backend-prod", remote_port: 8789, local_port: 18789 },
    Mapping { name: "leetcode-capture", host: "leetcode-capture", remote_port: 8792, local_port: 18792 },
];

/// The name MagicDNS resolves for `host`: a peer's requested HostName and its DNSName differ once a stale device holds the
/// bare name (`x-prod` registers as `x-prod-2`), so the tailnet is asked rather than the pattern guessed. An unknown host
/// is returned unchanged.
fn live_host(host: &str) -> String {
    let Ok(out) = Command::new("tailscale").args(["status", "--json"]).output() else { return host.to_string() };
    if !out.status.success() {
        return host.to_string();
    }
    let Ok(status) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return host.to_string() };
    status["Peer"]
        .as_object()
        .into_iter()
        .flat_map(|peers| peers.values())
        .find(|p| p["HostName"].as_str() == Some(host))
        .and_then(|p| p["DNSName"].as_str())
        .and_then(|dns| dns.split('.').next())
        .filter(|label| !label.is_empty())
        .map_or_else(|| host.to_string(), str::to_string)
}

pub fn run(args: &[String]) -> Option<i32> {
    let parsed = wf_kit::args::parse_spec(args, &wf_kit::args::Spec::NONE)?;
    let args = &parsed.positional;
    let chosen: Vec<&Mapping> = if args.is_empty() {
        SERVICES.iter().collect()
    } else {
        let unknown: Vec<&str> = args.iter().map(String::as_str).filter(|n| !SERVICES.iter().any(|s| s.name == *n)).collect();
        if !unknown.is_empty() {
            let known: Vec<&str> = SERVICES.iter().map(|s| s.name).collect();
            wf_kit::wf_err!("wf: unknown service(s): {}. Known: {}", unknown.join(", "), known.join(", "));
            return Some(1);
        }
        args.iter().filter_map(|n| SERVICES.iter().find(|s| s.name == n)).collect()
    };
    let mut listening = 0;
    for m in &chosen {
        let line = format!("  localhost:{}  ->  {}:{}  ({})", m.local_port, m.host, m.remote_port, m.name);
        match crate::tailnet_forward::ensure(&live_host(m.host), m.remote_port, Some(m.local_port)) {
            Ok((_, true)) => println!("{line} — already forwarding"),
            Ok((_, false)) => {
                listening += 1;
                println!("{line}");
            }
            Err(e) => {
                wf_kit::wf_err!("wf: {e}");
                return Some(1);
            }
        }
    }
    let url_of = |name: &str| chosen.iter().find(|m| m.name == name).map(|m| format!("http://localhost:{}", m.local_port));
    let exports: Vec<String> = [("remote-chat-agent", "REMOTE_CHAT_AGENT_BASE_URL"), ("leetcode-backend", "LEETCODE_BACKEND_BASE_URL")]
        .iter()
        .filter_map(|(name, var)| url_of(name).map(|u| format!("{var}={u}")))
        .collect();
    if !exports.is_empty() {
        println!("\nRun a suite against the DEPLOYED services with:");
        for e in &exports {
            println!("  {e}");
        }
    }
    if listening == 0 {
        println!("\nAlready forwarding — nothing to start.");
        return Some(0);
    }
    println!("\nForwarding. Press Ctrl-C to stop.");
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        if signal_hook::flag::register(signal, std::sync::Arc::clone(&stop)).is_err() {
            wf_kit::wf_err!("wf: cannot install the stop handler for signal {signal}");
            return Some(1);
        }
    }
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
    Some(130)
}
