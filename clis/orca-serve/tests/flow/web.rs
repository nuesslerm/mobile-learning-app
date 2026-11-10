use crate::support::*;
use orca_serve::adapters::DirBundle;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

use orca_map_mobile::testing::Recorded;

fn goldens() -> Value {
    Recorded::load().goldens()
}

fn hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

fn serving(dir: &Path) -> Harness {
    Harness::start_with_bundle(recorded_status(), Box::new(DirBundle(dir.to_path_buf())))
}

/// A fresh bundle the test may damage, generated from the recorded set.
fn copy_of_bundle() -> tempfile::TempDir {
    Recorded::load().bundle()
}

/// What is wrong with a manifest, and the edit that makes it so.
type Edit<'a> = (&'a str, Box<dyn Fn(&mut Value)>);

fn base64_decode(text: &str) -> Vec<u8> {
    orca_crypto::b64::decode_canonical(text).expect("canonical base64")
}

fn gunzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data).read_to_end(&mut out).expect("a gzip window");
    out
}

fn error_message(reply: &Value) -> String {
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(reply["error"]["code"], "invalid_argument", "{reply}");
    reply["error"]["message"].as_str().unwrap().to_string()
}

#[test]
fn given_a_bundle_when_the_phone_asks_for_the_manifest_then_it_is_the_one_the_pinned_schemas_accepted_and_the_capability_is_advertised() {
    // Given a host that holds the recorded bundle and a paired phone
    let bundle = copy_of_bundle();
    let h = serving(bundle.path());
    let mut phone = paired(&h);
    // When it asks for the manifest and the status
    let manifest = phone.call("mobileWeb.bundle.manifest", json!({}));
    let status = phone.call("status.get", json!({}));
    // Then the reply is the recorded one, and the status says the bundle is there
    assert_eq!(manifest["result"], goldens()["manifestResult"]);
    assert!(status["result"]["capabilities"].as_array().unwrap().contains(&json!("mobileWeb.bundle.v1")), "{status}");
}

#[test]
fn given_every_window_of_every_asset_when_fetched_as_chunks_and_as_ranges_then_the_bytes_reassemble_to_the_hash_the_manifest_names() {
    // Given a phone and the windows the pinned schemas accepted for each asset
    let bundle = copy_of_bundle();
    let h = serving(bundle.path());
    let mut phone = paired(&h);
    let g = goldens();
    let build = g["manifestResult"]["manifest"]["buildId"].clone();
    let mut chunks: std::collections::BTreeMap<String, Vec<u8>> = Default::default();
    let mut ranges: std::collections::BTreeMap<String, Vec<u8>> = Default::default();
    let mut encodings = std::collections::BTreeSet::new();
    // When it reads each window both ways
    for w in g["windows"].as_array().unwrap() {
        let params = json!({"buildId": build, "path": w["path"], "offset": w["offset"]});
        let chunk = phone.call("mobileWeb.bundle.chunk", params.clone())["result"].clone();
        let range = phone.call("mobileWeb.bundle.range", params)["result"].clone();
        // Then each reply restates the window as the golden does, and its bytes are the window's
        for reply in [&chunk, &range] {
            for field in ["buildId", "path", "offset", "assetByteLength", "sha256", "eof"] {
                assert_eq!(reply[field], w[field], "{field} of {w}");
            }
        }
        let piece = base64_decode(chunk["dataBase64"].as_str().unwrap());
        assert_eq!((piece.len() as u64, hex(&piece)), (w["windowLength"].as_u64().unwrap(), w["windowSha256"].as_str().unwrap().to_string()));
        let encoding = range["encoding"].as_str().unwrap().to_string();
        let wire = base64_decode(range["dataBase64"].as_str().unwrap());
        let decoded = if encoding == "gzip" { gunzip(&wire) } else { wire.clone() };
        assert!(encoding == "identity" || wire.len() < decoded.len(), "gzip is only used when it shrinks the window");
        assert_eq!(decoded, piece, "a range carries the same bytes as the chunk");
        encodings.insert(encoding);
        let path = w["path"].as_str().unwrap().to_string();
        chunks.entry(path.clone()).or_default().extend(piece);
        ranges.entry(path).or_default().extend(decoded);
    }
    // And the whole assets match the manifest's sizes and hashes, and the files on disk
    for asset in g["manifestResult"]["manifest"]["assets"].as_array().unwrap() {
        let path = asset["path"].as_str().unwrap();
        assert_eq!(hex(&chunks[path]), asset["sha256"].as_str().unwrap(), "{path}");
        assert_eq!(chunks[path].len() as u64, asset["byteLength"].as_u64().unwrap());
        assert_eq!(chunks[path], ranges[path]);
        assert_eq!(chunks[path], std::fs::read(bundle.path().join(path)).unwrap());
    }
    assert_eq!(encodings, ["gzip", "identity"].iter().map(|s| s.to_string()).collect(), "the recorded bundle has a text asset that shrinks and noise that does not");
}

#[test]
fn given_requests_the_bundle_cannot_answer_when_made_then_each_gets_the_pinned_code_in_the_pinned_order() {
    // Given a phone and the recorded build
    let bundle = copy_of_bundle();
    let h = serving(bundle.path());
    let mut phone = paired(&h);
    let g = goldens();
    let build = g["manifestResult"]["manifest"]["buildId"].as_str().unwrap().to_string();
    let grid = g["chunkBytes"].as_u64().unwrap();
    let app = "assets/app.js";
    let size = g["manifestResult"]["manifest"]["assets"].as_array().unwrap().iter().find(|a| a["path"] == app).unwrap()["byteLength"].as_u64().unwrap();
    let other_build = "0".repeat(64);
    let codes: Vec<String> = g["errorCodes"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect();
    // When it asks for another build, an unknown or a not-exact path, a misaligned or past-the-end offset
    let cases = [
        (json!({"buildId": other_build, "path": "no/such.js", "offset": 0}), "mobile_web_bundle_build_changed"),
        (json!({"buildId": build, "path": "no/such.js", "offset": 0}), "mobile_web_bundle_asset_unknown"),
        (json!({"buildId": build, "path": "assets/../index.html", "offset": 0}), "Invalid path"),
        (json!({"buildId": build, "path": "Assets/app.js", "offset": 0}), "mobile_web_bundle_asset_unknown"),
        (json!({"buildId": build, "path": app, "offset": 1}), "mobile_web_bundle_offset_invalid"),
        (json!({"buildId": build, "path": app, "offset": size.div_ceil(grid) * grid}), "mobile_web_bundle_offset_invalid"),
        (json!({"buildId": build, "path": "assets/empty.txt", "offset": grid}), "mobile_web_bundle_offset_invalid"),
        (json!({"buildId": build, "path": app, "offset": 0, "extra": true}), "Unrecognized key"),
        (json!({"buildId": "not-a-hash", "path": app, "offset": 0}), "Invalid buildId"),
        (json!({"buildId": build, "path": app, "offset": -1}), "Invalid offset"),
    ];
    // Then each is refused with its own code, and every code of the pinned contract the cases name is one the contract lists
    for (params, expect) in cases {
        for method in ["mobileWeb.bundle.chunk", "mobileWeb.bundle.range"] {
            let message = error_message(&phone.call(method, params.clone()));
            assert!(message.starts_with(expect), "{method} {params}: {message}");
            if expect.starts_with("mobile_web_bundle_") {
                assert!(codes.contains(&message), "{message} is a code of the pinned contract");
            }
        }
    }
    // And the empty asset is fetched at offset zero, which a past-the-end offset on a non-empty asset is not
    let empty = phone.call("mobileWeb.bundle.chunk", json!({"buildId": build, "path": "assets/empty.txt", "offset": 0}));
    assert_eq!((empty["result"]["eof"].clone(), empty["result"]["dataBase64"].clone()), (json!(true), json!("")));
}

#[test]
fn given_an_asset_changed_on_disk_after_the_manifest_when_a_window_is_read_then_it_is_asset_changed_and_no_byte_of_it_is_sent() {
    // Given a host serving a copy of the bundle whose script was edited after the manifest was written
    let copy = copy_of_bundle();
    let g = goldens();
    let build = g["manifestResult"]["manifest"]["buildId"].clone();
    let mut script = std::fs::read(copy.path().join("assets/app.js")).unwrap();
    script[10] ^= 1;
    std::fs::write(copy.path().join("assets/app.js"), &script).unwrap();
    let h = serving(copy.path());
    let mut phone = paired(&h);
    // When the phone reads that asset and an untouched one
    let damaged = phone.call("mobileWeb.bundle.chunk", json!({"buildId": build, "path": "assets/app.js", "offset": 0}));
    let intact = phone.call("mobileWeb.bundle.chunk", json!({"buildId": build, "path": "index.html", "offset": 0}));
    // Then the first is refused as changed, with the error alone, and the second is served
    assert_eq!(error_message(&damaged), "mobile_web_bundle_asset_changed");
    assert!(damaged["result"].is_null());
    assert_eq!(intact["ok"], true);
}

#[test]
fn given_a_host_with_no_bundle_or_a_broken_manifest_when_asked_then_it_is_unavailable_and_the_capability_is_not_advertised() {
    // Given a host with no bundle, and one whose manifest names a size the files do not add up to
    let none = harness();
    let broken = copy_of_bundle();
    let mut manifest: Value = serde_json::from_str(&std::fs::read_to_string(broken.path().join("manifest.json")).unwrap()).unwrap();
    manifest["totalBytes"] = json!(manifest["totalBytes"].as_u64().unwrap() + 1);
    std::fs::write(broken.path().join("manifest.json"), manifest.to_string()).unwrap();
    let wrong = serving(broken.path());
    for h in [&none, &wrong] {
        // When the phone asks for the manifest, a window and the status
        let mut phone = paired(h);
        let build = "a".repeat(64);
        let manifest = phone.call("mobileWeb.bundle.manifest", json!({}));
        let chunk = phone.call("mobileWeb.bundle.chunk", json!({"buildId": build, "path": "index.html", "offset": 0}));
        let status = phone.call("status.get", json!({}));
        // Then each says unavailable and the status does not promise the bundle
        assert_eq!(error_message(&manifest), "mobile_web_bundle_unavailable");
        assert_eq!(error_message(&chunk), "mobile_web_bundle_unavailable");
        assert!(!status["result"]["capabilities"].as_array().unwrap().contains(&json!("mobileWeb.bundle.v1")), "{status}");
    }
}

#[test]
fn given_a_host_that_relays_to_no_other_server_when_the_phone_lists_hosts_then_the_list_is_empty_and_the_params_are_checked_as_the_pin_checks_them() {
    // Given a paired phone
    let h = harness();
    let mut phone = paired(&h);
    // When it lists the relay hosts, asks for one's worktrees and sends params the strict schema refuses
    let list = phone.call("mobileRelay.hosts.list", json!({}));
    let worktrees = phone.call("mobileRelay.hosts.worktrees", json!({"hostId": "runtime:abc", "afterSnapshotId": null, "acceptRowDelta": true}));
    let refused = [json!({}), json!({"hostId": ""}), json!({"hostId": "runtime:abc", "acceptRowDelta": false}), json!({"hostId": "runtime:abc", "surprise": 1})];
    // Then the list is empty, the worktrees are none, and each malformed request is an invalid argument
    assert_eq!(list["result"], json!({"hosts": []}));
    assert_eq!(worktrees["result"], json!({"worktrees": null}));
    for params in refused {
        let reply = phone.call("mobileRelay.hosts.worktrees", params.clone());
        assert_eq!((reply["ok"].clone(), reply["error"]["code"].clone()), (json!(false), json!("invalid_argument")), "{params}");
    }
}

#[test]
fn given_a_manifest_that_breaks_one_pinned_rule_when_the_host_loads_it_then_no_bundle_is_served() {
    // Given the recorded manifest and one edit for each rule of the pinned schema it must keep
    let original: Value = Recorded::load().manifest_result()["manifest"].clone();
    let edits: Vec<Edit> = vec![
        ("a build id that is not the content hash", Box::new(|m| m["buildId"] = json!("f".repeat(64)))),
        ("a changed byte length", Box::new(|m| m["assets"][1]["byteLength"] = json!(m["assets"][1]["byteLength"].as_u64().unwrap() + 1))),
        ("assets out of order", Box::new(|m| m["assets"].as_array_mut().unwrap().reverse())),
        ("an unknown key", Box::new(|m| m["surprise"] = json!(true))),
        ("an inverted protocol window", Box::new(|m| m["minCompatibleRuntimeProtocolVersion"] = json!(m["runtimeProtocolVersion"].as_u64().unwrap() + 1))),
        ("another entrypoint", Box::new(|m| m["entrypoint"] = json!("assets/app.js"))),
        ("a path that climbs out of the bundle", Box::new(|m| m["assets"][0]["path"] = json!("../manifest.json"))),
    ];
    for (what, edit) in edits {
        let copy = copy_of_bundle();
        let mut manifest = original.clone();
        edit(&mut manifest);
        std::fs::write(copy.path().join("manifest.json"), manifest.to_string()).unwrap();
        // When the phone asks for the manifest
        let h = serving(copy.path());
        let reply = paired(&h).call("mobileWeb.bundle.manifest", json!({}));
        // Then the host holds no bundle
        assert_eq!(error_message(&reply), "mobile_web_bundle_unavailable", "{what}");
    }
}
