//! Recognising a tailnet URL so navigation goes through a loopback TCP forward instead of Chromium's SOCKS5 proxy setting
//! (whose WebSockets fail): the CGNAT range Tailscale assigns (100.64.0.0/10) and MagicDNS names under `.ts.net`.

struct Parts<'a> {
    scheme: &'a str,
    userinfo: &'a str,
    host: &'a str,
    port: Option<u16>,
    rest: &'a str,
}

fn split(url: &str) -> Option<Parts<'_>> {
    let (scheme, after) = url.split_once("://")?;
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, rest) = after.split_at(end);
    let (userinfo, hostport) = authority.rsplit_once('@').map_or(("", authority), |(u, h)| (u, h));
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !hostport.ends_with(']') => (h, Some(p.parse::<u16>().ok()?)),
        _ => (hostport, None),
    };
    Some(Parts { scheme, userinfo, host, port, rest })
}

fn is_tailnet_ip(host: &str) -> bool {
    let octets: Vec<Option<u32>> = host.split('.').map(|p| if !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()) { p.parse().ok() } else { None }).collect();
    octets.len() == 4 && octets.iter().all(|o| o.is_some_and(|v| v <= 255)) && octets[0] == Some(100) && octets[1].is_some_and(|v| (64..=127).contains(&v))
}

fn is_magic_dns(host: &str) -> bool {
    host == "ts.net" || host.ends_with(".ts.net")
}

/// The tailnet host and port a URL targets, or `None` for a public host, loopback or a non-http(s)/ws(s) scheme.
pub fn parse_target(url: &str) -> Option<(String, u16)> {
    let p = split(url)?;
    if !["http", "https", "ws", "wss"].contains(&p.scheme) || !(is_tailnet_ip(p.host) || is_magic_dns(p.host)) {
        return None;
    }
    let secure = p.scheme == "https" || p.scheme == "wss";
    let port = p.port.unwrap_or(if secure { 443 } else { 80 });
    (port > 0).then(|| (p.host.to_string(), port))
}

fn rebuild(p: &Parts, host: &str, port: u16) -> String {
    let user = if p.userinfo.is_empty() { String::new() } else { format!("{}@", p.userinfo) };
    format!("{}://{user}{host}:{port}{}", p.scheme, p.rest)
}

/// The same URL with its authority replaced by the loopback forward; the scheme is kept (TLS still ends at the real host).
pub fn with_loopback_authority(url: &str, local_port: u16) -> String {
    split(url).map_or_else(|| url.to_string(), |p| rebuild(&p, "127.0.0.1", local_port))
}

/// A MagicDNS name on a secure scheme must keep its hostname: rewriting it to an IP literal drops SNI, and
/// `tailscale serve` routes by SNI.
pub fn needs_sni_preserving_hosts_mapping(url: &str) -> bool {
    split(url).is_some_and(|p| (p.scheme == "https" || p.scheme == "wss") && is_magic_dns(p.host))
}

/// The same URL with only its port replaced by the forward's, the hostname untouched.
pub fn with_loopback_port_only(url: &str, local_port: u16) -> String {
    split(url).map_or_else(|| url.to_string(), |p| rebuild(&p, p.host, local_port))
}

/// Points one MagicDNS hostname at 127.0.0.1 in /etc/hosts (idempotent, never removed).
pub fn ensure_hosts_file_mapping(hostname: &str) {
    let Ok(existing) = std::fs::read_to_string("/etc/hosts") else { return };
    let mapped = existing.lines().any(|l| l.trim().starts_with("127.0.0.1") && l.trim().ends_with(&format!(" {hostname}")));
    if !mapped {
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open("/etc/hosts") {
            use std::io::Write;
            let _ = writeln!(f, "127.0.0.1 {hostname}");
        }
    }
}
