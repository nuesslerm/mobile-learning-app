//! Stabilizer: from several recordings of the same corpus, infer the mask rules that make equal behaviour equal text. A pure function of
//! the recordings and the current rules: a leaf that differs in value but keeps its shape becomes a reviewable rule, frames that arrive in
//! another interleaving become a canonical order, and what no rule can honestly hide stays unstable with a root cause.
use crate::cassette::{Dir, Interaction};
use crate::rules::{Rule, RuleSet, RulesError};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "mimic-stability/1";
pub const ALLOWLIST_SCHEMA: &str = "mimic-stability-allowlist/1";
const ROUNDS: usize = 4;
const MIN_DIGITS: usize = 4;
const MIN_ID: usize = 6;
const SEPARATORS: &str = "_-.:/+=";
const UUID: &str = "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
const ISO: &str = r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:.]+(?:Z|[+-][0-9]{2}:[0-9]{2})";

/// Why a case still differs between recordings after every inferable rule; the table of the report and the key of the allowlist.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    /// A number or an id-shaped token that changes per run (fixed by a mask).
    Volatile,
    /// A short value (a counter, a one-digit status) that differs: masking it would hide a real answer.
    ShortValue,
    /// Free text that differs: a model or a host-dependent listing, no shape to mask.
    TextChange,
    ArrayLength,
    KeySet,
    TypeChange,
}

impl Cause {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Volatile => "a value that changes every run and keeps its shape",
            Self::ShortValue => "a short value differs between runs; masking it would hide the answer",
            Self::TextChange => "free text differs between runs and has no id or timestamp shape",
            Self::ArrayLength => "the number of items or frames differs between runs (a race or a host-dependent listing)",
            Self::KeySet => "an object has different keys between runs",
            Self::TypeChange => "a value changes type between runs",
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Sample {
    pub pointer: String,
    pub cause: Cause,
    pub a: String,
    pub b: String,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Proposal {
    pub rule: Rule,
    pub why: String,
    /// `global` or the one interaction a pointer rule is scoped to.
    pub scope: String,
    pub seen_in: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CaseStability {
    pub id: String,
    pub row: Option<String>,
    pub stable_before: bool,
    pub stable_after: bool,
    /// The primary cause of what is still unstable, with its first samples.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<Sample>,
}

#[derive(Debug, PartialEq)]
pub struct Stabilization {
    pub rules: RuleSet,
    pub proposals: Vec<Proposal>,
    pub cases: Vec<CaseStability>,
    /// Ids present in the first run but missing from another run: not judged.
    pub unobserved: Vec<String>,
}

impl Stabilization {
    pub fn stable_after(&self) -> usize {
        self.cases.iter().filter(|c| c.stable_after).count()
    }
    pub fn stable_before(&self) -> usize {
        self.cases.iter().filter(|c| c.stable_before).count()
    }
    pub fn unstable(&self) -> Vec<&CaseStability> {
        self.cases.iter().filter(|c| !c.stable_after).collect()
    }
}

/// One allowed instability: the id, the cause the stabilizer must still find, and why it cannot be removed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AllowEntry {
    pub id: String,
    pub cause: Cause,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StabilityAllowlist {
    pub schema: String,
    pub entries: Vec<AllowEntry>,
}

impl StabilityAllowlist {
    pub fn parse(text: &str) -> Result<Self, String> {
        let a: Self = serde_json::from_str(text).map_err(|e| format!("stability allowlist: {e}"))?;
        if a.schema != ALLOWLIST_SCHEMA {
            return Err(format!("stability allowlist schema {:?}, this tool reads {ALLOWLIST_SCHEMA}", a.schema));
        }
        match a.entries.iter().find(|e| e.reason.trim().is_empty()) {
            Some(e) => Err(format!("stability allowlist entry {} has no reason", e.id)),
            None => Ok(a),
        }
    }
}

/// The verdict against an allowlist: every unstable case must be listed with the cause it still has, and a listed case must still be unstable.
#[derive(Serialize, Debug, PartialEq, Default)]
pub struct Accounting {
    pub allowlisted: Vec<String>,
    pub unlisted: Vec<String>,
    pub stale: Vec<String>,
    pub wrong_cause: Vec<String>,
}

impl Accounting {
    pub fn clean(&self) -> bool {
        self.unlisted.is_empty() && self.stale.is_empty() && self.wrong_cause.is_empty()
    }
}

pub fn account(s: &Stabilization, allow: &StabilityAllowlist) -> Accounting {
    let mut out = Accounting::default();
    for c in s.unstable() {
        match allow.entries.iter().find(|e| e.id == c.id) {
            Some(e) if Some(e.cause) == c.cause => out.allowlisted.push(c.id.clone()),
            Some(_) => out.wrong_cause.push(c.id.clone()),
            None => out.unlisted.push(c.id.clone()),
        }
    }
    for e in &allow.entries {
        if !s.cases.iter().any(|c| c.id == e.id && !c.stable_after) {
            out.stale.push(e.id.clone());
        }
    }
    out
}

fn value(i: &Interaction) -> Value {
    serde_json::to_value(i).expect("interaction serialises")
}

fn pointer_escape(k: &str) -> String {
    k.replace('~', "~0").replace('/', "~1")
}

enum Diff {
    Text(String, String),
    Number(String, String),
    Structure(Cause, String, String),
}

struct Leaf {
    pointer: String,
    key: Option<String>,
    diff: Diff,
}

fn shown(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if s.chars().count() > 60 { format!("{}...", s.chars().take(60).collect::<String>()) } else { s }
}

fn leaves(path: &str, key: Option<&str>, a: &Value, b: &Value, out: &mut Vec<Leaf>) {
    if a == b {
        return;
    }
    let here = |diff| Leaf { pointer: path.to_string(), key: key.map(String::from), diff };
    match (a, b) {
        (Value::String(x), Value::String(y)) => out.push(here(Diff::Text(x.clone(), y.clone()))),
        (Value::Number(x), Value::Number(y)) => out.push(here(Diff::Number(x.to_string(), y.to_string()))),
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                leaves(&format!("{path}/{i}"), key, p, q, out);
            }
        }
        (Value::Array(x), Value::Array(y)) => out.push(here(Diff::Structure(Cause::ArrayLength, format!("{} items", x.len()), format!("{} items", y.len())))),
        (Value::Object(x), Value::Object(y)) => {
            let (kx, ky): (BTreeSet<_>, BTreeSet<_>) = (x.keys().collect(), y.keys().collect());
            if kx != ky {
                let diff = |p: &BTreeSet<&String>, q: &BTreeSet<&String>| p.difference(q).map(|s| s.as_str()).collect::<Vec<_>>().join(",");
                out.push(here(Diff::Structure(Cause::KeySet, format!("+{}", diff(&kx, &ky)), format!("+{}", diff(&ky, &kx)))));
            }
            for (k, p) in x {
                if let Some(q) = y.get(k) {
                    leaves(&format!("{path}/{}", pointer_escape(k)), Some(k), p, q, out);
                }
            }
        }
        _ => out.push(here(Diff::Structure(Cause::TypeChange, shown(a), shown(b)))),
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || SEPARATORS.contains(c)
}

/// The tokens that differ between two strings, one pair per differing region, each grown to the word around it.
fn differing_tokens(a: &str, b: &str) -> Vec<(String, String)> {
    let (mut a, mut b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut out = Vec::new();
    for _ in 0..16 {
        if a == b {
            break;
        }
        let p = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let s = a[p..].iter().rev().zip(b[p..].iter().rev()).take_while(|(x, y)| x == y).count();
        let (ea, eb) = (a.len() - s, b.len() - s);
        let grow = |v: &[char], from: usize, to: usize| {
            let (mut l, mut r) = (from, to);
            while l > 0 && is_word(v[l - 1]) {
                l -= 1;
            }
            while r < v.len() && is_word(v[r]) {
                r += 1;
            }
            (l, r)
        };
        let (la, ra) = grow(&a, p, ea);
        let (lb, rb) = grow(&b, p, eb);
        let (ta, tb) = (a[la..ra].iter().collect::<String>(), b[lb..rb].iter().collect::<String>());
        a.splice(la..ra, ['\u{0}']);
        b.splice(lb..rb, ['\u{0}']);
        out.push((ta, tb));
    }
    out
}

fn trim_seps(t: &str) -> &str {
    t.trim_matches(|c| SEPARATORS.contains(c))
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Digit,
    Hex,
    Upper,
    Lower,
    Alnum,
    Other,
}

impl Class {
    fn of(s: &str) -> Self {
        let all = |f: fn(char) -> bool| !s.is_empty() && s.chars().all(f);
        if all(|c| c.is_ascii_digit()) {
            Self::Digit
        } else if all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
            Self::Hex
        } else if all(|c| c.is_ascii_digit() || c.is_ascii_uppercase()) {
            Self::Upper
        } else if all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()) {
            Self::Lower
        } else if all(|c| c.is_ascii_alphanumeric()) {
            Self::Alnum
        } else {
            Self::Other
        }
    }
    fn join(self, o: Self) -> Self {
        if self == o {
            return self;
        }
        match (self, o) {
            (Self::Digit, Self::Hex) | (Self::Hex, Self::Digit) => Self::Hex,
            (Self::Digit, Self::Upper) | (Self::Upper, Self::Digit) => Self::Upper,
            (Self::Digit, Self::Lower) | (Self::Lower, Self::Digit) => Self::Lower,
            (Self::Other, _) | (_, Self::Other) => Self::Other,
            _ => Self::Alnum,
        }
    }
    fn set(self) -> &'static str {
        match self {
            Self::Digit => "[0-9]",
            Self::Hex => "[0-9a-f]",
            Self::Upper => "[A-Z0-9]",
            Self::Lower => "[a-z0-9]",
            Self::Alnum => "[A-Za-z0-9]",
            Self::Other => "[A-Za-z0-9]",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Digit => "digits",
            Self::Hex => "hex",
            Self::Upper => "upper",
            Self::Lower => "lower",
            Self::Alnum => "alnum",
            Self::Other => "chars",
        }
    }
}

fn parts(t: &str) -> (Vec<&str>, Vec<char>) {
    let mut words = Vec::new();
    let mut seps = Vec::new();
    let mut start = 0;
    for (i, c) in t.char_indices() {
        if SEPARATORS.contains(c) {
            words.push(&t[start..i]);
            seps.push(c);
            start = i + c.len_utf8();
        }
    }
    words.push(&t[start..]);
    (words, seps)
}

/// A pattern that matches both tokens and every other value of their shape, with the placeholder that names the shape; `None` when the
/// difference is a word, a counter or anything a mask would turn into a lie.
fn shape(ta: &str, tb: &str) -> Option<(String, String)> {
    let (ta, tb) = (trim_seps(ta), trim_seps(tb));
    let iso = Regex::new(&format!("^{ISO}$")).expect("iso pattern");
    if iso.is_match(ta) && iso.is_match(tb) {
        return Some((ISO.to_string(), "<ts>".into()));
    }
    let ((wa, sa), (wb, sb)) = (parts(ta), parts(tb));
    if wa.len() != wb.len() || sa != sb {
        return None;
    }
    let mut pattern = String::new();
    let mut label = String::new();
    let mut varying = 0;
    for (i, (x, y)) in wa.iter().zip(&wb).enumerate() {
        if i > 0 {
            pattern.push_str(&regex::escape(&sa[i - 1].to_string()));
        }
        if x == y {
            pattern.push_str(&regex::escape(x));
            continue;
        }
        let class = Class::of(x).join(Class::of(y));
        let (lo, hi) = (x.len().min(y.len()), x.len().max(y.len()));
        let idlike = match class {
            Class::Digit => lo >= MIN_DIGITS,
            Class::Other => false,
            _ => lo >= MIN_ID && (x.chars().chain(y.chars()).any(|c| c.is_ascii_digit()) || class == Class::Hex),
        };
        if !idlike {
            return None;
        }
        varying += 1;
        let count = if lo == hi { format!("{{{lo}}}") } else { format!("{{{lo},{hi}}}") };
        pattern.push_str(&format!("{}{count}", class.set()));
        if label.is_empty() {
            label = format!("{}{}", class.label(), if lo == hi { lo.to_string() } else { format!("{lo}-{hi}") });
        }
    }
    (varying > 0).then(|| (format!(r"\b{pattern}\b"), format!("<volatile:{label}>")))
}

fn keep_unique(proposals: &mut Vec<Proposal>, p: Proposal) {
    match proposals.iter_mut().find(|q| q.rule == p.rule) {
        Some(q) => {
            for id in p.seen_in {
                if !q.seen_in.contains(&id) {
                    q.seen_in.push(id);
                }
            }
        }
        None => proposals.push(p),
    }
}

fn strings_of(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Array(a) => a.iter().for_each(|x| strings_of(x, out)),
        Value::Object(m) => m.values().for_each(|x| strings_of(x, out)),
        _ => {}
    }
}

/// Everything the recordings agree on, as text: what a proposed pattern must not match, or it would mask a stable answer.
fn stable_text(runs: &[Vec<Value>], id_of: &[Vec<String>]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, id) in id_of[0].iter().enumerate() {
        let others: Vec<&Value> = runs[1..].iter().zip(&id_of[1..]).filter_map(|(r, ids)| ids.iter().position(|x| x == id).map(|j| &r[j])).collect();
        let mut stable = Vec::new();
        agree(&runs[0][i], &others, &mut stable);
        out.extend(stable);
    }
    out
}

fn agree(a: &Value, others: &[&Value], out: &mut Vec<String>) {
    if others.iter().all(|o| *o == a) {
        strings_of(a, out);
        return;
    }
    match a {
        Value::Array(x) => {
            for (i, v) in x.iter().enumerate() {
                let next: Vec<&Value> = others.iter().filter_map(|o| o.as_array().and_then(|arr| arr.get(i))).collect();
                if next.len() == others.len() {
                    agree(v, &next, out);
                }
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                let next: Vec<&Value> = others.iter().filter_map(|o| o.get(k)).collect();
                if next.len() == others.len() {
                    agree(v, &next, out);
                }
            }
        }
        _ => {}
    }
}

/// How many times each key's numbers agreed across runs and how many times they varied.
fn key_stats(a: &Value, others: &[&Value], key: Option<&str>, stats: &mut BTreeMap<String, (usize, usize)>) {
    match a {
        Value::Number(_) => {
            if let Some(k) = key {
                let e = stats.entry(k.to_string()).or_default();
                if others.iter().all(|o| *o == a) { e.0 += 1 } else { e.1 += 1 }
            }
        }
        Value::Array(x) => {
            for (i, v) in x.iter().enumerate() {
                let next: Vec<&Value> = others.iter().filter_map(|o| o.as_array().and_then(|arr| arr.get(i))).collect();
                if next.len() == others.len() {
                    key_stats(v, &next, key, stats);
                }
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                let next: Vec<&Value> = others.iter().filter_map(|o| o.get(k)).collect();
                if next.len() == others.len() {
                    key_stats(v, &next, Some(k), stats);
                }
            }
        }
        _ => {}
    }
}

struct Evidence {
    stable_text: Vec<String>,
    stats: BTreeMap<String, (usize, usize)>,
}

fn propose(id: &str, leaf: &Leaf, ev: &Evidence, out: &mut Vec<Proposal>) -> Option<(Cause, String, String)> {
    let scoped = |why: &str| Proposal { rule: Rule::Pointer { pointer: leaf.pointer.clone(), placeholder: "<volatile>".into(), interaction: Some(id.to_string()) }, why: why.into(), scope: id.to_string(), seen_in: vec![id.to_string()] };
    match &leaf.diff {
        Diff::Structure(cause, a, b) => Some((*cause, a.clone(), b.clone())),
        Diff::Number(a, b) => {
            let (agreed, varied) = leaf.key.as_deref().and_then(|k| ev.stats.get(k)).copied().unwrap_or((1, 1));
            match (&leaf.key, agreed) {
                _ if a.len().max(b.len()) < MIN_DIGITS => return Some((Cause::ShortValue, a.clone(), b.clone())),
                (Some(k), 0) if varied > 0 => keep_unique(out, Proposal { rule: Rule::VolatileNumber { keys: BTreeMap::from([(k.clone(), format!("<volatile:{k}>"))]) }, why: format!("the number under `{k}` differed in every recording that carried it ({varied} places)"), scope: "global".into(), seen_in: vec![id.to_string()] }),
                _ => keep_unique(out, scoped("a number that differs between runs under a key that is stable elsewhere")),
            }
            None
        }
        Diff::Text(a, b) => {
            let tokens = differing_tokens(a, b);
            let mut all_shaped = true;
            for (ta, tb) in &tokens {
                let uuid = Regex::new(&format!("^{UUID}$")).expect("uuid pattern");
                if uuid.is_match(trim_seps(ta)) && uuid.is_match(trim_seps(tb)) {
                    keep_unique(out, Proposal { rule: Rule::UuidOrdinal, why: "two runs answered different uuids".into(), scope: "global".into(), seen_in: vec![id.to_string()] });
                    continue;
                }
                match shape(ta, tb) {
                    Some((pattern, placeholder)) => {
                        let re = Regex::new(&pattern).expect("generated pattern compiles");
                        if ev.stable_text.iter().any(|s| re.is_match(s)) {
                            keep_unique(out, scoped("a value of a shape a stable answer also has: masked in this interaction only"));
                        } else {
                            keep_unique(out, Proposal { rule: Rule::Token { pattern: pattern.clone(), placeholder }, why: format!("`{}` and `{}` differ and share the shape {pattern}", shown_token(ta), shown_token(tb)), scope: "global".into(), seen_in: vec![id.to_string()] });
                        }
                    }
                    None => all_shaped = false,
                }
            }
            if all_shaped && !tokens.is_empty() {
                return None;
            }
            let short = tokens.iter().all(|(x, y)| x.len().max(y.len()) < MIN_ID);
            Some((if short { Cause::ShortValue } else { Cause::TextChange }, shown(&Value::String(a.clone())), shown(&Value::String(b.clone()))))
        }
    }
}

fn shown_token(t: &str) -> String {
    t.chars().take(24).collect()
}

fn canonical_channels(a: &Interaction, b: &Interaction, rules: &RuleSet) -> Vec<String> {
    let have: BTreeSet<&str> = rules.rules.iter().filter_map(|r| if let Rule::CanonicalOrder { channel } = r { Some(channel.as_str()) } else { None }).collect();
    let channels: BTreeSet<&str> = a.steps.iter().filter(|s| s.dir == Dir::S2c).filter_map(|s| s.channel.as_deref()).collect();
    channels
        .into_iter()
        .filter(|c| !have.contains(c))
        .filter(|c| {
            let rule = RuleSet::new(vec![Rule::CanonicalOrder { channel: (*c).to_string() }]);
            rule.compile().is_ok_and(|k| k.interaction(a) == k.interaction(b) && a != b)
        })
        .map(String::from)
        .collect()
}

/// Run the inference over every case present in all recordings; `runs[0]` fixes the order of the cases.
pub fn stabilize(rules: &RuleSet, runs: &[Vec<Interaction>]) -> Result<Stabilization, RulesError> {
    let mut rules = rules.clone();
    let (first, rest) = runs.split_first().expect("at least one run");
    let ids: Vec<String> = first.iter().filter(|i| rest.iter().all(|r| r.iter().any(|j| j.id == i.id))).map(|i| i.id.clone()).collect();
    let unobserved: Vec<String> = first.iter().filter(|i| !ids.contains(&i.id)).map(|i| i.id.clone()).collect();
    let pick = |run: &[Interaction], id: &str| run.iter().find(|i| i.id == id).expect("id is in every run").clone();
    let by_id: Vec<Vec<Interaction>> = runs.iter().map(|r| ids.iter().map(|id| pick(r, id)).collect()).collect();
    let stable = |rules: &RuleSet, i: usize| -> Result<bool, RulesError> {
        let k = rules.compile()?;
        let a = k.interaction(&by_id[0][i]);
        Ok(by_id[1..].iter().all(|r| k.interaction(&r[i]) == a))
    };
    let mut before = Vec::new();
    for i in 0..ids.len() {
        before.push(stable(&rules, i)?);
    }
    let mut proposals: Vec<Proposal> = Vec::new();
    for _ in 0..ROUNDS {
        let k = rules.compile()?;
        let masked: Vec<Vec<Interaction>> = by_id.iter().map(|r| r.iter().map(|i| k.interaction(i)).collect()).collect();
        let values: Vec<Vec<Value>> = masked.iter().map(|r| r.iter().map(value).collect()).collect();
        let id_lists: Vec<Vec<String>> = vec![ids.clone(); runs.len()];
        let mut ev = Evidence { stable_text: stable_text(&values, &id_lists), stats: BTreeMap::new() };
        for i in 0..ids.len() {
            let others: Vec<&Value> = values[1..].iter().map(|r| &r[i]).collect();
            key_stats(&values[0][i], &others, None, &mut ev.stats);
        }
        let mut changed = false;
        for (i, id) in ids.iter().enumerate() {
            if (1..runs.len()).all(|r| masked[r][i] == masked[0][i]) {
                continue;
            }
            let mut found: Vec<Proposal> = Vec::new();
            for r in 1..runs.len() {
                for channel in canonical_channels(&by_id[0][i], &by_id[r][i], &rules) {
                    keep_unique(&mut found, Proposal { rule: Rule::CanonicalOrder { channel: channel.clone() }, why: format!("the `{channel}` frames of {id} are the same set in another interleaving"), scope: "global".into(), seen_in: vec![id.clone()] });
                }
                if found.is_empty() {
                    let mut ls = Vec::new();
                    leaves("", None, &values[0][i], &values[r][i], &mut ls);
                    for leaf in &ls {
                        propose(id, leaf, &ev, &mut found);
                    }
                }
            }
            for p in found {
                if !rules.rules.contains(&p.rule) {
                    rules.rules.push(p.rule.clone());
                    changed = true;
                }
                keep_unique(&mut proposals, p);
            }
        }
        if !changed {
            break;
        }
    }
    let k = rules.compile()?;
    let masked: Vec<Vec<Value>> = by_id.iter().map(|r| r.iter().map(|i| value(&k.interaction(i))).collect()).collect();
    let none = Evidence { stable_text: Vec::new(), stats: BTreeMap::new() };
    let mut cases = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let mut samples = Vec::new();
        for r in 1..runs.len() {
            let mut ls = Vec::new();
            leaves("", None, &masked[0][i], &masked[r][i], &mut ls);
            for leaf in &ls {
                let mut sink = Vec::new();
                let cause = propose(id, leaf, &none, &mut sink).unwrap_or_else(|| match &leaf.diff {
                    Diff::Text(a, b) | Diff::Number(a, b) => (Cause::Volatile, a.clone(), b.clone()),
                    Diff::Structure(c, a, b) => (*c, a.clone(), b.clone()),
                });
                samples.push(Sample { pointer: leaf.pointer.clone(), cause: cause.0, a: cause.1, b: cause.2 });
            }
        }
        samples.dedup_by(|a, b| a.pointer == b.pointer);
        let cause = samples.iter().map(|s| s.cause).min();
        samples.truncate(3);
        cases.push(CaseStability { id: id.clone(), row: by_id[0][i].row.clone(), stable_before: before[i], stable_after: cause.is_none(), cause, samples });
    }
    Ok(Stabilization { rules, proposals, cases, unobserved })
}

#[cfg(test)]
#[path = "stabilize_tests.rs"]
mod tests;
