//! The provenance scan of a fixture tree: every file under the tree's `include` globs is classified by the first rule that matches its
//! path (`authored`: our own contract; `mirrors-reference`: a claimed copy of reference behaviour, to become a recorded cassette;
//! `cli`: output of a real CLI, an X3 cassette). The rules are data (`mimic-fixture-rules/1`), a file matching none is a finding.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const RULES_SCHEMA: &str = "mimic-fixture-rules/1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureKind {
    Authored,
    MirrorsReference,
    Cli,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRule {
    #[serde(rename = "match")]
    pub pattern: String,
    pub kind: FixtureKind,
    pub reason: String,
    /// The parity row whose oracle the file is to be re-recorded against; required for `mirrors-reference`.
    pub row: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixtureRules {
    pub schema: String,
    pub include: Vec<String>,
    pub rules: Vec<FixtureRule>,
}

impl FixtureRules {
    pub fn parse(text: &str) -> Result<Self, String> {
        let rules: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if rules.schema != RULES_SCHEMA {
            return Err(format!("schema is {}, this wf mimic reads {RULES_SCHEMA}", rules.schema));
        }
        if rules.include.is_empty() {
            return Err("include names no glob: nothing would be scanned".into());
        }
        for r in &rules.rules {
            if r.reason.trim().is_empty() {
                return Err(format!("rule {} has no reason", r.pattern));
            }
            if r.kind == FixtureKind::MirrorsReference && r.row.as_deref().is_none_or(str::is_empty) {
                return Err(format!("rule {} is mirrors-reference and names no parity row to re-record it against (row)", r.pattern));
            }
        }
        Ok(rules)
    }

    pub fn includes(&self, path: &str) -> bool {
        self.include.iter().any(|g| glob(g, path))
    }

    fn classify(&self, path: &str) -> Option<usize> {
        self.rules.iter().position(|r| glob(&r.pattern, path))
    }
}

/// `*` and `?` match within a path segment, a `**` segment any number of segments (also none).
pub fn glob(pattern: &str, path: &str) -> bool {
    let (p, s): (Vec<&str>, Vec<&str>) = (pattern.split('/').collect(), path.split('/').collect());
    segments(&p, &s)
}

fn segments(p: &[&str], s: &[&str]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((&"**", rest)) => (0..=s.len()).any(|i| segments(rest, &s[i..])),
        Some((head, rest)) => s.split_first().is_some_and(|(seg, tail)| segment(head.as_bytes(), seg.as_bytes()) && segments(rest, tail)),
    }
}

fn segment(p: &[u8], s: &[u8]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| segment(rest, &s[i..])),
        Some((b'?', rest)) => !s.is_empty() && segment(rest, &s[1..]),
        Some((c, rest)) => s.first() == Some(c) && segment(rest, &s[1..]),
    }
}

pub struct FixtureFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Classified {
    pub path: String,
    pub kind: Option<FixtureKind>,
    pub rule: Option<String>,
    pub exchanges: usize,
    pub volatile_pointers: usize,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct Counts {
    pub files: usize,
    pub authored: usize,
    pub mirrors_reference: usize,
    pub cli: usize,
    pub unclassified: usize,
    pub with_volatile: usize,
    pub exchanges: usize,
}

impl Counts {
    fn add(&mut self, c: &Classified) {
        self.files += 1;
        match c.kind {
            Some(FixtureKind::Authored) => self.authored += 1,
            Some(FixtureKind::MirrorsReference) => self.mirrors_reference += 1,
            Some(FixtureKind::Cli) => self.cli += 1,
            None => self.unclassified += 1,
        }
        self.with_volatile += usize::from(c.volatile_pointers > 0);
        self.exchanges += c.exchanges;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Scan {
    pub total: Counts,
    /// First path segment (the crate, for a workspace) to its counts.
    pub groups: BTreeMap<String, Counts>,
    pub unclassified: Vec<String>,
    /// Rules that matched no file: stale, or waiting for a file kind that does not exist yet.
    pub unused_rules: Vec<String>,
    pub files: Vec<Classified>,
}

impl Scan {
    pub fn ok(&self) -> bool {
        self.unclassified.is_empty()
    }
}

/// The `exchanges` of an `authored` JSON fixture and how many `volatile` pointers they carry; other files count none.
fn exchanges_of(bytes: &[u8]) -> (usize, usize) {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else { return (0, 0) };
    let Some(list) = v.get("exchanges").and_then(Value::as_array) else { return (0, 0) };
    let volatile = list.iter().filter_map(|e| e.get("volatile").and_then(Value::as_array)).map(Vec::len).sum();
    (list.len(), volatile)
}

pub fn scan(rules: &FixtureRules, files: &[FixtureFile]) -> Scan {
    let mut used = vec![false; rules.rules.len()];
    let mut classified: Vec<Classified> = files
        .iter()
        .filter(|f| rules.includes(&f.path))
        .map(|f| {
            let rule = rules.classify(&f.path);
            if let Some(i) = rule {
                used[i] = true;
            }
            let (exchanges, volatile_pointers) = exchanges_of(&f.bytes);
            Classified { path: f.path.clone(), kind: rule.map(|i| rules.rules[i].kind), rule: rule.map(|i| rules.rules[i].pattern.clone()), exchanges, volatile_pointers }
        })
        .collect();
    classified.sort_by(|a, b| a.path.cmp(&b.path));
    let mut total = Counts::default();
    let mut groups: BTreeMap<String, Counts> = BTreeMap::new();
    for c in &classified {
        total.add(c);
        groups.entry(c.path.split('/').next().unwrap_or_default().to_string()).or_default().add(c);
    }
    let unclassified = classified.iter().filter(|c| c.kind.is_none()).map(|c| c.path.clone()).collect();
    let unused_rules = rules.rules.iter().zip(&used).filter(|(_, u)| !**u).map(|(r, _)| r.pattern.clone()).collect();
    Scan { total, groups, unclassified, unused_rules, files: classified }
}
