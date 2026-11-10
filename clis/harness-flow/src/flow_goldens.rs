//! `wf flow goldens`: the flow harness's golden pass. Each frozen DOM a flow test wrote (`<snapshots>/<suite>/<name>.html`)
//! is rendered in headless Chromium at phone size, offline, and checked against `<goldens>/<suite>/<name>.<scheme>.png`.
use wf_kit::args::{parse_spec, Positional, Spec};
use crate::browser::VisualBrowser;
use harness_core::golden::{self, Outcome, Tolerance};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Transitions caught mid-flight made the same snapshot render differently from run to run.
const STILL: &str = "(()=>{const s=document.createElement('style');s.textContent='*,*::before,*::after{animation:none!important;transition:none!important;caret-color:transparent!important}';document.head.appendChild(s);document.getAnimations().forEach(a=>{try{a.finish()}catch(e){a.cancel()}});return 1})()";
const SUMMARY: &str = "(()=>{const ids=[...document.querySelectorAll('[data-testid]')].map(e=>e.getAttribute('data-testid'));return JSON.stringify({testids:[...new Set(ids)].slice(0,60),text:((document.body&&document.body.innerText)||'').slice(0,400)})})()";

fn snapshots(root: &Path) -> Result<Vec<(String, String, PathBuf)>, String> {
    let mut out = vec![];
    let mut suites: Vec<_> = std::fs::read_dir(root).map_err(|e| format!("cannot read {}: {e}", root.display()))?.flatten().filter(|e| e.path().is_dir()).collect();
    suites.sort_by_key(|e| e.file_name());
    for suite in suites {
        let mut files: Vec<_> = std::fs::read_dir(suite.path()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "html")).collect();
        files.sort();
        for f in files {
            let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
            out.push((suite.file_name().to_string_lossy().into_owned(), stem, f));
        }
    }
    Ok(out)
}

/// Goldens of this scheme that no snapshot produced (a renamed or deleted test).
fn unused(goldens: &Path, scheme: &str, seen: &[PathBuf]) -> Vec<PathBuf> {
    let suffix = format!(".{scheme}.png");
    let mut out: Vec<PathBuf> = std::fs::read_dir(goldens)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .flat_map(|d| std::fs::read_dir(d.path()).into_iter().flatten().flatten().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(&suffix) && !seen.contains(p))
        .collect();
    out.sort();
    out
}

fn render(browser: &mut VisualBrowser, html: &Path, scheme: &str) -> Result<(golden::Image, Value), String> {
    let abs = std::fs::canonicalize(html).map_err(|e| format!("{}: {e}", html.display()))?;
    browser.page.set_color_scheme(scheme)?;
    browser.navigate_loaded(&format!("file://{}", abs.display()))?;
    browser.page.eval_page(STILL)?;
    let settle = Instant::now();
    while settle.elapsed() < Duration::from_secs(3) {
        if browser.page.eval_page("document.fonts.status").ok().and_then(|v| v.as_str().map(str::to_string)).as_deref() == Some("loaded") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(100));
    let summary = browser.page.eval_page(SUMMARY).ok().and_then(|v| v.as_str().and_then(|s| serde_json::from_str(s).ok())).unwrap_or(Value::Null);
    let png = browser.page.screenshot_png(true)?;
    Ok((golden::decode(&png)?, summary))
}

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(
        args,
        &Spec { values: &["snapshots", "goldens", "color-scheme", "report", "threshold", "pixel-threshold", "width", "height", "max-width", "max-height"], flags: &["update", "keep-unused", "save"], required: &["snapshots", "goldens"], positional: Positional::None, ..Spec::NONE },
    )?;
    let (snap_dir, goldens) = (PathBuf::from(p.val("snapshots")?), PathBuf::from(p.val("goldens")?));
    let scheme = p.val("color-scheme").unwrap_or("light").to_string();
    let report = p.val("report").map(PathBuf::from).unwrap_or_else(|| snap_dir.parent().unwrap_or(Path::new(".")).join("report.html"));
    let report_dir = report.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    let num = |k: &str, d: f64| p.val(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d);
    let tol = Tolerance { threshold: num("pixel-threshold", golden::THRESHOLD), max_ratio: num("threshold", golden::MAX_RATIO), ignore: vec![] };
    let (width, height, max_width) = (num("width", 393.0) as i64, num("height", 852.0) as i64, num("max-width", golden::MAX_WIDTH as f64) as usize);
    let max_height = num("max-height", 2.0 * height as f64) as usize;
    let update = p.flag("update");
    // --save paints the screenshots for the run's report and compares nothing (a run without --goldens)
    let save_only = p.flag("save") && !update;
    let found = match snapshots(&snap_dir) {
        Ok(f) => f,
        Err(e) => {
            println!("{}", json!({ "ok": false, "error": e, "next": "run the flow tests with FLOW_GOLDENS=1 first (pnpm --filter mla-tamagui run test:goldens)" }));
            return Some(1);
        }
    };
    let mut browser = match VisualBrowser::open_with((width, height), None) {
        Ok(b) => b,
        Err(e) => {
            println!("{}", json!({ "ok": false, "error": format!("cannot start Chromium: {e}"), "next": "set WF_CHROMIUM to a Chromium binary" }));
            return Some(1);
        }
    };
    let (mut outcomes, mut seen, mut errors): (Vec<Outcome>, Vec<PathBuf>, Vec<Value>) = (vec![], vec![], vec![]);
    for (suite, step, html) in &found {
        let name = format!("{suite}/{step}.{scheme}");
        let gold = goldens.join(suite).join(format!("{step}.{scheme}.png"));
        seen.push(gold.clone());
        let started = Instant::now();
        let checked = render(&mut browser, html, &scheme).and_then(|(mut full, summary)| {
            let page_height = full.height;
            if full.height > max_height {
                full.rgba.truncate(full.width * max_height * 4);
                full.height = max_height;
            }
            let img = golden::downscale(&full, max_width);
            let meta = json!({
                "suite": suite, "step": step, "scheme": scheme, "snapshot": format!("{suite}/{step}.html"),
                "viewport": { "width": width, "height": height }, "screen": { "width": full.width, "height": full.height, "page_height": page_height },
                "summary": summary, "timings": { "render_ms": started.elapsed().as_millis() as u64 },
            });
            let stem = report_dir.join("out").join(suite).join(format!("{step}.{scheme}"));
            if save_only {
                return golden::save(&name, &img, &stem.with_extension(format!("{scheme}.actual")), meta);
            }
            golden::check(&name, &img, &gold, &stem, update, &tol, meta)
        });
        match checked {
            Ok(o) => {
                if !o.ok() {
                    wf_kit::wf_err!("wf: {}", o.message);
                }
                outcomes.push(o);
            }
            Err(e) => {
                wf_kit::wf_err!("wf: {name}: {e}");
                errors.push(json!({ "name": name, "error": e }));
            }
        }
    }
    browser.close();
    let stale = if save_only { vec![] } else { unused(&goldens, &scheme, &seen) };
    // a run of one test file sees only its own snapshots: the other suites' goldens are not stale, so they stay
    let prune = update && !p.flag("keep-unused");
    if prune {
        for s in &stale {
            let _ = std::fs::remove_file(s);
            let _ = std::fs::remove_file(s.with_extension("json"));
        }
    }
    let _ = std::fs::create_dir_all(&report_dir);
    let _ = std::fs::write(&report, golden::gallery(&format!("Flow goldens ({scheme})"), &report_dir, &outcomes));
    let count = |v: &str| outcomes.iter().filter(|o| o.verdict == v).count();
    let ok = errors.is_empty() && (save_only || !found.is_empty()) && outcomes.iter().all(Outcome::ok);
    let summary = json!({
        "ok": ok,
        "scheme": scheme,
        "report": report.display().to_string(),
        "counts": { "saved": count("saved"), "match": count("match"), "updated": count("updated"), "mismatch": count("mismatch"), "missing": count("missing"), "errors": errors.len() },
        "unused": stale.iter().map(|s| s.display().to_string()).collect::<Vec<_>>(),
        "removed_unused": prune,
        "errors": errors,
        "goldens": outcomes.iter().map(Outcome::json).collect::<Vec<_>>(),
        "next": if ok { Value::Null } else if found.is_empty() { json!("no snapshots: run the flow tests with FLOW_GOLDENS=1") } else { json!(format!("open {} to see expected, actual and diff; rerun with --update when the change is intended", report.display())) },
    });
    println!("{summary}");
    Some(if ok { 0 } else { 1 })
}
