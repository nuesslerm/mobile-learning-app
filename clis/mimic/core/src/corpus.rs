//! `mimic-corpus/1`: where a set's inputs come from. Generated inputs are stored in the cassette, so replay never regenerates and is
//! deterministic and offline; generators run only at record time. `corpus` takes hand-picked inputs (inline or from files of the set),
//! `seeded` draws a template's holes from a seeded stream and derives mutated cases.
use crate::json::{hash_value, set_pointer};
use crate::ports::{Rng, Store, StoreError};
use crate::rng::{fnv1a, SplitMix64};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "mimic-corpus/1";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CorpusSpec {
    pub schema: String,
    #[serde(default)]
    pub seed: u64,
    pub generators: Vec<GeneratorSpec>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GeneratorSpec {
    Corpus {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op: Option<String>,
        // files of the set, each a JSON array of cases
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        files: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        cases: Vec<CaseSpec>,
    },
    Seeded {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op: Option<String>,
        count: usize,
        template: Value,
        fields: Vec<FieldSpec>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mutators: Vec<Mutator>,
    },
    /// Seeded draws like `Seeded`, but each case also stores its shrink ladder (smaller inputs of the same shape) so a replay that
    /// finds a diff can name the smallest stored case that still shows it. Structured fields (`list`, `record`, `one-of`) are allowed.
    Property {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        op: Option<String>,
        count: usize,
        template: Value,
        fields: Vec<FieldSpec>,
        #[serde(default = "default_shrink_steps")]
        shrink_steps: usize,
    },
}

fn default_shrink_steps() -> usize {
    6
}

pub const TAG_PROPERTY: &str = "property";
pub const TAG_SHRINK: &str = "shrink";
pub const TAG_SHRINK_OF: &str = "shrink-of:";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CaseSpec {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    pub input: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FieldSpec {
    pub pointer: String,
    #[serde(flatten)]
    pub gen: Gen,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "gen", rename_all = "lowercase")]
pub enum Gen {
    Int { min: i64, max: i64 },
    Hex { bytes: usize },
    Choice { values: Vec<Value> },
    Text { min_len: usize, max_len: usize },
    List { min_len: usize, max_len: usize, item: Box<Gen> },
    Record { fields: BTreeMap<String, Gen> },
    #[serde(rename = "one-of")]
    OneOf { variants: Vec<Gen> },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Mutator {
    Empty,
    Truncate,
    ControlChars,
    Boundary,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Case {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row: Option<String>,
    pub input: Value,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub enum CorpusError {
    Schema(String),
    Store(String),
    BadFile { file: String, message: String },
    BadPointer { generator: String, pointer: String },
    DuplicateId(String),
}

impl std::fmt::Display for CorpusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Schema(s) => write!(f, "unsupported corpus schema {s:?}, this tool reads {SCHEMA}"),
            Self::Store(e) => write!(f, "corpus file unreadable: {e}"),
            Self::BadFile { file, message } => write!(f, "corpus file {file}: {message}"),
            Self::BadPointer { generator, pointer } => write!(f, "generator {generator}: template has no parent for pointer {pointer}"),
            Self::DuplicateId(id) => write!(f, "corpus case id {id} appears twice"),
        }
    }
}

impl std::error::Error for CorpusError {}

impl From<StoreError> for CorpusError {
    fn from(e: StoreError) -> Self {
        Self::Store(e.to_string())
    }
}

fn draw(gen: &Gen, rng: &mut dyn Rng) -> Value {
    match gen {
        Gen::Int { min, max } => {
            let span = (i128::from(*max) - i128::from(*min) + 1).max(1);
            json!((i128::from(*min) + i128::from(rng.next_u64()) % span) as i64)
        }
        Gen::Hex { bytes } => Value::String((0..*bytes).map(|_| format!("{:02x}", rng.next_u64() & 0xff)).collect()),
        Gen::Choice { values } if values.is_empty() => Value::Null,
        Gen::Choice { values } => values[(rng.next_u64() % values.len() as u64) as usize].clone(),
        Gen::Text { min_len, max_len } => {
            let span = (max_len.saturating_sub(*min_len) + 1) as u64;
            let len = min_len + (rng.next_u64() % span) as usize;
            Value::String((0..len).map(|_| char::from(0x21 + (rng.next_u64() % 94) as u8)).collect())
        }
        Gen::List { min_len, max_len, item } => {
            let span = (max_len.saturating_sub(*min_len) + 1) as u64;
            let len = min_len + (rng.next_u64() % span) as usize;
            Value::Array((0..len).map(|_| draw(item, rng)).collect())
        }
        Gen::Record { fields } => Value::Object(fields.iter().map(|(k, g)| (k.clone(), draw(g, rng))).collect()),
        Gen::OneOf { variants } if variants.is_empty() => Value::Null,
        Gen::OneOf { variants } => {
            let at = (rng.next_u64() % variants.len() as u64) as usize;
            draw(&variants[at], rng)
        }
    }
}

/// Smaller values of the same shape, most aggressive first: lists shrink to their minimum, a half and one element fewer, text to
/// its minimum and a half, integers toward their lower bound, records one field at a time.
fn shrinks(gen: &Gen, value: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    match (gen, value) {
        (Gen::List { min_len, item, .. }, Value::Array(items)) => {
            let n = items.len();
            if n > *min_len {
                out.push(Value::Array(items[..*min_len].to_vec()));
                if n / 2 > *min_len {
                    out.push(Value::Array(items[..n / 2].to_vec()));
                }
                for i in 0..n {
                    let mut fewer = items.clone();
                    fewer.remove(i);
                    out.push(Value::Array(fewer));
                }
            }
            for (i, it) in items.iter().enumerate() {
                for smaller in shrinks(item, it) {
                    let mut next = items.clone();
                    next[i] = smaller;
                    out.push(Value::Array(next));
                }
            }
        }
        (Gen::Text { min_len, .. }, Value::String(s)) => {
            let n = s.chars().count();
            if n > *min_len {
                out.push(Value::String(s.chars().take(*min_len).collect()));
                if n / 2 > *min_len {
                    out.push(Value::String(s.chars().take(n / 2).collect()));
                }
            }
        }
        (Gen::Int { min, .. }, Value::Number(n)) => {
            if let Some(v) = n.as_i64().filter(|v| v != min) {
                out.push(json!(min));
                let mid = (i128::from(v) + i128::from(*min)) / 2;
                if mid != i128::from(v) && mid != i128::from(*min) {
                    out.push(json!(mid as i64));
                }
            }
        }
        (Gen::Record { fields }, Value::Object(map)) => {
            for (k, g) in fields {
                for smaller in map.get(k).map(|v| shrinks(g, v)).unwrap_or_default() {
                    let mut next = map.clone();
                    next.insert(k.clone(), smaller);
                    out.push(Value::Object(next));
                }
            }
        }
        (Gen::OneOf { variants }, v) => {
            for g in variants {
                out.extend(shrinks(g, v));
            }
        }
        _ => {}
    }
    out.retain(|c| c != value);
    out
}

fn mutate(mutator: Mutator, gen: &Gen, value: &Value) -> Vec<(String, Value)> {
    match (mutator, value) {
        (Mutator::Empty, Value::String(_)) => vec![("empty".into(), json!(""))],
        (Mutator::Truncate, Value::String(s)) if s.chars().count() >= 2 => {
            vec![("truncate".into(), Value::String(s.chars().take(s.chars().count() / 2).collect()))]
        }
        (Mutator::ControlChars, Value::String(s)) => vec![("control".into(), Value::String(format!("{s}\u{0}\n\t")))],
        (Mutator::Boundary, _) => match gen {
            Gen::Int { min, max } => vec![("boundary-min".into(), json!(min)), ("boundary-max".into(), json!(max))],
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Every case of the spec, in generator order. The same spec and seed always give the same cases.
pub fn expand(spec: &CorpusSpec, store: &dyn Store, set: &str) -> Result<Vec<Case>, CorpusError> {
    if spec.schema != SCHEMA {
        return Err(CorpusError::Schema(spec.schema.clone()));
    }
    let mut cases = Vec::new();
    for g in &spec.generators {
        match g {
            GeneratorSpec::Corpus { op, files, cases: inline, .. } => {
                let mut specs = inline.clone();
                for file in files {
                    let raw = store.read_corpus_file(set, file)?;
                    let list: Vec<CaseSpec> = serde_json::from_value(raw).map_err(|e| CorpusError::BadFile { file: file.clone(), message: e.to_string() })?;
                    specs.extend(list);
                }
                cases.extend(specs.into_iter().map(|c| Case { id: c.id, op: op.clone(), row: c.row, input: c.input, tags: c.tags }));
            }
            GeneratorSpec::Seeded { name, op, count, template, fields, mutators } => {
                let mut rng = SplitMix64::new(spec.seed ^ fnv1a(name));
                for i in 0..*count {
                    let drawn: Vec<Value> = fields.iter().map(|f| draw(&f.gen, &mut rng)).collect();
                    let build = |values: &[Value]| -> Result<Value, CorpusError> {
                        let mut input = template.clone();
                        for (f, v) in fields.iter().zip(values) {
                            if !set_pointer(&mut input, &f.pointer, v.clone()) {
                                return Err(CorpusError::BadPointer { generator: name.clone(), pointer: f.pointer.clone() });
                            }
                        }
                        Ok(input)
                    };
                    let base = format!("{name}-{i:04}");
                    cases.push(Case { id: base.clone(), op: op.clone(), row: None, input: build(&drawn)?, tags: vec!["seeded".into()] });
                    for m in mutators {
                        for (fi, f) in fields.iter().enumerate() {
                            for (label, v) in mutate(*m, &f.gen, &drawn[fi]) {
                                let mut values = drawn.clone();
                                values[fi] = v;
                                cases.push(Case {
                                    id: format!("{base}.{label}.{fi}"),
                                    op: op.clone(),
                                    row: None,
                                    input: build(&values)?,
                                    tags: vec!["seeded".into(), "mutated".into()],
                                });
                            }
                        }
                    }
                }
            }
            GeneratorSpec::Property { name, op, count, template, fields, shrink_steps } => {
                let mut rng = SplitMix64::new(spec.seed ^ fnv1a(name));
                let build = |values: &[Value]| -> Result<Value, CorpusError> {
                    let mut input = template.clone();
                    for (f, v) in fields.iter().zip(values) {
                        if !set_pointer(&mut input, &f.pointer, v.clone()) {
                            return Err(CorpusError::BadPointer { generator: name.clone(), pointer: f.pointer.clone() });
                        }
                    }
                    Ok(input)
                };
                for i in 0..*count {
                    let drawn: Vec<Value> = fields.iter().map(|f| draw(&f.gen, &mut rng)).collect();
                    let base = format!("{name}-{i:04}");
                    cases.push(Case { id: base.clone(), op: op.clone(), row: None, input: build(&drawn)?, tags: vec![TAG_PROPERTY.into()] });
                    let ladder = fields.iter().enumerate().flat_map(|(fi, f)| shrinks(&f.gen, &drawn[fi]).into_iter().map(move |v| (fi, v)));
                    for (k, (fi, v)) in ladder.take(*shrink_steps).enumerate() {
                        let mut values = drawn.clone();
                        values[fi] = v;
                        cases.push(Case {
                            id: format!("{base}.shrink.{k}"),
                            op: op.clone(),
                            row: None,
                            input: build(&values)?,
                            tags: vec![TAG_PROPERTY.into(), TAG_SHRINK.into(), format!("{TAG_SHRINK_OF}{base}")],
                        });
                    }
                }
            }
        }
    }
    let mut seen = BTreeSet::new();
    for c in &cases {
        if !seen.insert(c.id.as_str()) {
            return Err(CorpusError::DuplicateId(c.id.clone()));
        }
    }
    Ok(cases)
}

/// The hash a cassette header records: of the expanded cases, so an edited file or a moved seed shows as a stale corpus.
pub fn corpus_hash(cases: &[Case]) -> String {
    hash_value(&serde_json::to_value(cases).expect("cases serialise"))
}
