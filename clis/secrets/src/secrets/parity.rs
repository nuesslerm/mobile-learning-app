//! `wf secret parity`: per CI scope, which registry keys the scope's vault project holds and a nonce-keyed hash of each value, so two
//! runner kinds can run it with the same `--nonce` and compare the reports without either printing a value.
use super::backend::Backend;
use super::registry::{Entry, Scope};
use hmac::{Hmac, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const SCOPES: [Scope; 2] = [Scope::CiOpen, Scope::CiDeploy];

pub fn fresh_nonce() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(hex::encode(bytes))
}

fn mac_hex(nonce: &str, key: &str, value: &str) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(nonce.as_bytes()).expect("hmac accepts any key length");
    mac.update(key.as_bytes());
    mac.update(&[0]);
    mac.update(value.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn names_hash(names: &[String]) -> String {
    hex::encode(Sha256::digest(names.join("\n").as_bytes()))
}

/// A key only ever delivered as optional (no deploy group requires it): absent from the vault, the app falls back, so it does not fail parity.
fn optional_only(key: &str) -> bool {
    let groups = &*super::groups::GROUPS;
    groups.iter().any(|g| g.optional.iter().any(|k| k == key)) && !groups.iter().any(|g| g.required.iter().any(|k| k == key))
}

/// The report and whether every registry key of both scopes is held (an optional-only key the vault lacks is listed as `missingOptional`). Values are hashed here and never leave this function.
pub fn report(registry: &[Entry], backend: &mut dyn Backend, nonce: &str) -> Result<(Value, bool), String> {
    let mut scopes = Vec::new();
    let mut ok = true;
    for scope in SCOPES {
        let (mut expected, mut held, mut missing, mut missing_optional) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut values = serde_json::Map::new();
        for entry in registry.iter().filter(|e| e.scope == scope && !e.bootstrap) {
            expected.push(entry.key.clone());
            match backend.resolve(entry)?.filter(|v| !v.is_empty()) {
                Some(value) => {
                    values.insert(entry.key.clone(), json!(mac_hex(nonce, &entry.key, &value)));
                    held.push(entry.key.clone());
                }
                None if optional_only(&entry.key) => missing_optional.push(entry.key.clone()),
                None => missing.push(entry.key.clone()),
            }
        }
        for list in [&mut expected, &mut held, &mut missing, &mut missing_optional] {
            list.sort();
        }
        let mut sorted: Vec<(String, Value)> = values.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        ok &= missing.is_empty();
        scopes.push(json!({
            "scope": scope.name(),
            "registry": expected,
            "held": held,
            "missing": missing,
            "missingOptional": missing_optional,
            "registryHash": names_hash(&expected),
            "heldHash": names_hash(&held),
            "valueHashes": sorted.into_iter().collect::<serde_json::Map<String, Value>>(),
        }));
    }
    Ok((json!({ "ok": ok, "nonce": nonce, "scopes": scopes }), ok))
}
