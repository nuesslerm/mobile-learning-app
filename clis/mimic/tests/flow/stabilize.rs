//! `wf mimic stabilize`: mask rules inferred from consecutive recordings of the toy reference, which makes up a uuid, a timestamp and a
//! token every run. The recordings are made under rules that mask nothing of that, so every difference is the reference's own.
use crate::support::*;
use std::path::{Path, PathBuf};

const WEAK: &str = r#"{"schema":"mimic-rules/1","rules":[{"kind":"sequence","keys":["sequence"]}]}"#;

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(e.file_name());
        if e.path().is_dir() { copy(&e.path(), &target) } else { std::fs::copy(e.path(), target).unwrap(); }
    }
}

/// `n` consecutive recordings of the world's corpus under the weak rules, each kept as its own set beside the world's.
fn recordings(w: &World, n: usize) -> Vec<PathBuf> {
    w.write("rules.json", WEAK);
    (0..n)
        .map(|i| {
            let r = w.run("record", &["--allow-nondeterministic"]);
            assert_eq!(r.code, 0, "record: {}\n{}", r.json, r.stderr);
            let dir = w.dir.path().join(format!("run-{i}"));
            copy(&w.set, &dir);
            dir
        })
        .collect()
}

fn stabilize(dirs: &[PathBuf], extra: &[&str]) -> Run {
    let d: Vec<String> = dirs.iter().map(|p| p.display().to_string()).collect();
    let mut args = vec!["stabilize"];
    args.extend(d.iter().map(String::as_str));
    args.extend_from_slice(extra);
    mimic(&args, &[])
}

fn interactions(dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(dir.join("interactions.jsonl")).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn recordings_that_differ_only_in_what_the_reference_makes_up_are_made_equal_by_inferred_rules() {
    // Given three consecutive recordings under rules that mask none of the made-up uuids, timestamps and tokens
    let w = World::new();
    let runs = recordings(&w, 3);
    let before = interactions(&runs[0]);
    assert!(before.iter().any(|i| i["tags"].as_array().is_some_and(|t| t.contains(&json!("nondeterministic")))), "the toy must differ run to run");
    // When they are stabilized
    let r = stabilize(&runs, &[]);
    // Then every case agrees after the proposed rules, and each proposal names the case that showed it
    assert_eq!((r.code, r.json["ok"].clone(), r.json["stable_pct"].clone()), (0, json!(true), json!(100.0)), "{}\n{}", r.json, r.stderr);
    assert_eq!(r.json["compared"], json!(before.len()));
    let proposals = r.json["proposals"].as_array().unwrap();
    assert!(!proposals.is_empty() && proposals.iter().all(|p| !p["seen_in"].as_array().unwrap().is_empty() && !p["why"].as_str().unwrap().is_empty()), "{proposals:?}");
    assert!(r.json["stable_before"].as_u64() < r.json["stable_after"].as_u64());
}

#[test]
fn written_rules_make_a_fresh_recording_of_the_reference_agree_with_itself_and_leave_stable_answers_alone() {
    // Given three recordings and the first one rewritten with the inferred rules
    let w = World::new();
    let runs = recordings(&w, 3);
    let echo_before = interactions(&runs[0]).into_iter().find(|i| i["id"] == "echo-1").unwrap();
    let r = stabilize(&runs, &["--write"]);
    assert_eq!(r.code, 0, "{}\n{}", r.json, r.stderr);
    // Then the stable answer of a case is untouched, and no case is tagged nondeterministic any more
    let after = interactions(&runs[0]);
    assert_eq!(after.iter().find(|i| i["id"] == "echo-1").unwrap()["steps"], echo_before["steps"]);
    assert!(after.iter().all(|i| !i["tags"].as_array().is_some_and(|t| t.contains(&json!("nondeterministic")))));
    // and the set verifies under the rules it was rewritten with
    let v = mimic(&["verify", &runs[0].display().to_string()], &[]);
    assert_eq!((v.code, v.json["ok"].clone()), (0, json!(true)), "{}", v.json);
    // and a new recording under those rules is deterministic: nothing is flagged
    copy(&runs[0], &w.set);
    let fresh = w.run("record", &[]);
    assert_eq!((fresh.code, fresh.json["nondeterministic"].clone()), (0, json!([])), "{}\n{}", fresh.json, fresh.stderr);
    assert!(runs[0].join("stability.json").is_file());
}

#[test]
fn an_answer_that_changes_in_structure_stays_unstable_with_its_cause_until_it_is_allowlisted_with_a_reason() {
    // Given two recordings where one answer of the second carries an extra step
    let w = World::new();
    let runs = recordings(&w, 2);
    let mut cassette = mimic_core::cassette::Cassette::parse(&std::fs::read_to_string(runs[1].join("cassette.json")).unwrap(), &std::fs::read_to_string(runs[1].join("interactions.jsonl")).unwrap()).unwrap();
    let planted = cassette.interactions.iter_mut().find(|i| !i.steps.is_empty()).unwrap();
    let first = planted.steps[0].clone();
    planted.steps.push(first);
    let id = planted.id.clone();
    cassette.seal();
    std::fs::write(runs[1].join("interactions.jsonl"), cassette.jsonl()).unwrap();
    std::fs::write(runs[1].join("cassette.json"), cassette.header_json()).unwrap();
    // When stabilized, then it fails and names the case and the cause
    let r = stabilize(&runs, &[]);
    assert_eq!(r.code, 1, "{}", r.json);
    let unstable = r.json["unstable"].as_array().unwrap();
    assert_eq!(unstable.iter().map(|u| u["id"].clone()).collect::<Vec<_>>(), vec![json!(id)], "{}", r.json);
    assert_eq!(unstable[0]["cause"], json!("array-length"));
    assert_eq!(r.json["accounting"]["unlisted"], json!([id]));
    // and an allowlist with the cause and a reason accounts for it
    let list = json!({"schema": "mimic-stability-allowlist/1", "entries": [{"id": id, "cause": "array-length", "reason": "planted by the test"}]});
    let file = w.dir.path().join("allow.json");
    std::fs::write(&file, list.to_string()).unwrap();
    let ok = stabilize(&runs, &["--allowlist", &file.display().to_string(), "--min-pct", "50"]);
    assert_eq!((ok.code, ok.json["accounting"]["allowlisted"].clone()), (0, json!([id])), "{}", ok.json);
    // and an entry for a case that agrees is stale
    let other = interactions(&runs[0]).iter().map(|i| i["id"].as_str().unwrap().to_string()).find(|i| *i != id).unwrap();
    let stale = json!({"schema": "mimic-stability-allowlist/1", "entries": [{"id": other, "cause": "array-length", "reason": "x"}]});
    std::fs::write(&file, stale.to_string()).unwrap();
    let r = stabilize(&runs, &["--allowlist", &file.display().to_string()]);
    assert_eq!(r.json["accounting"]["stale"], json!([other]));
}

#[test]
fn one_recording_is_not_enough_to_tell_what_varies() {
    let w = World::new();
    let runs = recordings(&w, 1);
    let r = stabilize(&runs, &[]);
    assert_eq!(r.code, 2, "{}", r.json);
    assert!(r.json["error"].as_str().unwrap().contains("two recordings"));
}

#[test]
fn a_set_with_nondeterministic_cases_fails_verify_until_every_one_is_accounted_for() {
    // Given a set recorded with unstable cases and no allowlist
    let w = World::new();
    let runs = recordings(&w, 2);
    let tagged: Vec<String> = interactions(&runs[0]).iter().filter(|i| i["tags"].as_array().is_some_and(|t| t.contains(&json!("nondeterministic")))).map(|i| i["id"].as_str().unwrap().to_string()).collect();
    assert!(!tagged.is_empty());
    // When it is verified, then every tagged id is a problem
    let v = mimic(&["verify", &runs[0].display().to_string()], &[]);
    let problems: Vec<String> = v.json["sets"][0]["problems"].as_array().unwrap().iter().map(|p| p["message"].as_str().unwrap().to_string()).collect();
    assert_eq!(v.code, 1, "{}", v.json);
    assert!(tagged.iter().all(|id| problems.iter().any(|m| m.contains(id.as_str()))), "{problems:?}");
    // and once stabilized (nothing tagged, nothing listed) it verifies, and a listed entry that is not tagged is stale
    assert_eq!(stabilize(&runs, &["--write"]).code, 0);
    assert_eq!(mimic(&["verify", &runs[0].display().to_string()], &[]).code, 0);
    let stale = json!({"schema": "mimic-stability-allowlist/1", "entries": [{"id": tagged[0], "cause": "text-change", "reason": "x"}]});
    std::fs::write(runs[0].join("stability-allowlist.json"), stale.to_string()).unwrap();
    let v = mimic(&["verify", &runs[0].display().to_string()], &[]);
    assert_eq!(v.code, 1);
    assert!(v.json["sets"][0]["problems"][0]["message"].as_str().unwrap().contains("stale"), "{}", v.json);
}

#[test]
fn a_rules_change_lands_on_a_committed_set_by_remasking_it_and_everything_but_the_rules_hash_is_kept() {
    // Given a recorded set, and a pointer rule added for one answer of its own
    let w = World::new();
    w.record();
    let before = w.interactions();
    let (id, pointer) = before.iter().find_map(|i| {
        let steps = i["steps"].as_array()?;
        let at = steps.iter().position(|s| s["payload"].as_object().is_some_and(|o| o.values().any(Value::is_string)))?;
        let key = steps[at]["payload"].as_object()?.iter().find(|(_, v)| v.is_string())?.0.clone();
        Some((i["id"].as_str()?.to_string(), format!("/steps/{at}/payload/{key}")))
    }).expect("the toy answers with a string");
    let mut rules: Value = serde_json::from_str(&w.read("rules.json")).unwrap();
    rules["rules"].as_array_mut().unwrap().push(json!({"kind": "pointer", "pointer": pointer, "placeholder": "<mask>", "interaction": id}));
    w.write("rules.json", &rules.to_string());
    // Then the set is stale
    assert_eq!(mimic(&["verify", &w.arg()], &[]).code, 1);
    // When it is remasked
    let r = w.run("remask", &[]);
    // Then only that answer changed, the set verifies and the rest of it is as recorded
    assert_eq!((r.code, r.json["changed"].clone()), (0, json!([id])), "{}\n{}", r.json, r.stderr);
    assert_eq!(mimic(&["verify", &w.arg()], &[]).code, 0);
    let after = w.interactions();
    let at: usize = pointer.split('/').nth(2).unwrap().parse().unwrap();
    let key = pointer.rsplit('/').next().unwrap();
    let hit = after.iter().find(|i| i["id"] == json!(id)).unwrap();
    assert_eq!(hit["steps"][at]["payload"][key], json!("<mask>"));
    assert_eq!(after.iter().filter(|i| i["id"] != json!(id)).collect::<Vec<_>>(), before.iter().filter(|i| i["id"] != json!(id)).collect::<Vec<_>>());
    assert_eq!(hit["tags"], before.iter().find(|i| i["id"] == json!(id)).unwrap()["tags"]);
    // and a second remask changes nothing
    assert_eq!(w.run("remask", &[]).json["changed"], json!([]));
}

#[test]
fn the_committed_cassette_sets_all_verify_including_those_recorded_before_the_stabilizer() {
    // Given the repo's committed sets, some recorded before any stability accounting existed
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    // When every set is verified, Then none fails on missing stability accounting
    let r = mimic(&["verify", "--all", "--root", root.to_str().unwrap()], &[]);
    assert_eq!(r.code, 0, "{}", r.json);
}
