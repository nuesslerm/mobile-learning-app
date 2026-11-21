//! The jobs a marker asks for, one per set, each set checked on the way: it exists, its mimic.toml loads, and it can do what the marker asks.
use crate::marker::{self, Action, Marker};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the plan needs to know of one set's mimic.toml and directory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetFacts {
    pub reference: bool,
    pub corpus: bool,
    pub provision: bool,
    pub drivers: bool,
    pub toolchain: BTreeMap<String, String>,
    pub cassette: bool,
    pub candidates: Vec<String>,
    /// The agent CLIs its provision block runs for real (`real_agents`).
    pub real_agents: Vec<String>,
}

/// The checkout the plan reads: every directory that holds a mimic.toml (from the root, sorted) and one set's facts.
pub trait SetSource {
    fn sets(&self) -> Vec<PathBuf>;
    fn facts(&self, dir: &Path) -> Result<SetFacts, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetPlan {
    pub name: String,
    pub dir: PathBuf,
    pub facts: SetFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkerPlan {
    pub marker: Marker,
    pub sets: Vec<SetPlan>,
}

/// Why a message or a set is refused, and what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub error: String,
    pub next: String,
}

fn refuse(error: impl Into<String>, next: &str) -> Refusal {
    Refusal { error: error.into(), next: next.to_string() }
}

const GRAMMAR: &str = "write the marker as [record <set>... [chunk=<minutes>] [fresh]] or [replay <set>... [rows=<prefix,...>]] (wf ci plan --help)";

/// The plan of a commit message: `Ok(None)` when it carries no marker.
pub fn from_message(message: &str, src: &dyn SetSource) -> Result<Option<MarkerPlan>, Refusal> {
    match marker::parse(message).map_err(|e| refuse(e, GRAMMAR))? {
        Some(m) => plan(m, src).map(Some),
        None => Ok(None),
    }
}

/// One job per set of the marker, in order.
pub fn plan(marker: Marker, src: &dyn SetSource) -> Result<MarkerPlan, Refusal> {
    let all = src.sets();
    let mut sets = Vec::new();
    for word in &marker.sets {
        let dir = marker::resolve(&all, word).map_err(|e| refuse(e, "name a directory that holds a mimic.toml"))?;
        let facts = src.facts(&dir).map_err(|e| refuse(format!("set {word}: {e}"), "fix the set or drop it from the marker"))?;
        check(&facts, marker.action, word).map_err(|e| refuse(e, "fix the set or drop it from the marker"))?;
        let name = dir.file_name().map_or_else(|| word.clone(), |n| n.to_string_lossy().into_owned());
        sets.push(SetPlan { name, dir, facts });
    }
    if marker.action == Action::Record {
        let codex: Vec<&str> = sets.iter().filter(|s| s.facts.real_agents.iter().any(|a| a == "codex")).map(|s| s.name.as_str()).collect();
        if codex.len() > 1 {
            return Err(refuse(format!("sets {} all run Codex for real: its login's refresh token is single-use, so one record holds the codex credential at a time and writes the rotated login back", codex.join(", ")), "name one of them per push, the next after its login was written back"));
        }
    }
    Ok(MarkerPlan { marker, sets })
}

fn check(f: &SetFacts, action: Action, name: &str) -> Result<(), String> {
    match action {
        Action::Record => {
            if !f.reference {
                return Err(format!("set {name} has no [reference] in its mimic.toml: there is no server to record"));
            }
            if !f.corpus {
                return Err(format!("set {name} has no corpus: a record needs corpus.json (schema mimic-corpus/1)"));
            }
            if !f.provision {
                return Err(format!("set {name} has no [provision] block (or provision_file) in its mimic.toml: the runner cannot install its reference; the schema is in docs/runbooks/record-reference-servers-on-the-vm.md"));
            }
        }
        Action::Replay => {
            if !f.cassette {
                return Err(format!("set {name} has no cassette.json: there is nothing recorded to replay (record it first: [record {name}])"));
            }
            if f.candidates.is_empty() {
                return Err(format!("set {name} has no [candidate.<name>] in its mimic.toml: there is nothing to replay against"));
            }
        }
    }
    Ok(())
}
