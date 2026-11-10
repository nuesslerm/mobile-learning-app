//! Small shared helpers: ids and timestamps, JavaScript-flavoured string conversion, path resolution like Node's (against a working
//! directory the caller supplies: the environment port knows it).

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde_json::Value;
use std::path::{Path, PathBuf};
use remote_chat_agent_core::domain::ids::iso_from_ms;
pub use remote_chat_agent_render::js::{js_string, truthy};

/// A random (version 4) UUID, as `crypto.randomUUID`.
pub fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `n` random bytes (at most 16) as lowercase hex.
pub fn random_hex(n: usize) -> String {
    let mut hex = uuid::Uuid::new_v4().simple().to_string();
    hex.truncate(2 * n.min(16));
    hex
}

/// Milliseconds since the Unix epoch, from the system clock.
pub fn now_ms() -> u64 {
    u64::try_from(time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000).unwrap_or(0)
}

/// ISO-8601 with milliseconds and a Z, as `Date#toISOString`.
pub fn iso_now() -> String {
    iso_from_ms(now_ms())
}

/// `encodeURIComponent`: everything except letters, digits and `-_.!~*'()` is percent-encoded as UTF-8.
const URI_COMPONENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'!').remove(b'~').remove(b'*').remove(b'\'').remove(b'(').remove(b')');

/// The `encodeurl` package Express runs a `Location` through: bytes outside the URL character set become `%XX`, an existing
/// escape stays.
const URL: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'!')
    .remove(b'$')
    .remove(b'&')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')')
    .remove(b'*')
    .remove(b'+')
    .remove(b',')
    .remove(b';')
    .remove(b'=')
    .remove(b':')
    .remove(b'@')
    .remove(b'/')
    .remove(b'?')
    .remove(b'#')
    .remove(b'[')
    .remove(b']')
    .remove(b'%');

pub fn encode_uri_component(text: &str) -> String {
    utf8_percent_encode(text, URI_COMPONENT).to_string()
}

pub fn encode_url(url: &str) -> String {
    utf8_percent_encode(url, URL).to_string()
}


/// JSON for embedding in a `<script>`: `<` and the two JavaScript line separators are escaped so a title cannot close the tag.
pub fn script_json(value: &Value) -> String {
    value.to_string().replace('<', "\\u003c").replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")
}

/// HTML-escape `&`, `<` and `>`.
pub fn escape_html(s: &str) -> String {
    html_escape::encode_text(s).into_owned()
}

/// Node's `path.resolve(...)`: absolute (against `cwd`), `.` and `..` folded, no trailing slash.
pub fn resolve_from(cwd: &Path, path: &str) -> PathBuf {
    let absolute = if Path::new(path).is_absolute() { PathBuf::from(path) } else { cwd.join(path) };
    path_clean::clean(absolute)
}

/// `path.resolve(base, rel)`: an absolute `rel` replaces `base`.
pub fn resolve_in_from(cwd: &Path, base: &str, rel: &str) -> PathBuf {
    if Path::new(rel).is_absolute() {
        resolve_from(cwd, rel)
    } else {
        resolve_from(cwd, &format!("{}/{}", resolve_from(cwd, base).to_string_lossy(), rel))
    }
}

/// JavaScript's `parseInt(text, 10)`: leading whitespace, an optional sign, then digits; None is NaN.
pub fn parse_int(text: &str) -> Option<i64> {
    let trimmed = text.trim_start();
    let end = trimmed.char_indices().find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+')))).map_or(trimmed.len(), |(i, _)| i);
    trimmed[..end].parse().ok()
}
