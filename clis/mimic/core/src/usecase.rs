//! The use cases over the ports: record a reference into a cassette set, replay a set against a candidate, verify a set's freshness and
//! turn a verdict into the evidence rows `wf parity coverage` ingests. No I/O: everything crosses a port.
use crate::allowlist::Allowlist;
use crate::cassette::{Cassette, Check, CheckStatus, Current, Interaction, Kind, Problem, Target, ToolInfo};
use crate::corpus::{Case, CorpusSpec, TAG_PROPERTY, TAG_SHRINK, TAG_SHRINK_OF};
use crate::differ::{diff_interaction, DiffOptions, InteractionResult, Label, Shrunk, Verdict};
use crate::ports::{CassetteSet, Driver, DriverError, Observed, Request};
use crate::rules::{RuleSet, RulesError};
use serde::Serialize;

#[derive(Debug, PartialEq)]
pub enum UseCaseError {
    Driver { id: Option<String>, error: DriverError },
    Rules(RulesError),
    Checkpoint(String),
}

impl std::fmt::Display for UseCaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Driver { id: Some(id), error } => write!(f, "{error} (interaction {id})"),
            Self::Driver { id: None, error } => write!(f, "{error}"),
            Self::Rules(e) => write!(f, "{e}"),
            Self::Checkpoint(e) => write!(f, "checkpoint: {e}"),
        }
    }
}

impl std::error::Error for UseCaseError {}

impl From<RulesError> for UseCaseError {
    fn from(e: RulesError) -> Self {
        Self::Rules(e)
    }
}

/// What a recording is stamped with besides what the driver reports.
#[derive(Clone, Debug)]
pub struct Meta {
    pub kind: Kind,
    pub target: Option<Target>,
    pub tool: ToolInfo,
    pub seed: Option<u64>,
    pub recorded_at: Option<String>,
    pub recorded_on: Option<String>,
    pub source: Option<String>,
    pub rules_path: String,
    pub corpus_path: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct Recording {
    pub set: CassetteSet,
    /// Ids whose two runs differed after masking: written only when the caller decides to, never silently passed.
    pub nondeterministic: Vec<String>,
    /// For each nondeterministic id, the first paths that differed between the two runs.
    pub differences: Vec<(String, Vec<crate::ports::Difference>)>,
    /// The un-normalised capture of the first run, for the raw sidecar.
    pub raw: Vec<(String, Observed)>,
}

fn build(id: &str, op: &Option<String>, row: &Option<String>, input: &serde_json::Value, tags: &[String], o: Observed) -> Interaction {
    Interaction { id: id.into(), row: row.clone(), op: op.clone(), input: input.clone(), steps: o.steps, effects: o.effects, outcome: o.outcome, tags: tags.to_vec() }
}

pub(crate) fn driver_err(id: &str) -> impl FnOnce(DriverError) -> UseCaseError + '_ {
    move |error| UseCaseError::Driver { id: Some(id.to_string()), error }
}

/// Run every case twice against the reference and keep the masked first run; equal masked runs are what makes a cassette stable. At scale
/// use `record::record_planned`, which this is the one-case-at-a-time, nothing-saved form of.
pub fn record(driver: &mut dyn Driver, cases: &[Case], rules: &RuleSet, corpus: Option<&CorpusSpec>, meta: Meta) -> Result<Recording, UseCaseError> {
    match crate::record::record_planned(driver, cases, rules, corpus, meta, &crate::record::Plan::ONE_BY_ONE, &mut crate::record::MemCheckpoint::default())? {
        crate::record::Progress::Done(r) => Ok(*r),
        crate::record::Progress::Partial { recorded, total } => Err(UseCaseError::Checkpoint(format!("{recorded} of {total} cases recorded"))),
    }
}

/// Feed the cassette's inputs to the candidate, mask what it did with the set's rules and diff it with what was recorded. A driver
/// failure on one interaction is that interaction's breaking result with the error as evidence, not an abort.
pub fn replay(set: &CassetteSet, driver: &mut dyn Driver, allow: &Allowlist, opts: &DiffOptions, only: Option<&[String]>) -> Result<Verdict, UseCaseError> {
    let compiled = set.rules.compile()?;
    let mut results = Vec::new();
    for expected in set.cassette.interactions.iter().filter(|i| only.is_none_or(|o| o.contains(&i.id))) {
        let request = Request { id: expected.id.clone(), op: expected.op.clone(), input: expected.input.clone() };
        results.push(match driver.run(&request) {
            Ok(o) => {
                let actual = compiled.interaction(&build(&expected.id, &expected.op, &expected.row, &expected.input, &expected.tags, o));
                InteractionResult::of(&expected.id, expected.row.clone(), diff_interaction(expected, &actual, allow, opts))
            }
            Err(e) => InteractionResult::failed(&expected.id, expected.row.clone(), e.to_string()),
        });
    }
    let mut verdict = Verdict::of(results, allow);
    verdict.shrunk = shrunk(set, &verdict.results);
    Ok(verdict)
}

/// The smallest failing input (by its serialised size, then id) of each property case and its ladder, when any of them fails.
fn shrunk(set: &CassetteSet, results: &[InteractionResult]) -> Vec<Shrunk> {
    let failing = |id: &str| results.iter().any(|r| r.id == id && !matches!(r.label, Label::Clean | Label::Allowlisted));
    let mut out = Vec::new();
    for base in set.cassette.interactions.iter().filter(|i| i.tags.iter().any(|t| t == TAG_PROPERTY) && !i.tags.iter().any(|t| t == TAG_SHRINK)) {
        let marker = format!("{TAG_SHRINK_OF}{}", base.id);
        let family = std::iter::once(base).chain(set.cassette.interactions.iter().filter(|i| i.tags.contains(&marker)));
        let smallest = family.filter(|i| failing(&i.id)).min_by_key(|i| (i.input.to_string().len(), i.id.clone()));
        if let Some(m) = smallest {
            out.push(Shrunk { case: base.id.clone(), minimal: m.id.clone(), input: m.input.clone() });
        }
    }
    out
}

#[derive(Serialize, Debug, PartialEq)]
pub struct VerifyReport {
    pub ok: bool,
    pub checks: Vec<Check>,
    pub problems: Vec<Problem>,
}

/// Freshness and integrity, offline: the pin, rules and corpus hashes against `now`, and the provenance each kind must carry.
pub fn verify(set: &CassetteSet, now: &Current) -> VerifyReport {
    let checks = set.cassette.freshness(now);
    let mut problems = set.cassette.problems();
    if set.rules.hash() != set.cassette.header.rules.hash {
        problems.push(Problem { field: "rules".into(), message: "rules.json is not the rule set the cassette was recorded with".into() });
    }
    let ok = problems.is_empty() && checks.iter().all(|c| c.status == CheckStatus::Ok);
    VerifyReport { ok, checks, problems }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct EvidenceRow {
    pub row: String,
    pub id: String,
    pub diff: Label,
    pub provenance: Kind,
}

/// One row per interaction that names a parity row: `wf parity coverage` counts it as served only when `diff` is clean or allowlisted
/// and `provenance` is recorded.
pub fn evidence(verdict: &Verdict, cassette: &Cassette) -> Vec<EvidenceRow> {
    verdict
        .results
        .iter()
        .filter_map(|r| r.row.clone().map(|row| EvidenceRow { row, id: r.id.clone(), diff: r.label, provenance: cassette.header.provenance.kind }))
        .collect()
}
