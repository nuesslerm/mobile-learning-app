//! `wf mimic import t3-golden` on the goldens of the pinned T3 server: the committed set rebuilds all six legacy files to the hashes recorded
//! at import (the variant ordering is a permutation inside the one set, not a copy), is what importing the goldens gives today, and an
//! ordering that is not a reordering of the recorded frames is refused.
use crate::support::*;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn set() -> PathBuf {
    repo().join("clis/t3-serve/cassettes/t3-v0.1.6")
}

fn goldens() -> PathBuf {
    repo().join("tooling/t3-conformance/goldens/v0.1.6")
}

fn text(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn given_the_committed_t3_set_when_exported_then_all_six_legacy_files_rebuild_to_their_recorded_hashes_from_three_interactions() {
    // Given the set imported from the goldens, which names the six legacy files it came from
    let header: Value = serde_json::from_str(&text(&set().join("cassette.json"))).unwrap();
    let from: Vec<&str> = header["provenance"]["imported_from"].as_array().unwrap().iter().map(|r| r["path"].as_str().unwrap()).collect();
    assert_eq!(from.len(), 6, "{from:?}");
    // When the legacy files are rebuilt from the cassette alone
    let r = mimic(&["export", "t3-golden", set().to_str().unwrap()], &[]);
    // Then each hashes to what the import recorded, and the set claims nothing was recorded
    assert_eq!(r.code, 0, "{}", r.json);
    assert!(r.json["files"].as_array().unwrap().iter().all(|f| f["ok"] == true), "{}", r.json);
    assert_eq!(header["provenance"]["kind"], "imported");
    // And the variant ordering is one more tag and a permutation of the same interaction, not a second copy of 41 frames
    let lines: Vec<Value> = text(&set().join("interactions.jsonl")).lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.iter().map(|l| l["id"].as_str().unwrap()).collect::<Vec<_>>(), ["descriptor", "protocol-gate-426", "session"]);
    let session = &lines[2];
    assert_eq!(session["tags"], json!(["variant:order"]));
    let frames = session["input"]["scenarios"].as_array().unwrap().iter().find(|s| s[0] == "rpc-phase1").unwrap()[1].as_u64().unwrap() as usize;
    assert_eq!(session["input"]["variants"]["order"]["permutation"].as_array().unwrap().len(), frames);
    // And the offline verify accepts the set: pin, rules and corpus are what they were imported against
    let v = mimic(&["verify", set().to_str().unwrap()], &[]);
    assert_eq!((v.code, &v.json["ok"]), (0, &json!(true)), "{}", v.json);
}

#[test]
fn given_the_goldens_when_imported_again_then_the_set_is_the_committed_one_and_the_allowlist_is_theirs_verbatim() {
    // Given the goldens and their allowlist
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("t3-v0.1.6");
    // When they are imported into a fresh directory
    let r = mimic(&["import", "t3-golden", goldens().to_str().unwrap(), "--out", out.to_str().unwrap()], &[]);
    assert_eq!(r.code, 0, "{}", r.json);
    // Then every file of the set but the header (which names the tool build) and the config (whose paths are relative to where the set is) equals the committed one, and the allowlist is the file the differ used
    for name in ["interactions.jsonl", "rules.json", "corpus.json", "allowlist.json"] {
        assert_eq!(text(&out.join(name)), text(&set().join(name)), "{name}");
    }
    assert_eq!(text(&out.join("allowlist.json")), text(&repo().join("tooling/t3-conformance/allowlist.json")));
}

#[test]
fn given_an_ordering_file_with_a_frame_the_recording_lacks_when_imported_then_it_is_refused_and_nothing_is_written() {
    // Given the goldens with one frame of the variant ordering changed
    let tmp = tempfile::tempdir().unwrap();
    let copy = tmp.path().join("goldens");
    std::fs::create_dir_all(&copy).unwrap();
    for e in std::fs::read_dir(goldens()).unwrap().flatten() {
        std::fs::copy(e.path(), copy.join(e.file_name())).unwrap();
    }
    let variant = copy.join("rpc-phase1.variant-order.jsonl");
    std::fs::write(&variant, text(&variant).replacen("subscribeServerConfig", "subscribeNothing", 1)).unwrap();
    // When it is imported
    let out = tmp.path().join("set");
    let r = mimic(&["import", "t3-golden", copy.to_str().unwrap(), "--out", out.to_str().unwrap(), "--allowlist", repo().join("tooling/t3-conformance/allowlist.json").to_str().unwrap(), "--pin", repo().join("tooling/t3-conformance/t3-pin.json").to_str().unwrap()], &[]);
    // Then it fails with exit 2 saying it is not a reordering, and no set exists
    assert_eq!(r.code, 2, "{}", r.json);
    assert!(r.json["error"].as_str().unwrap().contains("not a reordering"), "{}", r.json);
    assert!(!out.join("cassette.json").exists());
}

#[test]
fn given_the_t3_set_when_every_set_is_verified_live_then_the_pinned_release_is_not_started_unless_the_set_is_named() {
    // Given a tree holding the set and the pin file its config points at
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("clis/t3-serve/cassettes/t3-v0.1.6");
    std::fs::create_dir_all(&dir).unwrap();
    for e in std::fs::read_dir(set()).unwrap().flatten() {
        std::fs::copy(e.path(), dir.join(e.file_name())).unwrap();
    }
    std::fs::create_dir_all(tmp.path().join("tooling/t3-conformance")).unwrap();
    std::fs::copy(repo().join("tooling/t3-conformance/t3-pin.json"), tmp.path().join("tooling/t3-conformance/t3-pin.json")).unwrap();
    // When all sets are verified with --live
    let r = mimic(&["verify", "--all", "--live", "--root", tmp.path().to_str().unwrap()], &[]);
    // Then the set is fresh and its live re-record is skipped with the reason, because downloading and running the release is for the VM
    assert_eq!((r.code, &r.json["ok"]), (0, &json!(true)), "{}", r.json);
    let skipped = r.json["sets"][0]["live"]["skipped"].as_str().unwrap_or_default();
    assert!(skipped.contains("pinned release") && skipped.contains("--live"), "{}", r.json);
}
