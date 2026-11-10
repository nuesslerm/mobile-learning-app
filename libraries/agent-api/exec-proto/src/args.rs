//! How an action's `args` object is read: the checks every family's parse shares, each failing `INVALID_ARGS` with the key it names.
use crate::{ExecError, ExecResult};
use serde_json::Value;

fn bad(message: String) -> ExecError {
    ExecError::invalid_args(message)
}

pub fn string(args: &Value, key: &str) -> ExecResult<String> {
    args[key].as_str().map(str::to_string).ok_or_else(|| bad(format!("`{key}` must be a string")))
}

pub fn opt_string(args: &Value, key: &str) -> ExecResult<Option<String>> {
    match &args[key] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.clone())),
        _ => Err(bad(format!("`{key}` must be a string"))),
    }
}

pub fn opt_uint(args: &Value, key: &str) -> ExecResult<Option<u64>> {
    match &args[key] {
        Value::Null => Ok(None),
        v => v.as_u64().map(Some).ok_or_else(|| bad(format!("`{key}` must be a non-negative integer"))),
    }
}

/// A flag that is absent or not a boolean is off.
pub fn flag(args: &Value, key: &str) -> bool {
    args[key].as_bool().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn given_arguments_of_the_wrong_kind_when_read_then_each_failure_names_its_key_and_absent_optionals_are_none() {
        // Given an args object with a number where a string belongs and a string where a count belongs
        let args = json!({"path": 5, "limit": "ten", "depth": -1, "text": "x", "on": true});
        // When each is read, Then a wrong kind is INVALID_ARGS naming the key
        for (result, key) in [(string(&args, "path").map(drop), "path"), (opt_string(&args, "path").map(drop), "path"), (opt_uint(&args, "limit").map(drop), "limit"), (opt_uint(&args, "depth").map(drop), "depth")] {
            let e = result.unwrap_err();
            assert!(e.code == crate::Code::InvalidArgs && e.message.contains(&format!("`{key}`")), "{key}: {e:?}");
        }
        // And an absent optional is none, a flag that is absent is off, and the right kinds read back
        assert_eq!((opt_string(&args, "gone").unwrap(), opt_uint(&args, "gone").unwrap(), flag(&args, "gone"), flag(&args, "on")), (None, None, false, true));
        assert_eq!((string(&args, "text").unwrap(), opt_uint(&json!({"n": 3}), "n").unwrap()), ("x".to_string(), Some(3)));
    }
}
