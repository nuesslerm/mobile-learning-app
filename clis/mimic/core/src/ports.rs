//! The ports mimic-core owns. Adapters (the file store, the stdio and in-process drivers, the system clock) live in the thin crate;
//! the doubles live beside these behind the `testing` feature.
use crate::cassette::{Cassette, DriverInfo, Effect, Outcome, Step};
use crate::corpus::CorpusSpec;
use crate::rules::RuleSet;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A cassette set as one unit: the cassette, the rules it was normalised with and the corpus spec that generated its inputs.
#[derive(Clone, Debug, PartialEq)]
pub struct CassetteSet {
    pub cassette: Cassette,
    pub rules: RuleSet,
    pub corpus: Option<CorpusSpec>,
}

#[derive(Debug, PartialEq)]
pub enum StoreError {
    NotFound(String),
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(s) => write!(f, "no cassette set {s}"),
            Self::Invalid(s) => write!(f, "invalid cassette set: {s}"),
            Self::Io(s) => write!(f, "store i/o failed: {s}"),
        }
    }
}

impl std::error::Error for StoreError {}

pub trait Store {
    fn list(&self) -> Result<Vec<String>, StoreError>;
    fn read(&self, set: &str) -> Result<CassetteSet, StoreError>;
    fn write(&mut self, set: &str, content: &CassetteSet) -> Result<(), StoreError>;
    /// A file the corpus spec names (a JSON array of cases), relative to the set.
    fn read_corpus_file(&self, set: &str, rel: &str) -> Result<Value, StoreError>;
    /// The un-normalised capture, kept out of git so rules can be revised and re-applied without re-recording.
    fn write_raw(&mut self, set: &str, name: &str, bytes: &[u8]) -> Result<(), StoreError>;
}

/// `mimic-driver/1` request: one interaction's input for the target.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Request {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    pub input: Value,
}

/// `mimic-driver/1` reply: what the target did.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Observed {
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub outcome: Outcome,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Hello {
    pub driver: DriverInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_version: Option<String>,
    #[serde(default)]
    pub coverage: bool,
}

#[derive(Debug, PartialEq)]
pub struct DriverError(pub String);

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "driver failed: {}", self.0)
    }
}

impl std::error::Error for DriverError {}

/// Runs one interaction against a target (the reference when recording, the candidate when replaying).
pub trait Driver {
    fn hello(&mut self) -> Result<Hello, DriverError>;
    fn run(&mut self, request: &Request) -> Result<Observed, DriverError>;
    /// Back to the state the target starts in (a new server, an emptied home); a target that keeps no state has nothing to do.
    fn reset(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
    /// Requests that may be answered at the same time; the results are in the order of `requests`. A driver that cannot overlap them
    /// answers one by one.
    fn run_many(&mut self, requests: &[Request]) -> Vec<Result<Observed, DriverError>> {
        requests.iter().map(|r| self.run(r)).collect()
    }
}

/// Where two stability passes of one interaction differ, after masking: the JSON pointer in the interaction and the kind and length of each
/// side, never the value (a capture may hold a secret).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Difference {
    pub path: String,
    pub first: String,
    pub second: String,
}

/// One line of a recording's checkpoint file: the plan it belongs to, then every case finished, in the order they finished.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "line", rename_all = "lowercase")]
pub enum CheckpointEntry {
    Plan { hash: String },
    Done {
        id: String,
        interaction: Box<crate::cassette::Interaction>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        nondeterministic: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        differences: Vec<Difference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        raw: Option<Box<Observed>>,
    },
}

/// The progress of a recording on disk (append only), so a killed run resumes where it stopped.
pub trait Checkpoint {
    fn load(&self) -> Result<Vec<CheckpointEntry>, StoreError>;
    fn append(&mut self, entries: &[CheckpointEntry]) -> Result<(), StoreError>;
    fn clear(&mut self) -> Result<(), StoreError>;
}

/// Time reaches a target only through the driver, from here, so a run is reproducible.
pub trait Clock {
    fn now_ms(&self) -> u64;
}

/// Randomness likewise.
pub trait Rng {
    fn next_u64(&mut self) -> u64;
}
