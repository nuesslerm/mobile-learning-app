//! The cassette sets of a directory: `<root>/<set>/{cassette.json, interactions.jsonl}` plus the rules and corpus files the header names
//! (relative to the set), and `raw/` for the un-normalised capture, which carries its own `.gitignore`.
use mimic_core::cassette::Cassette;
use mimic_core::ports::{CassetteSet, Store, StoreError};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub struct FsStore {
    root: PathBuf,
}

fn io(path: &Path, e: std::io::Error) -> StoreError {
    StoreError::Io(format!("{}: {e}", path.display()))
}

fn read_text(path: &Path, set: &str) -> Result<String, StoreError> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(StoreError::NotFound(format!("{set} ({} is missing)", path.display()))),
        Err(e) => Err(io(path, e)),
    }
}

fn write_atomically(path: &Path, text: &str) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, path).map_err(|e| io(path, e))
}

/// A path the set names for one of its own files: relative, and never out of the set by an absolute root.
fn inside(dir: &Path, rel: &str) -> Result<PathBuf, StoreError> {
    let path = Path::new(rel);
    if path.is_absolute() || path.components().any(|c| matches!(c, Component::Prefix(_))) {
        return Err(StoreError::Invalid(format!("{rel} is not relative to the set")));
    }
    Ok(dir.join(path))
}

impl FsStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn dir(&self, set: &str) -> PathBuf {
        self.root.join(set)
    }

    /// The files `write` leaves for a set, for the report.
    pub fn files_of(&self, set: &str) -> Vec<PathBuf> {
        ["cassette.json", "interactions.jsonl"].iter().map(|f| self.dir(set).join(f)).collect()
    }
}

impl Store for FsStore {
    fn list(&self) -> Result<Vec<String>, StoreError> {
        let entries = match std::fs::read_dir(&self.root) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io(&self.root, e)),
        };
        let mut sets: Vec<String> = entries.flatten().filter(|e| e.path().join("cassette.json").is_file()).filter_map(|e| e.file_name().into_string().ok()).collect();
        sets.sort();
        Ok(sets)
    }

    fn read(&self, set: &str) -> Result<CassetteSet, StoreError> {
        let dir = self.dir(set);
        let header = read_text(&dir.join("cassette.json"), set)?;
        let lines = read_text(&dir.join("interactions.jsonl"), set)?;
        let cassette = Cassette::parse(&header, &lines).map_err(|e| StoreError::Invalid(format!("{set}: {e}")))?;
        let rules_path = inside(&dir, &cassette.header.rules.path)?;
        let rules = serde_json::from_str(&read_text(&rules_path, set)?).map_err(|e| StoreError::Invalid(format!("{}: {e}", rules_path.display())))?;
        let corpus = match &cassette.header.corpus {
            None => None,
            Some(c) => {
                let path = inside(&dir, &c.path)?;
                Some(serde_json::from_str(&read_text(&path, set)?).map_err(|e| StoreError::Invalid(format!("{}: {e}", path.display())))?)
            }
        };
        Ok(CassetteSet { cassette, rules, corpus })
    }

    fn write(&mut self, set: &str, content: &CassetteSet) -> Result<(), StoreError> {
        let dir = self.dir(set);
        write_atomically(&dir.join("interactions.jsonl"), &content.cassette.jsonl())?;
        write_atomically(&dir.join("cassette.json"), &content.cassette.header_json())?;
        // the rules and corpus files are inputs a person edits: written only when the header names one that is not there yet
        let pretty = |v: &Value| serde_json::to_string_pretty(v).map(|s| s + "\n").expect("prints");
        let rules_path = inside(&dir, &content.cassette.header.rules.path)?;
        if !rules_path.exists() {
            write_atomically(&rules_path, &pretty(&serde_json::to_value(&content.rules).expect("rules serialise")))?;
        }
        if let (Some(c), Some(spec)) = (&content.cassette.header.corpus, &content.corpus) {
            let path = inside(&dir, &c.path)?;
            if !path.exists() {
                write_atomically(&path, &pretty(&serde_json::to_value(spec).expect("corpus serialises")))?;
            }
        }
        Ok(())
    }

    fn read_corpus_file(&self, set: &str, rel: &str) -> Result<Value, StoreError> {
        let path = inside(&self.dir(set), rel)?;
        serde_json::from_str(&read_text(&path, set)?).map_err(|e| StoreError::Invalid(format!("{}: {e}", path.display())))
    }

    fn write_raw(&mut self, set: &str, name: &str, bytes: &[u8]) -> Result<(), StoreError> {
        if name.contains('/') || name.contains("..") {
            return Err(StoreError::Invalid(format!("raw capture name {name}")));
        }
        let raw = self.dir(set).join("raw");
        write_atomically(&raw.join(".gitignore"), "*\n")?;
        write_atomically(&raw.join(name), &String::from_utf8_lossy(bytes))
    }
}
