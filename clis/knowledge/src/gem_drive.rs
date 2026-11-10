//! The Gem's knowledge as Google Docs: each part of the bundle is one Doc in a Drive folder, created once and overwritten in place afterwards
//! (the same Doc id, so the Gem's attachments never move), then read back. Plain Drive REST over `cli-http`; `WF_GOOGLE_DRIVE_BASE` points it
//! at a fake in tests. A `local:<dir>` destination writes `<name>.md` files instead.
use serde_json::{json, Value};
use std::path::Path;
use wf_kit::time::now_ms;

pub const DOC_MIME: &str = "application/vnd.google-apps.document";
/// How recent a Doc's modified time must be after an upload for the Doc to count as replaced.
const FRESH_MS: i64 = 15 * 60 * 1000;
/// Drive's export of a Doc differs from the text put in by line endings and a trailing newline; more than this share missing means Google cut it.
const TOLERANCE_PERCENT: usize = 2;

pub struct Part {
    pub name: String,
    pub text: String,
}

fn base() -> String {
    std::env::var("WF_GOOGLE_DRIVE_BASE").unwrap_or_else(|_| "https://www.googleapis.com".into())
}

pub fn chars(s: &str) -> usize {
    s.encode_utf16().count()
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// Milliseconds since the epoch of an RFC 3339 time with a `Z` suffix and any number of fraction digits, as Drive's `modifiedTime` is.
pub fn parse_ms(t: &str) -> Option<i64> {
    let (date, rest) = t.split_once('T')?;
    let clock = rest.strip_suffix('Z')?;
    let mut d = date.split('-');
    let (y, mo, da): (i64, i64, i64) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    let (hms, frac) = clock.split_once('.').unwrap_or((clock, ""));
    let mut t = hms.split(':');
    let (hh, mm, ss): (i64, i64, i64) = (t.next()?.parse().ok()?, t.next()?.parse().ok()?, t.next()?.parse().ok()?);
    let ms: i64 = if frac.is_empty() { 0 } else { format!("{:0<3}", frac.chars().take(3).collect::<String>()).parse().ok()? };
    Some(((days_from_civil(y, mo, da) * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms)
}

/// Whether a Doc now holds its part: a Google Doc (not a plain file), its text within the tolerance of what was sent (a Doc over the size
/// limit loses its tail), modified just now. The Ok line names the Doc id, which stays the same from run to run.
pub fn judge_doc(name: &str, id: &str, mime: &str, sent: usize, exported: usize, modified: &str, now: i64) -> Result<String, String> {
    if mime != DOC_MIME {
        return Err(format!("{name} is {mime} in Drive, not a Google Doc: the Gem would not refresh it"));
    }
    if exported * 100 < sent * (100 - TOLERANCE_PERCENT) {
        return Err(format!("{name} holds {exported} characters in Drive but {sent} were sent: Google cut the Doc"));
    }
    let age = now - parse_ms(modified).ok_or_else(|| format!("{name}: unreadable modified time \"{modified}\""))?;
    if age > FRESH_MS {
        return Err(format!("{name} was last modified {} minutes ago in Drive, so the upload did not replace it", age / 60_000));
    }
    Ok(format!("doc {name} id={id} chars={exported} modified={modified}"))
}

fn call(token: &str, request: cli_http::Request) -> Result<cli_http::Response, String> {
    let response = request.header("Authorization", &format!("Bearer {token}")).timeout(std::time::Duration::from_secs(300)).send()?;
    if response.ok() {
        return Ok(response);
    }
    Err(format!("Drive answered {}: {}", response.status, response.text().chars().take(400).collect::<String>()))
}

fn query(q: &str) -> String {
    cli_http::encode_uri_component(q)
}

/// `(id, mimeType)` of every non-trashed file of the folder whose name starts with `prefix`.
fn files_named(token: &str, folder: &str, prefix: &str) -> Result<Vec<(String, String, String)>, String> {
    let q = format!("'{folder}' in parents and name contains '{}' and trashed=false", prefix.replace('\'', "\\'"));
    let url = format!("{}/drive/v3/files?q={}&fields={}&pageSize=1000&supportsAllDrives=true&includeItemsFromAllDrives=true", base(), query(&q), query("files(id,name,mimeType)"));
    let reply: Value = serde_json::from_str(&call(token, cli_http::Request::get(&url))?.text()).map_err(|e| format!("the Drive listing is not JSON: {e}"))?;
    Ok(reply["files"].as_array().map(Vec::as_slice).unwrap_or_default().iter().map(|f| (f["id"].as_str().unwrap_or("").to_string(), f["name"].as_str().unwrap_or("").to_string(), f["mimeType"].as_str().unwrap_or("").to_string())).collect())
}

fn create(token: &str, folder: &str, part: &Part) -> Result<String, String> {
    let meta = json!({ "name": part.name, "parents": [folder], "mimeType": DOC_MIME });
    let boundary = "wfgembound7d2f";
    let body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{}\r\n--{boundary}--", part.text);
    let url = format!("{}/upload/drive/v3/files?uploadType=multipart&supportsAllDrives=true&fields=id", base());
    let request = cli_http::Request::post(&url).header("Content-Type", &format!("multipart/related; boundary={boundary}")).body(body);
    let reply: Value = serde_json::from_str(&call(token, request)?.text()).map_err(|e| format!("the Drive create answer is not JSON: {e}"))?;
    reply["id"].as_str().filter(|id| !id.is_empty()).map(String::from).ok_or_else(|| format!("Drive created {} but returned no id", part.name))
}

fn overwrite(token: &str, id: &str, part: &Part) -> Result<(), String> {
    let url = format!("{}/upload/drive/v3/files/{id}?uploadType=media&supportsAllDrives=true", base());
    call(token, cli_http::Request::new("PATCH", &url).header("Content-Type", "text/plain; charset=UTF-8").body(part.text.clone())).map(drop)
}

/// What is read back: the Doc's mime type and modified time, and its text exported as plain text.
fn read_back(token: &str, id: &str) -> Result<(String, String, usize), String> {
    let url = format!("{}/drive/v3/files/{id}?fields={}&supportsAllDrives=true", base(), query("mimeType,modifiedTime"));
    let meta: Value = serde_json::from_str(&call(token, cli_http::Request::get(&url))?.text()).map_err(|e| format!("the Drive metadata is not JSON: {e}"))?;
    let export = format!("{}/drive/v3/files/{id}/export?mimeType={}", base(), query("text/plain"));
    let text = call(token, cli_http::Request::get(&export))?.text();
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    Ok((meta["mimeType"].as_str().unwrap_or("").to_string(), meta["modifiedTime"].as_str().unwrap_or("").to_string(), chars(&text)))
}

/// Puts every part in the folder as a Google Doc (overwriting the Doc of the same name, creating it when there is none), reads each back,
/// and returns one proof line per Doc. A Doc of the family that no part replaced (an older layout, a pack that shrank) is moved to Drive's trash, never deleted outright.
pub fn publish_drive(token: &str, folder: &str, family: &str, parts: &[Part]) -> Result<Vec<String>, String> {
    let existing = files_named(token, folder, family)?;
    let mut lines = Vec::new();
    for part in parts {
        let same: Vec<_> = existing.iter().filter(|f| f.1 == part.name).collect();
        let id = match same.as_slice() {
            [] => create(token, folder, part)?,
            [one] => {
                overwrite(token, &one.0, part)?;
                one.0.clone()
            }
            many => return Err(format!("{} files named {} are in the folder: the Gem's attachment would be ambiguous", many.len(), part.name)),
        };
        let (mime, modified, exported) = read_back(token, &id)?;
        lines.push(judge_doc(&part.name, &id, &mime, chars(&part.text), exported, &modified, now_ms())?);
    }
    for f in existing.iter().filter(|f| f.1.starts_with(family) && !parts.iter().any(|p| p.name == f.1)) {
        let url = format!("{}/drive/v3/files/{}?supportsAllDrives=true", base(), f.0);
        call(token, cli_http::Request::new("PATCH", &url).json(&json!({ "trashed": true })))?;
        lines.push(format!("trashed stale {} id={} (no longer produced; it is in Drive's trash for 30 days)", f.1, f.0));
    }
    Ok(lines)
}

/// `local:<dir>`: the parts as `<name>.md` files, each read back for its size.
pub fn publish_local(dir: &Path, parts: &[Part]) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create the destination {}: {e}", dir.display()))?;
    let mut lines = Vec::new();
    for part in parts {
        let path = dir.join(format!("{}.md", part.name));
        std::fs::write(&path, &part.text).map_err(|e| format!("{}: {e}", path.display()))?;
        let back = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        if back != part.text {
            return Err(format!("{} differs after the write", path.display()));
        }
        lines.push(format!("doc {} id={} chars={}", part.name, path.display(), chars(&back)));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_793_000_000_000;

    fn iso_of(ms: i64) -> String {
        let secs = ms / 1000;
        let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", rem / 3600, rem % 3600 / 60, rem % 60)
    }

    #[test]
    fn given_a_time_when_parsed_then_it_round_trips() {
        assert_eq!(parse_ms(&iso_of(NOW)), Some(NOW));
    }

    #[test]
    fn given_a_fresh_whole_doc_when_judged_then_the_proof_line_names_its_id() {
        let line = judge_doc("gem-01", "abc", DOC_MIME, 1000, 1000, &iso_of(NOW - 60_000), NOW).unwrap();
        assert!(line.contains("id=abc") && line.contains("chars=1000"), "{line}");
    }

    #[test]
    fn given_a_doc_with_its_tail_cut_when_judged_then_it_fails() {
        let err = judge_doc("gem-01", "abc", DOC_MIME, 1_000_000, 900_000, &iso_of(NOW), NOW).unwrap_err();
        assert!(err.contains("cut"), "{err}");
    }

    #[test]
    fn given_a_plain_file_instead_of_a_doc_when_judged_then_it_fails() {
        let err = judge_doc("gem-01", "abc", "text/plain", 10, 10, &iso_of(NOW), NOW).unwrap_err();
        assert!(err.contains("not a Google Doc"), "{err}");
    }

    #[test]
    fn given_a_doc_not_touched_by_the_upload_when_judged_then_it_fails() {
        let err = judge_doc("gem-01", "abc", DOC_MIME, 10, 10, &iso_of(NOW - 3_600_000), NOW).unwrap_err();
        assert!(err.contains("did not replace"), "{err}");
    }
}
