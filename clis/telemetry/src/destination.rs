/// Where records go: an OTLP/HTTP ingest and its bearer token.
#[derive(Clone, Debug)]
pub struct Destination {
    base: String,
    token: String,
}

impl Destination {
    /// `base` is the scheme and authority, e.g. `https://ingest.example`.
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
        Self { base: base.into(), token: token.into() }
    }

    /// An ingest at `https://<host>`; `host` must be a bare lowercase host name with an optional port, nothing else.
    pub fn from_host(host: &str, token: &str) -> Option<Self> {
        let (name, port) = host.split_once(':').map_or((host, None), |(n, p)| (n, Some(p)));
        let name_ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        let port_ok = port.is_none_or(|p| !p.is_empty() && p.len() <= 5 && p.chars().all(|c| c.is_ascii_digit()) && p.parse::<u32>().is_ok_and(|n| n <= 65535));
        (name_ok && port_ok).then(|| Self::new(format!("https://{host}"), token))
    }

    /// `BETTERSTACK_OTLP_HOST` and `BETTERSTACK_OTLP_TOKEN`, unless `SECRETS_LOG_BETTERSTACK=false`.
    pub fn from_env() -> Option<Self> {
        if std::env::var("SECRETS_LOG_BETTERSTACK").as_deref() == Ok("false") {
            return None;
        }
        let get = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        Self::from_host(&get("BETTERSTACK_OTLP_HOST")?, &get("BETTERSTACK_OTLP_TOKEN")?)
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

/// The official client words a rejection as `... Status(401, ...)`; callers read it as `HTTP 401`.
pub(crate) fn why(error: &str) -> String {
    match error.split("Status(").nth(1).and_then(|rest| rest.split(',').next()) {
        Some(status) => format!("HTTP {}", status.trim()),
        None => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_host_text_when_checked_then_only_a_bare_host_and_optional_port_is_accepted() {
        for good in ["ingest.example", "s1.eu-fsn-3.betterstackdata.com", "localhost:4318"] {
            assert!(Destination::from_host(good, "t").is_some(), "{good}");
        }
        for bad in ["", "ingest.example/path", "user@ingest.example", "ingest.example:port", "ingest.example:", "INGEST EXAMPLE", "a b", "h:99999"] {
            assert!(Destination::from_host(bad, "t").is_none(), "{bad}");
        }
    }
}
