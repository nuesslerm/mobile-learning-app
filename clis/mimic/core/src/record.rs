//! Recording at scale: one real server records many cases. The plan orders the corpus so the cases that cannot dirty the target run first,
//! many at once, and the ones that can (tag `state:write`) run in chunks with a `reset` between, so the cost of a server start is paid per
//! chunk and not per case. Each batch is appended to a checkpoint, so a killed run resumes where it stopped and a long recording is
//! driven in short calls (`chunk`). Only a sample of the state-free cases is run twice, as a stability study; state-changing cases are recorded once, in their chunk.
use crate::cassette::{Cassette, Interaction, Provenance, RefHash, SCHEMA};
use crate::corpus::{corpus_hash, Case, CorpusSpec};
use crate::json::sha256_label;
use crate::ports::{Checkpoint, CheckpointEntry, Difference, Driver, Observed, Request, StoreError};
use crate::rng::fnv1a;
use crate::rules::{Compiled as CompiledRules, RuleSet};
use crate::usecase::{driver_err, Meta, Recording, UseCaseError};
use std::collections::BTreeMap;

/// A case that can change what the target answers next: it runs after the read-only ones, in a chunk.
pub const TAG_WRITE: &str = "state:write";
/// A case that needs a pristine target of its own.
pub const TAG_FRESH: &str = "state:fresh";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DoubleRun {
    All,
    /// This percent of the cases (chosen by a hash of seed and id, so the choice does not depend on the order or on a resume).
    Sample { pct: u8, seed: u64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub double_run: DoubleRun,
    /// Cases of the target's state-free group sent at once (1: one at a time).
    pub batch: usize,
    /// State-changing cases per target state; 0 runs them all on one.
    pub write_chunk: usize,
    /// Stop at the first group boundary after this many newly recorded cases (a call of bounded length, never in the middle of a chunk
    /// whose state the next call could not reproduce); `None` runs to the end.
    pub chunk: Option<usize>,
    /// Stop at the first group boundary after this instant, once at least one group ran in this call (so every call makes progress).
    pub until: Option<std::time::Instant>,
    /// Keep the un-normalised capture of every case, not only of the ones that differed between two runs.
    pub keep_raw: bool,
}

impl Plan {
    /// One case after the other, every one twice, nothing kept in between: what `record` always did.
    pub const ONE_BY_ONE: Self = Self { double_run: DoubleRun::All, batch: 1, write_chunk: 0, chunk: None, until: None, keep_raw: true };
}

/// A checkpoint that lives and dies with the call: nothing to resume.
#[derive(Default)]
pub struct MemCheckpoint(pub Vec<CheckpointEntry>);

impl Checkpoint for MemCheckpoint {
    fn load(&self) -> Result<Vec<CheckpointEntry>, StoreError> {
        Ok(self.0.clone())
    }
    fn append(&mut self, entries: &[CheckpointEntry]) -> Result<(), StoreError> {
        self.0.extend_from_slice(entries);
        Ok(())
    }
    fn clear(&mut self) -> Result<(), StoreError> {
        self.0.clear();
        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub enum Progress {
    Done(Box<Recording>),
    /// `recorded` of `total` cases are in the checkpoint; the same call again carries on.
    Partial { recorded: usize, total: usize },
}

#[derive(Debug, PartialEq)]
enum Group {
    Read(Vec<usize>),
    Write(Vec<usize>),
    /// A case on a pristine target of its own: tagged `state:fresh`. Whether a case is in the stability sample never moves it, so a
    /// recorded answer does not depend on the sample.
    Fresh(usize),
}

impl Group {
    fn cases(&self) -> Vec<usize> {
        match self {
            Self::Read(v) | Self::Write(v) => v.clone(),
            Self::Fresh(i) => vec![*i],
        }
    }
}

fn sampled(plan: &Plan, id: &str) -> bool {
    match plan.double_run {
        DoubleRun::All => true,
        DoubleRun::Sample { pct, seed } => fnv1a(&format!("{seed}:{id}")) % 100 < u64::from(pct),
    }
}

fn has(c: &Case, tag: &str) -> bool {
    c.tags.iter().any(|t| t == tag)
}

/// The order the cases run in: the state-free ones first, in corpus order, then the state-changing ones in corpus order.
fn groups(cases: &[Case], plan: &Plan) -> Vec<Group> {
    let batch = plan.batch.max(1);
    let mut out: Vec<Group> = Vec::new();
    let free: Vec<usize> = (0..cases.len()).filter(|&i| !has(&cases[i], TAG_WRITE) && !has(&cases[i], TAG_FRESH)).collect();
    out.extend(free.chunks(batch).map(|c| Group::Read(c.to_vec())));
    let mut pending: Vec<usize> = Vec::new();
    let flush = |pending: &mut Vec<usize>, out: &mut Vec<Group>| {
        if !pending.is_empty() {
            out.push(Group::Write(std::mem::take(pending)));
        }
    };
    for i in (0..cases.len()).filter(|&i| has(&cases[i], TAG_WRITE) || has(&cases[i], TAG_FRESH)) {
        if has(&cases[i], TAG_FRESH) {
            flush(&mut pending, &mut out);
            out.push(Group::Fresh(i));
            continue;
        }
        pending.push(i);
        if plan.write_chunk > 0 && pending.len() == plan.write_chunk {
            flush(&mut pending, &mut out);
        }
    }
    flush(&mut pending, &mut out);
    out
}

/// What a checkpoint is only good for: the same cases in the same order, the same rules and seed and the same way of running them.
fn plan_hash(cases: &[Case], rules: &RuleSet, plan: &Plan, seed: Option<u64>) -> String {
    let ids: Vec<&str> = cases.iter().map(|c| c.id.as_str()).collect();
    let how = format!("{:?}|{}|{}", plan.double_run, plan.batch, plan.write_chunk);
    sha256_label(format!("{}|{}|{how}|{seed:?}|{}", corpus_hash(cases), rules.hash(), ids.join(",")).as_bytes())
}

struct Ran {
    first: Observed,
    second: Option<Observed>,
}

fn request(c: &Case) -> Request {
    Request { id: c.id.clone(), op: c.op.clone(), input: c.input.clone() }
}

fn interaction(c: &Case, o: Observed) -> Interaction {
    Interaction { id: c.id.clone(), row: c.row.clone(), op: c.op.clone(), input: c.input.clone(), steps: o.steps, effects: o.effects, outcome: o.outcome, tags: c.tags.clone() }
}

fn entry(compiled: &CompiledRules, c: &Case, ran: Ran, keep_raw: bool) -> CheckpointEntry {
    let a = compiled.interaction(&interaction(c, ran.first.clone()));
    let b = ran.second.map(|s| compiled.interaction(&interaction(c, s)));
    let different = b.as_ref().is_some_and(|b| *b != a);
    let differences = b.filter(|_| different).map(|b| differences(&a, &b)).unwrap_or_default();
    CheckpointEntry::Done { id: c.id.clone(), interaction: Box::new(a), nondeterministic: different, differences, raw: (keep_raw || different).then(|| Box::new(ran.first)) }
}

const MAX_PATHS: usize = 8;

fn kind(v: Option<&serde_json::Value>) -> String {
    use serde_json::Value::*;
    match v {
        None => "missing".into(),
        Some(Null) => "null".into(),
        Some(Bool(_)) => "bool".into(),
        Some(Number(_)) => "number".into(),
        Some(String(s)) => format!("string(len {})", s.chars().count()),
        Some(Array(a)) => format!("array(len {})", a.len()),
        Some(Object(o)) => format!("object(len {})", o.len()),
    }
}

fn walk(path: &str, a: Option<&serde_json::Value>, b: Option<&serde_json::Value>, out: &mut Vec<Difference>) {
    use serde_json::Value::{Array, Object};
    if out.len() >= MAX_PATHS || a == b {
        return;
    }
    match (a, b) {
        (Some(Object(x)), Some(Object(y))) => {
            let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
            for k in keys {
                walk(&format!("{path}/{}", k.replace('~', "~0").replace('/', "~1")), x.get(k), y.get(k), out);
            }
        }
        (Some(Array(x)), Some(Array(y))) if x.len() == y.len() => {
            for i in 0..x.len() {
                walk(&format!("{path}/{i}"), x.get(i), y.get(i), out);
            }
        }
        _ => out.push(Difference { path: path.into(), first: kind(a), second: kind(b) }),
    }
}

/// The first paths where two masked interactions differ.
fn differences(a: &Interaction, b: &Interaction) -> Vec<Difference> {
    let (a, b) = (serde_json::to_value(a).unwrap_or_default(), serde_json::to_value(b).unwrap_or_default());
    let mut out = Vec::new();
    walk("", Some(&a), Some(&b), &mut out);
    out
}

/// Record `cases` under `plan`, appending to `checkpoint` after every batch. With the checkpoint of an earlier call of the same plan the
/// cases it holds are not run again; with another plan's checkpoint the call is refused.
pub fn record_planned(
    driver: &mut dyn Driver,
    cases: &[Case],
    rules: &RuleSet,
    corpus: Option<&CorpusSpec>,
    meta: Meta,
    plan: &Plan,
    checkpoint: &mut dyn Checkpoint,
) -> Result<Progress, UseCaseError> {
    let compiled = rules.compile()?;
    let hello = driver.hello().map_err(|error| UseCaseError::Driver { id: None, error })?;
    let seed = meta.seed;
    let wanted = plan_hash(cases, rules, plan, seed);
    let mut done: BTreeMap<String, CheckpointEntry> = BTreeMap::new();
    let loaded = checkpoint.load().map_err(|e| UseCaseError::Checkpoint(e.to_string()))?;
    match loaded.first() {
        None => checkpoint.append(&[CheckpointEntry::Plan { hash: wanted.clone() }]).map_err(|e| UseCaseError::Checkpoint(e.to_string()))?,
        Some(CheckpointEntry::Plan { hash }) if *hash == wanted => {}
        Some(_) => return Err(UseCaseError::Checkpoint("the checkpoint was written for other cases, rules, seed or batching: record with --fresh, or restore what changed".into())),
    }
    for e in loaded {
        if let CheckpointEntry::Done { id, .. } = &e {
            done.insert(id.clone(), e);
        }
    }
    let mut ran_now = 0usize;
    let mut dirty = false;
    let room = |ran: usize| plan.chunk.map_or(usize::MAX, |c| c.saturating_sub(ran));
    'groups: for group in groups(cases, plan) {
        let todo: Vec<usize> = group.cases().into_iter().filter(|&i| !done.contains_key(&cases[i].id)).collect();
        if todo.is_empty() {
            continue;
        }
        if room(ran_now) == 0 || (ran_now > 0 && plan.until.is_some_and(|t| std::time::Instant::now() >= t)) {
            break 'groups;
        }
        if dirty {
            driver.reset().map_err(|error| UseCaseError::Driver { id: None, error })?;
            dirty = false;
        }
        let mut written: Vec<CheckpointEntry> = Vec::new();
        match &group {
            Group::Read(_) => {
                let requests: Vec<Request> = todo.iter().map(|&i| request(&cases[i])).collect();
                let firsts = driver.run_many(&requests);
                let again: Vec<usize> = (0..todo.len()).filter(|&k| sampled(plan, &cases[todo[k]].id)).collect();
                let seconds = if again.is_empty() { Vec::new() } else { driver.run_many(&again.iter().map(|&k| requests[k].clone()).collect::<Vec<_>>()) };
                let mut seconds = again.into_iter().zip(seconds).collect::<BTreeMap<_, _>>();
                for (k, first) in firsts.into_iter().enumerate() {
                    let c = &cases[todo[k]];
                    let first = first.map_err(driver_err(&c.id))?;
                    let second = seconds.remove(&k).map(|s| s.map_err(driver_err(&c.id))).transpose()?;
                    written.push(entry(&compiled, c, Ran { first, second }, plan.keep_raw));
                }
            }
            Group::Write(_) => {
                for &i in &todo {
                    let first = driver.run(&request(&cases[i])).map_err(driver_err(&cases[i].id))?;
                    written.push(entry(&compiled, &cases[i], Ran { first, second: None }, plan.keep_raw));
                }
                dirty = true;
            }
            Group::Fresh(i) => {
                let c = &cases[*i];
                let first = driver.run(&request(c)).map_err(driver_err(&c.id))?;
                let second = if sampled(plan, &c.id) {
                    driver.reset().map_err(|error| UseCaseError::Driver { id: Some(c.id.clone()), error })?;
                    Some(driver.run(&request(c)).map_err(driver_err(&c.id))?)
                } else {
                    None
                };
                written.push(entry(&compiled, c, Ran { first, second }, plan.keep_raw));
                dirty = true;
            }
        }
        checkpoint.append(&written).map_err(|e| UseCaseError::Checkpoint(e.to_string()))?;
        ran_now += written.len();
        for e in written {
            if let CheckpointEntry::Done { id, .. } = &e {
                done.insert(id.clone(), e);
            }
        }
    }
    if done.len() < cases.len() {
        return Ok(Progress::Partial { recorded: done.len(), total: cases.len() });
    }
    let (mut interactions, mut raw, mut nondeterministic) = (Vec::new(), Vec::new(), Vec::new());
    let mut diffs = Vec::new();
    for c in cases {
        if let Some(CheckpointEntry::Done { interaction, nondeterministic: nd, differences, raw: r, .. }) = done.remove(&c.id) {
            if nd {
                nondeterministic.push(c.id.clone());
                diffs.push((c.id.clone(), differences));
            }
            if let Some(r) = r {
                raw.push((c.id.clone(), *r));
            }
            interactions.push(*interaction);
        }
    }
    let mut target = meta.target.clone();
    if let Some(t) = target.as_mut() {
        t.version = t.version.take().or(hello.target_version);
    }
    let corpus_ref = meta.corpus_path.clone().zip(corpus).map(|(path, _)| RefHash { path, hash: corpus_hash(cases) });
    let provenance = Provenance {
        kind: meta.kind,
        target,
        driver: Some(hello.driver),
        tool: meta.tool,
        seed: meta.seed,
        rules_hash: rules.hash(),
        corpus_hash: corpus_ref.as_ref().map(|c| c.hash.clone()),
        recorded_at: meta.recorded_at,
        recorded_on: meta.recorded_on,
        source: meta.source,
        note: None,
        imported_from: Vec::new(),
    };
    let mut cassette = Cassette {
        header: crate::cassette::Header { schema: SCHEMA.into(), provenance, rules: RefHash { path: meta.rules_path, hash: rules.hash() }, corpus: corpus_ref, interactions: 0, content_hash: String::new() },
        interactions,
    };
    cassette.seal();
    checkpoint.clear().map_err(|e| UseCaseError::Checkpoint(e.to_string()))?;
    Ok(Progress::Done(Box::new(Recording { set: crate::ports::CassetteSet { cassette, rules: rules.clone(), corpus: corpus.cloned() }, nondeterministic, differences: diffs, raw })))
}

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;
