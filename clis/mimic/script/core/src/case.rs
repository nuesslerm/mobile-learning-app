//! A case's input: `{script: [Entry], settle_ms?}`. An entry is one object with exactly one op key (`http`, `ws-open`, `ws-send`, `ws-close`)
//! beside an optional `capture` and, on a `ws-send`, an optional `await: "exit"`.
use serde_json::Value;

pub const OPS: [&str; 11] = ["http", "ws-open", "ws-send", "ws-close", "git", "connect", "request", "subscribe", "listen", "unsubscribe", "cli.script"];

/// The agent CLIs a `cli.script` can script.
pub const CLIS: [&str; 3] = ["claude", "codex", "opencode"];

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
        if input.get("requests").is_some() {
            return Case::parse_legacy(input);
        }
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
            if op.as_str() == "cli.script" {
                let cli = object[*op].get("cli").and_then(Value::as_str).unwrap_or_default();
                if !CLIS.contains(&cli) {
                    return Err(format!("script: unknown cli {cli}"));
                }
                if !object[*op].get("turns").is_some_and(Value::is_array) {
                    return Err("script: `cli.script` needs a `turns` list".into());
                }
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

    /// The Orca input of the committed sets, `{requests: [{method, params, timeout_ms, capture}], connect_timeout_ms?, setup: [{git}]}`, as
    /// entries: the repositories, one `connect`, one `request` per row whose `rid` is its index.
    fn parse_legacy(input: &Value) -> Result<Case, String> {
        let requests = input.get("requests").and_then(Value::as_array).ok_or("script: `requests` is not a list")?;
        let mut entries = Vec::new();
        for setup in input.get("setup").and_then(Value::as_array).into_iter().flatten() {
            let name = setup.get("git").and_then(Value::as_str).ok_or("script: a setup entry needs `git`")?;
            entries.push(Entry { op: "git".into(), arg: serde_json::json!({ "name": name }), capture: None, await_exit: false });
        }
        let timeout = input.get("connect_timeout_ms").and_then(Value::as_u64).unwrap_or(10_000);
        entries.push(Entry { op: "connect".into(), arg: serde_json::json!({ "timeout_ms": timeout }), capture: None, await_exit: false });
        for (i, request) in requests.iter().enumerate() {
            let method = request.get("method").and_then(Value::as_str).ok_or("script: a request needs `method`")?;
            let arg = serde_json::json!({ "method": method, "params": request.get("params").cloned().unwrap_or(Value::Null), "timeout_ms": request.get("timeout_ms").and_then(Value::as_u64).unwrap_or(10_000), "rid": i.to_string() });
            entries.push(Entry { op: "request".into(), arg, capture: request.get("capture").cloned(), await_exit: false });
        }
        Ok(Case { entries, settle_ms: None, prelude: false })
    }

    /// The entries of a case's `script` as a list, for a prelude kept as data (`prelude.json` of a set).
    pub fn entries_of(script: &Value) -> Result<Vec<Entry>, String> {
        Ok(Case::parse(&serde_json::json!({ "script": script }))?.entries)
    }

    /// The names any `capture` of the script declares: a `{name}` with one of these and no value yet is reported, never sent as text.
    pub fn declared(&self) -> Vec<String> {
        let git = self.entries.iter().filter(|e| e.op == "git").filter_map(|e| e.arg.get("name")?.as_str().map(|n| format!("git.{n}")));
        git.chain(self.entries.iter().filter_map(|e| e.capture.as_ref()?.as_object()).flat_map(|c| c.keys().cloned())).collect()
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
            (json!({"script": [{"cli.script": {"cli": "gemini", "turns": []}}]}), "unknown cli gemini"),
            (json!({"script": [{"cli.script": {"cli": "codex"}}]}), "needs a `turns` list"),
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
