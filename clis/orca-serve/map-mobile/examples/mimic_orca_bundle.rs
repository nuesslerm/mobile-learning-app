//! The candidate of the `orca-mobile-bundle` set: the host's `mobileWeb.bundle.chunk` behind a `mimic-driver/1` process. It answers
//! `bundle.windows` by serving each window of the recorded bundle through a real `Host` and connection (the wire path a phone takes) and
//! says what came back in the shape the cassette recorded; the ops the Rust host has no counterpart of (the manifest builder, the zod
//! schema probes, the error-code list) are refused. `--tweak` plants a defect (the last window of an asset forgets its `eof`).
use base64::Engine;
use orca_core::testing::{RevocablePrincipals, ScriptedAgent};
use orca_core::{Host, HostInfo, ServiceStatus};
use orca_map_mobile::testing::generate;
use orca_map_mobile::{BundleSource, MobileWebArea, CHUNK, WINDOW_BYTES};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::sync::Arc;

fn hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

/// The bundle held in memory: its `manifest.json` and the bytes of each asset.
struct Held {
    manifest: Value,
    files: BTreeMap<String, Vec<u8>>,
}

impl BundleSource for Held {
    fn manifest(&self) -> Result<Option<Value>, String> {
        Ok(Some(self.manifest.clone()))
    }

    fn digest(&self, path: &str) -> Result<(u64, String), String> {
        let data = self.files.get(path).ok_or_else(|| format!("no asset {path}"))?;
        Ok((data.len() as u64, hex(data)))
    }

    fn read(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, String> {
        let data = self.files.get(path).ok_or_else(|| format!("no asset {path}"))?;
        Ok(data.iter().skip(offset as usize).take(len).copied().collect())
    }
}

/// The manifest a desktop would build for `input`'s files: assets by path, the id the phone recomputes.
fn bundle(input: &Value) -> Result<Held, String> {
    let mut files = BTreeMap::new();
    let mut types = BTreeMap::new();
    for file in input["files"].as_array().ok_or("input.files is not a list")? {
        let path = file["path"].as_str().ok_or("a file without a path")?.to_string();
        types.insert(path.clone(), file["type"].as_str().ok_or("a file without a type")?.to_string());
        files.insert(path, generate(&file["gen"]));
    }
    let assets: Vec<Value> = files.iter().map(|(p, d)| json!({"path": p, "sha256": hex(d), "byteLength": d.len(), "contentType": types[p]})).collect();
    let build_id = hex(Value::Array(assets.clone()).to_string().as_bytes());
    let manifest = json!({
        "schemaVersion": 1, "buildId": build_id, "desktopVersion": "0.0.0",
        "minCompatibleRuntimeProtocolVersion": input["minCompatibleRuntimeProtocolVersion"], "runtimeProtocolVersion": input["runtimeProtocolVersion"],
        "entrypoint": input["entrypoint"], "totalBytes": files.values().map(Vec::len).sum::<usize>(), "assets": assets, "routes": input["routes"],
    });
    Ok(Held { manifest, files })
}

fn windows(input: &Value, tweak: bool) -> Result<Vec<Value>, String> {
    let held = bundle(input)?;
    let (build_id, lengths): (String, Vec<(String, usize)>) = (held.manifest["buildId"].as_str().unwrap_or_default().to_string(), held.files.iter().map(|(p, d)| (p.clone(), d.len())).collect());
    let agent = Arc::new(ScriptedAgent::serving(ServiceStatus { protocol: 1, version: "v".into(), commit: "c".into(), groups: vec![] }));
    let info = HostInfo { platform: "linux".into(), machine_name: "m".into(), app_version: "0".into() };
    let host = Host::new("rt".into(), vec![Box::new(MobileWebArea::new(Box::new(held)))], agent, Arc::new(RevocablePrincipals::default()), info)?;
    let mut conn = host.connection("device", "token", vec![]);
    let mut steps = vec![];
    for (path, len) in lengths {
        let mut offset = 0;
        while offset == 0 || offset < len {
            let frame = json!({"id": format!("{path}@{offset}"), "method": CHUNK, "params": {"buildId": build_id, "path": path, "offset": offset}}).to_string();
            let reply: Value = serde_json::from_str(conn.on_message(&frame).first().ok_or("no answer")?).map_err(|e| e.to_string())?;
            if reply["ok"] != true {
                return Err(format!("{path}@{offset}: {}", reply["error"]));
            }
            let mut payload = reply["result"].clone();
            let data = base64::engine::general_purpose::STANDARD.decode(payload["dataBase64"].as_str().ok_or("no data")?).map_err(|e| e.to_string())?;
            payload.as_object_mut().expect("a result is an object").remove("dataBase64");
            payload["windowSha256"] = json!(hex(&data));
            payload["windowLength"] = json!(data.len());
            if tweak && payload["eof"] == true {
                payload["eof"] = json!(false);
            }
            steps.push(json!({"dir": "ret", "channel": "window", "kind": "json", "rid": format!("{path}@{offset}"), "payload": payload}));
            offset += WINDOW_BYTES;
        }
    }
    Ok(steps)
}

fn main() {
    let tweak = std::env::args().any(|a| a == "--tweak");
    let out = std::io::stdout();
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.expect("stdin is text")).unwrap_or(Value::Null);
        let reply = if request.get("hello").is_some() {
            json!({"hello": {"driver": {"name": "orca-map-mobile/bundle", "version": "1", "protocol": "mimic-driver/1"}, "coverage": false}})
        } else {
            match request["op"].as_str() {
                Some("bundle.windows") => match windows(&request["input"], tweak) {
                    Ok(steps) => json!({"id": request["id"], "steps": steps, "effects": [], "outcome": {"status": "ok", "reference_accepts": true}}),
                    Err(error) => json!({"id": request["id"], "error": error}),
                },
                other => json!({"id": request["id"], "error": format!("this host has no counterpart of {}", other.unwrap_or("an unnamed op"))}),
            }
        };
        writeln!(out.lock(), "{reply}").expect("stdout is open");
    }
}
