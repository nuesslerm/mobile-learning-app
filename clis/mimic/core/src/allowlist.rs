//! The allowlist of known divergences, byte-compatible with the T3 `allowlist.json`: `ignored` (a differing value is fine), `omitted`
//! (the candidate may drop the key) and `shapeOnly` (only the JSON type is compared). Every entry says why; an entry may carry the pin
//! hash it was written against and then expires when the pin moves.
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub path: String,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_hash: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Allowlist {
    #[serde(default)]
    pub ignored: Vec<Entry>,
    #[serde(default)]
    pub omitted: Vec<Entry>,
    #[serde(default, rename = "shapeOnly")]
    pub shape_only: Vec<Entry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Group {
    Ignored,
    Omitted,
    ShapeOnly,
    // a recorded feature advertisement: reported, never failing (see DiffOptions)
    Advertised,
}

#[derive(Debug, PartialEq)]
pub struct AllowlistError(pub String);

impl std::fmt::Display for AllowlistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AllowlistError {}

/// Array indices are not part of a rule's address.
pub fn plain(path: &str) -> String {
    path.split('.').filter(|s| !s.starts_with('[')).collect::<Vec<_>>().join(".")
}

impl Allowlist {
    pub fn parse(text: &str) -> Result<Self, AllowlistError> {
        let list: Allowlist = serde_json::from_str(text).map_err(|e| AllowlistError(format!("allowlist is not valid: {e}")))?;
        for (group, entries) in [("ignored", &list.ignored), ("omitted", &list.omitted), ("shapeOnly", &list.shape_only)] {
            for e in entries {
                for (field, v) in [("path", &e.path), ("reason", &e.reason), ("phase", &e.phase)] {
                    if v.is_empty() {
                        return Err(AllowlistError(format!("allowlist.{group} entry {}: {field} is required", e.path)));
                    }
                }
            }
        }
        Ok(list)
    }

    pub fn find<'a>(entries: &'a [Entry], path: &str) -> Option<&'a Entry> {
        let p = plain(path);
        entries.iter().find(|e| p == e.path || p.ends_with(&format!(".{}", e.path)))
    }

    /// The entries still valid against this pin, and the ones the pin outlived (a pin-bound entry whose hash differs).
    pub fn active(&self, pin_hash: Option<&str>) -> (Allowlist, Vec<Entry>) {
        let mut expired = Vec::new();
        let mut keep = |entries: &[Entry]| -> Vec<Entry> {
            entries
                .iter()
                .filter(|e| match (&e.pin_hash, pin_hash) {
                    (Some(bound), Some(now)) if bound != now => {
                        expired.push((*e).clone());
                        false
                    }
                    _ => true,
                })
                .cloned()
                .collect()
        };
        let live = Allowlist { ignored: keep(&self.ignored), omitted: keep(&self.omitted), shape_only: keep(&self.shape_only) };
        (live, expired)
    }

    pub fn len(&self) -> usize {
        self.ignored.len() + self.omitted.len() + self.shape_only.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
