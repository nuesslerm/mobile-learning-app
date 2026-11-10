//! `wf offload ask --stats` and `--savings` (the rca service's run log, or `--log <file>`) and `--clear-cooldown <pool>` (the shared
//! cooldown ledger, through the service).
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_kit::time::now_ms;
use rca_core::savings::{
    compute_savings, default_ratios, format_savings, parse_delegation_report, DelegationReportRow, SavingsRatios,
};
use rca_core::stats::{filter_runs, format_stats, js_json_pretty, parse_since, stats, StatsFilter};
use rca_core::time::parse as parse_time;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Nearest ancestor holding `.git`, or `from` itself outside a checkout.
fn repo_root_from(from: &Path) -> PathBuf {
    let mut dir = from.to_path_buf();
    for _ in 0..20 {
        if dir.join(".git").exists() {
            return dir;
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    from.to_path_buf()
}

/// Warms a pool again in the shared ledger, through the rca service.
fn clear_cooldown(pool: &str) -> Option<i32> {
    match crate::rca_client::clear_cooldown(pool) {
        Ok(was_cold) => {
            println!("{pool}: {}", if was_cold { "cold mark cleared" } else { "already warm, counters cleared" });
            Some(0)
        }
        Err(e) => {
            wf_kit::wf_err!("wf: {e}");
            Some(1)
        }
    }
}

struct Window {
    log_path: String,
    since_ms: Option<i64>,
    rows: Vec<rca_core::run_log::RunLogEntry>,
}

/// The run log narrowed by `--since`, `--session` and `caller`; `None` where the TypeScript would report a
/// `--since` error or read a date shape only JavaScript knows.
fn window(p: &wf_kit::args::Parsed, caller: Option<String>) -> Result<Window, i32> {
    let since_ms = match p.opts.get("since").filter(|s| !s.is_empty()) {
        Some(spec) => match parse_since(Some(spec), now_ms()) {
            Some(ms) => Some(ms),
            None => {
                wf_kit::wf_err!("wf: --since must be like 7d, 24h, 90m or an ISO date, got \"{spec}\"");
                return Err(1);
            }
        },
        None => None,
    };
    let (log_path, text) = match p.opts.get("log") {
        Some(file) => (file.clone(), std::fs::read_to_string(file).unwrap_or_default()),
        None => match crate::rca_client::run_log() {
            Ok(text) => ("the rca service's run log".to_string(), text),
            Err(e) => {
                wf_kit::wf_err!("wf: {e}");
                return Err(1);
            }
        },
    };
    let rows = filter_runs(
        &rca_core::run_log::parse_run_log(&text),
        &StatsFilter {
            since_ms,
            claude_session_id: p.opts.get("session").filter(|s| !s.is_empty()).cloned(),
            caller,
        },
    );
    Ok(Window { log_path, since_ms, rows })
}

fn print_json(log_path: String, extra: Option<(&str, String)>, body: Value) {
    let mut doc = serde_json::Map::new();
    doc.insert("logPath".into(), Value::String(log_path));
    if let Some((k, v)) = extra {
        doc.insert(k.into(), Value::String(v));
    }
    if let Value::Object(fields) = body {
        doc.extend(fields);
    }
    println!("{}", js_json_pretty(&Value::Object(doc)));
}

fn stats_run(p: &wf_kit::args::Parsed) -> Option<i32> {
    let w = match window(p, p.opts.get("caller").filter(|s| !s.is_empty()).cloned()) {
        Ok(w) => w,
        Err(code) => return Some(code),
    };
    let summary = stats(&w.rows, None);
    if p.flags.iter().any(|f| f == "json") {
        print_json(w.log_path, None, serde_json::to_value(&summary).expect("stats serialize"));
    } else {
        println!("{} ({} run(s))\n\n{}", w.log_path, w.rows.len(), format_stats(&summary));
    }
    Some(0)
}

fn run_savings(p: &wf_kit::args::Parsed) -> Option<i32> {
    // The TypeScript hands `windowRows` the whole option bag, whose `caller` defaults to "dispatch", so by default
    // only dispatch rows are counted although mcp, cheap and batch runs are delegations too. Kept for parity.
    let caller = p.opts.get("caller").filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| "dispatch".into());
    let w = match window(p, Some(caller)) {
        Ok(w) => w,
        Err(code) => return Some(code),
    };
    let report_path = p.opts.get("report").cloned().unwrap_or_else(|| {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        format!("{}/.agents/tmp/delegation-report.jsonl", repo_root_from(&cwd).display())
    });
    let report: Vec<DelegationReportRow> = std::fs::read_to_string(&report_path)
        .map(|t| parse_delegation_report(&t))
        .unwrap_or_default()
        .into_iter()
        .filter(|r| w.since_ms.is_none_or(|since| parse_time(&r.ts).is_some_and(|t| t >= since)))
        .collect();
    let ratios = match p.opts.get("ratios") {
        Some(path) => serde_json::from_str::<SavingsRatios>(&std::fs::read_to_string(path).ok()?).ok()?,
        None => default_ratios(),
    };
    let savings = compute_savings(&w.rows, &ratios, &report);
    if p.flags.iter().any(|f| f == "json") {
        print_json(w.log_path, Some(("reportPath", report_path)), serde_json::to_value(&savings).expect("savings serialize"));
    } else {
        println!("{} ({} run(s) in window)\n\n{}", w.log_path, w.rows.len(), format_savings(&savings));
    }
    Some(0)
}

/// The values and flags of `dispatch --stats`, `--savings` and `--clear-cooldown`.
const STATS: Spec = Spec {
    values: &["since", "session", "caller", "log", "clear-cooldown", "report", "ratios"],
    flags: &["stats", "savings", "json"],
    positional: Positional::Any,
    ..Spec::NONE
};
const PRICING_SYNC: Spec = Spec { flags: &["pricing-sync"], positional: Positional::None, ..Spec::NONE };

pub fn run(args: &[String]) -> Option<i32> {
    // The mode is the one whose option is on the line, wherever it stands; anything that is not a mode is a prompt run.
    let modes = wf_kit::args::probe(args, &[&crate::dispatch_batch::SPEC, &crate::runbook::SPEC, &PRICING_SYNC, &STATS, &crate::dispatch_run::SPEC])?;
    let asked = |mode: &str| modes.flag(mode);
    if asked("batch") {
        return crate::dispatch_batch::run(args);
    }
    if asked("runbook") {
        return crate::runbook::run(args);
    }
    if asked("pricing-sync") {
        parse_spec(args, &PRICING_SYNC)?;
        return crate::pricing_sync::run();
    }
    if !["stats", "savings", "clear-cooldown"].iter().any(|m| asked(m)) {
        return crate::dispatch_run::run(args);
    }
    let p = parse_spec(args, &STATS)?;
    if !p.positional.is_empty() {
        return None;
    }
    let has = |f: &str| p.flags.iter().any(|x| x == f);
    if has("stats") {
        return stats_run(&p);
    }
    if has("savings") {
        return run_savings(&p);
    }
    let pool = p.opts.get("clear-cooldown").filter(|s| !s.is_empty())?;
    let others = ["since", "session", "caller", "log", "report", "ratios"];
    if others.iter().any(|o| p.opts.contains_key(*o)) {
        None
    } else {
        clear_cooldown(pool)
    }
}
