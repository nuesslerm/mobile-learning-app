//! `wf mimic proxy replay`: a cassette set answers an unmodified client on a loopback address. The verb opens no connection of
//! its own, and a request the recording lacks fails the run.
use super::proxy_record_verb::masks;
use super::proxy_verb::{listen_addr, serve_until};
use super::{fail, load_config, set_ref, Done};
use crate::adapters::conn;
use crate::adapters::replay_server::ReplayServer;
use mimic_proxy_core::application::record::policy_for;
use mimic_proxy_core::application::replay::Replayer;
use mimic_proxy_core::domain::matching::Tolerance;
use mimic_proxy_core::domain::sanitize::Policy;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use wf_kit::args::Parsed;

const NEXT: &str = "wf mimic proxy replay <set> --listen 127.0.0.1:<port>";
const SETTLE_MS: u64 = 300;
const MAX_REPORTED: usize = 20;

/// `$WF_TEST_OUTPUT_DIR/<suite>/report.json`: the run as the harness record the CI bundle collects.
fn write_report(suite: &str, set: &str, summary: &Value, mismatches: &[Value], started: i64) -> Result<PathBuf, String> {
    let base = std::env::var_os("WF_TEST_OUTPUT_DIR").filter(|d| !d.is_empty()).ok_or("--suite needs WF_TEST_OUTPUT_DIR")?;
    let dir = PathBuf::from(base).join(suite);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (finished, ok) = (wf_kit::time::now_ms(), mismatches.is_empty());
    let verdict = if ok { "pass" } else { "fail" };
    let mut cases = vec![json!({"name": format!("{set}::replay"), "verdict": verdict, "ms": (finished - started).max(0), "message": Value::Null})];
    cases.extend(mismatches.iter().map(|m| json!({"name": format!("{set}::{}", m["what"].as_str().unwrap_or("request")), "verdict": "fail", "ms": 0, "message": m["reason"]})));
    let report = json!({
        "schema": 1, "verdict": verdict, "ok": ok, "exitCode": i32::from(!ok), "message": Value::Null, "expectedCases": cases.len(), "script": suite,
        "platform": "integration", "sha": Value::Null, "startedMs": started, "finishedMs": finished, "started": wf_kit::time::iso(started),
        "finished": wf_kit::time::iso(finished), "durationS": ((finished - started).max(0) as f64 / 100.0).round() / 10.0, "recordDir": dir.display().to_string(),
        "cases": cases, "summary": summary, "mismatches": mismatches, "warnings": [],
    });
    let path = dir.join("report.json");
    std::fs::write(&path, serde_json::to_string_pretty(&report).expect("prints") + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn replay_verb(p: &Parsed) -> Done {
    let set = set_ref(&p.positional[0]);
    let content = set.read()?;
    let listen = listen_addr(p.val("listen").unwrap_or("127.0.0.1:0"))?;
    let number = |name: &str| p.val(name).map(|v| v.parse::<u64>().map_err(|_| fail(format!("--{name} is not a number"), "wf mimic proxy replay --help"))).transpose();
    let (duration, settle) = (number("duration")?.filter(|d| *d > 0), number("settle-ms")?.unwrap_or(SETTLE_MS));
    let policy = policy_for(&content.rules, Policy::default());
    let toml = set.dir.join("mimic.toml");
    let ignore_keys = if toml.is_file() { load_config(&toml)?.replay_ignore_keys } else { Vec::new() };
    let tolerance = Tolerance { ignore_keys, machine: masks(p)? };
    let replayer = Replayer::new(&content, &policy.secret_keys, tolerance, settle).map_err(|e| fail(e, format!("fix {}", set.dir.join("rules.json").display())))?;
    let replayer = Arc::new(Mutex::new(replayer));
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        let _ = signal_hook::flag::register(signal, stop.clone());
    }
    let started = wf_kit::time::now_ms();
    let server = ReplayServer::listen(listen, replayer.clone(), stop.clone()).map_err(|e| fail(format!("the replay did not listen on {listen}: {e}"), NEXT))?;
    let bound = server.addr();
    if let Some(ready) = p.val("ready") {
        let note = json!({"listen": bound.to_string(), "set": set.dir.display().to_string()});
        std::fs::write(ready, format!("{note}\n")).map_err(|e| fail(format!("{ready}: {e}"), "name a writable --ready file"))?;
    }
    eprintln!("mimic proxy replay: listening on {bound}, answering from {}", set.dir.display());
    let fail_fast = p.flag("fail-fast");
    serve_until(&stop, duration, &|| {
        if fail_fast && !replayer.lock().unwrap_or_else(|e| e.into_inner()).mismatches().is_empty() {
            stop.store(true, Ordering::SeqCst);
        }
        server.live()
    });
    drop(server);
    let replayer = replayer.lock().unwrap_or_else(|e| e.into_inner());
    let summary = replayer.summary();
    let mismatches: Vec<Value> = replayer.mismatches().iter().map(|m| serde_json::to_value(m).expect("a mismatch serialises")).collect();
    let ok = mismatches.is_empty();
    let report = match p.val("suite") {
        Some(suite) => Some(write_report(suite, &set.name, &summary, &mismatches, started).map_err(|e| fail(e, "set WF_TEST_OUTPUT_DIR to a writable directory"))?),
        None => None,
    };
    let next = match mismatches.first() {
        None => "nothing to do".to_string(),
        Some(first) => format!("read mismatches[0] ({}): record the journey again with wf mimic proxy record, or fix the client", first["what"].as_str().unwrap_or("request")),
    };
    let out = json!({
        "ok": ok, "set": set.name, "kind": content.cassette.header.provenance.kind, "listen": bound.to_string(), "upstream_connections": conn::dials(),
        "http": summary["http"], "ws": summary["ws"], "mismatch_count": mismatches.len(), "mismatches": mismatches.iter().take(MAX_REPORTED).collect::<Vec<_>>(),
        "unused": summary["unused"], "evidence": {"report": report.map(|r| r.display().to_string())}, "warnings": [], "next": next,
    });
    Ok((out, i32::from(!ok)))
}
