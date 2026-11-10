//! The drift report of a tracker run: what the plan found (each app's pin against the newest release), what the capture jobs recorded
//! against the new pin, what the replay of each new set against our candidates differed in, and what the coverage rebuild said. Pure:
//! it reads the texts it is given and returns the files to write.
use serde_json::{json, Value};

pub const REPORT_SCHEMA: &str = "parity-track-report/1";

pub struct Input<'a> {
    pub plan: &'a str,
    /// `(file name, text)` of the capture outputs: `<set>.record.json`, `<set>.replay.json`, `coverage-<app>.json`.
    pub files: &'a [(String, String)],
    pub run: &'a str,
    pub sha: &'a str,
    pub now: &'a str,
}

fn named(files: &[(String, String)], name: &str) -> Option<Value> {
    files.iter().find(|(n, _)| n == name).and_then(|(_, t)| serde_json::from_str(t).ok())
}

fn recorded(job: Option<Value>) -> &'static str {
    match job {
        None => "missing",
        Some(j) if j["incomplete"] == true => "incomplete",
        Some(j) if j["ok"] == true => "complete",
        Some(_) => "failed",
    }
}

fn replay_of(job: Option<Value>) -> Value {
    let Some(job) = job else { return Value::Null };
    let rows: Vec<Value> = job["candidates"].as_array().into_iter().flatten().map(|c| json!({"candidate": c["candidate"], "severity": c["severity"], "total": c["summary"]["total"], "clean": c["summary"]["clean"], "allowlisted": c["summary"]["allowlisted"], "drift": c["summary"]["drift"], "breaking": c["summary"]["breaking"]})).collect();
    json!({"ok": job["ok"], "candidates": rows})
}

fn differs(replay: &Value) -> u64 {
    replay["candidates"].as_array().into_iter().flatten().map(|c| c["drift"].as_u64().unwrap_or(0) + c["breaking"].as_u64().unwrap_or(0)).sum()
}

/// Which of the sets belongs to the app: its name starts with the app's (`t3-recorded-1`, `orca-recorded`).
fn app_sets<'a>(sets: &'a [String], app: &str) -> Vec<&'a String> {
    sets.iter().filter(|s| s.starts_with(&format!("{app}-"))).collect()
}

pub fn build(input: &Input) -> Result<Vec<(String, String)>, String> {
    let plan: Value = serde_json::from_str(input.plan).map_err(|e| format!("the plan is not JSON: {e}"))?;
    let verdicts = plan["verdicts"].as_array().ok_or("the plan has no verdicts: it is the output of wf parity track plan")?;
    let sets: Vec<String> = plan["sets"].as_array().into_iter().flatten().filter_map(|s| s.as_str().map(String::from)).collect();
    let mut apps = vec![];
    for v in verdicts {
        let app = v["app"].as_str().unwrap_or("");
        let drifted = v["drifted"] == true;
        let set_rows: Vec<Value> = if drifted {
            app_sets(&sets, app).iter().map(|set| json!({"set": set, "recorded": recorded(named(input.files, &format!("{set}.record.json"))), "replay": replay_of(named(input.files, &format!("{set}.replay.json")))})).collect()
        } else {
            vec![]
        };
        let coverage = named(input.files, &format!("coverage-{app}.json")).map(|c| json!({"ok": c["ok"], "pending": c["summary"]["pending"], "problems": c["problems"]}));
        let status = if !drifted {
            "current"
        } else if set_rows.iter().any(|s| s["recorded"] != "complete") {
            "incomplete"
        } else if set_rows.iter().map(|s| differs(&s["replay"])).sum::<u64>() > 0 {
            "discrepancies"
        } else if coverage.as_ref().is_some_and(|c| c["ok"] == false || c["problems"].as_array().is_some_and(|p| !p.is_empty())) {
            "needs-work"
        } else {
            "clean"
        };
        apps.push(json!({"app": app, "pinned": v["pinned"], "latest": v["latest"], "drifted": drifted, "status": status, "sets": set_rows, "coverage": coverage}));
    }
    let incomplete = apps.iter().any(|a| a["status"] == "incomplete");
    let next = if apps.iter().all(|a| a["status"] == "current") {
        "every pin is at the newest release: nothing to do".to_string()
    } else if incomplete {
        "a capture did not finish: push the same [track ...] marker again to resume it".to_string()
    } else {
        "bump the pin (wf parity track bump --app <app>), fetch the new sets (wf mimic fetch <run>), regenerate the tables and fix the discrepancies through the normal gate; what cannot be fixed goes in the body of issue #365".to_string()
    };
    let report = json!({"schema": REPORT_SCHEMA, "ok": !incomplete, "run": input.run, "sha": input.sha, "apps": apps, "next": next});
    let tests: Vec<Value> = apps
        .iter()
        .map(|a| {
            let failed = a["status"] == "incomplete";
            json!({"name": format!("track {}: {}", a["app"].as_str().unwrap_or(""), a["status"].as_str().unwrap_or("")), "file": "track-contracts", "status": if failed { "failed" } else { "passed" }, "goldens": [], "stops": [], "warnings": [], "errors": if failed { json!([{"message": format!("the capture of {} did not finish", a["app"])}]) } else { json!([]) }})
        })
        .collect();
    let failed = tests.iter().filter(|t| t["status"] == "failed").count();
    let flow = json!({"schema": "flow-report/1", "root": "track-contracts", "startedAt": input.now, "finishedAt": input.now, "verdict": if failed == 0 { "passed" } else { "failed" }, "durationMs": 0, "summary": {"total": tests.len(), "failed": failed},
        "tests": tests, "warnings": [], "errors": [], "paths": {"html": "report.html"}});
    let markdown = markdown(&report);
    let html = format!("<!doctype html><meta charset=\"utf-8\"><title>Contract tracker</title><pre>{}</pre>\n", escape(&markdown));
    let text = |v: &Value| serde_json::to_string_pretty(v).map(|t| t + "\n").map_err(|e| e.to_string());
    Ok(vec![("drift-report.json".into(), text(&report)?), ("report.json".into(), text(&flow)?), ("report.html".into(), html), ("summary.md".into(), markdown)])
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn markdown(report: &Value) -> String {
    let mut md = String::from("### Contract tracker\n\n| app | pinned | latest | status | sets | differences | coverage problems |\n|---|---|---|---|---|---|---|\n");
    for a in report["apps"].as_array().into_iter().flatten() {
        let sets = a["sets"].as_array().map(|s| s.iter().map(|r| format!("{} {}", r["set"].as_str().unwrap_or(""), r["recorded"].as_str().unwrap_or(""))).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let diffs: u64 = a["sets"].as_array().into_iter().flatten().map(|s| differs(&s["replay"])).sum();
        let problems = a["coverage"]["problems"].as_array().map_or(0, Vec::len);
        md += &format!("| {} | {} | {} | {} | {} | {} | {} |\n", a["app"].as_str().unwrap_or(""), a["pinned"].as_str().unwrap_or(""), a["latest"].as_str().unwrap_or(""), a["status"].as_str().unwrap_or(""), sets, diffs, problems);
    }
    md + &format!("\nNext: {}\n", report["next"].as_str().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> String {
        json!({"verdicts": [{"app": "t3", "pinned": "0.1.6", "latest": "0.1.7", "drifted": true}, {"app": "orca", "pinned": "1.4.214", "latest": "1.4.214", "drifted": false}], "sets": ["t3-recorded-1", "t3-recorded-2"]}).to_string()
    }

    fn file(name: &str, v: Value) -> (String, String) {
        (name.to_string(), v.to_string())
    }

    fn built(files: &[(String, String)]) -> BTreeMapOf {
        let input = Input { plan: &plan(), files, run: "42", sha: "abc", now: "2026-11-09T00:00:00Z" };
        build(&input).unwrap().into_iter().collect()
    }

    type BTreeMapOf = std::collections::BTreeMap<String, String>;

    fn status_of(out: &BTreeMapOf, app: &str) -> String {
        let report: Value = serde_json::from_str(&out["drift-report.json"]).unwrap();
        report["apps"].as_array().unwrap().iter().find(|a| a["app"] == app).unwrap()["status"].as_str().unwrap().to_string()
    }

    #[test]
    fn a_pin_at_the_newest_release_is_current_and_needs_no_capture() {
        // Given a plan where only T3 drifted and no capture output at all
        let out = built(&[]);
        // Then Orca is current, and T3 is incomplete because its sets were never recorded
        assert_eq!(status_of(&out, "orca"), "current");
        assert_eq!(status_of(&out, "t3"), "incomplete");
        // And the run is not ok and its flow report carries the failure for the dashboards
        let flow: Value = serde_json::from_str(&out["report.json"]).unwrap();
        assert_eq!((flow["schema"].as_str(), flow["verdict"].as_str(), flow["summary"]["failed"].as_u64()), (Some("flow-report/1"), Some("failed"), Some(1)));
    }

    #[test]
    fn a_replay_that_differs_makes_the_drift_a_list_of_discrepancies() {
        // Given both T3 sets recorded and one replayed with drift against the candidate
        let ok = json!({"ok": true, "complete": true});
        let replay = json!({"ok": false, "candidates": [{"candidate": "t3-serve", "severity": "drift", "summary": {"total": 10, "clean": 7, "allowlisted": 0, "drift": 3, "breaking": 0}}]});
        let out = built(&[file("t3-recorded-1.record.json", ok.clone()), file("t3-recorded-2.record.json", ok), file("t3-recorded-1.replay.json", replay)]);
        // Then T3 has discrepancies, the report names the set and its counts, and the run is ok (a discrepancy is a finding, not a failed run)
        assert_eq!(status_of(&out, "t3"), "discrepancies");
        let report: Value = serde_json::from_str(&out["drift-report.json"]).unwrap();
        assert_eq!(report["ok"], true);
        assert_eq!(report["apps"][0]["sets"][0]["replay"]["candidates"][0]["drift"], 3);
        assert!(out["summary.md"].contains("t3-recorded-1 complete"));
    }

    #[test]
    fn a_checkpointed_record_is_incomplete_and_a_clean_replay_with_clean_tables_is_clean() {
        // Given one set that ran out of its budget
        let out = built(&[file("t3-recorded-1.record.json", json!({"ok": false, "incomplete": true})), file("t3-recorded-2.record.json", json!({"ok": true}))]);
        // Then T3 is incomplete and the next step says to push the marker again
        assert_eq!(status_of(&out, "t3"), "incomplete");
        assert!(serde_json::from_str::<Value>(&out["drift-report.json"]).unwrap()["next"].as_str().unwrap().contains("push the same [track"));
        // Given both recorded, replays clean and a coverage rebuild without problems
        let clean = json!({"ok": true, "candidates": [{"candidate": "t3-serve", "severity": "clean", "summary": {"total": 4, "clean": 4, "allowlisted": 0, "drift": 0, "breaking": 0}}]});
        let out = built(&[file("t3-recorded-1.record.json", json!({"ok": true})), file("t3-recorded-2.record.json", json!({"ok": true})), file("t3-recorded-1.replay.json", clean), file("coverage-t3.json", json!({"ok": true, "summary": {"pending": 12}, "problems": []}))]);
        // Then it is clean, and a coverage problem would turn it into needs-work
        assert_eq!(status_of(&out, "t3"), "clean");
        let out = built(&[file("t3-recorded-1.record.json", json!({"ok": true})), file("t3-recorded-2.record.json", json!({"ok": true})), file("coverage-t3.json", json!({"ok": false, "problems": [{"rule": "stale"}]}))]);
        assert_eq!(status_of(&out, "t3"), "needs-work");
    }

    #[test]
    fn a_plan_that_is_not_the_output_of_the_planner_is_refused() {
        // Given a text that is no plan, Then building says what it expected
        let e = build(&Input { plan: "{}", files: &[], run: "1", sha: "s", now: "n" }).unwrap_err();
        assert!(e.contains("wf parity track plan"), "{e}");
    }
}
