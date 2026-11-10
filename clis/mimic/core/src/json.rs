//! JSON helpers every module shares: the canonical form (sorted keys, so a cassette line diff is a behaviour diff and hashes are
//! stable whatever order a producer wrote), the JSON type name the differ compares, and sha256 labels.
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub fn canonical(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let mut out = Map::new();
            for k in keys {
                out.insert(k.clone(), canonical(&m[k]));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonical).collect()),
        other => other.clone(),
    }
}

pub fn canonical_string(v: &Value) -> String {
    canonical(v).to_string()
}

pub fn sha256_label(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::from("sha256:");
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub fn hash_value(v: &Value) -> String {
    sha256_label(canonical_string(v).as_bytes())
}

pub fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

/// Set the value at an RFC 6901 pointer, creating the last object key when missing; false when the parent does not exist.
pub fn set_pointer(root: &mut Value, pointer: &str, value: Value) -> bool {
    if pointer.is_empty() {
        *root = value;
        return true;
    }
    let Some(cut) = pointer.rfind('/') else {
        return false;
    };
    let (parent_ptr, last) = (&pointer[..cut], unescape(&pointer[cut + 1..]));
    let parent = if parent_ptr.is_empty() {
        Some(root)
    } else {
        root.pointer_mut(parent_ptr)
    };
    match parent {
        Some(Value::Object(m)) => {
            m.insert(last, value);
            true
        }
        Some(Value::Array(a)) => match last.parse::<usize>() {
            Ok(i) if i < a.len() => {
                a[i] = value;
                true
            }
            _ => false,
        },
        _ => false,
    }
}
