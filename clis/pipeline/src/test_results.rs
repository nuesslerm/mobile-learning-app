//! `wf test results [--query "<terms>"] [--json]`: what `wf test publish` wrote, read back. Two sources, joined on runId: the
//! test_runs table (one row per published run, kept for good) and the bundle index (`verification/test-runs/index.json`,
//! the newest 500 runs with their bundle, report, read-back and timing). It runs nothing.
use serde_json::{json, Map, Value};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Debug, PartialEq)]
struct Term {
    key: String,
    op: Op,
    value: String,
}

#[derive(Debug, Default, PartialEq)]
struct Query {
    terms: Vec<Term>,
    newest: bool,
    /// `units`: every unit of each focused e2e test the test and platform terms reach gets an entry, `missing` when it has no run.
    units: bool,
    /// `YYYY-MM-DDTHH:MM:SS`, compared with the first 19 characters of a run's publishedAt or ts.
    since: Option<String>,
    limit: Option<usize>,
}

/// Fields a term names directly; any other `key=value` matches the run's `--arg` of that name (`chunk=3`).
const FIELDS: [&str; 12] = ["test", "unit", "platform", "sha", "verdict", "screen", "runId", "durationS", "warnings", "wallS", "appStart", "startupS"];

fn duration_ms(v: &str) -> Option<u64> {
    let (n, unit) = v.split_at(v.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = n.parse().ok()?;
    let per = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 604_800_000,
        _ => return None,
    };
    Some(n * per)
}

fn parse(q: &str, now_ms: i64) -> Result<Query, String> {
    let mut out = Query::default();
    for word in q.split_whitespace() {
        if word == "newest" || word == "units" {
            out.newest |= word == "newest";
            out.units |= word == "units";
            continue;
        }
        let at = word.find(['=', '<', '>', '!']).ok_or_else(|| format!("'{word}' is not a term: key=value, key>n, key<n, key!=value, newest"))?;
        let (key, rest) = word.split_at(at);
        let (op, value) = [(">=", Op::Ge), ("<=", Op::Le), ("!=", Op::Ne), (">", Op::Gt), ("<", Op::Lt), ("=", Op::Eq)]
            .iter()
            .find_map(|(sym, op)| rest.strip_prefix(sym).map(|v| (*op, v)))
            .ok_or_else(|| format!("'{word}' has no operator"))?;
        if key.is_empty() || value.is_empty() {
            return Err(format!("'{word}' needs a key and a value"));
        }
        match (key, op) {
            ("since", Op::Eq) => {
                let iso = match duration_ms(value) {
                    Some(ms) => rca_core::time::iso(now_ms.saturating_sub(ms as i64)),
                    None if value.len() >= 10 && value.as_bytes()[4] == b'-' => format!("{value}{}", &"T00:00:00"[(value.len() - 10).min(9)..]),
                    None => return Err(format!("since={value}: a duration (30m, 6h, 2d, 1w) or a date (2026-10-16)")),
                };
                out.since = Some(iso.chars().take(19).collect());
            }
            ("limit", Op::Eq) => out.limit = Some(value.parse().map_err(|_| format!("limit={value} is not a number"))?),
            _ => out.terms.push(Term { key: key.to_string(), op, value: value.to_string() }),
        }
    }
    Ok(out)
}

/// The part of the query the table can answer itself, as an ORDS filter; the rest is applied after the join.
fn table_filter(q: &Query) -> Value {
    let mut f = Map::new();
    for t in q.terms.iter().filter(|t| t.op == Op::Eq && !t.value.contains(',')) {
        match t.key.as_str() {
            "verdict" => f.insert("verdict".into(), json!(t.value)),
            "platform" => f.insert("harness".into(), json!(t.value)),
            "sha" => f.insert("sha".into(), json!({ "$like": format!("{}%", t.value) })),
            "test" => f.insert("suite_or_runbook".into(), json!({ "$like": format!("%{}%", t.value) })),
            _ => None,
        };
    }
    if let Some(s) = &q.since {
        f.insert("ts".into(), json!({ "$gt": { "$date": format!("{s}Z") } }));
    }
    f.insert("$orderby".into(), json!({ "ts": "DESC" }));
    Value::Object(f)
}

/// `e2e/voice-edit.lua` → `voice-edit`.
fn test_name(suite: &str) -> String {
    let file = suite.rsplit('/').next().unwrap_or(suite);
    file.strip_suffix(".lua").unwrap_or(file).to_string()
}

fn verbs(timing: &Value) -> Value {
    if timing["verbs"].is_array() {
        timing["verbs"].clone()
    } else {
        timing["byVerb"].clone()
    }
}

fn from_index(e: &Value) -> Value {
    let suite = e["suiteOrRunbook"].as_str().unwrap_or_default();
    json!({
        "runId": e["runId"], "test": test_name(suite), "unit": crate::e2e_tests::args_key(&e["args"]), "platform": e["harness"], "sha": e["sha"],
        "verdict": e["verdict"], "durationS": e["durationS"], "publishedAt": e["publishedAt"], "screen": e["screen"], "warnings": e["warnings"],
        "recordings": e["recordings"], "screenshots": e["screenshots"], "readBack": e["readBack"], "wallS": e["wallS"], "phases": e["timing"]["phases"], "appStart": e["timing"]["appStart"], "startupS": e["timing"]["startupS"],
        "byVerb": verbs(&e["timing"]), "timing": e["timing"], "report": e["report"], "bundle": e["bundle"], "args": e["args"], "suiteOrRunbook": suite,
        "inIndex": true, "inTable": false,
    })
}

fn from_row(r: &Value) -> Value {
    let suite = r["suite_or_runbook"].as_str().unwrap_or_default();
    let timing: Value = r["timing_json"].as_str().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null);
    json!({
        "runId": r["run_id"], "test": test_name(suite), "unit": "", "platform": r["harness"], "sha": r["sha"], "verdict": r["verdict"],
        "durationS": r["duration_s"], "publishedAt": r["ts"], "screen": r["device_or_screen"], "warnings": r["warnings"], "recordings": r["recordings"],
        "screenshots": r["screenshots"], "readBack": null, "wallS": timing["wallS"], "phases": timing["phases"], "appStart": timing["appStart"], "startupS": timing["startupS"], "byVerb": verbs(&timing), "timing": timing,
        "report": r["report_path"], "bundle": null, "args": null, "suiteOrRunbook": suite, "ranOn": r["ran_on"], "ts": r["ts"],
        "inIndex": false, "inTable": true,
    })
}

/// Index runs first (they carry the bundle), each completed from its table row; then the table's runs the index no longer holds.
fn join(index: &[Value], rows: &[Value]) -> Vec<Value> {
    let mut out: Vec<Value> = index.iter().map(from_index).collect();
    for row in rows.iter().map(from_row) {
        match out.iter_mut().find(|r| r["runId"] == row["runId"]) {
            Some(r) => {
                r["inTable"] = json!(true);
                r["ts"] = row["ts"].clone();
                r["ranOn"] = row["ranOn"].clone();
                if r["timing"].is_null() {
                    for k in ["timing", "phases", "byVerb", "wallS", "appStart", "startupS"] {
                        r[k] = row[k].clone();
                    }
                }
            }
            None => out.push(row),
        }
    }
    out
}

/// A term's value in a run: a field, `timing.<phase|key>`, `timing.<verb>.<calls|s|p50|p95|max>` (seconds), else the `--arg`.
fn resolve(run: &Value, key: &str) -> Value {
    let parts: Vec<&str> = key.split('.').collect();
    match parts.as_slice() {
        ["timing", k] => {
            let t = &run["timing"];
            if t[*k].is_null() {
                t["phases"][*k].clone()
            } else {
                t[*k].clone()
            }
        }
        ["timing", verb, field] => {
            let Some(v) = verb_row(run, verb) else { return Value::Null };
            match *field {
                "p50" | "p95" | "max" => v[format!("{field}Ms")].as_f64().map_or(Value::Null, |ms| json!(ms / 1000.0)),
                f => v[f].clone(),
            }
        }
        [k] if FIELDS.contains(k) || !run[*k].is_null() => run[*k].clone(),
        [k] => run["args"][*k].clone(),
        _ => parts.iter().fold(run.clone(), |v, p| v[*p].clone()),
    }
}

/// The span names runs recorded before plan step 7 renamed the Lua methods, beside the command's span that replaced each.
const SPAN_RENAMES: &[(&str, &str)] = &[
    ("open", "open_link"),
    ("tap", "tap_on"),
    ("type_text", "input_text"),
    ("press", "press_key"),
    ("find", "get_element"),
    ("text", "get_element"),
    ("editor_text", "get_element"),
    ("keyboard_shown", "get_device_state"),
    ("wait", "assert_visible"),
    ("wait_slow", "assert_visible"),
    ("wait_gone", "assert_not_visible"),
    ("wait_gone_slow", "assert_not_visible"),
    ("appears", "extended_wait_until:visible:optional"),
    ("wait_keyboard", "extended_wait_until:condition:optional"),
    ("wait_recording", "extended_wait_until:condition:optional"),
    ("wait_pixel", "extended_wait_until:condition:optional"),
    ("wait_editor_longer", "extended_wait_until:condition"),
    ("screenshot", "take_screenshot"),
    ("pixel", "get_pixel"),
    ("trace", "draw_strokes_on"),
    ("audio_inject", "set_microphone_input"),
    ("warnings", "get_logs"),
    ("clear_warnings", "clear_logs"),
    ("shell", "run_shell"),
    ("stub", "add_network_stub"),
    ("stub_hits", "get_network_stub_hits"),
    ("debug_hits", "get_debug_hits"),
];

/// A verb's timing row; a verb named by its span name before the rename or by the command that replaced it finds the
/// other's row too, so a query reads the same calls on both sides of the rename (`SPAN_RENAMES`).
fn verb_row<'a>(run: &'a Value, verb: &str) -> Option<&'a Value> {
    let rows = run["byVerb"].as_array()?;
    let row = |name: &str| rows.iter().find(|v| v["verb"] == name);
    row(verb).or_else(|| SPAN_RENAMES.iter().find_map(|(old, new)| match (*old == verb, *new == verb) {
        (true, _) => row(new),
        (_, true) => row(old),
        _ => None,
    }))
}

fn text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn holds(run: &Value, t: &Term) -> bool {
    let v = resolve(run, &t.key);
    match t.op {
        Op::Eq | Op::Ne => {
            let num = |s: &str| s.parse::<f64>().ok();
            let eq = text(&v).is_some_and(|s| match (num(&s), num(&t.value)) {
                _ if t.key == "sha" => s.starts_with(&t.value),
                (Some(a), Some(b)) => a == b,
                _ => t.value.split(',').any(|alt| alt == s),
            });
            eq == (t.op == Op::Eq)
        }
        op => {
            let (Some(a), Ok(b)) = (v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok())), t.value.parse::<f64>()) else { return false };
            match op {
                Op::Gt => a > b,
                Op::Ge => a >= b,
                Op::Lt => a < b,
                _ => a <= b,
            }
        }
    }
}

/// `units`: only the runs of the focused tests' current units the query reaches, and `{test, unit, platform, missing: true}`
/// for each of those units no selected run covers (a unit renamed or split away no longer counts).
fn by_units(q: &Query, tests: &[crate::e2e_tests::E2eTest], mut runs: Vec<Value>) -> Vec<Value> {
    let reach = |key: &str, v: &str| q.terms.iter().filter(|t| t.key == key && t.op == Op::Eq).all(|t| t.value.split(',').any(|alt| alt == v));
    let mut wanted: Vec<(String, String, String)> = Vec::new();
    for t in tests.iter().filter(|t| t.focus && t.script.is_some() && reach("test", &t.name) && reach("platform", &t.platform)) {
        let units: Vec<String> = if t.units.is_empty() { vec![String::new()] } else { t.units.iter().map(|u| crate::e2e_tests::unit_key(u)).collect() };
        wanted.extend(units.into_iter().filter(|u| reach("unit", u)).map(|u| (t.name.clone(), u, t.platform.clone())));
    }
    let key = |r: &Value| (r["test"].as_str().unwrap_or_default().to_string(), r["unit"].as_str().unwrap_or_default().to_string(), r["platform"].as_str().unwrap_or_default().to_string());
    runs.retain(|r| wanted.contains(&key(r)));
    let missing: Vec<Value> = wanted.iter().filter(|w| !runs.iter().any(|r| key(r) == **w)).map(|(t, u, p)| json!({ "test": t, "unit": u, "platform": p, "missing": true })).collect();
    runs.extend(missing);
    runs
}

fn at(run: &Value) -> String {
    run["publishedAt"].as_str().or_else(|| run["ts"].as_str()).unwrap_or_default().chars().take(19).collect()
}

fn select(q: &Query, mut runs: Vec<Value>) -> Vec<Value> {
    runs.retain(|r| q.terms.iter().all(|t| holds(r, t)) && q.since.as_ref().is_none_or(|s| at(r) > *s));
    runs.sort_by_key(|r| std::cmp::Reverse(at(r)));
    if q.newest {
        let mut seen = std::collections::HashSet::new();
        runs.retain(|r| seen.insert((r["test"].to_string(), r["unit"].to_string(), r["platform"].to_string())));
    }
    if let Some(n) = q.limit {
        runs.truncate(n);
    }
    runs
}

pub fn run(query: &str, as_json: bool) -> i32 {
    let q = match parse(query, wf_kit::time::now_ms()) {
        Ok(q) => q,
        Err(e) => {
            println!("{}", json!({ "error": e, "next": "wf test results --query \"test=voice-edit chunk=3 newest since=2d durationS>60\"" }));
            return 2;
        }
    };
    let (index, index_status) = match wf_bucket::artifacts::existing_index() {
        Ok(Some(t)) => (serde_json::from_str::<Value>(&t).ok().and_then(|v| v["runs"].as_array().cloned()).unwrap_or_default(), "ok".to_string()),
        Ok(None) => (Vec::new(), "absent".to_string()),
        Err(e) => (Vec::new(), format!("failed: {e}")),
    };
    let (rows, table_status) = match crate::test_metrics::query_rows(&table_filter(&q), 1000) {
        Ok(rows) => (rows, "ok".to_string()),
        Err(e) if e.ends_with("unset") => (Vec::new(), format!("disabled: {e}")),
        Err(e) => (Vec::new(), format!("failed: {e}")),
    };
    let mut runs = select(&q, join(&index, &rows));
    if q.units {
        runs = by_units(&q, &crate::e2e_tests::e2e_tests(), runs);
    }
    let failed = index_status.starts_with("failed") && !table_status.starts_with("ok");
    if as_json {
        println!("{}", json!({ "query": query, "sources": { "index": index_status, "table": table_status }, "runs": runs }));
    } else {
        for r in &runs {
            let s = |k: &str| text(&r[k]).unwrap_or_else(|| "-".into());
            println!("{} {} {}s {} {} {} {} {}", at(r), s("verdict"), s("durationS"), s("test"), if s("unit").is_empty() { "-".into() } else { s("unit") }, s("sha").chars().take(9).collect::<String>(), s("platform"), s("report"));
        }
        eprintln!("index: {index_status}; table: {table_status}; {} runs", runs.len());
    }
    if failed {
        1
    } else {
        0
    }
}
