//! D2b spike (docs/plans/mimic/plan.md, X1): a cassette set recorded and replayed by the `wf-mimic` binary of the `clis` workspace
//! against this crate, which lives in a Cargo workspace of its own. The candidate is the crate's own `mimic_driver` example (a
//! `mimic-driver/1` process), so nothing of `clis` is a dependency here: the binary comes from `WF_MIMIC_BIN`, or else it is built from `clis`.
use agent_api_proto::testing;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const FRAMES: [&str; 4] = [
    r#"{"id":1,"method":"agent.threads.list","params":{"limit":5}}"#,
    r#"{"id":"a","method":"agent.threads.list"}"#,
    r#"{"method":"agent.threads.list"}"#,
    "not json",
];

fn mimic(args: &[&str]) -> (i32, Value) {
    testing::mimic::run(args)
}

fn write_set(dir: &Path, driver: &Path) {
    let exe = driver.display().to_string();
    let cases: Vec<Value> = FRAMES.iter().enumerate().map(|(i, f)| json!({"id": format!("frame-{i}"), "input": {"frame": f}})).collect();
    let files = [
        ("pin.json", json!({"version": env!("CARGO_PKG_VERSION")}).to_string()),
        ("rules.json", json!({"schema": "mimic-rules/1", "rules": []}).to_string()),
        ("corpus.json", json!({"schema": "mimic-corpus/1", "seed": 0, "generators": [{"kind": "corpus", "name": "wire", "op": "wire.request", "cases": cases}]}).to_string()),
        (
            "mimic.toml",
            format!(
                "rules = \"rules.json\"\ncorpus = \"corpus.json\"\n[reference]\ncommand = [{exe:?}]\npin = \"pin.json\"\nversion_pointer = \"/version\"\n\n\
                 [candidate.same]\ncommand = [{exe:?}]\n\n[candidate.tweaked]\ncommand = [{exe:?}, \"--tweak\"]\n"
            ),
        ),
    ];
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
}

#[test]
fn given_a_set_recorded_from_this_crate_when_wf_mimic_replays_it_then_the_crate_is_clean_and_a_planted_defect_is_found() {
    // Given a set whose reference is this crate's driver
    let dir = tempfile_dir();
    write_set(&dir, &testing::mimic::example("mimic_driver"));
    let set = dir.display().to_string();
    // When wf-mimic records it, Then one interaction per frame is written
    let (code, record) = mimic(&["record", &set]);
    assert_eq!((code, record["ok"].clone(), record["interactions"].clone()), (0, json!(true), json!(FRAMES.len())), "{record}");
    // And replaying it against the crate again is clean
    let (code, same) = mimic(&["replay", &set, "--candidate", "same"]);
    assert_eq!((code, same["severity"].clone()), (0, json!("none")), "{same}");
    // And a candidate whose frames lost a key is found, which proves the replay reached it
    let (code, tweaked) = mimic(&["replay", &set, "--candidate", "tweaked"]);
    assert_eq!((code, tweaked["severity"].clone()), (1, json!("breaking")), "{tweaked}");
    // And the recorded set verifies offline
    let (code, verify) = mimic(&["verify", &set]);
    assert_eq!((code, verify["ok"].clone()), (0, json!(true)), "{verify}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn tempfile_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agent-api-proto-mimic-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn given_the_fixtures_of_every_crate_when_masked_in_process_and_by_wf_mimic_then_the_values_are_identical_and_the_placeholder_is_the_one_goldens_hold() {
    // Given every exchanges fixture of the workspace, found on disk
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    // The executor crates (`exec-*`) spell their exchanges by `action`; they share `mask()` through `exec_proto::testing`
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root).unwrap().flatten().filter(|e| !e.file_name().to_string_lossy().starts_with("exec-")).map(|e| e.path().join("tests/fixtures")).filter(|d| d.is_dir()).collect();
    dirs.sort();
    let fixtures: Vec<testing::Fixture> = dirs.iter().flat_map(|d| testing::load_dir::<testing::Fixture>(d)).collect();
    assert!(fixtures.iter().any(|f| f.exchanges.iter().any(|e| !e.volatile.is_empty())), "no fixture masks anything");
    // When each is masked in process and by wf mimic, Then they agree, and the placeholder is the text the recorded fixtures hold
    testing::mimic::assert_masks_match_wf_mimic(&fixtures);
    assert_eq!(testing::VOLATILE, "<volatile>");
    assert!(fixtures.iter().flat_map(|f| &f.exchanges).filter(|e| !e.volatile.is_empty()).any(|e| e.volatile.iter().any(|p| e.response.pointer(p) == Some(&serde_json::json!(testing::VOLATILE)))));
}
