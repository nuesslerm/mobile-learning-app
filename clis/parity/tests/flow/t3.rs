//! `wf parity coverage --app t3` over the fixture tree: a miniature T3 Code whose names say whether the phone reaches them (a
//! name with a part starting `reached` does, `unreached` does not), and miniature `t3 serve` tables.
use super::support::*;

fn reached(name: &str) -> bool {
    name.split('.').any(|part| part.starts_with("reached"))
}

#[test]
fn a_write_scans_the_pinned_reference_and_counts_only_what_the_phone_uses() {
    // Given the fixture with its reference at the pin
    let f = Fixture::at_pin("t3");

    // When the tables are written
    let run = f.run(&[]);

    // Then the scan read the reference and wrote the list and both tables
    assert_eq!(run.code, 0, "{}", run.stdout);
    assert_eq!((run.json["ok"].as_bool(), run.json["source"].as_str()), (Some(true), Some("reference-scan")));
    assert_eq!(run.json["wrote"], json!([f.table("-methods.json"), f.table(".json"), f.table(".md")]));
    // And a row is the phone's exactly when its name says so: test files, `typeof`, unread factory members, unused exports and
    // type unions do not count
    let rows = f.methods();
    let wrong: Vec<&String> = rows.iter().filter(|(_, r)| r["phone"].as_bool() != Some(reached(r["name"].as_str().unwrap()))).map(|(id, _)| id).collect();
    assert!(wrong.is_empty(), "rows whose phone flag contradicts their name: {wrong:?}");
    // And a nested `type` field of a command is no command, and the one streaming RPC is the subscription
    assert!(rows.keys().all(|id| !id.contains("nested")), "{:?}", rows.keys());
    let subscriptions: Vec<&String> = rows.iter().filter(|(_, r)| r["subscription"] == true).map(|(id, _)| id).collect();
    assert_eq!(subscriptions.len(), 1);
    assert_eq!(run.json["counts"]["catalog"]["subscriptions"], 1);
    // And a client indexed by a variable is flagged as a dynamic method name
    let flags = f.json(&f.table("-methods.json"))["flags"].clone();
    assert!(flags["dynamicMethodNames"]["sites"].as_array().unwrap().iter().any(|s| s["method"] == "tag"), "{flags}");
    // And every phone row has a site in the reference
    assert!(rows.values().filter(|r| r["phone"] == true).all(|r| r["sites"][0].as_str().is_some_and(|s| s.contains(".ts"))));
    // And an RPC carries the scope its last scope-table entry names (the server's over the client's guarded one), and no other
    // RPC has one
    let scoped: Vec<(&str, &str)> = rows.values().filter_map(|r| Some((r["name"].as_str()?, r["scope"].as_str()?))).collect();
    assert_eq!(scoped, [("reached.dispatch", "orchestration:operate"), ("reached.probe", "orchestration:read"), ("reached.watch", "orchestration:read"), ("reached.write", "source-control:write")]);

    // When the same tables are checked
    let check = f.run(&["--check"]);
    // Then nothing is stale or open
    assert_eq!((check.code, problems(&check)), (0, vec![]), "{}", check.stdout);
}

#[test]
fn rows_the_mapper_answers_stay_pending_with_their_source_table_named() {
    // Given the fixture's t3 serve tables
    let f = Fixture::at_pin("t3");
    let quoted = |rel: &str| -> Vec<String> { f.text(rel).split('"').skip(1).step_by(2).map(String::from).collect() };
    let mut answered: Vec<String> = quoted("clis/t3-serve/contracts/src/rpc.rs").iter().map(|n| format!("rpc:{n}")).collect();
    answered.extend(quoted("clis/t3-serve/core/src/domain/decider.rs").iter().map(|n| format!("command:{n}")));
    answered.extend(quoted("clis/t3-serve/core/src/domain/command.rs").iter().map(|n| format!("mutation:{n}")));

    // When the tables are written
    assert_eq!(f.run(&[]).code, 0);

    // Then each listed row and the HTTP row its route matches is pending with a note naming where it is answered, and no other
    let rows = f.coverage_rows();
    let history = f.methods().into_values().find(|r| r["path"].as_str().is_some_and(|p| p.ends_with("/history"))).unwrap();
    answered.push(history["id"].as_str().unwrap().to_string());
    let noted: Vec<String> = rows.iter().filter(|(_, r)| r.get("note").is_some()).map(|(id, _)| id.clone()).collect();
    answered.sort();
    assert_eq!(noted, answered);
    for id in &noted {
        let note = rows[id]["note"].as_str().unwrap();
        assert_eq!(rows[id]["state"], "pending", "{id}");
        assert!(note.contains("clis/t3-serve/") && note.contains("no run-bundle evidence yet"), "{id}: {note}");
    }
    assert_eq!(f.json(&f.table(".json"))["summary"]["served"], 0);
}

#[test]
fn a_check_names_unassigned_untargeted_served_and_stale_rows() {
    // Given written tables
    let f = Fixture::at_pin("t3");
    assert_eq!(f.run(&[]).code, 0);
    let phone_commands: Vec<String> = f.methods().into_iter().filter(|(id, r)| id.starts_with("command:") && r["phone"] == true).map(|(id, _)| id).collect();

    // When the decisions lose the command milestone and a target, claim a row served, and the markdown is edited by hand
    let mut decisions = f.json(&f.table("-decisions.json"));
    decisions["milestones"].as_object_mut().unwrap().remove("command:*");
    let dropped = decisions["targets"].as_object().unwrap().keys().find(|k| k.starts_with("command:")).unwrap().clone();
    decisions["targets"].as_object_mut().unwrap().remove(&dropped);
    decisions["decisions"]["rpc:reached.write"] = json!({"state": "served"});
    f.write(&f.table("-decisions.json"), &serde_json::to_string_pretty(&decisions).unwrap());
    f.write(&f.table(".md"), "edited by hand\n");
    let check = f.run(&["--check"]);

    // Then the check fails naming each problem's rows
    assert_eq!(check.code, 1, "{}", check.stdout);
    let found = problems(&check);
    let rows_of = |rule: &str| found.iter().find(|(r, _)| r == rule).map(|(_, rows)| rows.clone()).unwrap_or_default();
    assert_eq!(rows_of("unassigned"), phone_commands);
    assert_eq!(rows_of("untargeted"), [dropped]);
    assert_eq!(rows_of("served-without-evidence"), ["rpc:reached.write"]);
    assert!(rows_of("stale").contains(&f.table(".md")), "{found:?}");
    assert!(check.json["next"].as_str().unwrap().contains("decisions.json"));
}

#[test]
fn a_reference_off_its_pin_refuses_a_write_and_a_check_reads_the_checked_in_list() {
    // Given written tables, then a new commit in the reference
    let f = Fixture::at_pin("t3");
    assert_eq!(f.run(&[]).code, 0);
    let before = f.text(&f.table("-methods.json"));
    std::fs::write(f.reference().join("apps/mobile/src/Extra.ts"), "export const extra = 1\n").unwrap();
    git(&f.reference(), &["add", "-A"]);
    git(&f.reference(), &["commit", "-qm", "moved on"]);
    let head = git(&f.reference(), &["rev-parse", "HEAD"]);

    // When a write is asked, Then it is refused naming the checkout's commit and the pin's, and nothing is written
    let write = f.run(&[]);
    assert_eq!(write.code, 1, "{}", write.stdout);
    let pin = f.json(f.pin_file())["commit"].as_str().unwrap().to_string();
    let error = write.json["error"].as_str().unwrap();
    assert!(error.contains(&head) && error.contains(&pin), "{error}");
    assert!(write.json["next"].as_str().unwrap().contains("--check"));
    assert_eq!(f.text(&f.table("-methods.json")), before);

    // When a check is asked, Then it passes from the checked-in list
    let check = f.run(&["--check"]);
    assert_eq!(check.code, 0, "{}", check.stdout);
    assert_eq!((check.json["source"].as_str(), check.json["reference"]["atPin"].as_bool()), (Some("methods-list"), Some(false)));
}

#[test]
fn without_the_reference_a_check_rebuilds_from_the_checked_in_list() {
    // Given written tables and no reference checkout
    let f = Fixture::at_pin("t3");
    assert_eq!(f.run(&[]).code, 0);
    std::fs::remove_dir_all(f.reference()).unwrap();

    // When checked, Then the tables are rebuilt from the list and match
    let check = f.run(&["--check"]);
    assert_eq!((check.code, check.json["source"].as_str()), (0, Some("methods-list")), "{}", check.stdout);

    // When the list is missing too, Then the check fails saying how to get the reference
    std::fs::remove_file(f.root().join(f.table("-methods.json"))).unwrap();
    let missing = f.run(&["--check"]);
    assert_eq!(missing.code, 1);
    assert!(missing.json["error"].as_str().unwrap().contains("methods.json is missing"), "{}", missing.stdout);
    assert!(missing.json["next"].as_str().unwrap().contains("submodule:init"));
}

#[test]
fn pending_rises_only_in_a_commit_marked_as_a_pin_bump() {
    // Given tables committed with one row decided not offered
    let f = Fixture::at_pin("t3");
    let mut decisions = f.json(&f.table("-decisions.json"));
    decisions["decisions"]["rpc:reached.probe"] = json!({"state": "not-offered", "reason": "a fixture decision", "decidedBy": "M5"});
    f.write(&f.table("-decisions.json"), &serde_json::to_string_pretty(&decisions).unwrap());
    assert_eq!(f.run(&[]).code, 0);
    f.write(".gitignore", "references/\n");
    git(f.root(), &["init", "-q"]);
    git(f.root(), &["add", "-A"]);
    git(f.root(), &["commit", "-qm", "tables"]);
    let pending = f.json(&f.table(".json"))["summary"]["pending"].as_u64().unwrap();

    // When the decision is dropped, the tables rewritten and committed
    decisions["decisions"].as_object_mut().unwrap().remove("rpc:reached.probe");
    f.write(&f.table("-decisions.json"), &serde_json::to_string_pretty(&decisions).unwrap());
    let rewrite = f.run(&[]);
    assert_eq!(rewrite.json["summary"]["pending"].as_u64(), Some(pending + 1));
    // Then, while uncommitted, the rise against HEAD is reported
    assert!(problems(&rewrite).iter().any(|(r, _)| r == "pending-rose"), "{}", rewrite.stdout);
    git(f.root(), &["commit", "-qam", "drop a decision"]);
    // And once committed, the check against HEAD^ fails
    let check = f.run(&["--check"]);
    assert_eq!(check.code, 1, "{}", check.stdout);
    assert!(problems(&check).iter().any(|(r, _)| r == "pending-rose"));
    // And against a base where pending was already this high it passes
    assert_eq!(f.run(&["--check", "--base", "HEAD"]).code, 0);

    // When the commit says it is a pin bump, Then the rise is allowed
    git(f.root(), &["commit", "-q", "--amend", "-m", "Bump the T3 pin\n\nparity: pin-bump"]);
    let bumped = f.run(&["--check"]);
    assert_eq!((bumped.code, problems(&bumped)), (0, vec![]), "{}", bumped.stdout);
}

#[test]
fn an_unknown_app_is_refused_with_the_usage() {
    let dir = tempfile::tempdir().unwrap();
    let run = coverage(dir.path(), &["--app", "slack"]);
    assert_eq!(run.code, 2);
    assert_eq!(run.json["ok"], false);
    assert!(run.json["next"].as_str().unwrap().contains("--app t3|orca"));
}

/// The `trace.json` of a chain run (`method-trace/1`): one test per `(name, verdict, calls)`, each call `(rows, ok, agent methods answered, executor tools answered)`.
type Calls<'a> = Vec<(Vec<&'a str>, bool, Vec<&'a str>, Vec<&'a str>)>;

fn trace(complete: bool, tests: &[(&str, &str, Calls)]) -> String {
    let hits = |key: &str, names: &[&str]| -> Vec<Value> { names.iter().map(|n| json!({key: n, "ok": true})).collect() };
    let tests: Vec<Value> = tests
        .iter()
        .map(|(name, verdict, calls)| json!({"test": name, "verdict": verdict, "ms": 5, "calls": calls.iter().map(|(rows, ok, agent, tools)| json!({"rows": rows, "ok": ok, "agent": hits("method", agent), "executor": hits("tool", tools)})).collect::<Vec<_>>()}))
        .collect();
    serde_json::to_string_pretty(&json!({"schema": "method-trace/1", "app": "t3", "suite": "t3-serve-integration", "sha": "abc", "complete": complete, "tests": tests})).unwrap()
}

/// A written fixture and a test output dir holding `<suite>/trace.json`.
fn with_trace(text: &str) -> (Fixture, tempfile::TempDir) {
    let f = Fixture::at_pin("t3");
    assert_eq!(f.run(&[]).code, 0);
    let out = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(out.path().join("t3-serve-integration")).unwrap();
    std::fs::write(out.path().join("t3-serve-integration/trace.json"), text).unwrap();
    (f, out)
}

#[test]
fn a_trace_serves_the_rows_its_passing_tests_called_and_the_evidence_file_is_what_a_later_check_reads() {
    // Given the written tables, and a trace whose passing test called a probe row and a command that forwards to thread.create, and whose failing test called another
    let f = Fixture::at_pin("t3");
    assert_eq!(f.run(&[]).code, 0);
    let rows: Vec<String> = f.methods().into_iter().filter(|(id, r)| r["phone"] == true && id.starts_with("rpc:")).map(|(id, _)| id).collect();
    let (probe, other) = (rows[0].as_str(), rows[1].as_str());
    let text = trace(
        true,
        &[
            ("chain::passes", "pass", vec![(vec![probe], true, vec![], vec![]), (vec!["command:reached.create"], true, vec!["thread.create", "turn.start"], vec!["write_file"])]),
            ("chain::fails", "fail", vec![(vec![other], true, vec![], vec![])]),
        ],
    );
    let out = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(out.path().join("t3-serve-integration")).unwrap();
    std::fs::write(out.path().join("t3-serve-integration/trace.json"), &text).unwrap();
    let dir = out.path().to_str().unwrap();

    // When the tables are written from the trace's directory
    let run = f.run(&["--traces", dir]);

    // Then the two rows are served, each with its test, and the failing test's row is not
    assert_eq!((run.code, problems(&run)), (0, vec![]), "{}", run.stdout);
    assert_eq!(run.json["wrote"], json!([f.table("-evidence.json"), f.table(".json"), f.table(".md")]));
    let table = f.coverage_rows();
    assert_eq!((table[probe]["state"].as_str(), table[probe]["evidence"]["test"].as_str()), (Some("served"), Some("chain::passes")));
    assert_eq!((table["command:reached.create"]["state"].as_str(), table["command:reached.create"]["evidence"]["agent"].clone()), (Some("served"), json!(["thread.create", "turn.start"])));
    assert_eq!(table["command:reached.create"]["evidence"]["executor"], json!(["write_file"]));
    assert_eq!(table[other]["state"], "pending");
    assert_eq!(run.json["summary"]["served"], 2);
    // And the evidence file is the record, naming the runs it was read from, and the markdown names the test
    let evidence = f.json(&f.table("-evidence.json"));
    assert_eq!((evidence["rows"].as_object().unwrap().len(), evidence["source"]["runs"][0]["suite"].as_str(), evidence["source"]["traces"].as_str()), (2, Some("t3-serve-integration"), None));
    assert_eq!(run.json["evidence"]["refreshedFrom"], Value::Null);
    assert!(f.text(&f.table(".md")).contains("evidence: `t3-serve-integration` `chain::passes`"));

    // When the tables are checked without the run, Then the checked-in evidence stands and nothing is open
    let check = f.run(&["--check"]);
    assert_eq!((check.code, problems(&check), check.json["summary"]["served"].clone()), (0, vec![], json!(2)), "{}", check.stdout);
    // And checked against the same run, nothing is stale either
    let again = f.run(&["--check", "--traces", dir]);
    assert_eq!((again.code, problems(&again)), (0, vec![]), "{}", again.stdout);

    // When another test would be the evidence (a name that sorts first), Then the check against that run names the evidence file stale
    std::fs::write(out.path().join("t3-serve-integration/trace.json"), text.replace("chain::passes", "a_chain::passes")).unwrap();
    let moved = f.run(&["--check", "--traces", dir]);
    assert_eq!(moved.code, 1);
    assert!(problems(&moved).iter().any(|(r, rows)| r == "stale" && rows.contains(&f.table("-evidence.json"))), "{}", moved.stdout);
}

#[test]
fn a_refused_call_a_failed_test_an_incomplete_run_and_a_forwarding_row_without_its_target_serve_nothing() {
    // Given a trace in which the probe row's call was refused, a command's call was answered without its target method, a failed test called a row, and the run did not finish
    let f = Fixture::at_pin("t3");
    assert_eq!(f.run(&[]).code, 0);
    let rows: Vec<String> = f.methods().into_iter().filter(|(id, r)| r["phone"] == true && id.starts_with("rpc:")).map(|(id, _)| id).collect();
    let calls: Calls = vec![(vec![rows[0].as_str()], false, vec![], vec![]), (vec!["command:reached.create"], true, vec!["file.list"], vec![]), (vec![rows[1].as_str()], true, vec![], vec![])];
    let incomplete = trace(false, &[("chain::stopped", "pass", calls)]);
    let (f2, out) = (Fixture::at_pin("t3"), tempfile::tempdir().unwrap());
    assert_eq!(f2.run(&[]).code, 0);
    std::fs::write(out.path().join("trace.json"), incomplete).unwrap();
    // When the tables are written from the incomplete run
    let run = f2.run(&["--traces", out.path().to_str().unwrap()]);
    // Then no row is served
    assert_eq!((run.code, run.json["summary"]["served"].clone()), (0, json!(0)), "{}", run.stdout);
    // And with the run complete, the refused call and the command without its target still serve nothing, while the unrefused row does
    let calls: Calls = vec![(vec![rows[0].as_str()], false, vec![], vec![]), (vec!["command:reached.create"], true, vec!["file.list"], vec![]), (vec![rows[1].as_str()], true, vec![], vec![])];
    std::fs::write(out.path().join("trace.json"), trace(true, &[("chain::finished", "pass", calls)])).unwrap();
    let done = f2.run(&["--traces", out.path().to_str().unwrap()]);
    let table = f2.coverage_rows();
    assert_eq!(done.code, 0, "{}", done.stdout);
    assert_eq!((table[rows[0].as_str()]["state"].as_str(), table["command:reached.create"]["state"].as_str(), table[rows[1].as_str()]["state"].as_str()), (Some("pending"), Some("pending"), Some("served")));
    drop(f);
}

#[test]
fn a_trace_naming_a_row_the_phone_does_not_reach_fails_the_check_and_a_reverify_note_holds_a_row_a_trace_hit_would_serve() {
    // Given a trace whose passing test called a row nobody has and the probe row, and a reverify note on the probe row
    let f = Fixture::at_pin("t3");
    assert_eq!(f.run(&[]).code, 0);
    let probe = f.methods().into_iter().find(|(id, r)| r["phone"] == true && id.starts_with("rpc:")).map(|(id, _)| id).unwrap();
    let mut decisions = f.json(&f.table("-decisions.json"));
    decisions["reverify"] = json!({probe.clone(): "the mapper answers it with a canned value"});
    f.write(&f.table("-decisions.json"), &serde_json::to_string_pretty(&decisions).unwrap());
    let out = tempfile::tempdir().unwrap();
    std::fs::write(out.path().join("trace.json"), trace(true, &[("chain::t", "pass", vec![(vec!["rpc:nobody.has.this", probe.as_str()], true, vec![], vec![])])])).unwrap();
    // When written from it
    let run = f.run(&["--traces", out.path().to_str().unwrap()]);
    // Then the unknown row is a problem, and the held row stays pending with its note and no evidence
    assert!(problems(&run).iter().any(|(r, rows)| r == "unknown-trace-row" && rows == &["rpc:nobody.has.this".to_string()]), "{}", run.stdout);
    let table = f.coverage_rows();
    assert_eq!((table[probe.as_str()]["state"].as_str(), table[probe.as_str()]["reverify"].as_str()), (Some("pending"), Some("the mapper answers it with a canned value")));
    assert!(table[probe.as_str()].get("evidence").is_none());
    // And a decision that holds a row the phone does not reach is a stale decision
    decisions["reverify"] = json!({"rpc:nobody.has.this": "x"});
    f.write(&f.table("-decisions.json"), &serde_json::to_string_pretty(&decisions).unwrap());
    assert!(problems(&f.run(&["--check"])).iter().any(|(r, rows)| r == "stale-decision" && rows == &["rpc:nobody.has.this".to_string()]));
}

#[test]
fn traces_are_read_from_a_trace_file_a_bundle_and_a_dir_and_a_source_with_none_is_refused_with_the_next_step() {
    // Given a written fixture and a trace
    let text = trace(true, &[("chain::t", "pass", vec![(vec!["rpc:reached.probe"], true, vec![], vec![])])]);
    let (f, out) = with_trace(&text);
    let file = out.path().join("t3-serve-integration/trace.json");
    // When the tables are written from the trace file, Then the row is served
    assert_eq!(f.run(&["--traces", file.to_str().unwrap()]).code, 0);
    assert_eq!(f.coverage_rows()["rpc:reached.probe"]["state"], "served");
    // And from a bundle that lists the trace beside it as a path, as a downloaded bundle does
    std::fs::write(out.path().join("bundle.json"), json!({"schema": 1, "runId": "run-1", "traces": ["t3-serve-integration/trace.json"]}).to_string()).unwrap();
    let bundle = f.run(&["--check", "--traces", out.path().join("bundle.json").to_str().unwrap()]);
    assert_eq!((bundle.code, problems(&bundle)), (0, vec![]), "{}", bundle.stdout);
    // And an empty dir, a missing path and a file that is neither a trace nor a bundle are refused with what to give instead
    let empty = tempfile::tempdir().unwrap();
    std::fs::write(empty.path().join("notes.json"), "{}").unwrap();
    for (source, says) in [(empty.path().to_str().unwrap().to_string(), "no trace.json"), (out.path().join("missing").to_str().unwrap().to_string(), "missing"), (empty.path().join("notes.json").to_str().unwrap().to_string(), "neither a method-trace/1 file nor a bundle")] {
        let refused = f.run(&["--traces", &source]);
        assert_eq!(refused.code, 1, "{source}: {}", refused.stdout);
        assert!(refused.json["error"].as_str().unwrap().contains(says) && refused.json["next"].as_str().is_some(), "{source}: {}", refused.stdout);
    }
}

/// A trace of oracle replays: one test per `(name, cassette provenance, [(diff label, answered)])`, each call naming `row`.
fn replay_trace(row: &str, tests: &[(&str, &str, &[&str])]) -> String {
    let tests: Vec<Value> = tests
        .iter()
        .map(|(name, cassette, diffs)| json!({"test": name, "cassette": cassette, "verdict": "pass", "ms": 1, "calls": diffs.iter().map(|d| json!({"id": d, "rows": [row], "ok": matches!(*d, "clean" | "allowlisted"), "diff": d, "agent": [], "executor": []})).collect::<Vec<_>>()}))
        .collect();
    serde_json::to_string_pretty(&json!({"schema": "method-trace/1", "app": "t3", "suite": "replay", "sha": null, "complete": true, "tests": tests})).unwrap()
}

#[test]
fn a_row_decided_oracle_mimic_is_served_only_by_a_clean_or_allowlisted_diff_of_a_recorded_cassette_and_the_table_names_it() {
    // Given the written tables, and the probe row decided oracle: mimic
    let f = Fixture::at_pin("t3");
    assert_eq!(f.run(&[]).code, 0);
    let probe = f.methods().into_iter().find(|(id, r)| r["phone"] == true && id.starts_with("rpc:")).map(|(id, _)| id).unwrap();
    let mut decisions = f.json(&f.table("-decisions.json"));
    decisions["decisions"][probe.as_str()] = json!({"oracle": "mimic"});
    f.write(&f.table("-decisions.json"), &serde_json::to_string_pretty(&decisions).unwrap());
    let out = tempfile::tempdir().unwrap();
    let write = |text: String| std::fs::write(out.path().join("trace.json"), text).unwrap();
    let served = |f: &Fixture| f.coverage_rows()[probe.as_str()].clone();
    // When the replays of the row drifted, or came from a cassette the reference did not record, Then the row stays pending
    write(replay_trace(&probe, &[("set::a", "recorded", &["drift"]), ("set::b", "authored", &["clean"]), ("set::c", "imported", &["clean"])]));
    let run = f.run(&["--traces", out.path().to_str().unwrap()]);
    assert_eq!((run.code, served(&f)["state"].as_str()), (0, Some("pending")), "{}", run.stdout);
    // And a chain call that carries no diff label never serves it either
    write(trace(true, &[("chain::t", "pass", vec![(vec![probe.as_str()], true, vec![], vec![])])]));
    assert_eq!(f.run(&["--traces", out.path().to_str().unwrap()]).code, 0);
    assert_eq!(served(&f)["state"], "pending");
    // When a clean replay of a recorded cassette is the evidence, Then the row is served and its entry and the markdown name the diff and the cassette
    write(replay_trace(&probe, &[("set::z", "recorded", &["clean"])]));
    let run = f.run(&["--traces", out.path().to_str().unwrap()]);
    assert_eq!((run.code, problems(&run)), (0, vec![]), "{}", run.stdout);
    let row = served(&f);
    assert_eq!((row["state"].as_str(), row["evidence"]["oracle"].as_str(), row["evidence"]["diff"].as_str(), row["evidence"]["cassette"].as_str()), (Some("served"), Some("mimic"), Some("clean"), Some("recorded")));
    assert!(f.text(&f.table(".md")).contains("(mimic replay: clean diff on a recorded cassette)"));
    // And the checked-in evidence stands for a later check
    assert_eq!(problems(&f.run(&["--check"])), vec![]);
    // And an allowlisted diff serves it too
    write(replay_trace(&probe, &[("set::z", "recorded", &["allowlisted"])]));
    f.run(&["--traces", out.path().to_str().unwrap()]);
    assert_eq!(served(&f)["evidence"]["diff"], "allowlisted");
}

#[test]
fn an_evidence_file_that_recorded_a_scratch_path_fails_the_check_until_a_refresh_replaces_it() {
    // Given a served probe row whose checked-in evidence says it was read from /tmp
    let text = trace(true, &[("chain::t", "pass", vec![(vec!["rpc:reached.probe"], true, vec![], vec![])])]);
    let (f, out) = with_trace(&text);
    assert_eq!(f.run(&["--traces", out.path().to_str().unwrap()]).code, 0);
    let mut evidence = f.json(&f.table("-evidence.json"));
    evidence["source"]["traces"] = json!("/tmp/orca-traces");
    f.write(&f.table("-evidence.json"), &serde_json::to_string_pretty(&evidence).unwrap());
    // When it is checked, Then the scratch path is a problem
    let check = f.run(&["--check"]);
    assert_eq!(check.code, 1, "{}", check.stdout);
    assert!(problems(&check).iter().any(|(r, _)| r == "scratch-trace-path"), "{}", check.stdout);
    // When the evidence is refreshed from the same run, Then the file is rewritten without it and the check passes
    let refreshed = f.run(&["--traces", out.path().to_str().unwrap()]);
    assert!(refreshed.json["wrote"].as_array().unwrap().contains(&json!(f.table("-evidence.json"))), "{}", refreshed.stdout);
    assert_eq!(f.json(&f.table("-evidence.json"))["source"].get("traces"), None);
    assert_eq!(problems(&f.run(&["--check"])), vec![]);
}
