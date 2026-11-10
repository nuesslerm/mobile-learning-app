//! The verification-artifacts bucket as `wf` reaches it: the public base objects are read from, the write link that uploads them
//! and the content types they carry. Run bundles, the verify report and the EAS Update pointer all live there.
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

pub fn put_par(url: &str, body: &[u8], content_type: &str) -> Result<(), String> {
    let response = cli_http::Request::new("PUT", url).header("content-type", content_type).body(body.to_vec()).send().map_err(|e| format!("report PUT failed: 0 {e}"))?;
    if !response.ok() {
        return Err(format!("report PUT failed: {} {}", response.status, response.text().trim()));
    }
    Ok(())
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

/// The bucket's write link: the vault; the environment only where no vault is configured, and there set empty (as the black-box
/// harness does) it is off, no vault call.
pub fn par_url() -> Result<String, String> {
    match secrets::ambient("VERIFICATION_ARTIFACTS_PAR_URL") {
        Some(u) if !u.is_empty() => return Ok(u),
        Some(_) => return Err("VERIFICATION_ARTIFACTS_PAR_URL is set empty: publishing is off in this environment".to_string()),
        None => {}
    }
    secrets::load_values(&["VERIFICATION_ARTIFACTS_PAR_URL"])
        .get("VERIFICATION_ARTIFACTS_PAR_URL")
        .cloned()
        .flatten()
        .filter(|u| !u.is_empty())
        .ok_or_else(|| "VERIFICATION_ARTIFACTS_PAR_URL is unset and not in the vault (the write link of the verification-artifacts bucket)".to_string())
}

/// Where the uploaded objects are read from: the bucket's public base, or `VERIFICATION_ARTIFACTS_READ_URL` (a stub in tests).
pub fn public_base() -> String {
    std::env::var("VERIFICATION_ARTIFACTS_READ_URL").ok().filter(|u| !u.is_empty()).map(|u| u.trim_end_matches('/').to_string()).unwrap_or_else(|| OCI_BUCKET_BASE.to_string())
}
