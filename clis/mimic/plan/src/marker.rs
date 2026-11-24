//! The marker an empty commit to `dev` carries to start a reference-capture job on a GitHub-hosted runner (`.github/workflows/mimic-record.yml`):
//!
//! ```text
//! marker  = "[" action 1*( " " word ) "]"
//! action  = "record" | "replay"
//! word    = set | "chunk=" minutes | "fresh" | "rows=" prefix *( "," prefix )   (chunk and fresh: record only; rows: replay only; each at most once)
//! set     = a cassette set: its directory name, or its path from the repository root; the directory holds a mimic.toml
//! ```
//!
//! The marker is a line of its own that begins with `[` (the workflow's `if:` looks for `[record ` or `[replay ` at the start of the message or of
//! a line, so the two agree): prose that mentions the text mid-line starts nothing. One marker per message; a message without one starts nothing. `chunk=<n>` is the minutes one `wf mimic record --max-minutes` call may take
//! (default 10, 1 to 60); `rows=` limits a replay to the interactions whose row starts with a prefix and checks the known gaps of that slice; `fresh` drops the checkpoint of an earlier run so the record starts over.
use std::path::{Path, PathBuf};

pub const DEFAULT_CHUNK_MINUTES: u32 = 10;
pub const MAX_CHUNK_MINUTES: u32 = 60;
pub const MAX_SETS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Record,
    Replay,
}

impl Marker {
    /// The marker of a commit message: `Ok(None)` when it has none, an error that names the rule it breaks when it has a malformed one.
    pub fn parse(message: &str) -> Result<Option<Marker>, String> {
        parse(message)
    }
}

impl Action {
    pub fn word(self) -> &'static str {
        match self {
            Action::Record => "record",
            Action::Replay => "replay",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    pub action: Action,
    pub sets: Vec<String>,
    pub chunk: u32,
    pub fresh: bool,
    pub rows: Vec<String>,
}

fn prefix_ok(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'/'))
}

fn set_name_ok(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('/')
        && s.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
}

/// The marker of a commit message: `Ok(None)` when it has none, an error that names what is wrong when it has a malformed one.
pub fn parse(message: &str) -> Result<Option<Marker>, String> {
    let mut found: Vec<&str> = Vec::new();
    for line in message.lines() {
        let Some(after) = line.strip_prefix('[') else { continue };
        let word = after.split(|c: char| c.is_whitespace() || c == ']').next().unwrap_or("");
        if word == "record" || word == "replay" {
            let close = after.find(']').ok_or_else(|| format!("the marker [{word} ...] is not closed with ] on its line"))?;
            found.push(&after[..close]);
        }
    }
    let body = match found.as_slice() {
        [] => return Ok(None),
        [one] => *one,
        _ => return Err("the message carries more than one [record ...] or [replay ...] marker: one per commit".into()),
    };
    let mut words = body.split_whitespace();
    let action = match words.next() {
        Some("record") => Action::Record,
        _ => Action::Replay,
    };
    let (mut sets, mut chunk, mut fresh, mut rows): (Vec<String>, Option<u32>, bool, Vec<String>) = (Vec::new(), None, false, Vec::new());
    for w in words {
        if let Some(list) = w.strip_prefix("rows=") {
            if action == Action::Record {
                return Err("[record ...] records whole sets: rows= belongs to [replay ...]".into());
            }
            if !rows.is_empty() {
                return Err("rows= is given twice".into());
            }
            rows = list.split(',').map(String::from).collect();
            if let Some(bad) = rows.iter().find(|r| !prefix_ok(r)) {
                return Err(format!("rows={list}: {bad:?} is not a row prefix (letters, digits, . _ - : and /; prefixes are separated by a comma)"));
            }
        } else if let Some(n) = w.strip_prefix("chunk=") {
            if action == Action::Replay {
                return Err("[replay ...] takes sets and rows= only: chunk= belongs to [record ...]".into());
            }
            let minutes: u32 = n.parse().map_err(|_| format!("chunk={n} is not a number of minutes"))?;
            if !(1..=MAX_CHUNK_MINUTES).contains(&minutes) {
                return Err(format!("chunk={minutes} is outside 1..={MAX_CHUNK_MINUTES} minutes"));
            }
            if chunk.replace(minutes).is_some() {
                return Err("chunk= is given twice".into());
            }
        } else if w == "fresh" {
            if action == Action::Replay {
                return Err("[replay ...] takes sets and rows= only: fresh belongs to [record ...]".into());
            }
            fresh = true;
        } else if set_name_ok(w) {
            if !sets.iter().any(|s| s == w) {
                sets.push(w.to_string());
            }
        } else {
            return Err(format!("{w:?} is neither a set name (letters, digits, . _ - and /), chunk=<minutes> nor fresh"));
        }
    }
    if sets.is_empty() {
        return Err(format!("[{} ...] names no set: [{} <set> [<set>...]]", action.word(), action.word()));
    }
    if sets.len() > MAX_SETS {
        return Err(format!("{} sets named: at most {MAX_SETS} per commit, so one trigger cannot hold the runner for a day", sets.len()));
    }
    Ok(Some(Marker { action, sets, chunk: chunk.unwrap_or(DEFAULT_CHUNK_MINUTES), fresh, rows }))
}

/// The set a marker word names, as a path from `root`: a path names it exactly, a bare name must match one directory.
pub(crate) fn resolve(all: &[PathBuf], name: &str) -> Result<PathBuf, String> {
    if name.contains('/') {
        let want = Path::new(name.trim_end_matches('/'));
        return all.iter().find(|p| *p == want).cloned().ok_or_else(|| unknown(name, &all_names(all)));
    }
    let hits: Vec<PathBuf> = all.iter().filter(|p| p.file_name().is_some_and(|n| n == name)).cloned().collect();
    match hits.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(unknown(name, &all_names(all))),
        many => Err(format!("{name:?} names {} sets ({}): write the path from the repository root", many.len(), many.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))),
    }
}

fn all_names(all: &[PathBuf]) -> Vec<String> {
    all.iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect()
}

fn unknown(name: &str, known: &[String]) -> String {
    let shown: Vec<&str> = known.iter().take(24).map(String::as_str).collect();
    let more = if known.len() > shown.len() { format!(", and {} more", known.len() - shown.len()) } else { String::new() };
    format!("unknown set {name:?}: no directory of that name holds a mimic.toml (sets: {}{more})", shown.join(", "))
}
