//! The retired verification-artifacts bucket's public base (read only) and the content types run bundles carry.
use serde_json::Value;

pub const OCI_BUCKET_BASE: &str = "https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o";
/// Under `verification/`: the bucket's write link is scoped to that prefix.
pub const TEST_RUNS_PREFIX: &str = "verification/test-runs";

pub fn content_type_for(rel: &str) -> &'static str {
    match rel.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("html") => "text/html",
        Some("png") => "image/png",
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("txt") => "text/plain",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// A public object of the bucket (the write link cannot read): none yet is `Ok(None)`, any other failure is an error, so a
/// caller never replaces an index it could not read.
pub fn read_public(url: &str) -> Result<Option<String>, String> {
    let r = cli_http::Request::get(url).send().map_err(|e| format!("cannot read {url}: {e}"))?;
    match r.status {
        404 => Ok(None),
        200 => Ok(Some(r.text())),
        s => Err(format!("{url} answered {s}")),
    }
}

/// The published index, read where everyone reads it (the write link cannot read): none yet is a fresh start, anything else
/// unreadable stops the publish rather than replace the history with one run.
pub fn existing_index() -> Result<Option<String>, String> {
    let url = format!("{}/{TEST_RUNS_PREFIX}/index.json", public_base());
    match read_public(&url)? {
        Some(t) if serde_json::from_str::<Value>(&t).is_ok_and(|v| v["runs"].is_array()) => Ok(Some(t)),
        Some(_) => Err(format!("{url} has no runs list; the index was left as it is")),
        None => Ok(None),
    }
}

/// Where the uploaded objects are read from: the bucket's public base, or `VERIFICATION_ARTIFACTS_READ_URL` (a stub in tests).
pub fn public_base() -> String {
    std::env::var("VERIFICATION_ARTIFACTS_READ_URL").ok().filter(|u| !u.is_empty()).map(|u| u.trim_end_matches('/').to_string()).unwrap_or_else(|| OCI_BUCKET_BASE.to_string())
}
