//! The report every harness run (`wf android test`, `wf browser test`) ends with, pass or fail: `<test-output-dir>/report.json` (stable schema, below) and
//! `report.html` (one self-contained page rendered from it). Links are relative to the test output dir, so the folder can be
//! fetched or published as a whole.
//!
//! report.json: {schema, verdict (pass|fail|error), ok, exitCode, message, traceback, notes[], script, platform, device,
//! screen, sha, speed, args{}, startedMs, finishedMs, started, finished, durationS, recordDir, timing, steps[{ms,kind,detail}],
//! screenshots[{path,href}], recordings[{path,href}], goldens[{name,verdict,ok,message,ratio,differingPixels,golden,
//! actual,expected,diff}], evidence[{path,href}] (each failed wait's evidence JSON), warnings[], debugHits[], logs (an Android run's
//! app lines from BetterStack: {runId, group, note, mapNote, lines[{ts,level,message,crash,stack,mappedStack,fields}]}, crashes first)}
use crate::golden::{esc, Outcome};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

const SCHEMA: u64 = 1;

/// What the script saw while it ran: the screenshots it saved and every JS warning a `device:get_logs()` returned.
#[derive(Default)]
pub struct Trail {
    pub screenshots: std::cell::RefCell<Vec<String>>,
    pub warnings: std::cell::RefCell<Vec<String>>,
    /// The stubs the script declared (`device:add_network_stub`), in order.
    pub stubs: std::cell::RefCell<Vec<Value>>,
}

impl Trail {
    pub fn saw_warnings(&self, lines: &[String]) {
        let mut all = self.warnings.borrow_mut();
        for l in lines {
            if !all.contains(l) {
                all.push(l.clone());
            }
        }
    }
}

/// What the runner knows when the run ends; screenshots, recordings and steps are also found on disk.
#[derive(Default)]
pub struct Facts {
    pub script: String,
    pub platform: String,
    pub device: Option<String>,
    /// The Android display density (`wm density`), shown with the screen size as `480x1040@213`.
    pub density: Option<u32>,
    pub sha: Option<String>,
    /// The run's speed (`WF_TEST_SPEED`, `--speed`): gestures and typing at that many times a user's pace.
    pub speed: Option<f64>,
    pub args: BTreeMap<String, String>,
    pub started_ms: i64,
    pub finished_ms: i64,
    pub exit_code: i32,
    pub verdict: &'static str,
    pub message: Option<String>,
    pub traceback: Option<String>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
    pub debug_hits: Vec<Value>,
    /// An Android run's app log lines from BetterStack (`crate::android_logs::collect`).
    pub logs: Option<Value>,
    /// The network stubs the script declared and the requests they answered: {declared: [...], hits: [...]}.
    pub stubs: Option<Value>,
    pub goldens: Vec<Outcome>,
    pub recordings: Vec<String>,
    pub screenshots: Vec<String>,
    /// The evidence JSON a failed wait named (the record dir's `wait-timeout-*.json` are found on disk).
    pub evidence: Vec<String>,
    /// The `WF_RUN_TIMING` lines this run appended.
    pub timing_lines: Option<String>,
    /// The input log (`run.inputs.jsonl`).
    pub inputs: Option<PathBuf>,
    /// More folders whose PNGs are the run's screenshots (the script's `out` arg).
    pub extra_dirs: Vec<PathBuf>,
    /// Android: `cold` when the run force-stopped Expo Go first (`--cold-start`), `reused` when it started from the running app.
    pub app_start: Option<&'static str>,
    /// The VM's load and memory over the run (`WF_VM_SAMPLE_SECS`).
    pub vm: Option<Value>,
}

/// A Lua error as (message, traceback): the traceback is everything from `stack traceback:` on.
pub fn split_traceback(e: &str) -> (String, Option<String>) {
    match e.find("stack traceback:") {
        Some(i) => (e[..i].trim_end().to_string(), Some(e[i..].trim_end().to_string())),
        None => (e.trim().to_string(), None),
    }
}

/// Folds `WF_RUN_TIMING` lines into calls and seconds per device verb, slowest first; the rest of the wall is script time.
/// The density `wm density` reports: the override when one is set, else the physical one.
pub fn parse_density(out: &str) -> Option<u32> {
    let find = |key: &str| out.lines().find_map(|l| l.trim().strip_prefix(key)).and_then(|v| v.trim().parse().ok());
    find("Override density:").or_else(|| find("Physical density:"))
}

/// Folds the run's spans (`crate::spans`; a line without `kind` is a call from before spans) into where the time went:
/// the phases, each device verb with its percentiles and outcomes, what happened inside the calls, the script's steps,
/// sleep, Lua's own time, the slowest calls with their lines, and a timeline for the report page.
/// The timing as charts read it: how the app started, and `startupS`, the time until the first screen answered (the
/// "first bundle" step of a cold start, else the "open route" step).
fn with_app_start(mut t: Value, app_start: Option<&str>) -> Value {
    let step = |name: &str| t["byStep"].as_array().and_then(|a| a.iter().find(|s| s["step"] == name)).map(|s| s["s"].clone());
    let startup = step("first bundle").or_else(|| step("open route"));
    t["appStart"] = json!(app_start);
    t["startupS"] = startup.unwrap_or(Value::Null);
    t
}

pub fn timing_summary(lines: &str, wall_ms: u64) -> Value {
    let spans: Vec<Value> = lines.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v["ms"].is_u64()).collect();
    let kind = |v: &Value| v["kind"].as_str().unwrap_or("call").to_string();
    let name = |v: &Value| v["name"].as_str().or(v["verb"].as_str()).unwrap_or("").to_string();
    let ms = |v: &Value| v["ms"].as_u64().unwrap_or(0);
    let by_id: BTreeMap<String, &Value> = spans.iter().filter_map(|v| v["id"].as_str().map(|id| (id.to_string(), v))).collect();
    let parent_kind = |v: &Value| v["parent"].as_str().and_then(|p| by_id.get(p)).map(|p| kind(p));
    // a driver call made outside a script call (setup, teardown, a verb run by hand) counts as a call of its own
    let is_call = |v: &Value| match kind(v).as_str() {
        "call" => true,
        "inner" => !matches!(parent_kind(v).as_deref(), Some("call") | Some("inner")),
        _ => false,
    };
    let secs = |ms: u64| (ms as f64 / 100.0).round() / 10.0;
    let pct = |sorted: &[u64], q: f64| sorted.get(((sorted.len().saturating_sub(1)) as f64 * q).round() as usize).copied().unwrap_or(0);
    let calls: Vec<&Value> = spans.iter().filter(|v| is_call(v)).collect();
    let mut verbs: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for c in &calls {
        verbs.entry(name(c)).or_default().push(c);
    }
    let child_ms = |id: &str| spans.iter().filter(|s| s["parent"].as_str() == Some(id)).map(&ms).sum::<u64>();
    let mut by_verb: Vec<(u64, Value)> = verbs
        .iter()
        .map(|(verb, list)| {
            let mut all: Vec<u64> = list.iter().map(|v| ms(v)).collect();
            all.sort_unstable();
            let total: u64 = all.iter().sum();
            let inside: u64 = list.iter().filter_map(|v| v["id"].as_str()).map(child_ms).sum();
            let count = |o: &str| list.iter().filter(|v| v["outcome"].as_str() == Some(o)).count();
            (total, json!({
                "verb": verb, "calls": list.len(), "s": secs(total), "p50Ms": pct(&all, 0.5), "p95Ms": pct(&all, 0.95), "maxMs": all.last().copied().unwrap_or(0),
                "insideS": secs(inside), "misses": count("miss"), "timeouts": count("timeout"), "errors": count("error"),
            }))
        })
        .collect();
    by_verb.sort_by_key(|r| std::cmp::Reverse(r.0));
    let device_ms: u64 = calls.iter().map(|v| ms(v)).sum();
    let mut inner: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for v in spans.iter().filter(|v| kind(v) == "inner" && !is_call(v)) {
        let parent = v["parent"].as_str().and_then(|p| by_id.get(p)).map(|p| name(p)).unwrap_or_default();
        inner.entry(format!("{parent} > {}", name(v))).or_default().push(ms(v));
    }
    let mut by_inner: Vec<(u64, Value)> = inner
        .into_iter()
        .map(|(k, mut all)| {
            all.sort_unstable();
            let total: u64 = all.iter().sum();
            (total, json!({ "part": k, "calls": all.len(), "s": secs(total), "p50Ms": pct(&all, 0.5) }))
        })
        .collect();
    by_inner.sort_by_key(|r| std::cmp::Reverse(r.0));
    let sum_kind = |k: &str| spans.iter().filter(|v| kind(v) == k).map(&ms).sum::<u64>();
    let sleep_ms = sum_kind("sleep");
    let phases: serde_json::Map<String, Value> = spans.iter().filter(|v| kind(v) == "phase").map(|v| (name(v), json!(secs(ms(v))))).collect();
    let script_ms = spans.iter().find(|v| kind(v) == "phase" && name(v) == "script").map(&ms).unwrap_or(wall_ms);
    let script_calls: u64 = spans.iter().filter(|v| kind(v) == "call").map(&ms).sum();
    let mut steps: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for v in spans.iter().filter(|v| kind(v) == "step") {
        let e = steps.entry(name(v)).or_default();
        e.0 += 1;
        e.1 += ms(v);
    }
    let mut slowest: Vec<&&Value> = calls.iter().collect();
    slowest.sort_by_key(|v| std::cmp::Reverse(ms(v)));
    let timeline: Vec<Value> = spans
        .iter()
        .filter(|v| kind(v) != "inner" || is_call(v))
        .take(3000)
        .map(|v| json!({ "kind": kind(v), "name": name(v), "t0": v["t0"], "ms": ms(v), "outcome": v["outcome"], "line": v["line"] }))
        .collect();
    json!({
        "wallS": secs(wall_ms), "deviceS": secs(device_ms), "scriptS": secs(wall_ms.saturating_sub(device_ms)),
        "sleepS": secs(sleep_ms), "luaS": secs(script_ms.saturating_sub(script_calls + sleep_ms)), "phases": phases,
        "calls": calls.len(),
        "byVerb": by_verb.into_iter().map(|r| r.1).collect::<Vec<_>>(),
        "byInner": by_inner.into_iter().map(|r| r.1).collect::<Vec<_>>(),
        "byStep": steps.iter().map(|(k, (n, t))| json!({ "step": k, "count": n, "s": secs(*t) })).collect::<Vec<_>>(),
        "slowest": slowest.iter().take(10).map(|v| json!({ "verb": name(v), "ms": ms(v), "line": v["line"], "outcome": v["outcome"], "t0": v["t0"] })).collect::<Vec<_>>(),
        "timeline": timeline,
    })
}

/// The report's timing section: phases, the verbs with percentiles and outcomes, what happened inside the calls, the
/// script's steps, the slowest calls with their lines, and a waterfall of phases, steps and calls over the run.
fn timing_html(t: &Value) -> String {
    let s = |v: &Value| match v { Value::String(x) => x.clone(), Value::Null => String::new(), other => other.to_string() };
    let device_s = t["deviceS"].as_f64().filter(|d| *d > 0.0).unwrap_or(1.0);
    let phases: String = t["phases"].as_object().map(|m| m.iter().map(|(k, v)| format!(" · {} <b>{}</b> s", esc(k), esc(&s(v)))).collect()).unwrap_or_default();
    let head = format!(
        "<p class=\"lede\">wall <b>{}</b> s · device calls <b>{}</b> s · sleep <b>{}</b> s · Lua <b>{}</b> s · <b>{}</b> calls{phases}</p>",
        esc(&s(&t["wallS"])), esc(&s(&t["deviceS"])), esc(&s(&t["sleepS"])), esc(&s(&t["luaS"])), esc(&s(&t["calls"]))
    );
    let verbs: String = list(&t["byVerb"])
        .iter()
        .map(|v| {
            let pct = (v["s"].as_f64().unwrap_or(0.0) / device_s * 100.0).clamp(0.0, 100.0);
            format!(
                "<tr><td><code>{}</code></td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"barcell\"><span class=\"bar\" style=\"width:{pct:.1}%\"></span></td></tr>",
                esc(&s(&v["verb"])), esc(&s(&v["calls"])), esc(&s(&v["s"])), esc(&s(&v["p50Ms"])), esc(&s(&v["p95Ms"])), esc(&s(&v["maxMs"])), esc(&s(&v["misses"])), esc(&s(&v["timeouts"]))
            )
        })
        .collect();
    let mut html = format!("{head}<div class=\"wrap\"><table class=\"timing\"><thead><tr><th>verb</th><th class=\"num\">calls</th><th class=\"num\">s</th><th class=\"num\">p50 ms</th><th class=\"num\">p95 ms</th><th class=\"num\">max ms</th><th class=\"num\">misses</th><th class=\"num\">timeouts</th><th>share of device time</th></tr></thead><tbody>{verbs}</tbody></table></div>");
    let inner = list(&t["byInner"]);
    if !inner.is_empty() {
        let rows: String = inner.iter().take(20).map(|v| format!("<tr><td><code>{}</code></td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>", esc(&s(&v["part"])), esc(&s(&v["calls"])), esc(&s(&v["s"])), esc(&s(&v["p50Ms"])))).collect();
        html.push_str(&format!("<h3>inside the calls</h3><div class=\"wrap\"><table class=\"timing\"><thead><tr><th>call &gt; part</th><th class=\"num\">calls</th><th class=\"num\">s</th><th class=\"num\">p50 ms</th></tr></thead><tbody>{rows}</tbody></table></div>"));
    }
    let steps = list(&t["byStep"]);
    if !steps.is_empty() {
        let rows: String = steps.iter().map(|v| format!("<tr><td>{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>", esc(&s(&v["step"])), esc(&s(&v["count"])), esc(&s(&v["s"])))).collect();
        html.push_str(&format!("<h3>steps</h3><div class=\"wrap\"><table class=\"timing\"><thead><tr><th>step</th><th class=\"num\">times</th><th class=\"num\">s</th></tr></thead><tbody>{rows}</tbody></table></div>"));
    }
    let slowest = list(&t["slowest"]);
    if !slowest.is_empty() {
        let rows: String = slowest.iter().map(|v| format!("<tr><td><code>{}</code></td><td class=\"num\">{}</td><td>{}</td><td><code>{}</code></td></tr>", esc(&s(&v["verb"])), esc(&s(&v["ms"])), esc(&s(&v["outcome"])), esc(&s(&v["line"])))).collect();
        html.push_str(&format!("<h3>slowest calls</h3><div class=\"wrap\"><table class=\"timing\"><thead><tr><th>verb</th><th class=\"num\">ms</th><th>outcome</th><th>script line</th></tr></thead><tbody>{rows}</tbody></table></div>"));
    }
    let timeline = list(&t["timeline"]);
    let end = timeline.iter().map(|v| v["t0"].as_u64().unwrap_or(0) + v["ms"].as_u64().unwrap_or(0)).max().unwrap_or(0).max(1) as f64;
    if !timeline.is_empty() {
        let rows: String = timeline
            .iter()
            .filter(|v| v["kind"] != "call" || v["ms"].as_u64().unwrap_or(0) >= 200)
            .take(400)
            .map(|v| {
                let (t0, ms) = (v["t0"].as_u64().unwrap_or(0) as f64, v["ms"].as_u64().unwrap_or(0) as f64);
                format!(
                    "<div class=\"wf-row {}\"><span class=\"wf-label\" title=\"{}\">{} {}</span><span class=\"wf-track\"><span class=\"wf-bar {}\" style=\"left:{:.2}%;width:{:.2}%\"></span></span><span class=\"num\">{:.1}s</span></div>",
                    esc(&s(&v["kind"])), esc(&s(&v["line"])), esc(&s(&v["kind"])), esc(&s(&v["name"])), esc(&s(&v["outcome"])), t0 / end * 100.0, (ms / end * 100.0).max(0.3), ms / 1000.0
                )
            })
            .collect();
        html.push_str(&format!("<h3>timeline (phases, steps, sleeps and calls of 200 ms or more)</h3><div class=\"waterfall\">{rows}</div>"));
    }
    section("Timing", None, &html)
}

fn absolute(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| if p.is_absolute() { p.to_path_buf() } else { std::env::current_dir().map(|c| c.join(p)).unwrap_or_else(|_| p.to_path_buf()) })
}

/// `target` as a link from a page in `base`: relative (with `..`) when they share more than the root, else `file://`.
fn rel_href(base: &Path, target: &Path) -> String {
    let (b, t) = (absolute(base), absolute(target));
    let (bc, tc): (Vec<Component>, Vec<Component>) = (b.components().collect(), t.components().collect());
    let common = bc.iter().zip(&tc).take_while(|(x, y)| x == y).count();
    let enc = |s: &str| s.replace('%', "%25").replace(' ', "%20").replace('#', "%23").replace('?', "%3F");
    if common <= 1 {
        return format!("file://{}", enc(&t.display().to_string()));
    }
    let mut parts: Vec<String> = vec!["..".to_string(); bc.len() - common];
    parts.extend(tc[common..].iter().map(|c| enc(&c.as_os_str().to_string_lossy())));
    parts.join("/")
}

fn files_under(dir: &Path, exts: &[&str], skip: Option<&Path>, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if skip.is_some_and(|s| p == s) {
            continue;
        }
        if p.is_dir() {
            files_under(&p, exts, skip, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)) {
            out.push(p);
        }
    }
}

fn mtime(p: &Path) -> std::time::SystemTime {
    std::fs::metadata(p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH)
}

fn unique_existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    paths.into_iter().filter(|p| p.is_file() && seen.insert(absolute(p))).collect()
}

fn png_size(p: &Path) -> Option<(u32, u32)> {
    let mut head = [0u8; 24];
    std::io::Read::read_exact(&mut std::fs::File::open(p).ok()?, &mut head).ok()?;
    (&head[12..16] == b"IHDR").then(|| (u32::from_be_bytes(head[16..20].try_into().unwrap()), u32::from_be_bytes(head[20..24].try_into().unwrap())))
}

/// The input log as steps: `ms` since the recording (browser `t`) or the run start (android `at`), the kind and the rest.
fn steps(text: &str, started_ms: i64) -> Vec<Value> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["at"].as_i64().is_none_or(|a| a >= started_ms))
        .map(|v| {
            let kind = v["kind"].as_str().or(v["verb"].as_str()).unwrap_or("?").to_string();
            let ms = v["t"].as_i64().or_else(|| v["at"].as_i64().map(|a| a - started_ms));
            let show = |x: &Value| x.as_str().map_or_else(|| x.to_string(), str::to_string);
            let detail = match v["args"].as_array() {
                Some(a) => a.iter().map(show).collect::<Vec<_>>().join(" "),
                None => v.as_object().into_iter().flatten().filter(|(k, x)| !matches!(k.as_str(), "t" | "at" | "kind" | "verb" | "action") && !x.is_null()).map(|(k, x)| format!("{k}={}", show(x))).collect::<Vec<_>>().join(" "),
            };
            let mut step = json!({ "ms": ms, "kind": kind, "detail": detail });
            if let Some(n) = v["action"].as_u64() {
                step["n"] = json!(n); // the action id the on-page marker is numbered with
            }
            step
        })
        .collect()
}

/// The report as data: the facts plus what the run left in `dir` (and `extra_dirs`), with links relative to `dir`.
pub fn build(dir: &Path, f: &Facts) -> Value {
    let link = |p: &Path| json!({ "path": p.display().to_string(), "href": rel_href(dir, p) });
    let mut shots: Vec<PathBuf> = f.screenshots.iter().map(PathBuf::from).collect();
    let goldens_dir = dir.join("goldens");
    files_under(dir, &["png"], Some(&goldens_dir), &mut shots);
    for d in &f.extra_dirs {
        files_under(d, &["png"], None, &mut shots);
    }
    let mut shots = unique_existing(shots);
    shots.sort_by_key(|p| mtime(p));
    let mut videos: Vec<PathBuf> = f.recordings.iter().map(PathBuf::from).collect();
    files_under(dir, &["mp4", "webm"], None, &mut videos);
    let mut videos = unique_existing(videos);
    videos.sort();
    let mut evidence: Vec<PathBuf> = f.evidence.iter().map(PathBuf::from).collect();
    files_under(dir, &["json"], Some(&goldens_dir), &mut evidence);
    let evidence: Vec<PathBuf> = unique_existing(evidence).into_iter().filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("wait-timeout-") && !n.ends_with(".hierarchy.json"))).collect();
    let screen = full_size_shot(&shots).and_then(|p| png_size(p)).map(|(w, h)| match f.density {
        Some(d) => format!("{w}x{h}@{d}"),
        None => format!("{w}x{h}"),
    });
    let goldens: Vec<Value> = f
        .goldens
        .iter()
        .map(|o| {
            let opt = |p: &Option<PathBuf>| p.as_deref().filter(|p| p.exists()).map(link);
            json!({
                "name": o.name, "verdict": o.verdict, "ok": o.ok(), "message": o.message, "ratio": o.ratio, "differingPixels": o.differing,
                "golden": o.golden.display().to_string(), "actual": Some(o.actual.as_path()).filter(|p| p.exists()).map(link), "expected": opt(&o.expected), "diff": opt(&o.diff),
            })
        })
        .collect();
    let wall_ms = (f.finished_ms - f.started_ms).max(0) as u64;
    json!({
        "schema": SCHEMA,
        "verdict": f.verdict, "ok": f.verdict == "pass", "exitCode": f.exit_code,
        "message": f.message, "traceback": f.traceback, "notes": f.notes,
        "script": f.script, "platform": f.platform, "device": f.device, "screen": screen, "sha": f.sha, "speed": f.speed, "args": f.args,
        "startedMs": f.started_ms, "finishedMs": f.finished_ms,
        "started": rca_core::time::iso(f.started_ms), "finished": rca_core::time::iso(f.finished_ms),
        "durationS": (wall_ms as f64 / 100.0).round() / 10.0,
        "recordDir": absolute(dir).display().to_string(),
        "appStart": f.app_start,
        "timing": f.timing_lines.as_deref().filter(|t| !t.trim().is_empty()).map(|t| with_app_start(timing_summary(t, wall_ms), f.app_start)),
        "steps": f.inputs.as_deref().and_then(|p| std::fs::read_to_string(p).ok()).map(|t| steps(&t, f.started_ms)).unwrap_or_default(),
        "screenshots": shots.iter().map(|p| link(p)).collect::<Vec<_>>(),
        "recordings": videos.iter().map(|p| link(p)).collect::<Vec<_>>(),
        "goldens": goldens,
        "evidence": evidence.iter().map(|p| link(p)).collect::<Vec<_>>(),
        "warnings": f.warnings,
        "debugHits": f.debug_hits,
        "logs": f.logs,
        "stubs": f.stubs,
        "vm": f.vm,
    })
}

/// The VM samples as one line of extremes and a table, one row per sample.
fn vm_html(vm: &Value) -> String {
    let num = |v: &Value| v.as_f64().map(|n| format!("{n}")).unwrap_or_else(|| "–".into());
    let rows: String = list(&vm["samples"])
        .iter()
        .map(|x| match x.get("error") {
            Some(e) => format!("<tr><td class=\"num\">{:.1}</td><td colspan=\"4\">{}</td></tr>", x["tMs"].as_f64().unwrap_or(0.0) / 1000.0, esc(&s(e))),
            None => format!(
                "<tr><td class=\"num\">{:.1}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td><td class=\"num\">{}</td></tr>",
                x["tMs"].as_f64().unwrap_or(0.0) / 1000.0, num(&x["load1"]), num(&x["memAvailableMb"]), num(&x["device"]["memMb"]), num(&x["device"]["cpuPct"])
            ),
        })
        .collect();
    let head = format!(
        "<p>every {} s; load peaked at {}, MemAvailable fell to {} MB; device <code>{}</code> peaked at {} MB and {} % CPU</p>",
        num(&vm["everyS"]), num(&vm["load1Max"]), num(&vm["memAvailableMinMb"]), esc(&s(&vm["container"])), num(&vm["deviceMemMaxMb"]), num(&vm["deviceCpuMaxPct"])
    );
    section("VM", None, &format!("{head}<table><tr><th>s</th><th>load 1 min</th><th>MemAvailable MB</th><th>device MB</th><th>device CPU %</th></tr>{rows}</table>"))
}

fn s(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(t) => t.clone(),
        other => other.to_string(),
    }
}

fn list(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

/// One row per name: a short, readable value (functions as `ƒ`, objects by shape, long strings cut), the full JSON folded.
fn short_value(v: &Value) -> String {
    match v {
        Value::String(t) if t.trim_start().starts_with("function") || (t.trim_start().starts_with('(') && t.contains("=>")) || t.contains("[native code]") => "ƒ".to_string(),
        Value::String(t) => {
            let one: String = t.chars().take(120).collect();
            if t.chars().count() > 120 { format!("{one}…") } else { one }
        }
        Value::Object(o) => format!("{{{} keys}}", o.len()),
        Value::Array(a) => format!("[{} items]", a.len()),
        other => other.to_string(),
    }
}

fn is_library(file: &str) -> bool {
    file.contains("/node_modules/") || file.starts_with("node_modules/")
}

/// `…/node_modules/.pnpm/react-dom@19.2.3_react@19.2.3/node_modules/react-dom/cjs/x.js` reads as `react-dom/cjs/x.js`.
fn short_frame_path(file: &str) -> String {
    match file.rfind("/node_modules/") {
        Some(i) => file[i + "/node_modules/".len()..].to_string(),
        None => file.trim_start_matches('/').to_string(),
    }
}

fn values_table(o: &Map<String, Value>) -> String {
    let rows: String = o.iter().map(|(k, v)| format!("<tr><td><code>{}</code></td><td><code>{}</code></td></tr>", esc(k), esc(&short_value(v)))).collect();
    format!("<div class=\"vals\"><table>{rows}</table></div>")
}

fn section(title: &str, count: Option<usize>, body: &str) -> String {
    let n = count.map(|c| format!(" <span class=\"n\">{c}</span>")).unwrap_or_default();
    format!("<section><h2>{}{n}</h2>{body}</section>\n", esc(title))
}

fn pretty(v: &Value) -> String {
    esc(&serde_json::to_string_pretty(v).unwrap_or_default())
}

/// The page for a report.json value; sections with nothing in them are left out.
pub fn html(r: &Value) -> String {
    let verdict = s(&r["verdict"]);
    let mut out = String::new();
    let mut facts = String::new();
    for (label, key) in [("script", "script"), ("platform", "platform"), ("device", "device"), ("screen", "screen"), ("commit", "sha"), ("speed", "speed"), ("started", "started"), ("finished", "finished"), ("duration", "durationS"), ("exit code", "exitCode")] {
        let v = s(&r[key]);
        if !v.is_empty() {
            let v = if key == "durationS" { format!("{v} s") } else { v };
            facts.push_str(&format!("<dt>{label}</dt><dd>{}</dd>", esc(&v)));
        }
    }
    if let Some(args) = r["args"].as_object().filter(|a| !a.is_empty()) {
        let a: Vec<String> = args.iter().map(|(k, v)| format!("{k}={}", s(v))).collect();
        facts.push_str(&format!("<dt>args</dt><dd>{}</dd>", esc(&a.join(" "))));
    }
    out.push_str(&format!("<section class=\"run\"><h2>Run</h2><dl class=\"facts\">{facts}</dl></section>\n"));
    let (message, traceback) = (s(&r["message"]), s(&r["traceback"]));
    if !message.is_empty() || !traceback.is_empty() {
        let mut body = String::new();
        if !message.is_empty() {
            body.push_str(&format!("<pre class=\"msg\">{}</pre>", esc(&message)));
        }
        if !traceback.is_empty() {
            body.push_str(&format!("<h3>traceback</h3><pre>{}</pre>", esc(&traceback)));
        }
        out.push_str(&section("Failure", None, &body));
    }
    if !list(&r["notes"]).is_empty() {
        let items: String = list(&r["notes"]).iter().map(|n| format!("<li>{}</li>", esc(&s(n)))).collect();
        out.push_str(&section("Harness notes", None, &format!("<ul>{items}</ul>")));
    }
    if r["timing"].is_object() {
        out.push_str(&timing_html(&r["timing"]));
    }
    if r["vm"].is_object() {
        out.push_str(&vm_html(&r["vm"]));
    }
    let steps = list(&r["steps"]);
    if !steps.is_empty() {
        let items: String = steps.iter().enumerate().map(|(i, st)| format!("<li><span class=\"n\">{}</span><span class=\"t\">{}</span><span class=\"d\"><b>{}</b> {}</span></li>", st["n"].as_u64().unwrap_or(i as u64 + 1), esc(&st["ms"].as_i64().map(|m| format!("{:.1}s", m as f64 / 1000.0)).unwrap_or_default()), esc(&s(&st["kind"])), esc(&s(&st["detail"])))).collect();
        out.push_str(&section("Steps", Some(steps.len()), &format!("<ol class=\"steps\">{items}</ol>")));
    }
    let shots = list(&r["screenshots"]);
    if !shots.is_empty() {
        let figs: String = shots
            .iter()
            .map(|p| {
                let (href, name) = (esc(&s(&p["href"])), esc(&Path::new(&s(&p["path"])).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                format!("<figure><a href=\"{href}\"><img src=\"{href}\" alt=\"{name}\" loading=\"lazy\"></a><figcaption>{name}</figcaption></figure>")
            })
            .collect();
        out.push_str(&section("Screenshots", Some(shots.len()), &format!("<div class=\"figs\">{figs}</div>")));
    }
    if r["logs"].is_object() {
        out.push_str(&crate::android_logs::html(&r["logs"]));
    }
    let videos = list(&r["recordings"]);
    if !videos.is_empty() {
        let items: String = videos.iter().map(|p| format!("<figure><video controls preload=\"metadata\" src=\"{h}\"></video><figcaption><a href=\"{h}\">{h}</a></figcaption></figure>", h = esc(&s(&p["href"])))).collect();
        out.push_str(&section("Recordings", Some(videos.len()), &items));
    }
    let evidence = list(&r["evidence"]);
    if !evidence.is_empty() {
        let items: String = evidence.iter().map(|p| format!("<li><a href=\"{h}\">{h}</a></li>", h = esc(&s(&p["href"])))).collect();
        out.push_str(&section("Wait evidence", Some(evidence.len()), &format!("<ul>{items}</ul>")));
    }
    let goldens = list(&r["goldens"]);
    if !goldens.is_empty() {
        let cards: String = goldens
            .iter()
            .map(|g| {
                let figs: String = ["expected", "actual", "diff"]
                    .iter()
                    .filter_map(|k| g[*k]["href"].as_str().map(|h| format!("<figure><img src=\"{h}\" alt=\"{n} {k}\" loading=\"lazy\"><figcaption>{k}</figcaption></figure>", h = esc(h), n = esc(&s(&g["name"])))))
                    .collect();
                format!("<div class=\"card {v}\"><header><h3>{n}</h3><span class=\"badge\">{v}</span></header><p>{m}</p><div class=\"figs\">{figs}</div></div>", v = esc(&s(&g["verdict"])), n = esc(&s(&g["name"])), m = esc(&s(&g["message"])))
            })
            .collect();
        out.push_str(&section("Goldens", Some(goldens.len()), &cards));
    }
    let warnings = list(&r["warnings"]);
    if !warnings.is_empty() {
        let items: String = warnings.iter().map(|w| format!("<li><code>{}</code></li>", esc(&s(w)))).collect();
        out.push_str(&section("JS warnings", Some(warnings.len()), &format!("<ul class=\"warn\">{items}</ul>")));
    }
    if warnings.is_empty() {
        out.push_str(&section("JS warnings", Some(0), "<p class=\"none\">none</p>"));
    }
    if !r["stubs"].is_null() {
        let declared = list(&r["stubs"]["declared"]);
        let answered = list(&r["stubs"]["hits"]);
        let rows: String = declared
            .iter()
            .map(|d| {
                let pattern = s(&d["pattern"]);
                let n = answered.iter().filter(|h| s(&h["pattern"]) == pattern).count();
                format!("<li><code>{} {}</code> → {} · answered {n}×</li>", esc(d["method"].as_str().unwrap_or("any")), esc(&pattern), esc(&d["status"].as_i64().unwrap_or(200).to_string()))
            })
            .collect();
        out.push_str(&section("Network stubs", Some(declared.len()), &format!("<ul class=\"warn\">{rows}</ul>")));
    }
    let hits = list(&r["debugHits"]);
    if hits.is_empty() {
        out.push_str(&section("Debugger hits", Some(0), "<p class=\"none\">none</p>"));
    }
    if !hits.is_empty() {
        let items: String = hits
            .iter()
            .map(|h| {
                let all = list(&h["frames"]);
                let row = |f: &Value| format!("<li><code>{} {}:{}</code></li>", esc(&s(&f["function"])), esc(&short_frame_path(&s(&f["file"]))), esc(&s(&f["line"])));
                let app: String = all.iter().filter(|f| !is_library(&s(&f["file"]))).map(row).collect();
                let lib: Vec<&Value> = all.iter().filter(|f| is_library(&s(&f["file"]))).collect();
                let mut body = String::new();
                if !all.is_empty() {
                    body.push_str(&format!("<h4>stack</h4><ol>{app}</ol>"));
                    if !lib.is_empty() {
                        let rows: String = lib.iter().map(|f| row(f)).collect();
                        body.push_str(&format!("<details><summary>{} library frames</summary><ol>{rows}</ol></details>", lib.len()));
                    }
                }
                for (label, key) in [("locals", "locals"), ("captures", "captures")] {
                    if let Some(o) = h[key].as_object().filter(|o| !o.is_empty()) {
                        body.push_str(&format!("<h4>{label} <span class=\"n\">{}</span></h4>{}<details><summary>full {label}</summary><pre>{}</pre></details>", o.len(), values_table(o), pretty(&h[key])));
                    }
                }
                format!("<details open><summary><code>{}:{}</code> {}</summary>{body}</details>", esc(&s(&h["file"])), esc(&s(&h["line"])), esc(&s(&h["function"])))
            })
            .collect();
        out.push_str(&section("Debugger hits", Some(hits.len()), &items));
    }
    let n = |k: &str| r[k].as_array().map_or(0, Vec::len);
    let wall = r["timing"]["wallS"].as_f64().map(|w| format!("{w:.1}")).or_else(|| r["durationS"].as_f64().map(|d| format!("{d:.1}"))).unwrap_or_else(|| "–".into());
    let sha_short: String = s(&r["sha"]).chars().take(9).collect();
    let chips = [
        ("platform", s(&r["platform"])),
        ("device", s(&r["device"])),
        ("commit", sha_short),
        ("started", s(&r["started"])),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .map(|(k, v)| format!("<span class=\"chip\"><span class=\"k\">{k}</span>{}</span>", esc(&v)))
    .collect::<String>();
    let tiles = [
        ("wall s", wall),
        ("steps", n("steps").to_string()),
        ("screenshots", n("screenshots").to_string()),
        ("recordings", n("recordings").to_string()),
        ("warnings", n("warnings").to_string()),
        ("debugger hits", n("debugHits").to_string()),
    ]
    .into_iter()
    .map(|(k, v)| format!("<div class=\"tile\"><div class=\"v\">{}</div><div class=\"k\">{k}</div></div>", esc(&v)))
    .collect::<String>();
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\"><title>{title}</title><style>\
/* layout: one column of cards under a verdict band; data in mono, labels small caps; the same tokens in light and dark */\
:root{{--bg:#f4f3ee;--fg:#1c1e1a;--muted:#6b6f66;--card:#fffefb;--line:#dedbd2;--band:#e9f0ec;--accent:#1f6f6b;--ok:#1f7a3f;--bad:#b4342a;--new:#6f4fb3;--link:#1f6f6b;--bar:#8fc5bf;--mono:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;--sans:system-ui,-apple-system,\"Segoe UI\",Roboto,sans-serif;color-scheme:light}}\
@media (prefers-color-scheme:dark){{:root{{--bg:#15171a;--fg:#e9e8e2;--muted:#9ea39a;--card:#1d2024;--line:#33373b;--band:#1b2a28;--accent:#7cc5bf;--ok:#6fcf8c;--bad:#f08a7a;--new:#b9a1ea;--link:#7cc5bf;--bar:#2f6f6a;color-scheme:dark}}}}\
*{{box-sizing:border-box}}body{{margin:0;padding:0 0 max(24px,env(safe-area-inset-bottom,0px));background:var(--bg);color:var(--fg);font:14px/1.5 var(--sans);overflow-wrap:anywhere}}\
a{{color:var(--link)}}code,pre,.num,.t,.chip{{font-family:var(--mono)}}\
.band{{background:var(--band);border-bottom:1px solid var(--line);padding:max(16px,env(safe-area-inset-top,0px)) max(16px,env(safe-area-inset-right,0px)) 18px max(16px,env(safe-area-inset-left,0px))}}\
.band .inner,main{{max-width:960px;margin:0 auto}}main{{padding:16px max(16px,env(safe-area-inset-right,0px)) 0 max(16px,env(safe-area-inset-left,0px));display:grid;gap:14px}}\
.eyebrow{{margin:0 0 6px;font-size:11px;letter-spacing:.09em;text-transform:uppercase;color:var(--muted)}}\
h1{{font-size:22px;line-height:1.25;margin:0 0 10px;font-weight:650;text-wrap:balance}}h2{{font-size:15px;margin:0 0 10px;font-weight:650}}h3,h4{{font-size:13px;margin:10px 0 4px;color:var(--muted);text-transform:uppercase;letter-spacing:.06em}}\
.verdict{{display:inline-flex;align-items:center;gap:8px;font-weight:700;font-size:13px;letter-spacing:.06em;padding:4px 12px;border-radius:999px;border:2px solid currentColor;text-transform:uppercase;vertical-align:middle;margin-right:8px}}\
.verdict::before{{content:\"\";width:8px;height:8px;border-radius:50%;background:currentColor}}.pass{{color:var(--ok)}}.fail,.error{{color:var(--bad)}}.n{{color:var(--muted);font-weight:400}}\
.chips{{display:flex;flex-wrap:wrap;gap:6px;margin-top:10px}}.chip{{font-size:12px;background:var(--card);border:1px solid var(--line);border-radius:6px;padding:2px 8px}}.chip .k{{color:var(--muted);margin-right:6px;font-family:var(--sans);text-transform:uppercase;font-size:10px;letter-spacing:.06em}}\
.tiles{{display:grid;grid-template-columns:repeat(auto-fit,minmax(110px,1fr));gap:8px;margin-top:14px}}.tile{{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:8px 10px}}.tile .v{{font:600 20px/1.1 var(--mono);font-variant-numeric:tabular-nums}}.tile .k{{font-size:11px;color:var(--muted);text-transform:uppercase;letter-spacing:.06em;margin-top:2px}}\
section{{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:14px 16px;min-width:0}}section.run{{padding-bottom:8px}}\
.facts{{display:grid;grid-template-columns:max-content 1fr;gap:4px 16px;margin:0}}.facts dt{{color:var(--muted);font-size:12px;text-transform:uppercase;letter-spacing:.05em;padding-top:2px}}.facts dd{{margin:0;font-family:var(--mono);font-size:13px}}\
@media (max-width:480px){{.facts{{grid-template-columns:1fr;gap:0}}.facts dt{{margin-top:8px}}img{{max-width:100%}}.figs figure{{flex:1 1 100%}}}}\
pre{{white-space:pre-wrap;word-break:break-word;background:var(--bg);border:1px solid var(--line);padding:10px;border-radius:8px;margin:0;font-size:12.5px}}pre.msg{{color:var(--bad);border-color:var(--bad)}}\
.lede{{margin:0 0 8px;color:var(--muted)}}.lede b{{color:var(--fg);font-family:var(--mono)}}.none{{color:var(--muted);margin:0}}\
.wrap{{overflow-x:auto}}table{{border-collapse:collapse;width:100%;font-variant-numeric:tabular-nums}}td,th{{border-bottom:1px solid var(--line);padding:5px 10px 5px 0;text-align:left;vertical-align:top}}th{{font-size:11px;color:var(--muted);text-transform:uppercase;letter-spacing:.05em}}td.num,th.num{{text-align:right;white-space:nowrap}}\
.waterfall{{display:grid;gap:2px;font-size:12px}}.wf-row{{display:grid;grid-template-columns:minmax(0,14em) 1fr 4em;gap:8px;align-items:center}}.wf-label{{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}}.wf-track{{position:relative;height:10px;background:var(--line,rgba(127,127,127,.15));border-radius:2px}}.wf-bar{{position:absolute;top:0;height:10px;border-radius:2px;background:var(--bar)}}.wf-row.phase .wf-bar{{opacity:.35}}.wf-bar.miss{{opacity:.5}}.wf-bar.timeout,.wf-bar.error{{background:#c0392b}}.barcell{{width:40%;min-width:120px}}.bar{{display:block;height:10px;border-radius:3px;background:var(--bar)}}\
.vals{{max-height:40vh;overflow:auto;border:1px solid var(--line);border-radius:8px;padding:2px 8px;margin:4px 0}}.vals code{{overflow-wrap:anywhere}}.vals td:first-child code{{white-space:nowrap}}\
.steps{{padding-left:0;margin:0;list-style:none;counter-reset:step}}.steps li{{display:grid;grid-template-columns:28px 52px minmax(0,1fr);gap:8px;padding:4px 0;border-bottom:1px solid var(--line);counter-increment:step}}.steps li::before{{content:counter(step);color:var(--muted);font-family:var(--mono);font-size:12px}}.steps li:last-child{{border-bottom:0}}.t{{color:var(--muted);font-size:12px;padding-top:1px}}.steps .d{{min-width:0;overflow-wrap:anywhere}}\
.figs{{display:flex;flex-wrap:wrap;gap:12px}}figure{{margin:0;min-width:0}}figcaption{{font-size:12px;color:var(--muted);word-break:break-all;margin-top:4px}}\
img{{max-width:min(240px,38vw);border:1px solid var(--line);border-radius:6px;display:block;background:#fff}}video{{max-width:100%;max-height:70vh;display:block;border-radius:6px;background:#000}}\
.card{{border:1px solid var(--line);border-radius:8px;padding:10px 12px;margin:0 0 10px}}.card header{{display:flex;justify-content:space-between;gap:8px;align-items:center}}.badge{{font-weight:600;padding:2px 8px;border-radius:99px;border:1px solid currentColor;font-size:12px}}\
.card.match .badge{{color:var(--ok)}}.card.updated .badge{{color:var(--new)}}.card.mismatch .badge,.card.missing .badge{{color:var(--bad)}}\
details{{border-top:1px solid var(--line);padding:8px 0}}details:first-of-type{{border-top:0}}summary{{cursor:pointer;overflow-wrap:anywhere;font-weight:600}}summary code,details code{{overflow-wrap:anywhere;word-break:break-all}}details details{{border:0;padding:4px 0}}details details summary{{font-weight:400;color:var(--muted);font-size:12px}}\
ol{{padding-left:1.4em;margin:4px 0}}.warn code{{word-break:break-word}}\
</style></head><body><header class=\"band\"><div class=\"inner\"><p class=\"eyebrow\">{platform} harness run</p><h1>{title}</h1><span class=\"verdict {v}\">{v}</span><div class=\"chips\">{chips}</div><div class=\"tiles\">{tiles}</div></div></header><main>\n{out}</main></body></html>\n",
        platform = esc(&s(&r["platform"])),
        title = esc(&s(&r["script"])),
        v = esc(&verdict),
    )
}

/// Writes `report.json` and `report.html` into `dir`; returns the page's path.
pub fn write(dir: &Path, f: &Facts) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let report = build(dir, f);
    let page = dir.join("report.html");
    std::fs::write(dir.join("report.json"), format!("{}\n", serde_json::to_string_pretty(&report).unwrap_or_default())).map_err(|e| format!("cannot write report.json: {e}"))?;
    std::fs::write(&page, html(&report)).map_err(|e| format!("cannot write report.html: {e}"))?;
    Ok(page)
}

/// The first screenshot taken at the device's size: a named one is kept downscaled under shots/ (the golden size).
fn full_size_shot(shots: &[PathBuf]) -> Option<&PathBuf> {
    shots.iter().find(|p| !p.components().any(|c| c.as_os_str() == "shots")).or(shots.first())
}

#[cfg(test)]
mod screen_tests {
    use super::full_size_shot;
    use std::path::PathBuf;

    #[test]
    fn given_a_named_screenshot_first_when_the_screen_is_measured_then_a_full_size_capture_is_used() {
        let shots = [PathBuf::from("rec/shots/tamil-uyir-1-open.png"), PathBuf::from("rec/out/a1-ink.png")];
        assert_eq!(full_size_shot(&shots), Some(&shots[1]));
        assert_eq!(full_size_shot(&shots[..1]), Some(&shots[0]));
    }
}
