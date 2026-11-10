//! The differ: expected (the cassette) against actual (the candidate), under an allowlist. Severity is `none | drift | breaking`: a
//! missing key, a changed type or length, a changed status are breaking; an extra key or a changed value drift. A divergence the
//! allowlist covers is reported with its reason and counts for nothing, so over-broad entries show in the report instead of hiding.
use crate::allowlist::{plain, Allowlist, Entry, Group};
use crate::cassette::{Cassette, Dir, Interaction, Step};
use crate::json::kind;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    None,
    Drift,
    Breaking,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiffKind {
    Missing,
    Extra,
    Changed,
    Kind,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Allowed {
    pub group: Group,
    pub path: String,
    pub reason: String,
    pub phase: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Diff {
    pub path: String,
    pub kind: DiffKind,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowlisted: Option<Allowed>,
}

/// How a stream of frames is compared when the reference's chunking and repeats are racy: the values of all chunks of one address (channel,
/// direction, request id) count as one list, a value matching `once` counts the first time only, and an array at a `squash` pointer
/// counts each run of equal elements once. Both sides are folded, so only the values and their order are compared.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fold {
    /// The channel of the frames (`ws`); only server-to-client steps fold.
    pub channel: String,
    /// JSON pointer to a frame's tag, and the tag of the frames that carry a list.
    pub tag: String,
    pub chunk: String,
    /// JSON pointer to the list inside such a frame.
    pub values: String,
    /// `[pointer, value]` pairs: a list value whose pointer holds that string is kept the first time an address has it.
    #[serde(default)]
    pub once: Vec<(String, String)>,
    #[serde(default)]
    pub squash: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DiffOptions {
    /// Steps of these directions are what the driver was given (a client's frames), not what the target did: they are not compared.
    pub ignore_dirs: Vec<Dir>,
    pub fold: Option<Fold>,
    /// A changed scalar at exactly one of these (index-free) paths is breaking, not drift: behaviour, not a value.
    pub breaking_paths: Vec<String>,
    /// A boolean under one of these prefixes is a feature advertisement of the recorded server: reported, never failing.
    pub advertised_prefixes: Vec<String>,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self { ignore_dirs: Vec::new(), fold: None, breaking_paths: vec!["outcome.status".into(), "outcome.code".into()], advertised_prefixes: Vec::new() }
    }
}

struct Ctx<'a> {
    allow: &'a Allowlist,
    opts: &'a DiffOptions,
    out: Vec<Diff>,
}

impl Ctx<'_> {
    fn push(&mut self, path: &str, kind: DiffKind, severity: Severity, expected: Option<&Value>, actual: Option<&Value>) {
        self.out.push(Diff {
            path: path.trim_start_matches('.').to_string(),
            kind,
            severity,
            expected: expected.cloned(),
            actual: actual.cloned(),
            allowlisted: None,
        });
    }

    fn push_allowed(&mut self, path: &str, group: Group, entry: &Entry, kind: DiffKind, expected: Option<&Value>, actual: Option<&Value>) {
        self.out.push(Diff {
            path: path.trim_start_matches('.').to_string(),
            kind,
            severity: Severity::None,
            expected: expected.cloned(),
            actual: actual.cloned(),
            allowlisted: Some(Allowed { group, path: entry.path.clone(), reason: entry.reason.clone(), phase: entry.phase.clone() }),
        });
    }

    fn value(&mut self, path: &str, expected: &Value, actual: &Value) {
        if let Some(e) = Allowlist::find(&self.allow.shape_only, path) {
            if kind(expected) != kind(actual) {
                self.push(path, DiffKind::Kind, Severity::Breaking, Some(expected), Some(actual));
            } else if expected != actual {
                let e = e.clone();
                self.push_allowed(path, Group::ShapeOnly, &e, DiffKind::Changed, Some(expected), Some(actual));
            }
            return;
        }
        if kind(expected) != kind(actual) {
            self.push(path, DiffKind::Kind, Severity::Breaking, Some(expected), Some(actual));
            return;
        }
        match (expected, actual) {
            (Value::Object(e), Value::Object(a)) => {
                for (k, ev) in e {
                    let p = format!("{path}.{k}");
                    match a.get(k) {
                        Some(av) => self.value(&p, ev, av),
                        None => match Allowlist::find(&self.allow.omitted, &p) {
                            Some(entry) => {
                                let entry = entry.clone();
                                self.push_allowed(&p, Group::Omitted, &entry, DiffKind::Missing, Some(ev), None)
                            }
                            None => self.push(&p, DiffKind::Missing, Severity::Breaking, Some(ev), None),
                        },
                    }
                }
                for (k, av) in a {
                    if !e.contains_key(k) {
                        self.push(&format!("{path}.{k}"), DiffKind::Extra, Severity::Drift, None, Some(av));
                    }
                }
            }
            (Value::Array(e), Value::Array(a)) => {
                if e.len() != a.len() {
                    self.push(path, DiffKind::Changed, Severity::Breaking, Some(&Value::from(e.len())), Some(&Value::from(a.len())));
                }
                for (i, (ev, av)) in e.iter().zip(a).enumerate() {
                    self.value(&format!("{path}.[{i}]"), ev, av);
                }
            }
            _ if expected == actual => {}
            _ => self.scalar(path, expected, actual),
        }
    }

    fn scalar(&mut self, path: &str, expected: &Value, actual: &Value) {
        let plain_path = plain(path);
        let plain_path = plain_path.trim_start_matches('.');
        if expected.is_boolean() && self.opts.advertised_prefixes.iter().any(|p| plain_path.contains(p.as_str())) {
            let entry = Entry { path: plain_path.to_string(), reason: "a recorded capability flag is a feature advertisement of the recorded server".into(), phase: "all".into(), pin_hash: None };
            self.push_allowed(path, Group::Advertised, &entry, DiffKind::Changed, Some(expected), Some(actual));
            return;
        }
        if let Some(entry) = Allowlist::find(&self.allow.ignored, path) {
            let entry = entry.clone();
            self.push_allowed(path, Group::Ignored, &entry, DiffKind::Changed, Some(expected), Some(actual));
            return;
        }
        let severity = if self.opts.breaking_paths.iter().any(|b| b == plain_path) { Severity::Breaking } else { Severity::Drift };
        self.push(path, DiffKind::Changed, severity, Some(expected), Some(actual));
    }
}

/// Diff two JSON values; paths are dot-separated keys with `[i]` for array positions.
pub fn diff_json(expected: &Value, actual: &Value, allow: &Allowlist, opts: &DiffOptions) -> Vec<Diff> {
    let mut ctx = Ctx { allow, opts, out: Vec::new() };
    ctx.value("", expected, actual);
    ctx.out
}

// A step is addressed by channel, direction and request id (or `_`), so a racy interleaving across ids is not a difference; within one
// address the order is.
fn address(s: &Step) -> String {
    let dir = serde_json::to_value(s.dir).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    format!("{}:{}:{}", s.channel.as_deref().unwrap_or("_"), dir, s.rid.as_deref().unwrap_or("_")).replace('.', "_")
}

fn group(steps: &[Step]) -> BTreeMap<String, Vec<&Step>> {
    let mut out: BTreeMap<String, Vec<&Step>> = BTreeMap::new();
    for s in steps {
        out.entry(address(s)).or_default().push(s);
    }
    out
}

fn fold(steps: Vec<Step>, f: &Fold) -> Vec<Step> {
    let mut out: Vec<Step> = Vec::new();
    let mut at: BTreeMap<String, usize> = BTreeMap::new();
    let mut once: BTreeSet<(String, usize)> = BTreeSet::new();
    for mut s in steps {
        let chunk = s.dir == Dir::S2c && s.channel.as_deref() == Some(f.channel.as_str()) && s.payload.pointer(&f.tag).and_then(Value::as_str) == Some(f.chunk.as_str());
        if !chunk {
            for p in &f.squash {
                if let Some(Value::Array(a)) = s.payload.pointer_mut(p) {
                    a.dedup();
                }
            }
            out.push(s);
            continue;
        }
        let addr = address(&s);
        let values = s.payload.pointer(&f.values).and_then(Value::as_array).cloned().unwrap_or_default();
        let kept: Vec<Value> = values
            .into_iter()
            .filter(|v| f.once.iter().enumerate().all(|(i, (ptr, want))| v.pointer(ptr).and_then(Value::as_str) != Some(want.as_str()) || once.insert((addr.clone(), i))))
            .collect();
        match at.get(&addr) {
            Some(&i) => {
                if let Some(Value::Array(a)) = out[i].payload.pointer_mut(&f.values) {
                    a.extend(kept);
                }
            }
            None => {
                if let Some(slot) = s.payload.pointer_mut(&f.values) {
                    *slot = Value::Array(kept);
                }
                at.insert(addr, out.len());
                out.push(s);
            }
        }
    }
    out
}

fn compared(steps: &[Step], opts: &DiffOptions) -> Vec<Step> {
    let kept: Vec<Step> = steps.iter().filter(|s| !opts.ignore_dirs.contains(&s.dir)).cloned().collect();
    match &opts.fold {
        Some(f) => fold(kept, f),
        None => kept,
    }
}

/// The observed side of two interactions: steps by address, effects and outcome as values. The input is not compared: replay feeds the
/// candidate the cassette's input.
pub fn diff_interaction(expected: &Interaction, actual: &Interaction, allow: &Allowlist, opts: &DiffOptions) -> Vec<Diff> {
    let mut ctx = Ctx { allow, opts, out: Vec::new() };
    ctx.value("outcome", &to(&expected.outcome), &to(&actual.outcome));
    ctx.value("effects", &to(&expected.effects), &to(&actual.effects));
    let (es, os) = (compared(&expected.steps, opts), compared(&actual.steps, opts));
    let (g, o) = (group(&es), group(&os));
    for (key, es) in &g {
        let os = o.get(key).map(Vec::as_slice).unwrap_or(&[]);
        for (i, e) in es.iter().enumerate() {
            let path = format!("steps.{key}.[{i}]");
            match os.get(i) {
                Some(a) => ctx.value(&path, &to(*e), &to(*a)),
                None => ctx.push(&path, DiffKind::Missing, Severity::Breaking, Some(&to(*e)), None),
            }
        }
    }
    for (key, os) in &o {
        let have = g.get(key).map_or(0, Vec::len);
        for (i, a) in os.iter().enumerate().skip(have) {
            ctx.push(&format!("steps.{key}.[{i}]"), DiffKind::Extra, Severity::Drift, None, Some(&to(*a)));
        }
    }
    ctx.out
}

fn to<T: Serialize + ?Sized>(t: &T) -> Value {
    serde_json::to_value(t).expect("observed data serialises")
}

pub fn severity_of(diffs: &[Diff]) -> Severity {
    diffs.iter().map(|d| d.severity).max().unwrap_or(Severity::None)
}

/// The evidence label of one interaction: what `wf parity coverage` filters on.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    Clean,
    Allowlisted,
    Drift,
    Breaking,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct InteractionResult {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    pub severity: Severity,
    pub label: Label,
    pub diffs: Vec<Diff>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl InteractionResult {
    pub fn of(id: &str, row: Option<String>, diffs: Vec<Diff>) -> Self {
        let severity = severity_of(&diffs);
        let label = match severity {
            Severity::Breaking => Label::Breaking,
            Severity::Drift => Label::Drift,
            Severity::None if diffs.is_empty() => Label::Clean,
            Severity::None => Label::Allowlisted,
        };
        Self { id: id.to_string(), row, severity, label, diffs, error: None }
    }

    pub fn failed(id: &str, row: Option<String>, error: String) -> Self {
        Self { id: id.to_string(), row, severity: Severity::Breaking, label: Label::Breaking, diffs: Vec::new(), error: Some(error) }
    }
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub total: usize,
    pub clean: usize,
    pub allowlisted: usize,
    pub drift: usize,
    pub breaking: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Verdict {
    pub severity: Severity,
    pub summary: Summary,
    pub results: Vec<InteractionResult>,
    /// Allowlist entries no divergence of this run matched: stale or over-broad.
    pub unused_allowlist: Vec<String>,
    /// For each failing property case, the smallest stored input of its shrink ladder that still fails.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shrunk: Vec<Shrunk>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Shrunk {
    pub case: String,
    pub minimal: String,
    pub input: serde_json::Value,
}

impl Verdict {
    pub fn of(results: Vec<InteractionResult>, allow: &Allowlist) -> Self {
        let mut summary = Summary { total: results.len(), ..Summary::default() };
        for r in &results {
            match r.label {
                Label::Clean => summary.clean += 1,
                Label::Allowlisted => summary.allowlisted += 1,
                Label::Drift => summary.drift += 1,
                Label::Breaking => summary.breaking += 1,
            }
        }
        let used = |group: Group, e: &Entry| {
            results.iter().flat_map(|r| &r.diffs).any(|d| d.allowlisted.as_ref().is_some_and(|a| a.group == group && a.path == e.path))
        };
        let mut unused = Vec::new();
        for (group, name, list) in [(Group::Ignored, "ignored", &allow.ignored), (Group::Omitted, "omitted", &allow.omitted), (Group::ShapeOnly, "shapeOnly", &allow.shape_only)] {
            unused.extend(list.iter().filter(|e| !used(group, e)).map(|e| format!("{name}:{}", e.path)));
        }
        let severity = results.iter().map(|r| r.severity).max().unwrap_or(Severity::None);
        Self { severity, summary, results, unused_allowlist: unused, shrunk: Vec::new() }
    }

    pub fn ok(&self) -> bool {
        self.severity == Severity::None
    }
}

/// Two cassettes by interaction id: the reference before and after a pin bump, or the reference and a candidate-recorded one. An id
/// only one side has is breaking when expected has it, drift when only actual does.
pub fn diff_cassettes(expected: &Cassette, actual: &Cassette, allow: &Allowlist, opts: &DiffOptions) -> Verdict {
    let mut results = Vec::new();
    for e in &expected.interactions {
        results.push(match actual.interactions.iter().find(|a| a.id == e.id) {
            Some(a) => InteractionResult::of(&e.id, e.row.clone(), diff_interaction(e, &Interaction { id: e.id.clone(), ..a.clone() }, allow, opts)),
            None => InteractionResult::of(
                &e.id,
                e.row.clone(),
                vec![Diff { path: e.id.clone(), kind: DiffKind::Missing, severity: Severity::Breaking, expected: None, actual: None, allowlisted: None }],
            ),
        });
    }
    for a in actual.interactions.iter().filter(|a| !expected.interactions.iter().any(|e| e.id == a.id)) {
        results.push(InteractionResult::of(
            &a.id,
            a.row.clone(),
            vec![Diff { path: a.id.clone(), kind: DiffKind::Extra, severity: Severity::Drift, expected: None, actual: None, allowlisted: None }],
        ));
    }
    Verdict::of(results, allow)
}
