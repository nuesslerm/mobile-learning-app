//! `mimic-cassette/1`: a header with machine-checked provenance plus one interaction per line. Pure: the files are read and written by a
//! `Store` adapter; this module only turns them to and from text.
use crate::json::{canonical, hash_value};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "mimic-cassette/1";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Recorded,
    Authored,
    Imported,
    Derived,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Target {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_hash: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct DriverInfo {
    pub name: String,
    pub version: String,
    pub protocol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain_lock_hash: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ToolInfo {
    pub mimic: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Provenance {
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driver: Option<DriverInfo>,
    pub tool: ToolInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    pub rules_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus_hash: Option<String>,
    // informational: left out of the content hash so a re-record of equal behaviour is a no-op diff
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    // why the kind is what it is, in words (an imported set says nothing was run)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    // the legacy files an imported set was converted from, each with the canonical sha256 its rebuild must equal
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imported_from: Vec<RefHash>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RefHash {
    pub path: String,
    pub hash: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Header {
    pub schema: String,
    pub provenance: Provenance,
    pub rules: RefHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus: Option<RefHash>,
    pub interactions: usize,
    pub content_hash: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    C2s,
    S2c,
    Call,
    Ret,
    Stdin,
    Stdout,
    Stderr,
    // a child process's exit code, the last step of a call (kind `exit`)
    Exit,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    Text,
    Binary,
    Json,
    Exit,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Step {
    pub dir: Dir,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    pub kind: StepKind,
    // request id: frames are compared by direction plus id so a racy interleaving cannot flake a diff
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rid: Option<String>,
    // byte length of a binary payload, which is stored as base64
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub len: Option<u64>,
    pub payload: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Effect {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub data: serde_json::Map<String, Value>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    #[default]
    Ok,
    Error,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Outcome {
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    // whether the reference itself accepted the input (schema probes record the rejection as data)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_accepts: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Interaction {
    pub id: String,
    // the parity row this interaction evidences, in the vocabulary of the decisions files
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    pub input: Value,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    #[serde(default)]
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Interaction {
    pub fn to_value(&self) -> Value {
        canonical(&serde_json::to_value(self).expect("an interaction serialises"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cassette {
    pub header: Header,
    pub interactions: Vec<Interaction>,
}

#[derive(Debug, PartialEq)]
pub enum CassetteError {
    Json(String),
    Schema(String),
    Count { header: usize, found: usize },
    ContentHash { header: String, computed: String },
}

impl std::fmt::Display for CassetteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "cassette is not valid JSON: {e}"),
            Self::Schema(s) => write!(f, "unsupported cassette schema {s:?}, this tool reads {SCHEMA}"),
            Self::Count { header, found } => write!(f, "header counts {header} interactions, the file has {found}"),
            Self::ContentHash { header, computed } => {
                write!(f, "content hash {header} does not match the content ({computed}): edited by hand, re-record")
            }
        }
    }
}

impl std::error::Error for CassetteError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Stale,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Check {
    pub name: String,
    pub status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recorded: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
}

/// What the world says now, to compare with what the cassette recorded.
#[derive(Clone, Debug, Default)]
pub struct Current {
    pub pin_hash: Option<String>,
    pub rules_hash: String,
    pub corpus_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Problem {
    pub field: String,
    pub message: String,
}

impl Cassette {
    pub fn lines(&self) -> Vec<String> {
        self.interactions.iter().map(|i| i.to_value().to_string()).collect()
    }

    pub fn content_hash(&self) -> String {
        let mut provenance = serde_json::to_value(&self.header.provenance).expect("provenance serialises");
        if let Value::Object(m) = &mut provenance {
            m.remove("recorded_at");
        }
        hash_value(&json!({
            "provenance": provenance,
            "rules": self.header.rules.hash,
            "corpus": self.header.corpus.as_ref().map(|c| &c.hash),
            "interactions": self.interactions.iter().map(Interaction::to_value).collect::<Vec<_>>(),
        }))
    }

    /// Fix the count and content hash after the interactions or provenance changed.
    pub fn seal(&mut self) {
        self.header.interactions = self.interactions.len();
        self.header.content_hash = self.content_hash();
    }

    pub fn header_json(&self) -> String {
        let v = canonical(&serde_json::to_value(&self.header).expect("header serialises"));
        let mut s = serde_json::to_string_pretty(&v).expect("header prints");
        s.push('\n');
        s
    }

    pub fn jsonl(&self) -> String {
        self.lines().iter().map(|l| format!("{l}\n")).collect()
    }

    pub fn parse(header: &str, jsonl: &str) -> Result<Self, CassetteError> {
        let header: Header = serde_json::from_str(header).map_err(|e| CassetteError::Json(e.to_string()))?;
        if header.schema != SCHEMA {
            return Err(CassetteError::Schema(header.schema));
        }
        let mut interactions = Vec::new();
        for line in jsonl.lines().filter(|l| !l.trim().is_empty()) {
            interactions.push(serde_json::from_str(line).map_err(|e| CassetteError::Json(e.to_string()))?);
        }
        let cassette = Cassette { header, interactions };
        if cassette.header.interactions != cassette.interactions.len() {
            return Err(CassetteError::Count { header: cassette.header.interactions, found: cassette.interactions.len() });
        }
        let computed = cassette.content_hash();
        if computed != cassette.header.content_hash {
            return Err(CassetteError::ContentHash { header: cassette.header.content_hash.clone(), computed });
        }
        Ok(cassette)
    }

    /// Provenance rules that need no reference: what each kind must and must not claim.
    pub fn problems(&self) -> Vec<Problem> {
        let p = &self.header.provenance;
        let mut out = Vec::new();
        let mut problem = |field: &str, message: String| out.push(Problem { field: field.to_string(), message });
        match p.kind {
            Kind::Recorded => {
                let t = p.target.as_ref();
                if t.and_then(|t| t.pin_hash.as_ref()).is_none() {
                    problem("provenance.target.pin_hash", "a recorded cassette names the hash of the pin file it was recorded against".into());
                }
                if t.is_none_or(|t| t.version.is_none() && t.commit.is_none()) {
                    problem("provenance.target", "a recorded cassette names the reference version or commit".into());
                }
                if p.driver.is_none() {
                    problem("provenance.driver", "a recorded cassette names the driver that recorded it".into());
                }
            }
            Kind::Authored => {
                if p.target.as_ref().and_then(|t| t.pin_hash.as_ref()).is_some() {
                    problem("provenance.target.pin_hash", "an authored cassette claims no pin: it was not recorded from a reference".into());
                }
            }
            Kind::Imported => {
                if p.source.is_none() {
                    problem("provenance.source", "an imported cassette names the legacy file it came from".into());
                }
            }
            Kind::Derived => {}
        }
        if p.rules_hash != self.header.rules.hash {
            problem("provenance.rules_hash", format!("provenance says {}, the header says {}", p.rules_hash, self.header.rules.hash));
        }
        let mut seen = BTreeSet::new();
        for i in &self.interactions {
            if i.id.is_empty() {
                problem("interactions.id", "an interaction has an empty id".into());
            } else if !seen.insert(i.id.as_str()) {
                problem("interactions.id", format!("id {} appears twice", i.id));
            }
        }
        out
    }

    pub fn freshness(&self, now: &Current) -> Vec<Check> {
        let p = &self.header.provenance;
        let mut checks = Vec::new();
        let recorded_pin = p.target.as_ref().and_then(|t| t.pin_hash.clone());
        checks.push(compare("pin_hash", recorded_pin, now.pin_hash.clone()));
        checks.push(compare("rules_hash", Some(self.header.rules.hash.clone()), Some(now.rules_hash.clone())));
        checks.push(compare("corpus_hash", self.header.corpus.as_ref().map(|c| c.hash.clone()), now.corpus_hash.clone()));
        checks
    }
}

fn compare(name: &str, recorded: Option<String>, current: Option<String>) -> Check {
    // nothing recorded and nothing current (an authored cassette has no pin) is fine; one side alone is a mismatch
    let status = match (&recorded, &current) {
        (None, None) => CheckStatus::Ok,
        (Some(a), Some(b)) if a == b => CheckStatus::Ok,
        (None, Some(_)) => CheckStatus::Missing,
        _ => CheckStatus::Stale,
    };
    Check { name: name.to_string(), status, recorded, current }
}
