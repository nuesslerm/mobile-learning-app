//! `wf bench`: the app benchmarks (build speed, memory, runtime speed, startup) with their committed scenarios
//! and baselines in `clis/app-bench/{scenarios,baselines}`. A suite run exports the app or serves the export in a
//! headless Chromium phone; `compare` checks the recorded baselines, `ab` times two cold-export arms side by side.
mod android;
mod domain;
mod export;
mod lab;
mod suites;

use wf_kit::args::parse_none;
use wf_kit::js::js_num;
use wf_kit::root::repo_root;
use domain::{jnum, spread_ratio, with_recorded};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

const SUITES: [&str; 5] = ["build", "memory", "runtime", "startup", "android"];

const USAGE: &str = "wf bench <command> [options]

Commands:
  build | memory | runtime | startup | android   Run one suite (android boots a slim redroid on the VM and removes it)
    --runs <n>               Runs to take the median over (default: the scenario's)
    --record <mode>          initial | current | both | none (default: none). \"both\" adds the metrics the initial block lacks and replaces current
  compare                    Check baselines, or a fresh result against them
    --suite <name|all>       Suite to check (default: all)
    --budgets                Also fail when a metric is above its budget (for example half of the initial memory)
  ab                         Same-session interleaved cold exports, baseline against a candidate
    --env KEY=VALUE          Environment of the candidate arm (repeatable)
    --platform <name>        web (default) or ios
    --pairs <n>              Baseline/candidate pairs (default 3)
  list                       Show suites, their metrics and recorded baselines
  --help
";

type Res<T> = Result<T, String>;

/// Where scenarios and baselines live (tests point it at a throwaway copy).
fn data_dir() -> PathBuf {
    match std::env::var_os("WF_APP_BENCH_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => repo_root().join("clis/app-bench"),
    }
}

/// Where runs keep their files: the export, the served roots, the startup attribution.
fn tmp_dir() -> PathBuf {
    match std::env::var_os("WF_APP_BENCH_TMP") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => repo_root().join(".agents/tmp/bench"),
    }
}

fn read(suite: &str, dir: &str) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(data_dir().join(dir).join(format!("{suite}.json"))).ok()?).ok()
}

fn write_baseline(file: &Value, suite: &str) -> Res<()> {
    let path = data_dir().join("baselines").join(format!("{suite}.json"));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(file).map_err(|e| e.to_string())?;
    std::fs::write(path, text + "\n").map_err(|e| e.to_string())
}

/// Whole and ordinary fractional numbers print the same in Rust and JavaScript; anything else is TypeScript's.
fn num(x: f64) -> Option<String> {
    (x.is_finite() && (x == 0.0 || (1e-6..1e21).contains(&x.abs()))).then(|| js_num(x))
}

/// `Number.prototype.toFixed`: the exact decimal expansion rounded half up.
pub(crate) fn to_fixed(x: f64, digits: usize) -> String {
    let exact = format!("{:.80}", x.abs());
    let (int, frac) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut kept: Vec<u8> = format!("{int}{}", &frac[..digits]).into_bytes();
    if frac.as_bytes()[digits] >= b'5' {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let split = kept.len() - digits;
    let (head, tail) = kept.split_at(split);
    let sign = if x < 0.0 { "-" } else { "" };
    let head = String::from_utf8_lossy(head);
    if digits == 0 { format!("{sign}{head}") } else { format!("{sign}{head}.{}", String::from_utf8_lossy(tail)) }
}

struct Comparison {
    verdict: &'static str,
    metric: String,
    reference: Option<f64>,
    candidate: Option<f64>,
    ratio: Option<f64>,
    detail: String,
}

fn median(v: &Value) -> Option<f64> {
    v["median"].as_f64()
}

fn spread(v: &Value) -> Option<f64> {
    let m = median(v)?;
    Some(if m == 0.0 { 0.0 } else { (v["max"].as_f64()? - v["min"].as_f64()?) / m.abs() })
}

fn compare_metrics(scenario: &Value, reference: &Value, candidate: &Value, budgets: Option<&Value>) -> Option<Vec<Comparison>> {
    let mut out = Vec::new();
    for (metric, rule) in scenario["metrics"].as_object()? {
        let (r, c) = (reference.get(metric).filter(|v| !v.is_null()), candidate.get(metric).filter(|v| !v.is_null()));
        let (Some(r), Some(c)) = (r, c) else {
            out.push(Comparison {
                verdict: "missing",
                metric: metric.clone(),
                reference: r.and_then(median),
                candidate: c.and_then(median),
                ratio: None,
                detail: "metric absent".into(),
            });
            continue;
        };
        let (rm, cm) = (median(r)?, median(c)?);
        let tolerance = rule["tolerance"].as_f64()?;
        let lower = match rule["better"].as_str()? {
            "lower" => true,
            "higher" => false,
            _ => return None,
        };
        let ratio = (rm != 0.0).then(|| cm / rm);
        let worse = if lower { cm > rm * (1.0 + tolerance) } else { cm < rm * (1.0 - tolerance) };
        // A metric marked `"gate": false` is reported but never fails: its `initial` was read by a different tool (see its `gateNote`).
        if worse && rule["gate"] == false {
            out.push(Comparison { verdict: "info", metric: metric.clone(), reference: Some(rm), candidate: Some(cm), ratio, detail: format!("not gated: {}", rule["gateNote"].as_str().unwrap_or("informational")) });
            continue;
        }
        if worse {
            out.push(Comparison {
                verdict: "regressed",
                metric: metric.clone(),
                reference: Some(rm),
                candidate: Some(cm),
                ratio,
                detail: format!("worse than {} beyond {}%", num(rm)?, to_fixed(tolerance * 100.0, 0)) + if spread(c)? > 0.5 { " (noisy: spread over 50%)" } else { "" },
            });
            continue;
        }
        if let (Some(max), Some(initial)) = (rule.get("maxRatioOfInitial").and_then(Value::as_f64), budgets.and_then(|b| b.get(metric)).filter(|v| !v.is_null())) {
            let cap = median(initial)? * max;
            if lower && cm > cap {
                out.push(Comparison {
                    verdict: "over-budget",
                    metric: metric.clone(),
                    reference: Some(rm),
                    candidate: Some(cm),
                    ratio,
                    detail: format!("above {} ({} of initial)", num(cap)?, num(max)?),
                });
                continue;
            }
        }
        out.push(Comparison {
            verdict: "ok",
            metric: metric.clone(),
            reference: Some(rm),
            candidate: Some(cm),
            ratio,
            detail: if spread(c)? > 0.5 { " (noisy: spread over 50%)".into() } else { String::new() },
        });
    }
    Some(out)
}

fn format_comparison(suite: &str, ok: bool, note: &str, comparisons: &[Comparison]) -> Option<String> {
    let mut lines = vec![format!("{} {suite} ({note})", if ok { "PASS" } else { "FAIL" })];
    for c in comparisons {
        let show = |v: Option<f64>| v.map_or(Some("-".to_string()), num);
        let ratio = c.ratio.map_or("-".to_string(), |r| to_fixed(r, 3));
        lines.push(
            format!("  {:<11} {:<22} ref={} now={} x{ratio} {}", c.verdict, c.metric, show(c.reference)?, show(c.candidate)?, c.detail)
                .trim_end()
                .to_string(),
        );
    }
    Some(lines.join("\n"))
}

/// A suite's baseline checked against itself (`result` absent) or a fresh result against the recorded current.
/// `None` for a number JavaScript would print differently.
fn compare_suite(suite: &str, budgets: bool, result: Option<&Value>) -> Option<(String, bool, usize)> {
    let scenario = read(suite, "scenarios")?;
    let baseline = read(suite, "baselines");
    let snapshots = baseline.as_ref().filter(|b| b["initial"].is_object() && b["current"].is_object());
    let Some(baseline) = snapshots else {
        return Some((format!("FAIL {suite} (no baseline recorded; run the suite with --record initial)"), false, 0));
    };
    let (reference, candidate) = match result {
        Some(r) => (&baseline["current"]["metrics"], r),
        None => (&baseline["initial"]["metrics"], &baseline["current"]["metrics"]),
    };
    let comparisons = compare_metrics(&scenario, reference, candidate, budgets.then(|| &baseline["initial"]["metrics"]))?;
    let ok = comparisons.iter().all(|c| c.verdict == "ok" || c.verdict == "info");
    let note = format!("{}{}", if result.is_some() { "result vs current baseline" } else { "current vs initial baseline" }, if budgets { ", budgets on" } else { "" });
    Some((format_comparison(suite, ok, &note, &comparisons)?, ok, comparisons.len()))
}

fn list() -> Option<i32> {
    let mut out = String::new();
    for suite in SUITES {
        let scenario = read(suite, "scenarios")?;
        let metrics: Vec<&str> = scenario["metrics"].as_object()?.keys().map(String::as_str).collect();
        let initial = match read(suite, "baselines") {
            Some(b) if b["initial"].is_object() => b["initial"]["commit"].as_str()?.chars().take(9).collect(),
            _ => "none".to_string(),
        };
        out.push_str(&format!("{suite}: {} | initial {initial}\n", metrics.join(", ")));
    }
    print!("{out}");
    Some(0)
}

/// What the TypeScript `wf` printed around a failing passthrough: the child's stderr, then `wf: <stderr tail>`.
fn failed_with(stderr: &str) -> Option<i32> {
    failed_with_code(stderr, 1)
}

fn failed_with_code(stderr: &str, code: i32) -> Option<i32> {
    eprint!("{stderr}");
    let tail: String = {
        let chars: Vec<char> = stderr.chars().collect();
        chars[chars.len().saturating_sub(4096)..].iter().collect()
    };
    if tail.is_empty() {
        wf_kit::wf_err!("wf: passthrough command failed with exit code {code}");
    } else {
        wf_kit::wf_err!("wf: {tail}");
    }
    Some(code)
}

fn failed(code: i32) -> Option<i32> {
    wf_kit::wf_err!("wf: passthrough command failed with exit code {code}");
    Some(code)
}

fn git_commit() -> Res<String> {
    let out = std::process::Command::new("git").args(["rev-parse", "HEAD"]).current_dir(repo_root()).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn format_snapshot(scenario: &Value, suite: &str, snapshot: &Value) -> Option<String> {
    let commit: String = snapshot["commit"].as_str()?.chars().take(9).collect();
    let mut lines = vec![format!("{suite}: {} run(s) at {commit}", js_num(snapshot["runs"].as_f64()?))];
    for (name, metric) in snapshot["metrics"].as_object()? {
        let scope = scenario["metrics"][name]["scope"].as_str().unwrap_or("");
        lines.push(format!(
            "  {:<22} median {} {} (min {}, max {}, spread {}%) [{scope}]",
            name,
            js_num(metric["median"].as_f64()?),
            metric["unit"].as_str()?,
            js_num(metric["min"].as_f64()?),
            js_num(metric["max"].as_f64()?),
            to_fixed(spread_ratio(metric) * 100.0, 1)
        ));
    }
    Some(lines.join("\n"))
}

fn run_suite_command(suite: &str, runs: Option<f64>, record: &str) -> Res<(String, bool)> {
    if !["initial", "current", "both", "none"].contains(&record) {
        return Err("--record must be initial, current, both or none".into());
    }
    let scenario = read(suite, "scenarios").ok_or_else(|| format!("no scenario for {suite}"))?;
    let runs = runs.unwrap_or_else(|| scenario["runs"].as_f64().unwrap_or(1.0));
    let result = suites::run(suite, &scenario, runs, &tmp_dir())?;
    let snapshot = json!({
        "recordedAt": rca_core::time::iso(wf_kit::time::now_ms()),
        "commit": git_commit()?,
        "runs": jnum(runs),
        "env": result["env"].clone(),
        "metrics": result["metrics"].clone(),
    });
    if record != "none" {
        let file = read(suite, "baselines").unwrap_or_else(|| json!({ "suite": suite }));
        write_baseline(&with_recorded(&file, record, &snapshot, suite)?, suite)?;
    }
    let mut text = format_snapshot(&scenario, suite, &snapshot).ok_or("a metric cannot be printed")? + "\n";
    let (report, ok, count) = compare_suite(suite, false, Some(&result["metrics"])).ok_or("a comparison cannot be printed")?;
    if count > 0 {
        text.push_str(&report);
        text.push('\n');
    }
    Ok((text, record == "none" && !ok && count > 0))
}

fn parse_env(pairs: &[String]) -> Res<Vec<(String, String)>> {
    let mut env = Vec::new();
    for pair in pairs {
        match pair.find('=') {
            Some(at) if at >= 1 => env.push((pair[..at].to_string(), pair[at + 1..].to_string())),
            _ => return Err(format!("--env wants KEY=VALUE, got {pair}")),
        }
    }
    if env.is_empty() {
        return Err("ab needs --env KEY=VALUE for the candidate arm".into());
    }
    Ok(env)
}

fn number(text: &str) -> f64 {
    let t = text.trim();
    if t.is_empty() {
        0.0
    } else {
        t.parse::<f64>().unwrap_or(f64::NAN)
    }
}

pub fn run(args: &[String]) -> Option<i32> {
    let Some((command, rest)) = args.split_first() else {
        print!("{USAGE}");
        return failed(1);
    };
    if command == "--help" || command == "-h" {
        print!("{USAGE}");
        return Some(0);
    }
    let Some(p) = parse_none(rest, &["runs", "record", "suite", "env", "platform", "pairs"], &["budgets"]) else {
        return failed_with("Unknown option or unexpected argument; see `wf bench --help`\n");
    };
    let envs: Vec<String> = p.all.iter().filter(|(k, _)| k == "env").map(|(_, v)| v.clone()).collect();
    match command.as_str() {
        "list" => list(),
        "compare" => {
            let suite = p.opts.get("suite").map_or("all", String::as_str);
            let wanted: Vec<&str> = if suite == "all" { SUITES.to_vec() } else { vec![suite] };
            if let Some(bad) = wanted.iter().find(|s| !SUITES.contains(s)) {
                return failed_with(&format!("unknown suite: {bad}\n"));
            }
            let budgets = p.flags.iter().any(|f| f == "budgets");
            let mut text = String::new();
            let mut ok = true;
            for name in wanted {
                let (report, passed, _) = compare_suite(name, budgets, None)?;
                text.push_str(&report);
                text.push('\n');
                ok &= passed;
            }
            let _ = std::io::stdout().write_all(text.as_bytes());
            if ok { Some(0) } else { failed(1) }
        }
        "ab" => {
            let platform = p.opts.get("platform").map_or("web", String::as_str);
            let pairs = number(p.opts.get("pairs").map_or("3", String::as_str));
            match parse_env(&envs).and_then(|env| suites::ab(&read("build", "scenarios").ok_or("no scenario for build")?, &tmp_dir(), platform, &env, pairs)) {
                Ok(()) => Some(0),
                Err(e) => failed_with(&format!("{e}\n")),
            }
        }
        suite if SUITES.contains(&suite) => {
            let runs = p.opts.get("runs").map(|r| number(r));
            match run_suite_command(suite, runs, p.opts.get("record").map_or("none", String::as_str)) {
                Ok((text, regressed)) => {
                    let _ = std::io::stdout().write_all(text.as_bytes());
                    if regressed { failed(1) } else { Some(0) }
                }
                Err(e) => failed_with(&format!("{e}\n")),
            }
        }
        other => failed_with_code(&format!("unknown command: {other}\n{USAGE}"), 2),
    }
}
