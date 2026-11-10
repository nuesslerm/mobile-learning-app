//! The recorded exchanges of an agent API method (`tests/fixtures/<method>.json` of each group crate): the request a mapper sends and the
//! response it must get, and the executor actions the service makes to answer.
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

pub mod mimic;

/// What stands for a field that differs between builds in a comparison (the placeholder of mimic's `pointer` rule).
pub const VOLATILE: &str = "<volatile>";

/// One mimic `pointer` mask rule (`mimic-rules/1`, `kind: pointer`): the value at `pointer`, when there is one, becomes `placeholder`. The
/// loader applies it in process, because a test masks every response it compares; `tests/integration.rs` of each user crate proves it
/// byte-identical to what `wf mimic record` writes under the same rules (this workspace never depends on a `clis` crate, decision D2b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    pub pointer: String,
    pub placeholder: String,
}

impl Mask {
    pub fn apply(&self, value: &mut Value) {
        if let Some(field) = value.pointer_mut(&self.pointer) {
            *field = Value::String(self.placeholder.clone());
        }
    }

    /// The rule as `rules.json` of a mimic set spells it, against an interaction whose first step's payload is the masked value.
    pub fn mimic_rule(&self) -> Value {
        serde_json::json!({"kind": "pointer", "pointer": format!("/steps/0/payload{}", self.pointer), "placeholder": self.placeholder})
    }
}

/// The `volatile` pointers of an exchange as `pointer` rules with the shared placeholder.
pub fn pointer_masks(pointers: &[String]) -> Vec<Mask> {
    pointers.iter().map(|pointer| Mask { pointer: pointer.clone(), placeholder: VOLATILE.into() }).collect()
}

/// `value` under every rule, in order.
pub fn mask(masks: &[Mask], mut value: Value) -> Value {
    for rule in masks {
        rule.apply(&mut value);
    }
    value
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exchange {
    pub name: String,
    /// Requests sent first, on the same socket, whose answers are not compared: they put the service in the state the request needs.
    #[serde(default)]
    pub before: Vec<Value>,
    pub request: Value,
    pub response: Value,
    /// The executor actions (`tool` of the executor channel) the service makes to answer, in order; none when it answers on its own.
    #[serde(default)]
    pub actions: Vec<String>,
    /// JSON pointers into the response of fields that carry the build (a version, a commit) and are compared by presence, not value.
    #[serde(default)]
    pub volatile: Vec<String>,
}

impl Exchange {
    pub fn succeeds(&self) -> bool {
        self.response.get("result").is_some()
    }

    /// The `volatile` pointers as mask rules.
    pub fn masks(&self) -> Vec<Mask> {
        pointer_masks(&self.volatile)
    }

    /// `response` with the volatile fields replaced, so a recorded response and an actual one compare equal when only those differ. A
    /// volatile field the response lacks stays absent, so the comparison still fails.
    pub fn masked(&self, response: Value) -> Value {
        mask(&self.masks(), response)
    }

    /// The recorded response with each volatile field that `stand_in` has a value for (by pointer) set to it, for a reader that needs a
    /// typed value where the recording keeps the placeholder text.
    pub fn filled(&self, stand_in: impl Fn(&str) -> Option<Value>) -> Value {
        let mut response = self.response.clone();
        for pointer in &self.volatile {
            if let (Some(field), Some(value)) = (response.pointer_mut(pointer), stand_in(pointer)) {
                *field = value;
            }
        }
        response
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub method: String,
    pub exchanges: Vec<Exchange>,
}

/// Every `*.json` file of `dir` as a `T`, in file-name order: the one loader of the `exchanges` encoding, for every contract crate's
/// fixture type. A file that is not a `T` stops the run naming it.
pub fn load_dir<T: DeserializeOwned>(dir: &Path) -> Vec<T> {
    let mut files: Vec<_> = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())).flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    files.iter().map(|path| serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).collect()
}

/// Every `*.json` file of `dir` as an agent API fixture.
pub fn load(dir: &Path) -> Vec<Fixture> {
    load_dir(dir)
}
