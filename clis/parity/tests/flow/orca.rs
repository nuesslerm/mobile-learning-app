//! `wf parity coverage --app orca` over the fixture tree: a miniature Orca whose method names say whether the phone reaches them,
//! with a flag-gated allowlist entry, an off-allowlist call, a capability flag sharing a method's name and raw fetches.
use super::support::*;

#[test]
fn the_scan_classes_off_allowlist_flagged_and_capability_names_and_reads_fetches_as_rows() {
    // Given the fixture with its reference at the pin
    let f = Fixture::at_pin("orca");

    // When the tables are written
    let run = f.run(&[]);
    assert_eq!(run.code, 0, "{}", run.stdout);
    let rows = f.methods();

    // Then every registered method is a row, the phone's exactly when its name says so (tests and scripts are no phone)
    let methods: Vec<&Value> = rows.values().filter(|r| r["kind"] == "rpc").collect();
    assert_eq!(run.json["counts"]["catalog"]["registered"].as_u64(), Some(methods.len() as u64));
    for r in &methods {
        let name = r["name"].as_str().unwrap();
        assert_eq!(r["phone"].as_bool(), Some(name.starts_with("reached.")), "{name}");
    }
    // And a reached method off the allowlist, or behind a flag, is classed so; the flag is named
    let class_of = |name: &str| rows[&format!("rpc:{name}")]["class"].as_str().map(String::from);
    let allowlisted = |name: &str| !rows[&format!("rpc:{name}")]["allowlist"].is_null();
    for r in methods.iter().filter(|r| r["phone"] == true) {
        let name = r["name"].as_str().unwrap();
        let want = match (allowlisted(name), r.get("flag")) {
            (false, _) => Some("offAllowlist".to_string()),
            (true, Some(_)) => Some("flagged".to_string()),
            _ => None,
        };
        assert_eq!(class_of(name), want, "{name}");
    }
    assert!(methods.iter().any(|r| r["flag"].is_string()) && methods.iter().any(|r| r["class"] == "offAllowlist"));
    // And the streaming registration is the subscription, and a name registered through a constant resolves
    assert_eq!(methods.iter().filter(|r| r["subscription"] == true).count(), run.json["counts"]["catalog"]["streaming"].as_u64().unwrap() as usize);
    let flags = f.json(&f.table("-methods.json"))["flags"].clone();
    // And a capability flag is reported, not counted, and a non-literal method name is flagged as dynamic
    assert!(flags["capabilityNames"]["sites"].as_array().unwrap().iter().any(|s| rows[&format!("rpc:{}", s["name"].as_str().unwrap())]["phone"] == false));
    assert!(flags["dynamicMethodNames"]["sites"].as_array().unwrap().iter().any(|s| s["method"] == "pickedMethod"));
    // And each raw fetch is an http row with its target, a forwarding wrapper is none, and the E2EE handshake is a transport row
    let http: Vec<&Value> = rows.values().filter(|r| r["kind"] == "http").collect();
    assert_eq!(http.iter().map(|r| r["target"].as_str().unwrap()).collect::<Vec<_>>(), ["'https://third.party/check'", "RESUME_ENDPOINT"]);
    assert!(rows.contains_key("transport:e2ee.handshake"));

    // When the coverage table is read
    let cov = f.json(&f.table(".json"));
    // Then off-allowlist and decided rows are excluded, flagged rows are outside the target, the rest pending, nothing open
    let state = |id: &str| f.coverage_rows()[id]["state"].as_str().unwrap().to_string();
    for r in methods.iter().filter(|r| r["phone"] == true) {
        let id = r["id"].as_str().unwrap();
        let want = match r["class"].as_str() {
            Some("offAllowlist") => "excluded",
            Some("flagged") => "flagged",
            _ => "pending",
        };
        assert_eq!(state(id), want, "{id}");
    }
    let decided = f.json(&f.table("-decisions.json"))["decisions"].as_object().unwrap().keys().next().unwrap().clone();
    assert_eq!(state(&decided), "excluded");
    assert_eq!(problems(&run), vec![]);
    assert_eq!(cov["summary"]["served"], 0);
    assert_eq!(f.run(&["--check"]).code, 0);
}

/// A trace of oracle replays against orca-serve: one test per `(name, reference provenance, diff)`, each call naming `row`.
fn dual_target_trace(row: &str, tests: &[(&str, &str, &str)]) -> String {
    let tests: Vec<Value> = tests
        .iter()
        .map(|(name, cassette, diff)| json!({"test": name, "cassette": cassette, "verdict": "pass", "ms": 1, "calls": [{"id": diff, "rows": [row], "ok": true, "diff": diff, "agent": [], "executor": []}]}))
        .collect();
    serde_json::to_string_pretty(&json!({"schema": "method-trace/1", "app": "orca", "suite": "wf-mimic-orca-files", "sha": null, "complete": true, "tests": tests})).unwrap()
}

#[test]
fn a_dual_target_replay_serves_an_oracle_row_only_when_orcad_recorded_the_reference_and_never_when_a_double_did() {
    // Given the written tables and a phone row decided oracle: mimic
    let f = Fixture::at_pin("orca");
    assert_eq!(f.run(&[]).code, 0);
    let probe = f.methods().into_iter().find(|(id, r)| r["phone"] == true && id.starts_with("rpc:")).map(|(id, _)| id).unwrap();
    let mut decisions = f.json(&f.table("-decisions.json"));
    decisions["decisions"][probe.as_str()] = json!({"oracle": "mimic"});
    f.write(&f.table("-decisions.json"), &serde_json::to_string_pretty(&decisions).unwrap());
    let out = tempfile::tempdir().unwrap();
    let write = |text: String| std::fs::write(out.path().join("trace.json"), text).unwrap();
    let row = |f: &Fixture| f.coverage_rows()[probe.as_str()].clone();
    // When the reference is the scripted host double (an authored set) and the diff against orca-serve is clean, Then the row stays pending
    write(dual_target_trace(&probe, &[("set::double", "authored", "clean")]));
    let run = f.run(&["--traces", out.path().to_str().unwrap()]);
    assert_eq!((run.code, row(&f)["state"].as_str()), (0, Some("pending")), "{}", run.stdout);
    // And an owner-paired golden (an imported set) does not serve it either
    write(dual_target_trace(&probe, &[("set::golden", "imported", "allowlisted")]));
    f.run(&["--traces", out.path().to_str().unwrap()]);
    assert_eq!(row(&f)["state"], "pending");
    // When orcad recorded the reference (a recorded set) and the diff is clean, Then the row is served and the entry cites the oracle, diff and cassette
    write(dual_target_trace(&probe, &[("set::double", "authored", "clean"), ("set::orcad", "recorded", "clean")]));
    let run = f.run(&["--traces", out.path().to_str().unwrap()]);
    assert_eq!((run.code, problems(&run)), (0, vec![]), "{}", run.stdout);
    let served = row(&f);
    assert_eq!((served["state"].as_str(), served["evidence"]["oracle"].as_str(), served["evidence"]["diff"].as_str(), served["evidence"]["cassette"].as_str(), served["evidence"]["test"].as_str()), (Some("served"), Some("mimic"), Some("clean"), Some("recorded"), Some("set::orcad")));
    // And the evidence file the run wrote cites it for a later check
    assert_eq!(f.json(&f.table("-evidence.json"))["rows"][probe.as_str()]["cassette"], "recorded");
}
