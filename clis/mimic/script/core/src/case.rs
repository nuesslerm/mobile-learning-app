//! A case's input: `{script: [Entry], settle_ms?}`. An entry is one object with exactly one op key (`http`, `ws-open`, `ws-send`, `ws-close`)
//! beside an optional `capture` and, on a `ws-send`, an optional `await: "exit"`.
use serde_json::Value;

pub const OPS: [&str; 4] = ["http", "ws-open", "ws-send", "ws-close"];

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub op: String,
    pub arg: Value,
    pub capture: Option<Value>,
    pub await_exit: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub entries: Vec<Entry>,
    pub settle_ms: Option<u64>,
    /// `prelude: "session"`: the interpreter's prelude entries run before these.
    pub prelude: bool,
}

impl Case {
    /// A malformed case is a request-level error, never a recorded behaviour.
    pub fn parse(input: &Value) -> Result<Case, String> {
        let script = input.get("script").and_then(Value::as_array).ok_or("script: the input has no `script` list")?;
        let mut entries = Vec::new();
        for entry in script {
            let object = entry.as_object().ok_or("script: an entry is not an object")?;
            let ops: Vec<&String> = object.keys().filter(|k| !matches!(k.as_str(), "await" | "capture")).collect();
            let [op] = ops.as_slice() else {
                return Err(format!("script: an entry needs exactly one op, got {}", ops.len()));
            };
            if !OPS.contains(&op.as_str()) {
                return Err(format!("script: unknown script step {op}"));
            }
            let await_exit = match object.get("await") {
                None => false,
                Some(Value::String(s)) if s == "exit" => true,
                Some(other) => return Err(format!("script: unknown await {}", other.as_str().map_or_else(|| other.to_string(), String::from))),
            };
            entries.push(Entry { op: (*op).clone(), arg: object[*op].clone(), capture: object.get("capture").cloned(), await_exit });
        }
        let prelude = match input.get("prelude") {
            None | Some(Value::Null) => false,
            Some(Value::String(s)) if s == "session" => true,
            Some(other) => return Err(format!("script: unknown prelude {other}")),
        };
        Ok(Case { entries, settle_ms: input.get("settle_ms").and_then(Value::as_u64), prelude })
    }

    /// The entries of a case's `script` as a list, for a prelude kept as data (`prelude.json` of a set).
    pub fn entries_of(script: &Value) -> Result<Vec<Entry>, String> {
        Ok(Case::parse(&serde_json::json!({ "script": script }))?.entries)
    }

    /// The names any `capture` of the script declares: a `{name}` with one of these and no value yet is reported, never sent as text.
    pub fn declared(&self) -> Vec<String> {
        self.entries.iter().filter_map(|e| e.capture.as_ref()?.as_object()).flat_map(|c| c.keys().cloned()).collect()
    }

    /// Whether the script asks for a credential, so one is minted only then.
    pub fn wants_credential(&self) -> bool {
        self.entries.iter().any(|e| crate::vars::placeholders(&e.arg.to_string()).contains(&"credential"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_malformed_entries_when_parsed_then_each_is_named() {
        let cases = [
            (json!({"script": [{"capture": {}}]}), "exactly one op, got 0"),
            (json!({"script": [{"http": {}, "ws-open": {}}]}), "exactly one op, got 2"),
            (json!({"script": [{"fetch": {}}]}), "unknown script step fetch"),
            (json!({"script": [{"ws-send": {}, "await": "quiet"}]}), "unknown await quiet"),
            (json!({"nothing": 1}), "no `script` list"),
        ];
        for (input, said) in cases {
            assert!(Case::parse(&input).unwrap_err().contains(said), "{input}");
        }
    }

    #[test]
    fn given_a_script_when_parsed_then_the_op_is_the_key_beside_capture_and_await() {
        let case = Case::parse(&json!({"script": [{"ws-send": {"id": "1"}, "await": "exit", "capture": {"run": {"from": ["/x"]}}}], "settle_ms": 400})).unwrap();

        assert_eq!((case.entries[0].op.as_str(), case.entries[0].await_exit, case.settle_ms), ("ws-send", true, Some(400)));
        assert_eq!(case.declared(), ["run"]);
        assert!(!case.wants_credential());
    }
}
