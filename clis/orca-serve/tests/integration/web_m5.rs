//! The phone fetches its web page from the host through the chain: the manifest, then every window of every asset as a chunk and as a range,
//! and the bytes it reassembles are the files' (the recorded set `map-mobile/fixtures/orca-mobile-bundle`: the replies the pinned strict schemas accepted, the files generated from it).
use crate::rig::*;
use orca_crypto::b64;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use orca_map_mobile::testing::Recorded;

fn goldens() -> Value {
    Recorded::load().goldens()
}

fn hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_phone_fetches_the_manifest_and_every_asset_in_chunks_and_ranges_and_the_bytes_are_the_hosts_files() {
    exec_testkit::case!(SUITE, |case| {
        // Given the chain with a host holding the recorded bundle, and the manifest the phone reads
        let bundle = Recorded::load().bundle();
        let chain = Chain::start(Some(bundle.path()));
        let mut phone = Phone::connect(case, &chain);
        let g = goldens();
        let manifest = phone.call(case, &chain, &["rpc:mobileWeb.bundle.manifest"], "mobileWeb.bundle.manifest", json!({}));
        assert_eq!(manifest["result"], g["manifestResult"], "{}", chain.evidence());
        let build = manifest["result"]["manifest"]["buildId"].clone();
        let (mut chunked, mut ranged): (std::collections::BTreeMap<String, Vec<u8>>, std::collections::BTreeMap<String, Vec<u8>>) = Default::default();
        // When it reads every window of every asset both ways
        for w in g["windows"].as_array().unwrap() {
            let params = json!({"buildId": build, "path": w["path"], "offset": w["offset"]});
            let chunk = phone.call(case, &chain, &["rpc:mobileWeb.bundle.chunk"], "mobileWeb.bundle.chunk", params.clone());
            let range = phone.call(case, &chain, &["rpc:mobileWeb.bundle.range"], "mobileWeb.bundle.range", params);
            let piece = b64::decode_canonical(chunk["result"]["dataBase64"].as_str().unwrap()).unwrap();
            let wire = b64::decode_canonical(range["result"]["dataBase64"].as_str().unwrap()).unwrap();
            let mut decoded = Vec::new();
            if range["result"]["encoding"] == "gzip" {
                flate2::read::GzDecoder::new(wire.as_slice()).read_to_end(&mut decoded).unwrap();
            } else {
                decoded = wire;
            }
            let path = w["path"].as_str().unwrap().to_string();
            chunked.entry(path.clone()).or_default().extend(piece);
            ranged.entry(path).or_default().extend(decoded);
        }
        // Then each asset reassembles to the size and hash the manifest names, by chunks and by ranges alike, and to the host's file
        for asset in manifest["result"]["manifest"]["assets"].as_array().unwrap() {
            let path = asset["path"].as_str().unwrap();
            assert_eq!(hex(&chunked[path]), asset["sha256"].as_str().unwrap(), "{path} by chunks");
            assert_eq!(hex(&ranged[path]), asset["sha256"].as_str().unwrap(), "{path} by ranges");
            assert_eq!(chunked[path].len() as u64, asset["byteLength"].as_u64().unwrap());
            assert_eq!(chunked[path], std::fs::read(bundle.path().join(path)).unwrap());
        }
    });
}

#[test]
fn a_window_the_bundle_cannot_answer_is_refused_with_its_code_and_a_host_with_no_bundle_says_unavailable() {
    exec_testkit::case!(SUITE, |case| {
        // Given one chain holding the recorded bundle and one holding none
        let bundle = Recorded::load().bundle();
        let with = Chain::start(Some(bundle.path()));
        let without = Chain::start(None);
        let (mut a, mut b) = (Phone::connect(case, &with), Phone::connect(case, &without));
        let build = goldens()["manifestResult"]["manifest"]["buildId"].clone();
        // When the first is asked for a path outside the manifest and the second for its manifest
        let unknown = a.refused(case, &with, &["rpc:mobileWeb.bundle.chunk"], "mobileWeb.bundle.chunk", json!({"buildId": build, "path": "assets/missing.js", "offset": 0}));
        let none = b.refused(case, &without, &["rpc:mobileWeb.bundle.manifest"], "mobileWeb.bundle.manifest", json!({}));
        // Then each is refused with the code the pinned host answers
        assert_eq!((unknown["error"]["code"].clone(), unknown["error"]["message"].clone()), (json!("invalid_argument"), json!("mobile_web_bundle_asset_unknown")));
        assert_eq!(none["error"]["message"], "mobile_web_bundle_unavailable");
    });
}

#[test]
fn a_phone_lists_the_hosts_this_host_relays_to_and_there_are_none() {
    exec_testkit::case!(SUITE, |case| {
        // Given a connected phone
        let chain = Chain::start(None);
        let mut phone = Phone::connect(case, &chain);
        // When it lists the relay hosts and asks for one's worktrees
        let hosts = phone.call(case, &chain, &["rpc:mobileRelay.hosts.list"], "mobileRelay.hosts.list", json!({}));
        let worktrees = phone.call(case, &chain, &["rpc:mobileRelay.hosts.worktrees"], "mobileRelay.hosts.worktrees", json!({"hostId": "runtime:none"}));
        // Then there are no hosts and no rows, which is what the pinned host answers when no desktop relays to a server
        assert_eq!(hosts["result"], json!({"hosts": []}));
        assert_eq!(worktrees["result"], json!({"worktrees": null}));
    });
}
