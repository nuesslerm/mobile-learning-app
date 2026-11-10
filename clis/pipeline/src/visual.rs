//! `wf check --visual`: the visual gate over changed routes. A web export is served locally, every planned route is opened in a
//! Chromium of our own and screenshotted, the screenshot is checked for console errors, blankness and a pixel diff
//! against its baseline, and a changed route over the diff threshold is put to a panel of vision judges.
use wf_kit::args::{parse_spec, Positional, Spec};
use harness_core::pixelmatch::pixelmatch;
use harness_flow::browser::VisualBrowser;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Res<T> = Result<T, String>;

const USAGE: &str = "usage:
  wf check --visual --files <a,b,…> [--export-dir <dir>] [--out-dir <dir>] [--base-path <p>]
                   [--baseline-store fs|oci] [--baseline-dir <dir>] [--sha <sha>] [--base-sha <sha>]
                   [--acceptance <file>] [--update-baselines]
  fs:  baselines in --baseline-dir/<route>.png (default)
  oci: baselines/<route>/<sha>.png in the bucket at $GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX
       via the oci CLI; compares against --base-sha (else newest), saves under --sha, keeps 20;
       with --sha also records the result at verify/<sha>.tier2.json for `wf check --deployed <sha>`
  --acceptance: a changed route over the diff threshold fails only if both vision judges agree";

const COLLECTOR: &str = include_str!("../../cdp/src/js/visual_collector.js");
const PROBE: &str = include_str!("../../cdp/src/js/visual_probe.js");
const LADDER_FILE: &str = "libraries/rca-core/plugins/ladder/data/model-ladders.json";
const BUCKET_ENV: &str = "GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX";
const APP_ROOT: &str = "frontend/mla-tamagui";
const APP_DIR: &str = "frontend/mla-tamagui/app";
const SMOKE_ROUTES: [&str; 4] = ["/", "/coding-agent", "/leetcode/[id]", "/tamil"];
const BLANK_MAX_RATIO: f64 = 0.0002;
const DIFF_MAX_RATIO: f64 = 0.001;
const PIXEL_THRESHOLD: f64 = 0.1;
const BASELINES_KEPT: usize = 20;
const VIEWPORT: (i64, i64) = (390, 844);
const IDLE_POLL_MS: u64 = 250;
const IDLE_MAX_MS: u128 = 15_000;

/* ---------- routes ---------- */

struct RouteFile {
    path: String,
    route: Option<String>,
    deps: Vec<String>,
}

fn ext_re() -> Regex {
    Regex::new(r"\.(?:tsx?|jsx?|mjs|cjs)$").unwrap()
}

fn route_of(rel: &str) -> Option<String> {
    let stripped = ext_re().replace(rel, "").into_owned();
    let segs: Vec<&str> = stripped.split('/').collect();
    let base = segs.last().copied().unwrap_or("");
    if base.starts_with('_') || base.starts_with('+') {
        return None;
    }
    let group = Regex::new(r"^\(.*\)$").unwrap();
    let mut kept: Vec<&str> = segs.into_iter().filter(|s| !group.is_match(s)).collect();
    if kept.last() == Some(&"index") {
        kept.pop();
    }
    Some(format!("/{}", kept.join("/")))
}

fn normalize_posix(path: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

fn resolve_spec(from: &str, spec: &str) -> Option<String> {
    if let Some(rest) = spec.strip_prefix("@feature/") {
        return Some(format!("{APP_ROOT}/features/{rest}"));
    }
    if let Some(rest) = spec.strip_prefix("@/") {
        return Some(format!("{APP_ROOT}/{rest}"));
    }
    if spec.starts_with('.') {
        let dir = from.rsplit_once('/').map_or(".", |(d, _)| d);
        return Some(normalize_posix(&format!("{dir}/{spec}")));
    }
    None
}

fn feature_key(path: &str) -> Option<String> {
    let re = Regex::new(r"^frontend/mla-tamagui/features/([^/]+)").unwrap();
    re.captures(path).map(|m| m[1].to_string())
}

fn build_app_index(files: &[(String, String)]) -> Vec<RouteFile> {
    let spec = Regex::new(r#"(?:from\s+|import\s*\(\s*|require\s*\(\s*)['"]([^'"]+)['"]"#).unwrap();
    files
        .iter()
        .filter(|(p, _)| p.starts_with(&format!("{APP_DIR}/")) && ext_re().is_match(p))
        .map(|(path, source)| RouteFile {
            path: path.clone(),
            route: route_of(&path[APP_DIR.len() + 1..]),
            deps: spec.captures_iter(source).filter_map(|m| resolve_spec(path, &m[1])).collect(),
        })
        .collect()
}

fn imports_file(dep: &str, file: &str) -> bool {
    let bare = ext_re().replace(file, "").into_owned();
    bare == dep || bare == format!("{dep}/index") || file.starts_with(&format!("{dep}/"))
}

fn is_frontend(file: &str) -> bool {
    file.starts_with(&format!("{APP_ROOT}/")) || file.starts_with("libraries/")
}

fn routes_for(file: &str, index: &[RouteFile]) -> Vec<String> {
    let routed = || index.iter().filter(|r| r.route.is_some());
    if file.starts_with(&format!("{APP_DIR}/")) {
        if let Some(route) = index.iter().find(|r| r.path == file).and_then(|r| r.route.clone()) {
            return vec![route];
        }
        let dir = file.rsplit_once('/').map_or(".", |(d, _)| d);
        if dir == APP_DIR {
            return vec![];
        }
        return routed().filter(|r| r.path.starts_with(&format!("{dir}/"))).filter_map(|r| r.route.clone()).collect();
    }
    let feature = feature_key(file);
    routed()
        .filter(|r| r.deps.iter().any(|d| if feature.is_some() { feature_key(d) == feature } else { imports_file(d, file) }))
        .filter_map(|r| r.route.clone())
        .collect()
}

fn plan_routes(files: &[String], index: &[RouteFile]) -> Vec<(String, bool)> {
    let mut changed: BTreeSet<String> = BTreeSet::new();
    let mut smoke = false;
    for file in files.iter().map(|f| f.strip_prefix("./").unwrap_or(f)) {
        if !is_frontend(file) {
            continue;
        }
        let hits = routes_for(file, index);
        if hits.is_empty() {
            smoke = true;
        }
        changed.extend(hits);
    }
    let mut all = changed.clone();
    if smoke {
        all.extend(SMOKE_ROUTES.iter().map(|s| s.to_string()));
    }
    all.into_iter().map(|r| (changed.contains(&r), r)).map(|(c, r)| (r, c)).collect()
}

fn route_url_path(route: &str) -> String {
    Regex::new(r"\[(?:\.\.\.)?[^\]]+\]").unwrap().replace_all(route, "sample").into_owned()
}

fn route_slug(route: &str) -> String {
    let s = Regex::new(r"[^\w-]+").unwrap().replace_all(route.strip_prefix('/').unwrap_or(route), "_").into_owned();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() {
        "index".into()
    } else {
        s
    }
}

fn load_app_sources(repo: &Path) -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut files = vec![];
    walk(&repo.join(APP_DIR), &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|f| {
            let rel = f.strip_prefix(repo).ok()?.to_string_lossy().replace('\\', "/");
            Some((rel, std::fs::read_to_string(&f).ok()?))
        })
        .collect()
}

/* ---------- images ---------- */

struct Rgba {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

fn decode_png(bytes: &[u8]) -> Res<Rgba> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let raw = &buf[..info.buffer_size()];
    let data = match info.color_type {
        png::ColorType::Rgba => raw.to_vec(),
        png::ColorType::Rgb => raw.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::Grayscale => raw.iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => raw.chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Indexed => return Err("indexed png".into()),
    };
    Ok(Rgba { width: info.width as usize, height: info.height as usize, data })
}

/// Share of pixels that are not the most common colour, and the standard deviation of luminance.
fn ink_of(img: &Rgba) -> (f64, f64) {
    let n = img.width * img.height;
    if n == 0 {
        return (0.0, 0.0);
    }
    let mut counts: HashMap<u32, usize> = HashMap::new();
    let (mut sum, mut sum_sq) = (0.0f64, 0.0f64);
    for px in img.data.chunks_exact(4).take(n) {
        let (r, g, b) = (px[0] as f64, px[1] as f64, px[2] as f64);
        *counts.entry(((px[0] as u32) << 16) | ((px[1] as u32) << 8) | px[2] as u32).or_default() += 1;
        let lum = 0.299 * r + 0.587 * g + 0.114 * b;
        sum += lum;
        sum_sq += lum * lum;
    }
    let mode = counts.values().copied().max().unwrap_or(0);
    let nf = n as f64;
    let mean = sum / nf;
    ((nf - mode as f64) / nf, (sum_sq / nf - mean * mean).max(0.0).sqrt())
}

/* ---------- judging ---------- */

enum Diff {
    Compared(f64),
    Size(String, String),
    Skipped(String),
    Error(String),
}

fn diff_exceeds(diff: &Diff) -> bool {
    matches!(diff, Diff::Size(..)) || matches!(diff, Diff::Compared(r) if *r > DIFF_MAX_RATIO)
}

fn pct(r: f64) -> String {
    format!("{:.3}%", r * 100.0)
}

struct Decision {
    fail: bool,
    note: String,
}

struct Observation<'a> {
    route: &'a str,
    changed: bool,
    screenshot: Option<&'a str>,
    errors: &'a [String],
    throw_message: Option<&'a str>,
    ink: Option<(f64, f64)>,
    diff: &'a Diff,
    judged: Option<&'a Decision>,
}

fn judge_route(o: &Observation) -> Value {
    let (mut fails, mut notes): (Vec<String>, Vec<String>) = (vec![], vec![]);
    if let Some(t) = o.throw_message {
        fails.push(format!("render threw: {t}"));
    }
    if !o.errors.is_empty() {
        fails.push(format!("{} console/page error(s): {}", o.errors.len(), o.errors.iter().take(3).cloned().collect::<Vec<_>>().join(" || ")));
    }
    if let Some((ink, stddev)) = o.ink {
        if ink < BLANK_MAX_RATIO {
            fails.push(format!("blank (ink {})", pct(ink)));
        } else {
            notes.push(format!("ink {}, stddev {stddev:.1}", pct(ink)));
        }
    }
    match o.diff {
        Diff::Compared(_) | Diff::Size(..) => {
            let msg = match o.diff {
                Diff::Compared(r) => format!("diff {} vs baseline", pct(*r)),
                Diff::Size(was, now) => format!("size changed vs baseline: {was} -> {now}"),
                _ => unreachable!(),
            };
            if !diff_exceeds(o.diff) {
                notes.push(msg);
            } else if !o.changed {
                fails.push(format!("{msg} on unchanged route (> {})", pct(DIFF_MAX_RATIO)));
            } else if let Some(j) = o.judged.filter(|j| j.fail) {
                fails.push(format!("{msg}; {}", j.note));
            } else if let Some(j) = o.judged {
                notes.push(format!("{msg}; {}", j.note));
            } else {
                notes.push(format!("{msg}, changed route: vision judge not run"));
            }
        }
        Diff::Error(reason) => fails.push(format!("diff could not be computed: {reason}")),
        Diff::Skipped(reason) => notes.push(format!("diff skipped: {reason}")),
    }
    let detail: String = fails.iter().chain(notes.iter()).cloned().collect::<Vec<_>>().join("; ").chars().take(2000).collect();
    let mut item = Map::new();
    item.insert("name".into(), json!(o.route));
    item.insert("status".into(), json!(if fails.is_empty() { "pass" } else { "fail" }));
    item.insert("detail".into(), json!(detail));
    if let Some(s) = o.screenshot {
        item.insert("screenshot".into(), json!(s));
    }
    Value::Object(item)
}

#[derive(Clone)]
struct Seat {
    model: String,
    pool: String,
}

struct Verdict {
    seat: Seat,
    violates: Option<bool>,
    reason: String,
}

fn pick_judge_panel(data: &Value) -> Vec<Seat> {
    let mut panel: Vec<Seat> = vec![];
    for model in data["ladders"]["judge:vision"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let cfg = &data["models"][model];
        let Some(pool) = cfg["pool"].as_str() else { continue };
        if cfg["vision"] == json!(false) || panel.iter().any(|s| s.pool == pool) {
            continue;
        }
        panel.push(Seat { model: model.to_string(), pool: pool.to_string() });
        if panel.len() == 2 {
            break;
        }
    }
    panel
}

fn parse_verdict(text: &str) -> (Option<bool>, String) {
    let re = Regex::new(r"(?is)^\W*(PASS|FAIL)\b[:.\s-]*(.*)$").unwrap();
    let t = text.trim();
    match re.captures(t) {
        None => (None, t.chars().take(300).collect()),
        Some(m) => (Some(m[1].eq_ignore_ascii_case("FAIL")), m[2].trim().chars().take(300).collect()),
    }
}

fn decide_route(diff: &Diff, judges: &[Verdict]) -> Decision {
    if !diff_exceeds(diff) {
        return Decision { fail: false, note: "diff within threshold".into() };
    }
    let say = |j: &Verdict| {
        let v = match j.violates {
            None => "no verdict",
            Some(true) => "FAIL",
            Some(false) => "PASS",
        };
        format!("{}: {v}{}", j.seat.model, if j.reason.is_empty() { String::new() } else { format!(" ({})", j.reason) })
    };
    let pools: BTreeSet<&str> = judges.iter().map(|j| j.seat.pool.as_str()).collect();
    let against = judges.iter().filter(|j| j.violates == Some(true)).count();
    let fail = judges.len() >= 2 && pools.len() >= 2 && against == judges.len();
    let summary = if judges.is_empty() { "no judges".to_string() } else { judges.iter().map(say).collect::<Vec<_>>().join(" | ") };
    Decision {
        fail,
        note: if fail { format!("vision judges agree it violates acceptance: {summary}") } else { format!("vision judges do not agree on a violation: {summary}") },
    }
}

fn judge_prompt(screenshot: &str, acceptance: &str) -> String {
    [
        "You are a visual QA judge. Do not edit any file.".to_string(),
        format!("View the screenshot image at {screenshot} (open it with your file read tool)."),
        "Acceptance criteria for this screen:".to_string(),
        acceptance.trim().to_string(),
        "Does the screenshot violate the acceptance criteria?".to_string(),
        "Answer with exactly one word, PASS (satisfies) or FAIL (violates), as the first word, then one short reason.".to_string(),
    ]
    .join("\n")
}

/// One vision judge: `wf offload ask` pinned to the seat's model on the `judge:vision` shape, read-only, in the repo.
fn dispatch_judge(repo: &Path, screenshot: &str, acceptance: &str, seat: &Seat) -> Verdict {
    let exe = std::env::current_exe().map_or_else(|_| "wf".to_string(), |p| p.to_string_lossy().into_owned());
    // The judge runs on the rca service, which has no access to this filesystem: the screenshot goes along, staged under a name the prompt uses.
    let staged = format!("screenshots/{}", Path::new(screenshot).file_name().map_or_else(|| "screen.png".into(), |n| n.to_string_lossy().into_owned()));
    let args: Vec<String> = ["offload", "ask", "--prompt"]
        .iter()
        .map(|s| s.to_string())
        .chain([
            judge_prompt(&staged, acceptance),
            "--files".into(),
            format!("{screenshot}={staged}"),
            "--cwd".into(),
            repo.to_string_lossy().into_owned(),
            "--shape".into(),
            "judge:vision".into(),
            "--model".into(),
            seat.model.clone(),
            "--caller".into(),
            "visual".into(),
            "--agent".into(),
            "read-only".into(),
            "--no-heartbeat".into(),
        ])
        .collect();
    let out = Command::new(&exe).args(&args).current_dir(repo).stdin(Stdio::null()).output();
    let (code, stdout, all) = match out {
        Ok(o) => (o.status.code().unwrap_or(1), String::from_utf8_lossy(&o.stdout).into_owned(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
        Err(e) => (1, String::new(), e.to_string()),
    };
    let tail = |s: &str, n: usize| -> String {
        let chars: Vec<char> = s.chars().collect();
        chars[chars.len().saturating_sub(n)..].iter().collect()
    };
    if code != 0 {
        return Verdict { seat: seat.clone(), violates: None, reason: format!("judge failed (exit {code}): {}", tail(&all, 200)) };
    }
    let (violates, reason) = parse_verdict(&tail(&stdout, 4000));
    Verdict { seat: seat.clone(), violates, reason }
}

/* ---------- baselines ---------- */

enum Baselines {
    Fs(PathBuf),
    Oci { namespace: String, bucket: String, sha: Option<String>, base_sha: Option<String> },
}

fn oci(args: &[String]) -> Res<String> {
    let out = Command::new("oci").args(args).stdin(Stdio::null()).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn bucket_from_env() -> Res<(String, String)> {
    let prefix = std::env::var(BUCKET_ENV).ok().filter(|p| !p.is_empty()).ok_or_else(|| format!("{BUCKET_ENV} is unset; it holds the golden-screenshots bucket URL prefix (terraform output golden_screenshots_bucket_url_prefix)"))?;
    let re = Regex::new(r"^https://objectstorage\.([^.]+)\.oraclecloud\.com/n/([^/]+)/b/([^/]+)/o/$").unwrap();
    re.captures(&prefix).map(|m| (m[2].to_string(), m[3].to_string())).ok_or_else(|| format!("{BUCKET_ENV} does not look like a bucket URL prefix (expected .../n/<namespace>/b/<bucket>/o/, got \"{prefix}\")"))
}

fn newest_first(mut objs: Vec<(String, String)>) -> Vec<(String, String)> {
    objs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    objs
}

impl Baselines {
    fn where_args(namespace: &str, bucket: &str) -> Vec<String> {
        vec!["--namespace-name".into(), namespace.into(), "--bucket-name".into(), bucket.into()]
    }

    fn list(namespace: &str, bucket: &str, slug: &str) -> Res<Vec<(String, String)>> {
        let mut args = s(&["os", "object", "list"]);
        args.extend(Self::where_args(namespace, bucket));
        args.extend(s(&["--prefix", &format!("baselines/{slug}/"), "--fields", "name,timeCreated", "--all"]));
        let out = oci(&args)?;
        let json: Value = if out.trim().is_empty() { json!({}) } else { serde_json::from_str(&out).map_err(|e| e.to_string())? };
        Ok(json["data"].as_array().into_iter().flatten().map(|d| (d["name"].as_str().unwrap_or("").to_string(), d["time-created"].as_str().unwrap_or("").to_string())).collect())
    }

    fn fetch(&self, slug: &str, dest: &Path) -> bool {
        match self {
            Baselines::Fs(dir) => {
                let src = dir.join(format!("{slug}.png"));
                src.exists() && std::fs::copy(src, dest).is_ok()
            }
            Baselines::Oci { namespace, bucket, base_sha, .. } => {
                let key = match base_sha {
                    Some(sha) => Some(format!("baselines/{slug}/{sha}.png")),
                    None => Self::list(namespace, bucket, slug).ok().and_then(|l| newest_first(l).into_iter().next().map(|o| o.0)),
                };
                let Some(key) = key else { return false };
                let mut args = s(&["os", "object", "get"]);
                args.extend(Self::where_args(namespace, bucket));
                args.extend(["--name".into(), key, "--file".into(), dest.to_string_lossy().into_owned()]);
                oci(&args).is_ok()
            }
        }
    }

    fn save(&self, slug: &str, src: &Path) -> Res<()> {
        match self {
            Baselines::Fs(dir) => {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                std::fs::copy(src, dir.join(format!("{slug}.png"))).map(|_| ()).map_err(|e| e.to_string())
            }
            Baselines::Oci { namespace, bucket, sha, .. } => {
                let sha = sha.as_ref().ok_or("oci baseline store needs --sha to save")?;
                let mut args = s(&["os", "object", "put"]);
                args.extend(Self::where_args(namespace, bucket));
                args.extend(["--name".into(), format!("baselines/{slug}/{sha}.png"), "--file".into(), src.to_string_lossy().into_owned(), "--force".into()]);
                oci(&args)?;
                for (name, _) in newest_first(Self::list(namespace, bucket, slug)?).into_iter().skip(BASELINES_KEPT) {
                    let mut del = s(&["os", "object", "delete"]);
                    del.extend(Self::where_args(namespace, bucket));
                    del.extend(["--name".into(), name, "--force".into()]);
                    oci(&del)?;
                }
                Ok(())
            }
        }
    }
}

fn record_tier(sha: &str, result: &Value) -> Res<()> {
    let (namespace, bucket) = bucket_from_env()?;
    let dir = std::env::temp_dir().join(format!("wf-verify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("body");
    std::fs::write(&file, result.to_string()).map_err(|e| e.to_string())?;
    let mut args = s(&["os", "object", "put"]);
    args.extend(Baselines::where_args(&namespace, &bucket));
    args.extend(["--name".into(), format!("verify/{sha}.tier2.json"), "--file".into(), file.to_string_lossy().into_owned(), "--force".into()]);
    let r = oci(&args);
    let _ = std::fs::remove_dir_all(&dir);
    r.map(|_| ())
}

/* ---------- browser ---------- */

struct Visit {
    errors: Vec<String>,
    throw_message: Option<String>,
    idle: bool,
}

fn open_browser() -> Res<VisualBrowser> {
    VisualBrowser::open_with(VIEWPORT, Some(COLLECTOR))
}

fn probe(browser: &mut VisualBrowser) -> Res<(String, i64, i64, Vec<String>)> {
    let raw = browser.page.eval_page(PROBE)?;
    let text = raw.as_str().ok_or_else(|| format!("page probe returned {raw}"))?;
    let v: Value = serde_json::from_str(text).map_err(|_| format!("page probe returned {text}"))?;
    match (v["ready"].as_str(), v["inflight"].as_i64(), v["resources"].as_i64(), v["errors"].as_array()) {
        (Some(r), Some(i), Some(n), Some(e)) => Ok((r.to_string(), i, n, e.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string)).collect())),
        _ => Err(format!("page probe returned {text}")),
    }
}

fn visit(browser: &mut VisualBrowser, url: &str) -> Visit {
    if let Err(e) = browser.navigate_loaded(url) {
        return Visit { errors: vec![], throw_message: Some(format!("navigate: {e}")), idle: false };
    }
    let started = Instant::now();
    let (mut last_resources, mut quiet) = (-1i64, 0);
    let mut errors: Vec<String> = vec![];
    while started.elapsed().as_millis() < IDLE_MAX_MS {
        match probe(browser) {
            Ok((ready, inflight, resources, errs)) => {
                errors = errs;
                let settled = ready == "complete" && inflight == 0 && resources == last_resources;
                quiet = if settled { quiet + 1 } else { 0 };
                last_resources = resources;
                if quiet >= 2 {
                    return Visit { errors, throw_message: None, idle: true };
                }
            }
            Err(e) => return Visit { errors, throw_message: Some(format!("page: {e}")), idle: false },
        }
        std::thread::sleep(Duration::from_millis(IDLE_POLL_MS));
    }
    Visit { errors, throw_message: None, idle: false }
}

/* ---------- the tier ---------- */

struct Opts {
    files: Vec<String>,
    repo: PathBuf,
    export_dir: PathBuf,
    baselines: Baselines,
    out_dir: PathBuf,
    acceptance: Option<String>,
    update_baselines: bool,
    base_path: Option<String>,
}

struct Shot {
    screenshot: Option<String>,
    ink: Option<(f64, f64)>,
    diff: Diff,
    error: Option<String>,
}

fn compare_to_baseline(now: &Rgba, baseline: Option<&Path>) -> Diff {
    let Some(baseline) = baseline else { return Diff::Skipped("updating baseline".into()) };
    if !baseline.exists() {
        return Diff::Skipped("no baseline".into());
    }
    match std::fs::read(baseline).map_err(|e| e.to_string()).and_then(|b| decode_png(&b)) {
        Err(e) => Diff::Error(format!("baseline: {e}")),
        Ok(was) => {
            if was.width != now.width || was.height != now.height {
                return Diff::Size(format!("{}x{}", was.width, was.height), format!("{}x{}", now.width, now.height));
            }
            let different = pixelmatch(&was.data, &now.data, now.width, now.height, PIXEL_THRESHOLD);
            Diff::Compared(different as f64 / (now.width * now.height) as f64)
        }
    }
}

fn capture(browser: &mut VisualBrowser, shot: &Path, baseline: Option<&Path>) -> Shot {
    let attempt = (|| -> Res<(Rgba, Diff)> {
        browser.screenshot(shot)?;
        let image = decode_png(&std::fs::read(shot).map_err(|e| e.to_string())?)?;
        let diff = compare_to_baseline(&image, baseline);
        Ok((image, diff))
    })();
    match attempt {
        Ok((image, diff)) => Shot { screenshot: Some(shot.to_string_lossy().into_owned()), ink: Some(ink_of(&image)), diff, error: None },
        Err(e) => Shot { screenshot: None, ink: None, diff: Diff::Skipped("no screenshot".into()), error: Some(format!("screenshot: {e}")) },
    }
}

fn check_route(browser: &mut VisualBrowser, site: &str, panel: &[Seat], opts: &Opts, route: &str, changed: bool) -> Res<Value> {
    let slug = route_slug(route);
    let baseline = opts.out_dir.join(format!("{slug}.baseline.png"));
    let _ = std::fs::remove_file(&baseline);
    if !opts.update_baselines {
        opts.baselines.fetch(&slug, &baseline);
    }
    let visit = visit(browser, &format!("{site}{}", route_url_path(route)));
    let shot = capture(browser, &opts.out_dir.join(format!("{slug}.png")), if opts.update_baselines { None } else { Some(&baseline) });
    let throw_message = visit.throw_message.clone().or_else(|| shot.error.clone());
    let verdict = if changed && shot.screenshot.is_some() && diff_exceeds(&shot.diff) {
        Some(match &opts.acceptance {
            None => Decision { fail: false, note: "no --acceptance, vision judge skipped".into() },
            Some(acceptance) => {
                let screenshot = shot.screenshot.clone().unwrap_or_default();
                let verdicts: Vec<Verdict> = std::thread::scope(|sc| {
                    let handles: Vec<_> = panel.iter().map(|seat| sc.spawn(|| dispatch_judge(&opts.repo, &screenshot, acceptance, seat))).collect();
                    handles.into_iter().filter_map(|h| h.join().ok()).collect()
                });
                decide_route(&shot.diff, &verdicts)
            }
        })
    } else {
        None
    };
    let mut item = judge_route(&Observation { route, changed, screenshot: shot.screenshot.as_deref(), errors: &visit.errors, throw_message: throw_message.as_deref(), ink: shot.ink, diff: &shot.diff, judged: verdict.as_ref() });
    let mut notes = vec![item["detail"].as_str().unwrap_or("").to_string()];
    if !visit.idle && throw_message.is_none() {
        notes.push("network not idle after 15s".into());
    }
    if opts.update_baselines {
        match (&shot.screenshot, item["status"] == json!("pass")) {
            (Some(s), true) => {
                opts.baselines.save(&slug, Path::new(s))?;
                notes.push("baseline written".into());
            }
            _ => notes.push("baseline NOT written (route failed)".into()),
        }
    }
    item["detail"] = json!(notes.into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join("; "));
    Ok(item)
}

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

fn tier_of(items: Vec<Value>, ms: u128) -> Value {
    let status = if items.is_empty() {
        "skip"
    } else if items.iter().any(|i| i["status"] == json!("fail")) {
        "fail"
    } else {
        "pass"
    };
    json!({ "tier": 2, "status": status, "ms": ms as u64, "items": items })
}

fn run_visual(opts: &Opts) -> Res<Value> {
    let started = now_ms();
    let index = build_app_index(&load_app_sources(&opts.repo));
    let planned = plan_routes(&opts.files, &index);
    if planned.is_empty() {
        return Ok(tier_of(vec![], now_ms() - started));
    }
    if !opts.export_dir.join("index.html").exists() {
        return Err(format!("no web export at {}; run: pnpm --filter mla-tamagui run export:web", opts.export_dir.display()));
    }
    std::fs::create_dir_all(&opts.out_dir).map_err(|e| e.to_string())?;
    let needs_judge = opts.acceptance.is_some() && planned.iter().any(|p| p.1);
    let panel = if needs_judge {
        let text = std::fs::read_to_string(opts.repo.join(LADDER_FILE)).map_err(|e| e.to_string())?;
        pick_judge_panel(&serde_json::from_str(&text).map_err(|e| e.to_string())?)
    } else {
        vec![]
    };
    let site = crate::export_site::serve(&opts.export_dir, opts.base_path.as_deref().unwrap_or(crate::export_site::BASE_PATH), 0)?.base_url;
    let mut browser = open_browser()?;
    let mut items = vec![];
    let mut failure = None;
    for (route, changed) in &planned {
        match check_route(&mut browser, &site, &panel, opts, route, *changed) {
            Ok(item) => items.push(item),
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }
    browser.close();
    match failure {
        Some(e) => Err(e),
        None => Ok(tier_of(items, now_ms() - started)),
    }
}

/* ---------- command line ---------- */

fn absolute(p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        normalize(path)
    } else {
        normalize(&std::env::current_dir().unwrap_or_default().join(path))
    }
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn run(args: &[String]) -> Option<i32> {
    let spec = Spec {
        values: &["files", "export-dir", "baseline-dir", "out-dir", "base-path", "baseline-store", "sha", "base-sha", "acceptance"],
        flags: &["update-baselines", "help"],
        positional: Positional::None,
        ..Spec::NONE
    };
    let Some(flags) = parse_spec(args, &spec) else {
        eprint!("{}", wf_kit::args::take_error().unwrap_or_default());
        return Some(2);
    };
    if flags.flag("help") {
        println!("{USAGE}");
        return Some(0);
    }
    let files: Vec<String> = flags.opts.get("files").map(|f| f.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    if files.is_empty() {
        eprintln!("--files is required\n{USAGE}");
        return Some(2);
    }
    let kind = flags.opts.get("baseline-store").map_or("fs", String::as_str);
    if kind != "fs" && kind != "oci" {
        eprintln!("--baseline-store must be fs or oci\n{USAGE}");
        return Some(2);
    }
    let fail = |e: String| {
        eprintln!("visual: {e}");
        Some(2)
    };
    let repo = wf_kit::root::repo_root();
    let at = |key: &str, dflt: &str| flags.opts.get(key).map_or_else(|| absolute(&repo.join(dflt).to_string_lossy()), |p| absolute(p));
    let baselines = if kind == "oci" {
        match bucket_from_env() {
            Ok((namespace, bucket)) => Baselines::Oci { namespace, bucket, sha: flags.opts.get("sha").cloned(), base_sha: flags.opts.get("base-sha").cloned() },
            Err(e) => return fail(e),
        }
    } else {
        Baselines::Fs(at("baseline-dir", ".agents/tmp/visual/baselines"))
    };
    let acceptance = match flags.opts.get("acceptance") {
        Some(p) => match std::fs::read_to_string(absolute(p)) {
            Ok(t) if !t.is_empty() => Some(t),
            Ok(_) => None,
            Err(e) => return fail(e.to_string()),
        },
        None => None,
    };
    let opts = Opts {
        files,
        export_dir: at("export-dir", "frontend/mla-tamagui/dist"),
        out_dir: at("out-dir", ".agents/tmp/visual/out"),
        repo,
        baselines,
        acceptance,
        update_baselines: flags.flag("update-baselines"),
        base_path: flags.opts.get("base-path").cloned(),
    };
    match run_visual(&opts) {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
            if kind == "oci" {
                if let Some(sha) = flags.opts.get("sha") {
                    if let Err(e) = record_tier(sha, &result) {
                        return fail(e);
                    }
                }
            }
            Some(if result["status"] == json!("fail") { 1 } else { 0 })
        }
        Err(e) => fail(e),
    }
}
