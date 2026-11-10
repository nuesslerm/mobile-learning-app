use crate::support::*;
use mimic_core::allowlist::plain;
use mimic_core::differ::Verdict;

fn recorded() -> World {
    let w = World::new();
    w.record();
    w
}

fn results(r: &Run) -> &Vec<Value> {
    r.json["results"].as_array().unwrap()
}

/// The paths of the divergences nothing allowlisted.
fn diff_paths(r: &Run) -> Vec<String> {
    results(r).iter().flat_map(|x| x["diffs"].as_array().unwrap().iter().filter(|d| d["allowlisted"].is_null()).map(|d| d["path"].as_str().unwrap().to_string())).collect()
}

#[test]
fn the_same_program_replays_clean_though_it_makes_up_other_ids_this_time() {
    // Given a recorded set
    let w = recorded();
    // When the same program is replayed as the candidate
    let r = w.run("replay", &["--candidate", "same"]);
    // Then every interaction is clean
    assert_eq!((r.code, r.json["ok"].clone(), r.json["severity"].clone()), (0, json!(true), json!("none")), "{}", r.json);
    let total = w.interactions().len();
    assert_eq!(r.json["summary"], json!({"total": total, "clean": total, "allowlisted": 0, "drift": 0, "breaking": 0}));
    assert!(results(&r).is_empty());
}

#[test]
fn a_dropped_key_is_breaking_and_names_the_json_path_and_the_next_command() {
    // Given a recorded set and a candidate that drops a key of every object it answers
    let w = recorded();
    // When it is replayed
    let r = w.run("replay", &["--candidate", "drop"]);
    // Then it is breaking, with the path of the key the cassette has and the candidate lacks
    assert_eq!((r.code, r.json["severity"].clone()), (1, json!("breaking")), "{}", r.json);
    let echo = results(&r).iter().find(|x| x["id"] == "echo-1").expect("echo-1 diverges");
    let diff = &echo["diffs"][0];
    let recorded = w.interactions().into_iter().find(|i| i["id"] == "echo-1").unwrap();
    let key = diff["path"].as_str().unwrap().rsplit('.').next().unwrap().to_string();
    assert!(recorded["steps"][0]["payload"].get(&key).is_some() && diff["kind"] == "missing" && diff["path"].as_str().unwrap().contains(".payload."), "{diff}");
    assert_eq!(echo["row"], "rpc:echo");
    // and the next command replays that interaction alone
    let next = r.json["next"].as_str().unwrap();
    assert!(next.contains("--candidate drop") && next.contains("--only "), "{next}");
    let id = next.split("--only ").nth(1).unwrap().split_whitespace().next().unwrap();
    let again = w.run("replay", &["--candidate", "drop", "--only", id]);
    assert_eq!((again.json["summary"]["total"].clone(), again.code), (json!(1), 1), "{}", again.json);
}

#[test]
fn an_extra_key_and_a_changed_value_are_drift_with_what_each_side_said() {
    let w = recorded();
    // When a candidate adds a key
    let extra = w.run("replay", &["--candidate", "extra"]);
    // Then it is drift at the added key, with the value the candidate gave
    assert_eq!((extra.code, extra.json["severity"].clone()), (1, json!("drift")), "{}", extra.json);
    let d = &results(&extra).iter().find(|x| x["id"] == "echo-1").unwrap()["diffs"][0];
    assert!(d["path"].as_str().unwrap().ends_with(".extra") && d["kind"] == "extra" && d["actual"] == true, "{d}");
    // When a candidate changes a string
    let change = w.run("replay", &["--candidate", "change"]);
    // Then it is drift with both values
    let d = &results(&change).iter().find(|x| x["id"] == "echo-1").unwrap()["diffs"][0];
    assert_eq!((change.json["severity"].clone(), d["kind"].clone(), d["actual"].clone()), (json!("drift"), json!("changed"), json!("changed")), "{}", change.json);
    assert_eq!(d["expected"], "x");
}

#[test]
fn an_allowlist_entry_removes_a_divergence_and_removing_the_entry_brings_it_back() {
    // Given a candidate that drifts, and an allowlist with one entry per divergence it was seen to have
    let w = recorded();
    let seen = w.run("replay", &["--candidate", "change"]);
    let mut paths: Vec<String> = diff_paths(&seen).iter().map(|p| plain(p)).collect();
    paths.sort();
    paths.dedup();
    let entries = |ps: &[String]| json!({"ignored": ps.iter().map(|p| json!({"path": p, "reason": "the candidate words it differently", "phase": "x0"})).collect::<Vec<_>>()});
    let allow = w.path("allow.json").display().to_string();
    w.write("allow.json", &entries(&paths).to_string());
    // When it is replayed under the allowlist
    let r = w.run("replay", &["--candidate", "change", "--allowlist", &allow]);
    // Then it is clean in effect, and every divergence carries the reason it was allowed
    assert_eq!((r.code, r.json["severity"].clone()), (0, json!("none")), "{}", r.json);
    assert!(r.json["summary"]["allowlisted"].as_u64().unwrap() > 0 && r.json["summary"]["drift"] == 0);
    let reasons: Vec<&Value> = results(&r).iter().flat_map(|x| x["diffs"].as_array().unwrap()).map(|d| &d["allowlisted"]["reason"]).collect();
    assert!(!reasons.is_empty() && reasons.iter().all(|x| *x == "the candidate words it differently"));
    // When one entry is removed
    let gone = paths[0].clone();
    w.write("allow.json", &entries(&paths[1..]).to_string());
    // Then exactly the divergences it covered are back
    let back = w.run("replay", &["--candidate", "change", "--allowlist", &allow]);
    let again = diff_paths(&back);
    assert_eq!(back.code, 1);
    assert!(!again.is_empty() && again.iter().all(|p| plain(p) == gone), "{again:?} vs {gone}");
    // When an entry matches nothing, and another was written against a different pin
    let mut stale = entries(&paths);
    stale["ignored"].as_array_mut().unwrap().push(json!({"path": "payload.nothing", "reason": "r", "phase": "x0"}));
    stale["ignored"][0]["pin_hash"] = json!("sha256:another-pin");
    w.write("allow.json", &stale.to_string());
    let r = w.run("replay", &["--candidate", "change", "--allowlist", &allow]);
    // Then the report warns of both and the expired entry no longer covers its path
    let warnings = r.json["warnings"].to_string();
    assert!(warnings.contains("matched nothing") && warnings.contains("expired") && r.code == 1, "{}", r.json);
}

#[test]
fn a_candidate_that_cannot_run_breaks_every_interaction_with_the_error_as_evidence() {
    let w = recorded();
    let r = w.run("replay", &["--candidate", "missing"]);
    assert_eq!((r.code, r.json["severity"].clone()), (1, json!("breaking")), "{}", r.json);
    let total = w.interactions().len();
    assert_eq!(results(&r).len(), total);
    assert!(results(&r).iter().all(|x| x["error"].as_str().unwrap().contains("/nonexistent/driver")));
}

#[test]
fn a_replay_that_names_no_candidate_or_an_interaction_that_is_not_there_is_refused() {
    let w = recorded();
    // When several candidates exist and none is named
    let r = w.run("replay", &[]);
    // Then it is refused with the names to choose from
    assert_eq!((r.code, r.json["ok"].clone()), (2, json!(false)));
    for name in ["same", "drop", "extra", "change", "missing"] {
        assert!(r.json["error"].as_str().unwrap().contains(name), "{}", r.json);
    }
    // When an interaction that is not in the cassette is asked for
    let r = w.run("replay", &["--candidate", "same", "--only", "no-such-id"]);
    assert_eq!(r.code, 2, "{}", r.json);
    assert!(r.json["error"].as_str().unwrap().contains("no-such-id"));
    // When the set was never recorded
    let empty = World::new();
    let r = empty.run("replay", &["--candidate", "same"]);
    assert_eq!(r.code, 2);
    assert!(r.json["next"].as_str().unwrap().contains("wf mimic record"), "{}", r.json);
}

#[test]
fn a_suite_replay_writes_the_run_record_and_a_trace_with_each_calls_diff_and_row() {
    let w = recorded();
    let out = tempfile::tempdir().unwrap();
    for (candidate, verdict) in [("same", "pass"), ("drop", "fail")] {
        // When a replay is run as a suite of a test run
        let r = mimic(&["replay", &w.arg(), "--candidate", candidate, "--suite", "wf-mimic-toy"], &[("WF_TEST_OUTPUT_DIR", out.path())]);
        // Then the run record carries the verdict and the trace one call per interaction with its diff label and parity row
        let dir = out.path().join("wf-mimic-toy");
        assert_eq!(r.json["evidence"]["report"], json!(dir.join("report.json").display().to_string()), "{}", r.json);
        let report: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("report.json")).unwrap()).unwrap();
        let trace: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("trace.json")).unwrap()).unwrap();
        assert_eq!((report["verdict"].clone(), trace["schema"].clone(), trace["complete"].clone()), (json!(verdict), json!("method-trace/1"), json!(true)));
        let calls: Vec<Value> = trace["tests"].as_array().unwrap().iter().flat_map(|t| t["calls"].as_array().unwrap().clone()).collect();
        // and each interaction is a test of its own, which passes exactly when its call did: one drifting interaction fails no other row's evidence
        let tests = trace["tests"].as_array().unwrap();
        assert_eq!(tests.len(), w.interactions().len());
        assert!(tests.iter().all(|t| (t["verdict"] == "pass") == (t["calls"][0]["ok"] == true)), "{trace}");
        let header: Value = serde_json::from_str(&std::fs::read_to_string(std::path::Path::new(&w.arg()).join("cassette.json")).unwrap()).unwrap();
        assert_eq!(trace["tests"][0]["cassette"], header["provenance"]["kind"], "the trace names how the cassette was made");
        for i in w.interactions() {
            let call = calls.iter().find(|c| c["id"] == i["id"]).expect("a call per interaction");
            assert_eq!(call["rows"], json!(i.get("row").map(|r| vec![r.clone()]).unwrap_or_default()));
            assert_eq!(call["ok"], json!(verdict == "pass" || i["steps"].as_array().unwrap().is_empty()), "{candidate} {call}");
        }
    }
    // When the test run has no output directory
    let r = w.run("replay", &["--candidate", "same", "--suite", "s"]);
    // Then the suite is refused, naming the variable
    assert!(r.code == 2 && r.json["error"].as_str().unwrap().contains("WF_TEST_OUTPUT_DIR"), "{}", r.json);
}

#[test]
fn diff_compares_two_sets_by_interaction_id() {
    // Given the toy recorded twice, and once more with a reference that drops a key
    let (a, b, c) = (recorded(), recorded(), World::new());
    let toml = c.read("mimic.toml").replacen(&format!("command = [{:?}]", toy().display().to_string()), &format!("command = [{:?}, \"--tweak\", \"drop-key\"]", toy().display().to_string()), 1);
    c.write("mimic.toml", &toml);
    c.record();
    // When equal behaviour is diffed
    let same = mimic(&["diff", &a.arg(), &b.arg()], &[]);
    // Then there is nothing to report
    assert_eq!((same.code, same.json["severity"].clone()), (0, json!("none")), "{}", same.json);
    // When the dropped key is diffed
    let r = mimic(&["diff", &a.arg(), &c.arg()], &[]);
    // Then it is breaking, with the same diff objects replay reports
    assert_eq!((r.code, r.json["severity"].clone()), (1, json!("breaking")), "{}", r.json);
    assert!(r.json["results"].as_array().unwrap().iter().any(|x| x["diffs"][0]["kind"] == "missing"));
}

#[test]
fn the_sdk_replays_a_set_in_process_with_the_same_verdict_as_the_cli() {
    use mimic_core::allowlist::Allowlist;
    use mimic_core::differ::DiffOptions;
    use mimic_core::ports::Store;
    use mimic_core::testing::ToyDriver;
    use mimic_core::usecase::replay;
    use wf_mimic::adapters::{FsStore, InProcessDriver};
    // Given a recorded set and the toy as a Rust closure
    let w = recorded();
    let store = FsStore::new(w.dir.path());
    let set = store.read("toy-set").unwrap();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
    let mut toy = ToyDriver::new(nanos, 1_700_000_000_000);
    let mut driver = InProcessDriver::new("toy", Some("toy-1.0"), |request| mimic_core::ports::Driver::run(&mut toy, request));
    // When it is replayed through the InProcessDriver
    let verdict: Verdict = replay(&set, &mut driver, &Allowlist::default(), &DiffOptions::default(), None).unwrap();
    // Then it is clean, as the CLI found the same program
    assert!(verdict.ok() && verdict.summary.clean == set.cassette.interactions.len(), "{verdict:?}");
}
