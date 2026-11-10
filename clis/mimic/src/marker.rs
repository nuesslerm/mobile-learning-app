//! The marker an empty commit to `dev` carries to start a reference-capture job on the VM runner (`.github/workflows/record-references.yml`):
//!
//! ```text
//! marker  = "[" action 1*( " " word ) "]"
//! action  = "record" | "replay"
//! word    = set | "chunk=" minutes | "fresh"       (chunk and fresh: record only, each at most once)
//! set     = a cassette set: its directory name, or its path from the repository root; the directory holds a mimic.toml
//! ```
//!
//! The marker is a line of its own that begins with `[` (the workflow's `if:` looks for `[record ` or `[replay ` at the start of the message or of
//! a line, so the two agree): prose that mentions the text mid-line starts nothing. One marker per message; a message without one starts nothing. `chunk=<n>` is the minutes one `wf mimic record --max-minutes` call may take
//! (default 10, 1 to 60); `fresh` drops the checkpoint of an earlier run so the record starts over.
use std::path::{Path, PathBuf};

pub const DEFAULT_CHUNK_MINUTES: u32 = 10;
pub const MAX_CHUNK_MINUTES: u32 = 60;
pub const MAX_SETS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Record,
    Replay,
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
    let (mut sets, mut chunk, mut fresh): (Vec<String>, Option<u32>, bool) = (Vec::new(), None, false);
    for w in words {
        if let Some(n) = w.strip_prefix("chunk=") {
            if action == Action::Replay {
                return Err("[replay ...] takes sets only: chunk= belongs to [record ...]".into());
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
                return Err("[replay ...] takes sets only: fresh belongs to [record ...]".into());
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
    Ok(Some(Marker { action, sets, chunk: chunk.unwrap_or(DEFAULT_CHUNK_MINUTES), fresh }))
}

const SKIP: [&str; 7] = ["node_modules", "target", ".git", "references", "dist", "tmp", ".nx"];

/// Every directory under `root` that holds a mimic.toml, as a path from `root`, sorted.
pub fn all_sets(root: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let Ok(kind) = e.file_type() else { continue };
            let name = e.file_name();
            if kind.is_dir() && !SKIP.iter().any(|s| name == *s) {
                walk(root, &e.path(), out);
            } else if kind.is_file() && name == "mimic.toml" {
                out.push(dir.strip_prefix(root).unwrap_or(dir).to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// The set a marker word names, as a path from `root`: a path names it exactly, a bare name must match one directory.
pub fn resolve(root: &Path, name: &str) -> Result<PathBuf, String> {
    let all = all_sets(root);
    if name.contains('/') {
        let want = Path::new(name.trim_end_matches('/'));
        return all.into_iter().find(|p| p == want).ok_or_else(|| unknown(name, &all_names(root)));
    }
    let hits: Vec<PathBuf> = all.iter().filter(|p| p.file_name().is_some_and(|n| n == name)).cloned().collect();
    match hits.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(unknown(name, &all_names(root))),
        many => Err(format!("{name:?} names {} sets ({}): write the path from the repository root", many.len(), many.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", "))),
    }
}

fn all_names(root: &Path) -> Vec<String> {
    all_sets(root).iter().filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect()
}

fn unknown(name: &str, known: &[String]) -> String {
    let shown: Vec<&str> = known.iter().take(24).map(String::as_str).collect();
    let more = if known.len() > shown.len() { format!(", and {} more", known.len() - shown.len()) } else { String::new() };
    format!("unknown set {name:?}: no directory of that name holds a mimic.toml (sets: {}{more})", shown.join(", "))
}
