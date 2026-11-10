//! Which origins may call the service: an explicit allowlist plus localhost on any port. The CORS headers themselves come from
//! `tower-http`.

/// The host of `http(s)://host[:port]`, where the port is digits only.
fn host_of(origin: &str) -> Option<&str> {
    let rest = origin.strip_prefix("https://").or_else(|| origin.strip_prefix("http://"))?;
    match rest.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => Some(host),
        Some(_) => None,
        None => Some(rest),
    }
}

fn is_localhost(origin: &str) -> bool {
    host_of(origin).is_some_and(|host| host == "localhost" || host == "127.0.0.1")
}

pub fn parse_allowed_origins(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|o| o.trim().trim_end_matches('/').to_string())
        .filter(|o| !o.is_empty())
        .collect()
}

pub fn is_origin_allowed(origin: Option<&str>, allowed: &[String]) -> bool {
    let Some(origin) = origin else { return false };
    let normalized = origin.trim_end_matches('/');
    is_localhost(normalized) || allowed.iter().any(|a| a == normalized)
}
