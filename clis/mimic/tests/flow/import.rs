//! `wf mimic import|export` on the CLI fixtures of libraries/agent-api: the committed imported sets rebuild every legacy file to the
//! canonical hash recorded at import (the legacy files themselves are gone: the sets are now the source), and a recorded claim is refused.
use crate::support::*;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The imported sets under libraries/agent-api, read from their headers: (set dir, format, the legacy paths its header records).
fn committed() -> Vec<(PathBuf, &'static str, Vec<String>)> {
    let mut found = Vec::new();
    let mut stack = vec![repo().join("libraries/agent-api")];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() && !matches!(e.file_name().to_str(), Some("target" | "node_modules" | ".git")) {
                stack.push(p.clone());
            }
            if p.file_name().is_some_and(|n| n == "cassette.json") {
                let header: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
                let from: Vec<String> = header["provenance"]["imported_from"].as_array().into_iter().flatten().map(|r| r["path"].as_str().unwrap().to_string()).collect();
                if let Some(first) = from.first() {
                    let format = if first.ends_with("recordings.json") { "exec-recording" } else { "exec-hosted" };
                    found.push((dir.clone(), format, from));
                }
            }
        }
    }
    found.sort();
    found
}

#[test]
fn given_the_committed_imported_sets_when_exported_then_every_legacy_file_rebuilds_to_its_recorded_hash() {
    // Given the imported sets in the repository (at least the recordings and the hosted fixtures)
    let sets = committed();
    assert!(sets.iter().any(|s| s.1 == "exec-recording") && sets.iter().any(|s| s.1 == "exec-hosted"), "{sets:?}");
    for (dir, format, from) in sets {
        // When the legacy files are rebuilt from the cassette alone
        let r = mimic(&["export", format, dir.to_str().unwrap()], &[]);
        // Then each one hashes to what the import recorded, none is missing and none extra, and the set claims nothing was recorded
        assert_eq!(r.code, 0, "{}", r.json);
        let files = r.json["files"].as_array().unwrap();
        assert_eq!(files.iter().map(|f| f["path"].as_str().unwrap().to_string()).collect::<Vec<_>>(), from, "{}", dir.display());
        assert!(files.iter().all(|f| f["ok"] == true && f["actual"] == f["expected"]), "{}", r.json);
        let header: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("cassette.json")).unwrap()).unwrap();
        assert_eq!(header["provenance"]["kind"], "imported");
        assert!(header["provenance"]["note"].as_str().is_some_and(|n| !n.is_empty()));
        // And the offline verify accepts the set
        let v = mimic(&["verify", dir.to_str().unwrap()], &[]);
        assert_eq!((v.code, &v.json["ok"]), (0, &json!(true)), "{}", v.json);
    }
}

#[test]
fn given_a_legacy_recording_that_claims_recorded_when_imported_then_it_is_refused_with_the_reason_and_nothing_is_written() {
    // Given the legacy recordings rebuilt from the committed set, with one entry that says it was recorded from a run
    let (dir, _, from) = committed().into_iter().find(|s| s.1 == "exec-recording").unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let rebuilt = tmp.path().join("rebuilt");
    assert_eq!(mimic(&["export", "exec-recording", dir.to_str().unwrap(), "--out", rebuilt.to_str().unwrap()], &[]).code, 0);
    let legacy = rebuilt.join(&from[0]).parent().unwrap().to_path_buf();
    let index = std::fs::read_to_string(legacy.join("recordings.json")).unwrap().replacen("\"documented-format\"", "\"recorded\"", 1);
    std::fs::write(legacy.join("recordings.json"), index).unwrap();
    // When it is imported
    let set = tmp.path().join("set");
    let r = mimic(&["import", "exec-recording", legacy.to_str().unwrap(), "--out", set.to_str().unwrap(), "--root", tmp.path().to_str().unwrap()], &[]);
    // Then it fails with exit 2, says a recorded claim needs a real run, and no set exists
    assert_eq!(r.code, 2, "{}", r.json);
    assert!(r.json["error"].as_str().unwrap().contains("not importable") && r.json["next"].as_str().unwrap().contains("real run"), "{}", r.json);
    assert!(!set.join("cassette.json").exists());
}
