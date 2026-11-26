//! The vocabulary of the proxy: the names of the pinned T3 wire, taken from the fixtures that hold it and checked against what the
//! proxy knows. A name the fixtures use that the proxy does not know would be masked, so the pin keeps the shape of the traffic
//! readable.
use crate::support::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use wf_mimic::proxy_shape;

/// The root of the repository.
fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").to_path_buf()
}

/// Every file under a directory.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(files(&path));
        } else {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The JSON object keys of a value and of everything under it.
fn keys_of(value: &Value, into: &mut BTreeSet<String>) {
    match value {
        Value::Object(fields) => {
            for (name, item) in fields {
                into.insert(name.clone());
                keys_of(item, into);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| keys_of(item, into)),
        _ => {}
    }
}

/// The names of one HTTP interaction of a fixture: the verb, the header names, the keys of the body and the form keys.
fn http_names(method: &str, headers: &Value, body: &Value, keys: &mut BTreeSet<String>, forms: &mut BTreeSet<String>, verbs: &mut BTreeSet<String>, headers_out: &mut BTreeSet<String>) {
    if !method.is_empty() {
        verbs.insert(method.to_uppercase());
    }
    if let Some(fields) = headers.as_object() {
        for name in fields.keys() {
            headers_out.insert(name.to_lowercase());
        }
    }
    let form = headers.get("content-type").and_then(Value::as_str).is_some_and(|t| t.contains("x-www-form-urlencoded"));
    if form {
        if let Some(fields) = body.as_object() {
            forms.extend(fields.keys().cloned());
        }
    } else {
        keys_of(body, keys);
    }
}

/// The names of one frame of a fixture: the tag, the method and the keys.
fn frame_names(frame: &Value, keys: &mut BTreeSet<String>, methods: &mut BTreeSet<String>, tags: &mut BTreeSet<String>) {
    if let Some(tag) = frame.get("_tag").and_then(Value::as_str) {
        tags.insert(tag.to_string());
    }
    if let Some(method) = frame.get("tag").and_then(Value::as_str) {
        methods.insert(method.to_string());
    }
    keys_of(frame, keys);
}

/// The names the pinned fixtures use: the JSON keys, the header names, the verbs, the form keys, the RPC methods and the tags.
#[derive(Default)]
struct Names {
    keys: BTreeSet<String>,
    headers: BTreeSet<String>,
    verbs: BTreeSet<String>,
    forms: BTreeSet<String>,
    methods: BTreeSet<String>,
    tags: BTreeSet<String>,
}

/// The goldens of the conformance suite: the pinned wire of the T3.
fn from_goldens(root: &Path, names: &mut Names) {
    for path in files(&root.join("tooling/t3-conformance/goldens")) {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
            if entry.get("kind").and_then(Value::as_str) == Some("http") {
                let method = entry.get("method").and_then(Value::as_str).unwrap_or("");
                http_names(method, entry.get("reqHeaders").unwrap_or(&Value::Null), entry.get("reqBody").unwrap_or(&Value::Null), &mut names.keys, &mut names.forms, &mut names.verbs, &mut names.headers);
                http_names(method, entry.get("resHeaders").unwrap_or(&Value::Null), entry.get("resBody").unwrap_or(&Value::Null), &mut names.keys, &mut names.forms, &mut names.verbs, &mut names.headers);
            } else if let Some(frame) = entry.get("frame") {
                frame_names(frame, &mut names.keys, &mut names.methods, &mut names.tags);
            }
        }
    }
}

/// The recorded cassettes of the T3 server.
fn from_cassettes(root: &Path, names: &mut Names) {
    for path in files(&root.join("clis/t3-serve/cassettes")) {
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
            let Some(steps) = entry.get("steps").and_then(Value::as_array) else { continue };
            for step in steps {
                let payload = step.get("payload").unwrap_or(&Value::Null);
                match step.get("channel").and_then(Value::as_str) {
                    Some("http") => http_names(payload.get("method").and_then(Value::as_str).unwrap_or(""), payload.get("headers").unwrap_or(&Value::Null), payload.get("body").unwrap_or(&Value::Null), &mut names.keys, &mut names.forms, &mut names.verbs, &mut names.headers),
                    Some("ws") => frame_names(payload, &mut names.keys, &mut names.methods, &mut names.tags),
                    _ => {}
                }
            }
        }
    }
}

/// The catalog of the T3 API: the methods of the wire and the verbs of the routes.
fn from_catalog(root: &Path, names: &mut Names) {
    let path = root.join("clis/t3-serve/cassettes/t3-api/inventory.json");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let Ok(catalog) = serde_json::from_str::<Value>(&text) else { return };
    for endpoint in catalog["endpoints"].as_array().cloned().unwrap_or_default() {
        match endpoint["kind"].as_str() {
            Some("rpc") => {
                names.methods.insert(endpoint["name"].as_str().unwrap_or_default().to_string());
            }
            Some("http") => {
                names.verbs.insert(endpoint["method"].as_str().unwrap_or_default().to_uppercase());
            }
            _ => {}
        }
    }
}

/// The scope table of the contracts: the methods the server answers for real.
fn from_contracts(root: &Path, names: &mut Names) {
    let path = root.join("clis/t3-serve/contracts/src/rpc.rs");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let Some((_, rest)) = text.split_once("IMPLEMENTED_RPCS") else { return };
    let Some((block, _)) = rest.split_once('[').and_then(|(_, rest)| rest.split_once(']').map(|(block, _)| (block, ()))) else { return };
    for name in block.split('"').skip(1).step_by(2) {
        names.methods.insert(name.to_string());
    }
}

/// The names the pinned fixtures use.
fn extracted() -> Names {
    let root = repo();
    let mut names = Names::default();
    from_goldens(&root, &mut names);
    from_cassettes(&root, &mut names);
    from_catalog(&root, &mut names);
    from_contracts(&root, &mut names);
    names
}

#[test]
fn every_name_the_pinned_fixtures_use_is_one_the_proxy_knows() {
    // Given the names the pinned T3 fixtures use
    let names = extracted();
    assert!(names.keys.len() > 100, "{} keys", names.keys.len());
    assert!(names.methods.len() > 100, "{} methods", names.methods.len());
    // Then the proxy knows every one of them, so none of them is masked away
    let unknown: Vec<&String> = names.keys.iter().filter(|k| !proxy_shape::known_key(k)).collect();
    assert!(unknown.is_empty(), "the proxy masks keys the fixtures use: {unknown:?}");
    let unknown: Vec<&String> = names.headers.iter().filter(|k| !proxy_shape::known_header(k)).collect();
    assert!(unknown.is_empty(), "the proxy masks header names the fixtures use: {unknown:?}");
    let unknown: Vec<&String> = names.verbs.iter().filter(|v| !proxy_shape::known_verb(v)).collect();
    assert!(unknown.is_empty(), "the proxy masks verbs the fixtures use: {unknown:?}");
    let unknown: Vec<&String> = names.forms.iter().filter(|f| !proxy_shape::known_form(f)).collect();
    assert!(unknown.is_empty(), "the proxy masks form keys the fixtures use: {unknown:?}");
    let unknown: Vec<&String> = names.methods.iter().filter(|m| !proxy_shape::known_method(m)).collect();
    assert!(unknown.is_empty(), "the proxy masks methods the fixtures use: {unknown:?}");
    let unknown: Vec<&String> = names.tags.iter().filter(|t| !proxy_shape::known_tag(t)).collect();
    assert!(unknown.is_empty(), "the proxy masks tags the fixtures use: {unknown:?}");
}

#[test]
fn a_name_the_fixtures_do_not_use_is_masked_and_the_names_they_do_use_are_kept() {
    // Given a name of the traffic
    for name in ["the-app-token", "the-user's-own-text", "x-the-pairing-code"] {
        // Then the proxy does not know it
        assert!(!proxy_shape::known_key(name), "{name}");
        assert!(!proxy_shape::known_header(name), "{name}");
        assert!(!proxy_shape::known_method(name), "{name}");
        assert!(!proxy_shape::known_tag(name), "{name}");
        assert!(!proxy_shape::known_verb(name), "{name}");
        assert!(!proxy_shape::known_form(name), "{name}");
        assert!(!proxy_shape::known_query(name), "{name}");
    }
    // And the names of the pinned wire are kept
    assert!(proxy_shape::known_key("threadId"));
    assert!(proxy_shape::known_header("authorization"));
    assert!(proxy_shape::known_method("orchestration.dispatchCommand"));
    assert!(proxy_shape::known_tag("Chunk"));
    assert!(proxy_shape::known_verb("POST"));
    assert!(proxy_shape::known_form("grant_type"));
    assert!(proxy_shape::known_query("wsTicket"));
}
