//! The four suites and the A/B run. Each returns `{suite, metrics, env}` with every metric summarized over the runs.
use super::domain::{
    executed_bytes_by_package, executed_bytes_by_source, merge_totals, metro_bundle_ms, median, platform_bundle,
    summarize, summarize_ab, tamagui_plugin_ms, totals_json,
};
use super::export::{ExportParams, Exporter};
use super::lab::{Lab, LabParams, Page};
use wf_kit::root::repo_root;
use serde_json::{json, Map, Value};
use sourcemap::SourceMap;
use std::path::Path;

type Res<T> = Result<T, String>;

/// Metric name, unit and one value per run, in the order the metrics were first seen.
#[derive(Default)]
struct Collected(Vec<(String, String, Vec<f64>)>);

impl Collected {
    fn push(&mut self, name: &str, unit: &str, value: f64) {
        match self.0.iter_mut().find(|(n, _, _)| n == name) {
            Some(slot) => slot.2.push(value),
            None => self.0.push((name.to_string(), unit.to_string(), vec![value])),
        }
    }

    fn summaries(&self) -> Res<Map<String, Value>> {
        let mut out = Map::new();
        for (name, unit, values) in &self.0 {
            out.insert(name.clone(), summarize(unit, values)?);
        }
        Ok(out)
    }
}

fn result(suite: &str, metrics: Map<String, Value>, env: &[(String, String)]) -> Value {
    let env: Map<String, Value> = env.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
    json!({ "suite": suite, "metrics": Value::Object(metrics), "env": Value::Object(env) })
}

pub fn run(suite: &str, scenario: &Value, runs: f64, tmp: &Path) -> Res<Value> {
    let out_dir = tmp.join("export");
    match suite {
        "build" => build(scenario, runs, &out_dir),
        "memory" => memory(scenario, runs, &out_dir, tmp),
        "runtime" => runtime(scenario, runs, &out_dir, tmp),
        "startup" => startup(scenario, runs, &tmp.join("export-startup"), tmp),
        "android" => super::android::run(scenario, runs),
        other => Err(format!("unknown suite: {other}")),
    }
}

// One run per platform is a cold export (caches cleared) followed by a warm one (caches kept). Web keeps the
// original metric names; every other platform is prefixed with its name.
fn build(scenario: &Value, runs: f64, out_dir: &Path) -> Res<Value> {
    let exporter = Exporter::new(ExportParams::from(&scenario["params"])?, &repo_root(), out_dir);
    let platforms = exporter.params.platforms.clone();
    let mut collected = Collected::default();
    let mut i = 0.0;
    while i < runs {
        for platform in &platforms {
            let web = platform == "web";
            let name = |web_name: &str, suffix: &str| if web { web_name.to_string() } else { format!("{platform}{suffix}") };
            exporter.clear_caches();
            let cold = exporter.run_export(platform)?;
            collected.push(&name("coldExportMs", "ColdExportMs"), "ms", cold.ms as f64);
            let (bundle_ms, modules) = platform_bundle(&cold.log, platform);
            collected.push(&name("webModuleCount", "ModuleCount"), "modules", modules as f64);
            if web {
                collected.push("tamaguiPluginMs", "ms", tamagui_plugin_ms(&cold.log) as f64);
                collected.push("metroBundleMs", "ms", metro_bundle_ms(&cold.log) as f64);
            } else {
                collected.push(&format!("{platform}BundleMs"), "ms", bundle_ms as f64);
            }
            let (raw, gzip) = exporter.measure_output(platform);
            collected.push(&name("jsBytesRaw", "BundleBytes"), "bytes", raw as f64);
            if web {
                collected.push("jsBytesGzip", "bytes", gzip as f64);
            }
            let warm = exporter.run_export(platform)?;
            collected.push(&name("warmExportMs", "WarmExportMs"), "ms", warm.ms as f64);
        }
        i += 1.0;
    }
    Ok(result("build", collected.summaries()?, &exporter.environment()))
}

fn open_lab(scenario: &Value, export_dir: &Path, tmp: &Path) -> Res<Lab> {
    Lab::open(LabParams::from(&scenario["params"])?, export_dir, tmp)
}

fn lab_runs<F>(scenario: &Value, runs: f64, export_dir: &Path, tmp: &Path, mut each: F) -> Res<(Collected, Vec<(String, String)>)>
where
    F: FnMut(&mut Lab, &mut Collected) -> Res<()>,
{
    let mut lab = open_lab(scenario, export_dir, tmp)?;
    let env = lab.environment();
    let mut collected = Collected::default();
    let mut outcome = Ok(());
    let mut i = 0.0;
    while i < runs {
        outcome = each(&mut lab, &mut collected);
        if outcome.is_err() {
            break;
        }
        i += 1.0;
    }
    lab.dispose();
    outcome?;
    Ok((collected, env))
}

struct Reading {
    heap: f64,
    dom_nodes: f64,
    listeners: f64,
}

fn read(page: &Page, label: &str) -> Res<Reading> {
    let sample = page.memory(label)?;
    Ok(Reading { heap: sample.heap_bytes, dom_nodes: sample.dom_nodes, listeners: sample.listeners })
}

fn memory(scenario: &Value, runs: f64, export_dir: &Path, tmp: &Path) -> Res<Value> {
    let params = &scenario["params"];
    let (collected, env) = lab_runs(scenario, runs, export_dir, tmp, |lab, collected| {
        let shell = match params["shellProbe"].as_str() {
            Some(probe) => {
                let page = lab.open_page()?;
                let reading = page.load(probe).and_then(|_| read(&page, "harness-shell"));
                page.close();
                reading?
            }
            None => Reading { heap: 0.0, dom_nodes: 0.0, listeners: 0.0 },
        };
        let page = lab.open_page()?;
        let measured = measure_memory(&page, params, &shell, collected);
        page.close();
        measured
    })?;
    Ok(result("memory", collected.summaries()?, &env))
}

/// The device frame's own page (the harness shell) shares the isolate with the app, so every reading also has an app-only
/// value with the shell's heap, DOM nodes and listeners taken off.
fn measure_memory(page: &Page, params: &Value, shell: &Reading, values: &mut Collected) -> Res<()> {
    page.load(params["start"].as_str().unwrap_or("/"))?;
    let cold = read(page, "cold-start")?;
    values.push("coldStartHeapBytes", "bytes", cold.heap);
    if shell.heap > 0.0 {
        values.push("harnessShellHeapBytes", "bytes", shell.heap);
        values.push("coldStartAppHeapBytes", "bytes", cold.heap - shell.heap);
        values.push("coldStartAppDomNodes", "nodes", cold.dom_nodes - shell.dom_nodes);
        values.push("coldStartAppListeners", "listeners", cold.listeners - shell.listeners);
    }
    values.push("coldStartDomNodes", "nodes", cold.dom_nodes);
    values.push("coldStartListeners", "listeners", cold.listeners);

    for route in params["routes"].as_array().into_iter().flatten() {
        let id = route["id"].as_str().unwrap_or("");
        page.go_to(route["path"].as_str().unwrap_or("/"))?;
        let reading = read(page, &format!("route-{id}"))?;
        values.push(&format!("route.{id}.heapBytes"), "bytes", reading.heap);
        if shell.heap > 0.0 {
            values.push(&format!("route.{id}.appHeapBytes"), "bytes", reading.heap - shell.heap);
            values.push(&format!("route.{id}.appDomNodes"), "nodes", reading.dom_nodes - shell.dom_nodes);
        }
        values.push(&format!("route.{id}.domNodes"), "nodes", reading.dom_nodes);
    }

    let (a, b) = (params["loop"]["a"].as_str().unwrap_or("/"), params["loop"]["b"].as_str().unwrap_or("/"));
    let (cycles, warmup) = (params["loop"]["cycles"].as_u64().unwrap_or(0), params["loop"]["warmup"].as_u64().unwrap_or(0));
    for _ in 0..warmup {
        page.go_to(a)?;
        page.go_to(b)?;
    }
    page.go_to(a)?;
    let before = read(page, "loop-before")?;
    for _ in 0..cycles {
        page.go_to(b)?;
        page.go_to(a)?;
    }
    let after = read(page, "loop-after")?;
    values.push("loopGrowthBytes", "bytes", (after.heap - before.heap).max(0.0));
    values.push("loopGrowthDomNodes", "nodes", (after.dom_nodes - before.dom_nodes).max(0.0));
    values.push("loopGrowthListeners", "listeners", (after.listeners - before.listeners).max(0.0));
    // A second window of the same length: code tier-up and lazily built modules fill the first one, a real leak
    // grows both. This is the number to hold near zero.
    for _ in 0..cycles {
        page.go_to(b)?;
        page.go_to(a)?;
    }
    let steady = read(page, "loop-steady")?;
    values.push("loopSteadyGrowthBytes", "bytes", (steady.heap - after.heap).max(0.0));
    Ok(())
}

// Long tasks (buffered, so the load counts) and the time from a finger lifting to the second frame after it: the
// handler, the React render and the paint.
const PROBES: &str = "(() => {
  if (window.__bench) return 'installed';
  const bench = { latencies: [], longTaskCount: 0, longTaskMs: 0 };
  window.__bench = bench;
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      bench.longTaskCount += 1;
      bench.longTaskMs += entry.duration;
    }
  }).observe({ type: 'longtask', buffered: true });
  document.addEventListener('pointerup', () => {
    const t0 = performance.now();
    requestAnimationFrame(() => requestAnimationFrame(() => bench.latencies.push(performance.now() - t0)));
  }, true);
  return 'installed';
})()";

const COLLECT: &str = "(() => {
  const b = window.__bench;
  const out = { latencies: b.latencies.slice(), longTaskCount: b.longTaskCount, longTaskMs: Math.round(b.longTaskMs) };
  b.latencies.length = 0;
  return JSON.stringify(out);
})()";

fn mount_expression(base_url: &str, path: &str, test_id: &str) -> String {
    let target = serde_json::to_string(&format!("{base_url}{path}")).unwrap_or_default();
    format!(
        "new Promise((resolve) => {{
    const t0 = performance.now();
    history.pushState({{}}, '', {target});
    window.dispatchEvent(new PopStateEvent('popstate'));
    const check = () => {{
      if (document.querySelector('[data-testid=\"{test_id}\"]')) {{
        requestAnimationFrame(() => requestAnimationFrame(() => resolve(performance.now() - t0)));
      }} else if (performance.now() - t0 > 15000) {{
        resolve(-1);
      }} else {{
        requestAnimationFrame(check);
      }}
    }};
    check();
  }})"
    )
}

struct Collect {
    latencies: Vec<f64>,
    long_task_count: f64,
    long_task_ms: f64,
}

fn collect(page: &Page) -> Res<Collect> {
    let text = page.evaluate(COLLECT)?;
    let parsed: Value = serde_json::from_str(text.as_str().unwrap_or("{}")).map_err(|e| e.to_string())?;
    Ok(Collect {
        latencies: parsed["latencies"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default(),
        long_task_count: parsed["longTaskCount"].as_f64().unwrap_or(0.0),
        long_task_ms: parsed["longTaskMs"].as_f64().unwrap_or(0.0),
    })
}

fn tap_test_id(page: &Page, test_id: &str) -> Res<()> {
    page.evaluate(&format!("document.querySelector('[data-testid=\"{test_id}\"]')?.scrollIntoView({{ block: 'center' }})"))?;
    page.tap(&format!("[data-testid=\"{test_id}\"]"))?;
    page.settle();
    Ok(())
}

fn round1(value: f64) -> f64 {
    (value * 10.0 + 0.5).floor() / 10.0
}

fn runtime(scenario: &Value, runs: f64, export_dir: &Path, tmp: &Path) -> Res<Value> {
    let params = &scenario["params"];
    let (collected, env) = lab_runs(scenario, runs, export_dir, tmp, |lab, collected| {
        let page = lab.open_page()?;
        let measured = measure_runtime(&page, params, collected);
        page.close();
        measured
    })?;
    Ok(result("runtime", collected.summaries()?, &env))
}

fn measure_runtime(page: &Page, params: &Value, values: &mut Collected) -> Res<()> {
    let base_url = params["baseUrl"].as_str().unwrap_or("");
    let (tap, variant, mount) = (&params["tap"], &params["variant"], &params["mount"]);
    page.load(tap["path"].as_str().unwrap_or("/"))?;
    page.evaluate(PROBES)?;

    collect(page)?;
    for _ in 0..tap["count"].as_u64().unwrap_or(0) {
        tap_test_id(page, tap["testId"].as_str().unwrap_or(""))?;
    }
    let taps = collect(page)?.latencies;

    let mut mounts = Vec::new();
    for _ in 0..mount["samples"].as_u64().unwrap_or(0) {
        let (path, ready) = (mount["path"].as_str().unwrap_or("/"), mount["readyTestId"].as_str().unwrap_or(""));
        let ms = page.evaluate(&mount_expression(base_url, path, ready))?.as_f64().filter(|ms| *ms >= 0.0).ok_or_else(|| format!("{ready} never appeared at {path}"))?;
        mounts.push((ms * 10.0).round() / 10.0);
        page.go_to(mount["awayPath"].as_str().unwrap_or("/"))?;
    }

    page.go_to(variant["path"].as_str().unwrap_or("/"))?;
    collect(page)?;
    let ids: Vec<&str> = variant["testIds"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    for i in 0..variant["count"].as_u64().unwrap_or(0) as usize {
        if let Some(id) = ids.get(i % ids.len().max(1)).filter(|id| !id.is_empty()) {
            tap_test_id(page, id)?;
        }
    }
    let variants = collect(page)?.latencies;
    let long = collect(page)?;

    values.push("tapLatencyMs", "ms", round1(median(&taps)?));
    values.push("variantUpdateMs", "ms", round1(median(&variants)?));
    values.push("deepTreeMountMs", "ms", round1(median(&mounts)?));
    values.push("longTasks", "tasks", long.long_task_count);
    values.push("longTaskMs", "ms", long.long_task_ms);
    Ok(())
}

struct Totals {
    by_package: Vec<(String, u64)>,
    by_source: Vec<(String, u64)>,
    script_bytes: u64,
}

fn startup(scenario: &Value, runs: f64, export_dir: &Path, tmp: &Path) -> Res<Value> {
    let params = &scenario["params"];
    let packages: Vec<String> = params["packages"].as_array().map(|a| a.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let filter = params["scriptFilter"].as_str().unwrap_or("");
    // The lab refuses a missing export, and this suite makes its own, so it opens after the export.
    let exporter = Exporter::new(ExportParams::from(params)?, &repo_root(), export_dir);
    exporter.run_export("web")?;
    let mut lab = open_lab(scenario, export_dir, tmp)?;
    let env = lab.environment();
    let mut samples: Vec<Totals> = Vec::new();
    let mut outcome = Ok(());
    let mut i = 0.0;
    while i < runs {
        outcome = cold_start(&mut lab, params, filter, export_dir).and_then(|scripts| {
            let mut parts = Vec::new();
            let mut script_bytes = 0u64;
            for script in &scripts {
                let (code, map) = source(export_dir, &script.0)?;
                script_bytes += script.1 as u64;
                parts.push(executed_bytes_by_source(&code, &map, &script.2));
            }
            let by_source = merge_totals(&parts);
            samples.push(Totals { by_package: executed_bytes_by_package(&by_source), by_source, script_bytes });
            Ok(())
        });
        if outcome.is_err() {
            break;
        }
        i += 1.0;
    }
    lab.dispose();
    outcome?;
    if let Some(last) = samples.last() {
        std::fs::create_dir_all(tmp).map_err(|e| e.to_string())?;
        let kept = json!({ "byPackage": totals_json(&last.by_package), "bySource": totals_json(&last.by_source) });
        std::fs::write(tmp.join("startup-packages.json"), serde_json::to_string_pretty(&kept).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    let series = |pick: &dyn Fn(&Totals) -> u64| -> Res<Value> { summarize("bytes", &samples.iter().map(|s| pick(s) as f64).collect::<Vec<_>>()) };
    let mut metrics = Map::new();
    metrics.insert("startupScriptBytes".into(), series(&|s| s.script_bytes)?);
    metrics.insert("startupExecutedBytes".into(), series(&|s| s.by_package.iter().map(|(_, b)| *b).sum())?);
    let mut listed: Vec<String> = packages.clone();
    listed.push("first-party".into());
    for name in &listed {
        metrics.insert(format!("startup.{name}.executedBytes"), series(&|s| s.by_package.iter().find(|(n, _)| n == name).map_or(0, |(_, b)| *b))?);
    }
    metrics.insert("startup.other.executedBytes".into(), series(&|s| s.by_package.iter().filter(|(n, _)| !listed.contains(n)).map(|(_, b)| *b).sum())?);
    Ok(result("startup", metrics, &env))
}

/// A fresh browser loads the start route; the scripts it ran: url, length, executed ranges.
type ScriptCoverage = (String, usize, Vec<(usize, usize)>);

fn cold_start(lab: &mut Lab, params: &Value, filter: &str, _export_dir: &Path) -> Res<Vec<ScriptCoverage>> {
    let page = lab.open_page()?;
    let run = || -> Res<Vec<ScriptCoverage>> {
        page.coverage_start()?;
        page.load(params["start"].as_str().unwrap_or("/"))?;
        let scripts: Vec<ScriptCoverage> = page.coverage_stop()?.into_iter().filter(|s| s.url.contains(filter)).map(|s| (s.url, s.length, s.ranges)).collect();
        if scripts.is_empty() {
            return Err(format!("the page loaded no script matching {filter}: the app did not start (is the export served and the device-frame page built?)"));
        }
        Ok(scripts)
    };
    let scripts = run();
    page.close();
    scripts
}

/// The script's text and its source map; fails when the export has no map for it.
fn source(export_dir: &Path, url: &str) -> Res<(String, SourceMap)> {
    let at = url.find("/_expo/").ok_or_else(|| format!("script outside the export: {url}"))?;
    let path = export_dir.join(url[at..].trim_start_matches('/'));
    let code = std::fs::read_to_string(&path).map_err(|e| format!("ENOENT: {}: {e}", path.display()))?;
    let map_path = format!("{}.map", path.display());
    // A map whose x_facebook_sources holds nulls is read as a Hermes map and refused; the plain v3 fields are enough here.
    let mut raw: Value = serde_json::from_slice(&std::fs::read(&map_path).map_err(|e| format!("ENOENT: {map_path}: {e}"))?).map_err(|e| format!("{map_path}: {e}"))?;
    if let Some(fields) = raw.as_object_mut() {
        fields.remove("x_facebook_sources");
    }
    let map = SourceMap::from_slice(&serde_json::to_vec(&raw).map_err(|e| e.to_string())?).map_err(|e| format!("{map_path}: {e}"))?;
    Ok((code, map))
}

/// Interleaved pairs of cold exports (baseline, then the candidate with extra environment), so a machine that
/// speeds up or slows down between runs hits both arms.
pub fn ab(scenario: &Value, tmp: &Path, platform: &str, candidate_env: &[(String, String)], pairs: f64) -> Res<()> {
    let params = ExportParams::from(&scenario["params"])?;
    let out_dir = tmp.join("export-ab");
    let arms = [Exporter::new(params.with_env(&[]), &repo_root(), &out_dir), Exporter::new(params.with_env(candidate_env), &repo_root(), &out_dir)];
    let (mut baseline, mut candidate) = (Vec::new(), Vec::new());
    let mut pair = 0.0;
    while pair < pairs {
        for (i, name) in ["baseline", "candidate"].into_iter().enumerate() {
            arms[i].clear_caches();
            let ms = arms[i].run_export(platform)?.ms as f64;
            println!("  {name} {} s", super::to_fixed(ms / 1000.0, 1));
            if i == 0 { baseline.push(ms) } else { candidate.push(ms) }
        }
        pair += 1.0;
    }
    let r = summarize_ab(&baseline, &candidate)?;
    let s = |ms: f64| format!("{} s", super::to_fixed(ms / 1000.0, 1));
    println!(
        "ab {platform}: baseline median {}, candidate median {}, ratio {}, candidate faster in {} of {} pairs",
        s(r.baseline_median),
        s(r.candidate_median),
        super::to_fixed(r.ratio, 3),
        r.candidate_wins,
        candidate.len()
    );
    Ok(())
}
