//! Flows through the ladder plugin `wf offload ask` and the service build on, replaying `tests/flow/golden/rca-ladder.json`:
//! a dispatch (ladder, cooldown ledger, `run_once`, the file stores).
#[path = "flow/golden.rs"]
mod golden;
#[path = "flow/script_host.rs"]
mod script_host;

use golden::{golden, norm, opt, s, TempDir};
use rca_core::file_store::FileRunLog;
use rca_ladder::file_store::FileLedgerStore;
use rca_ladder::ladder::{classify_failure, model_ladders, next_rung, next_rung_index, resolve_ladder, should_escalate, Attempt, LadderFile};
use rca_ladder::pool_ledger::{clear_cooldown, cold_pools, cold_window_for_reset_hint, mark_cold, next_ledger, observe_ok, record_timeout, Ledger};
use rca_ladder::run::{run_once, RunInput};
use rca_core::run_log::{parse_run_log, RunLogEntry};
use rca_core::stats::ShapeStats;
use rca_core::time::next_utc_midnight;
use script_host::ScriptHost;
use serde_json::Value;
use std::collections::HashSet;
use std::io::Write;

type AttemptRow = (usize, String, Option<String>, bool, Option<String>);

fn ledger_of(v: &Value) -> Ledger {
    serde_json::from_value(v.clone()).unwrap()
}

#[test]
fn given_the_ladder_and_a_cooldown_ledger_on_disk_when_dispatches_run_then_rungs_rotate_and_escalate_and_the_ledger_and_run_log_persist_as_the_typescript_did() {
    let g = golden();
    let ladders = model_ladders();

    // Given the failure, escalation and ladder policy tables
    // Then each decision matches the recorded one
    for c in g["classify"].as_array().unwrap() {
        let i = &c["input"];
        assert_eq!(classify_failure(s(&i["reason"]), s(&i["output"]), i["wroteNothing"].as_bool().unwrap_or(false)), s(&c["out"]), "{i}");
    }
    for c in g["escalate"].as_array().unwrap() {
        assert_eq!(should_escalate(opt(&c["previous"]), s(&c["current"])), c["out"].as_bool().unwrap(), "{c}");
    }
    for c in g["resolve"].as_array().unwrap() {
        let i = &c["input"];
        let cold: HashSet<String> = i["cold"].as_array().unwrap().iter().map(|v| s(v).to_string()).collect();
        let stats: Option<ShapeStats> = (!i["stats"].is_null()).then(|| serde_json::from_value(i["stats"].clone()).unwrap());
        let pins: Option<Vec<String>> = (!i["pins"].is_null()).then(|| serde_json::from_value(i["pins"].clone()).unwrap());
        let want: Vec<String> = serde_json::from_value(c["out"].clone()).unwrap();
        assert_eq!(resolve_ladder(s(&i["shape"]), &ladders, &cold, stats.as_ref(), pins.as_deref()), want, "{i}");
    }
    let file: LadderFile = serde_json::from_value(g["nextRung"]["ladderFile"].clone()).unwrap();
    let ladder: Vec<String> = serde_json::from_value(g["nextRung"]["ladder"].clone()).unwrap();
    for c in g["nextRung"]["cases"].as_array().unwrap() {
        let (index, escalated) = next_rung(&file, &ladder, c["from"].as_u64().unwrap() as usize, s(&c["current"]), opt(&c["previous"]));
        assert_eq!((index as u64, escalated), (c["out"]["index"].as_u64().unwrap(), c["out"]["escalated"].as_bool().unwrap()), "{c}");
    }
    let file: LadderFile = serde_json::from_value(g["nextRungIndex"]["ladderFile"].clone()).unwrap();
    let ladder: Vec<String> = serde_json::from_value(g["nextRungIndex"]["ladder"].clone()).unwrap();
    for c in g["nextRungIndex"]["cases"].as_array().unwrap() {
        let tried: Vec<Attempt> = serde_json::from_value(c["tried"].clone()).unwrap();
        assert_eq!(next_rung_index(&file, &ladder, &tried).map(|i| i as u64), c["out"].as_u64(), "{c}");
    }

    // Given ledger scripts of cold marks, recoveries, timeouts with reset hints and clears
    // Then every intermediate ledger and the cold pools at each probe time match
    for script in g["ledgerScripts"].as_array().unwrap() {
        let mut ledger = ledger_of(&script["seed"]);
        let probes: Vec<i64> = script["probes"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
        for (op, step) in script["ops"].as_array().unwrap().iter().zip(script["steps"].as_array().unwrap()) {
            let (pool, at) = (s(&op["pool"]), op["at"].as_i64().unwrap());
            ledger = match s(&op["op"]) {
                "markCold" => mark_cold(&ledger, pool, at),
                "observeOk" => observe_ok(&ledger, pool, at),
                "recordTimeout" => record_timeout(&ledger, pool, at, opt(&op["resetHint"])),
                "clearCooldown" => clear_cooldown(&ledger, pool, at),
                other => panic!("unknown op {other}"),
            };
            assert_eq!(serde_json::to_value(&ledger).unwrap(), step["ledger"], "{op}");
            for (probe, want) in probes.iter().zip(step["cold"].as_array().unwrap()) {
                let want: Vec<String> = serde_json::from_value(want.clone()).unwrap();
                assert_eq!(cold_pools(&ledger, *probe).into_iter().collect::<Vec<_>>(), want, "{op} at {probe}");
            }
        }
    }
    for c in g["nextLedger"].as_array().unwrap() {
        let verdict = &c["verdict"];
        let class = if verdict["ok"].as_bool().unwrap() { None } else { Some(s(&verdict["failureClass"])) };
        let got = next_ledger(&ledger_of(&c["ledger"]), s(&c["pool"]), class, c["at"].as_i64().unwrap(), c["attemptMs"].as_f64().unwrap_or(f64::INFINITY), opt(&c["resetHint"]));
        assert_eq!(serde_json::to_value(&got).unwrap(), c["out"], "{c}");
    }
    for c in g["coldWindow"].as_array().unwrap() {
        assert_eq!(cold_window_for_reset_hint(opt(&c["resetHint"]), c["now"].as_i64().unwrap()), c["out"].as_i64().unwrap(), "{c}");
    }
    assert_eq!(next_utc_midnight(1_791_288_000_000), 1_791_331_200_000);

    // Given each recorded dispatch scenario, its seed ledger on disk and a scripted opencode
    for case in g["runner"].as_array().unwrap() {
        let sc = &case["scenario"];
        let name = s(&sc["name"]);
        let dir = TempDir::new(&format!("run-{name}").replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "_"));
        let store = FileLedgerStore::new(dir.0.join("nested/pool-cooldowns.json"));
        assert!(store.read().is_empty(), "{name}: a missing ledger reads as empty");
        store.write(&ledger_of(&sc["seedLedger"])).unwrap();
        let input = RunInput {
            run_id: "run-1".into(),
            cwd: "/repo".into(),
            writes_expected: sc["auto"].as_bool().unwrap(),
            prompt: s(&sc["prompt"]).into(),
            shape: opt(&sc["shape"]).map(String::from),
            pins: serde_json::from_value(sc["pins"].clone()).unwrap(),
            honor_cold: sc["honorCold"].as_bool().unwrap(),
            caller: s(&sc["caller"]).into(),
            tier: opt(&sc["tier"]).map(String::from),
            claude_session_id: None,
            run_started_at: 1_791_288_000_000,
        };
        let mut host = ScriptHost::new(sc["script"].as_array().unwrap(), input.run_started_at, store, FileRunLog::new(dir.0.join("deep/runs.jsonl")), opt(&sc["badOutputContains"]).map(String::from));

        // When the run plays out
        let out = run_once(&mut host, &input, &ladders);

        // Then its outcome, attempts, run log rows and ledger writes are the recorded ones, and both files hold them
        let want = &case["result"];
        assert_eq!(out.ok, want["ok"].as_bool().unwrap(), "{name}: ok");
        assert_eq!(out.ladder_exhausted, want["reason"].as_str() == Some("ladder-exhausted"), "{name}: reason");
        assert_eq!(out.ok_attempt.map(|i| i as u64), want["okAttempt"].as_u64(), "{name}: okAttempt");
        let got_attempts: Vec<AttemptRow> = out
            .attempts
            .iter()
            .map(|a| (a.index, a.model.clone(), a.pool.clone(), a.skipped.is_some(), a.verdict.as_ref().map(|v| if v.ok { "ok".to_string() } else { v.failure_class.clone().unwrap_or_default() })))
            .collect();
        let want_attempts: Vec<AttemptRow> = want["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| {
                let verdict = a["verdict"].as_object().map(|v| if v["ok"].as_bool().unwrap() { "ok".to_string() } else { s(&v["failureClass"]).to_string() });
                (a["index"].as_u64().unwrap() as usize, s(&a["model"]).to_string(), opt(&a["pool"]).map(String::from), a["skipped"].is_object(), verdict)
            })
            .collect();
        assert_eq!(got_attempts, want_attempts, "{name}: attempts");
        assert_eq!(norm(&Value::Array(host.rows.clone())), norm(&case["runLog"]), "{name}: run log rows");
        assert_eq!(norm(&Value::Array(host.ledger_writes.clone())), norm(&case["ledgerWrites"]), "{name}: ledger writes");
        assert_eq!(norm(&serde_json::to_value(host.store.read()).unwrap()), norm(&case["finalLedger"]), "{name}: final ledger on disk");
        assert!(!dir.0.join("nested/pool-cooldowns.json.tmp").exists(), "{name}: the ledger is renamed into place");
        let logged: Vec<RunLogEntry> = host.rows.iter().map(|r| serde_json::from_value(r.clone()).unwrap()).collect();
        assert_eq!(host.log.read_all(), logged, "{name}: run log on disk");
        assert_eq!(host.executed as u64, case["executed"].as_u64().unwrap(), "{name}: executed");

        // When the ledger file is corrupted, then it reads as empty
        std::fs::write(dir.0.join("nested/pool-cooldowns.json"), "{nope").unwrap();
        assert!(host.store.read().is_empty(), "{name}: corrupt ledger");
    }

    // Given a run log with a torn line between two rows, one a legacy row without a caller
    let dir = TempDir::new("torn");
    let log = FileRunLog::new(dir.0.join("deep/runs.jsonl"));
    assert!(log.read_all().is_empty());
    let row = |ts: &str| parse_run_log(&format!("{{\"ts\":\"{ts}\",\"ok\":true,\"model\":\"m\"}}")).remove(0);
    log.append(&row("2026-10-06T10:00:00.000Z")).unwrap();
    std::fs::OpenOptions::new().append(true).open(log.path()).unwrap().write_all(b"{\"ts\":\"torn\n").unwrap();
    log.append(&row("2026-10-06T11:00:00.000Z")).unwrap();
    // When it is read back
    let rows = log.read_all();
    // Then the torn line is skipped and the legacy row is a dispatch
    assert_eq!(rows.iter().map(|r| r.ts.as_str()).collect::<Vec<_>>(), ["2026-10-06T10:00:00.000Z", "2026-10-06T11:00:00.000Z"]);
    assert_eq!(rows[0].caller, "dispatch");
}

