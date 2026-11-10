//! `wf secret verify`: a secret that is present can still be unusable for what it is for (a revoked token, a pasted key with a trailing
//! newline, a URL with no scheme). The checks are data in the registry, beside the secret they judge (`clis/secrets/src/data/registry.json`):
//!
//! - `check.shape`: one of `{"url": true}`, `{"pem": true}`, `{"prefix": "tskey-"}`, `{"regex": "…"}`;
//! - `check.live`: one read-only HTTP call to the issuing provider: `url`, optional `header` (name → value), `basic` (`[user, password]`)
//!   and `contains` (a marker the body must hold). `{value}` is the secret, `{KEY}` another resolved secret or env var, `{KEY|default}`
//!   with a default; a key that is not there leaves the call "unchecked", never failed;
//! - `uncheckable`: the written reason a secret has no check.
//!
//! Hygiene (no stray whitespace or newline) is checked for every value. Values are never printed.
use super::registry::Entry;
use cli_http::Request;
use regex::Regex;
use serde_json::{json, Value};
use std::time::Duration;

pub enum Status {
    Ok,
    Failed(String),
    /// Not checkable now (the provider did not answer, or a value the call needs is absent): reported, never a failure, so an
    /// outage cannot block a deploy.
    Unchecked(String),
}

pub struct Finding {
    pub key: String,
    pub checks: Vec<&'static str>,
    pub status: Status,
}

impl Finding {
    pub fn to_json(&self) -> Value {
        let (status, reason) = match &self.status {
            Status::Ok => ("ok", String::new()),
            Status::Failed(reason) => ("failed", reason.clone()),
            Status::Unchecked(reason) => ("unchecked", reason.clone()),
        };
        json!({ "key": self.key, "status": status, "checks": self.checks, "reason": reason })
    }

    pub fn failed(&self) -> bool {
        matches!(self.status, Status::Failed(_))
    }
}

/// `WF_VERIFY_ORIGIN` replaces the scheme and host of every provider call (the flow tests' seam); the paths stay.
fn origin_of(real: &str) -> String {
    match std::env::var("WF_VERIFY_ORIGIN").ok().filter(|o| !o.is_empty()) {
        Some(origin) => {
            let path = real.strip_prefix("https://").or_else(|| real.strip_prefix("http://")).and_then(|r| r.split_once('/')).map_or("", |(_, p)| p);
            format!("{}/{path}", origin.trim_end_matches('/'))
        }
        None => real.to_string(),
    }
}

fn hygiene(value: &str) -> Result<(), String> {
    if value != value.trim() {
        return Err("has leading or trailing whitespace or a newline".into());
    }
    if value.contains('\n') && !value.contains("-----BEGIN") {
        return Err("contains a newline".into());
    }
    Ok(())
}

fn is_http_url(value: &str) -> bool {
    value.strip_prefix("https://").or_else(|| value.strip_prefix("http://")).is_some_and(|rest| rest.split('/').next().is_some_and(|host| !host.is_empty() && !host.contains(' ')))
}

fn shape(spec: &Value, value: &str) -> Result<(), String> {
    if spec.get("url").is_some() {
        return if is_http_url(value) { Ok(()) } else { Err("is not an http(s) URL with a host".into()) };
    }
    if spec.get("pem").is_some() {
        let pem = value.replace("\\n", "\n");
        return if pem.contains("-----BEGIN") && pem.contains("-----END") { Ok(()) } else { Err("is not a PEM private key".into()) };
    }
    if let Some(prefix) = spec.get("prefix").and_then(Value::as_str) {
        return if value.starts_with(prefix) { Ok(()) } else { Err(format!("does not start with {prefix}")) };
    }
    if let Some(pattern) = spec.get("regex").and_then(Value::as_str) {
        let regex = Regex::new(pattern).map_err(|e| format!("the registry's pattern is invalid: {e}"))?;
        return if regex.is_match(value) { Ok(()) } else { Err(format!("does not match {pattern}")) };
    }
    Err("the registry's shape names no known kind".into())
}

/// `{value}`, `{KEY}` and `{KEY|default}` in `template`; `Err(KEY)` when a key is neither resolved, in the environment nor defaulted.
fn fill(template: &str, value: &str, resolved: &[(String, String)]) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else { return Err(format!("unclosed placeholder in {template}")) };
        let name = &rest[start + 1..start + end];
        let (key, default) = name.split_once('|').map_or((name, None), |(k, d)| (k, Some(d)));
        let found = if key == "value" {
            Some(value.to_string())
        } else {
            resolved.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).or_else(|| std::env::var(key).ok().filter(|v| !v.is_empty())).or_else(|| default.map(str::to_string))
        };
        out.push_str(&found.ok_or_else(|| key.to_string())?);
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn live(spec: &Value, value: &str, resolved: &[(String, String)]) -> Status {
    let timeout = Duration::from_secs(8);
    let fill_or = |template: &str| fill(template, value, resolved);
    let url = match spec.get("url").and_then(Value::as_str).map(fill_or) {
        Some(Ok(url)) => url,
        Some(Err(missing)) => return Status::Unchecked(format!("needs {missing}, which is not available")),
        None => return Status::Failed("the registry's live check has no url".into()),
    };
    let mut request = Request::get(&origin_of(&url)).header("User-Agent", "wf-preflight").timeout(timeout);
    for (name, template) in spec.get("header").and_then(Value::as_object).into_iter().flatten() {
        match template.as_str().map(fill_or) {
            Some(Ok(v)) => request = request.header(name, &v),
            _ => return Status::Unchecked(format!("header {name} could not be filled")),
        }
    }
    if let Some([user, password]) = spec.get("basic").and_then(Value::as_array).map(Vec::as_slice) {
        match (user.as_str().map(fill_or), password.as_str().map(fill_or)) {
            (Some(Ok(user)), Some(Ok(password))) => request = request.basic_auth(&user, &password),
            (Some(Err(missing)), _) | (_, Some(Err(missing))) => return Status::Unchecked(format!("needs {missing}, which is not available")),
            _ => return Status::Failed("the registry's basic auth is malformed".into()),
        }
    }
    let who = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).and_then(|r| r.split('/').next()).unwrap_or("the provider").to_string();
    match request.send() {
        Err(error) => Status::Unchecked(format!("{who} did not answer: {error}")),
        Ok(response) if response.ok() && spec.get("contains").and_then(Value::as_str).is_none_or(|marker| response.text().contains(marker)) => Status::Ok,
        Ok(response) if matches!(response.status, 401 | 403 | 404) => Status::Failed(format!("rejected by {who} (HTTP {})", response.status)),
        Ok(response) if response.status >= 500 => Status::Unchecked(format!("{who} answered HTTP {}", response.status)),
        Ok(response) => Status::Failed(format!("{who} answered HTTP {} without what a valid secret gets", response.status)),
    }
}

/// The findings for the resolved `(env var, value)` pairs, in order, each under its registry key.
pub fn check(registry: &[Entry], resolved: &[(String, String)]) -> Vec<Finding> {
    resolved
        .iter()
        .map(|(env_var, value)| {
            let entry = registry.iter().find(|e| &e.env_var == env_var);
            let key = entry.map_or_else(|| env_var.clone(), |e| e.key.clone());
            let mut checks = vec!["hygiene"];
            if let Err(reason) = hygiene(value) {
                return Finding { key, checks, status: Status::Failed(reason) };
            }
            let spec = entry.and_then(|e| e.check.as_ref());
            if let Some(shaped) = spec.and_then(|s| s.get("shape")) {
                checks.push("shape");
                if let Err(reason) = shape(shaped, value) {
                    return Finding { key, checks, status: Status::Failed(reason) };
                }
            }
            let status = match spec.and_then(|s| s.get("live")) {
                Some(call) => {
                    checks.push("live");
                    live(call, value, resolved)
                }
                None => Status::Ok,
            };
            Finding { key, checks, status }
        })
        .collect()
}
