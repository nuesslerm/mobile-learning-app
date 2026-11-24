//! What must not reach a bucket anyone can read: a token-shaped value (a Bearer token, a JWT) anywhere in the masked set, and the exact value of a
//! key the set declared (`[reference] env_from_vault`) anywhere in the raw recording or the masked set. A finding names the file, the line and the
//! key (or the shape), never the value.
use std::path::Path;

const MAX_SCANNED_BYTES: u64 = 8 * 1024 * 1024;

/// A token-shaped value in masked output: what the runbook's grep looks for.
pub fn token_like(line: &str) -> Option<&'static str> {
    if let Some(i) = line.find("Bearer ") {
        if line[i + 7..].bytes().take(8).filter(u8::is_ascii_alphanumeric).count() == 8 {
            return Some("a Bearer token");
        }
    }
    let mut rest = line;
    while let Some(i) = rest.find("eyJ") {
        let tail = &rest[i + 3..];
        if tail.bytes().take(10).filter(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-').count() == 10 {
            return Some("a JWT");
        }
        rest = tail;
    }
    None
}

fn files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            files(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn name(path: &Path, root: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).display().to_string()
}

/// Token-shaped values anywhere under `dir` (the checkpoint is unmasked, so it only travels when none are found) and, in every file of any kind,
/// the exact value of a declared key.
pub fn scan_tree(dir: &Path, root: &Path, secrets: &[(String, String)]) -> Vec<String> {
    let (mut all, mut hits) = (Vec::new(), Vec::new());
    files(dir, &mut all);
    all.sort();
    for path in all {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let text = String::from_utf8_lossy(&bytes);
        hits.extend(secrets.iter().filter(|(_, v)| !v.is_empty() && text.contains(v.as_str())).map(|(k, _)| format!("{} holds the value of {k}", name(&path, root))));
        if bytes.len() as u64 <= MAX_SCANNED_BYTES {
            if let Some((i, what)) = text.lines().enumerate().find_map(|(i, l)| token_like(l).map(|w| (i + 1, w))) {
                hits.push(format!("{}:{i} has {what}", name(&path, root)));
            }
        }
    }
    hits
}

/// The masked set a record leaves: its json, jsonl and toml files beside `cassette.json` for token shapes, everything under it for a declared value.
pub fn scan_set(dir: &Path, secrets: &[(String, String)]) -> Vec<String> {
    let mut hits = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return hits };
    let mut top: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.is_file() && matches!(p.extension().and_then(|x| x.to_str()), Some("json" | "jsonl" | "toml"))).collect();
    top.sort();
    for path in top {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if let Some((i, what)) = text.lines().enumerate().find_map(|(i, l)| token_like(l).map(|w| (i + 1, w))) {
            hits.push(format!("{}:{i} has {what}", name(&path, dir)));
        }
    }
    hits.extend(scan_tree(dir, dir, secrets).into_iter().filter(|h| h.contains(" holds the value of ")));
    hits
}
