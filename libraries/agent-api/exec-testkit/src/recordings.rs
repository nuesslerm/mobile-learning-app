//! The stream-json each CLI printed, as the interactions of the mimic cassette set `cassettes/exec-recordings` (one stdout step per line of the
//! CLI's output). A user of the kit replays them through the scripted CLI or reads them to check its own translation.
use crate::cassette::{Cassette, Interaction};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub name: String,
    /// The driver id of `rca-cli-driver` whose CLI printed it (`claudeAgent`, `codex`, `opencode`).
    pub driver: String,
    pub scenario: String,
    /// The set's `provenance.kind`: `imported` and `authored` are not from a run of the CLI, only `recorded` is.
    pub provenance: String,
    pub source: String,
    /// The neutral events (`ExecEvent::to_json()["type"]` of `rca-cli-driver`) its lines translate to, in order.
    pub expect: Vec<String>,
    pub lines: Vec<String>,
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The cassette set that holds the recordings.
pub fn set_dir() -> PathBuf {
    root().join("cassettes/exec-recordings")
}

pub fn cassette() -> Cassette {
    Cassette::open(&set_dir())
}

fn recording(set: &Cassette, i: &Interaction) -> Recording {
    let field = |k: &str| i.input[k].as_str().unwrap_or_else(|| panic!("{}: input has no {k}", i.id)).to_string();
    let expect = i.effects.iter().find(|e| e["type"] == "neutral-events").and_then(|e| e["events"].as_array()).unwrap_or_else(|| panic!("{}: no neutral events", i.id));
    let lines = i.lines();
    assert!(!lines.is_empty(), "{} has no line", i.id);
    Recording {
        name: i.id.clone(),
        driver: i.op.clone().unwrap_or_else(|| panic!("{}: no driver", i.id)),
        scenario: field("scenario"),
        provenance: set.provenance.kind.clone(),
        source: field("source"),
        expect: expect.iter().map(|e| e.as_str().unwrap_or_default().to_string()).collect(),
        lines,
    }
}

/// Every recording of the set, with its lines.
pub fn all() -> Vec<Recording> {
    let set = cassette();
    assert!(!set.provenance.note.is_empty(), "{} needs its note", set_dir().display());
    set.interactions.iter().map(|i| recording(&set, i)).collect()
}

/// The recording with this name.
pub fn named(name: &str) -> Recording {
    all().into_iter().find(|r| r.name == name).unwrap_or_else(|| panic!("no recording named {name}"))
}
