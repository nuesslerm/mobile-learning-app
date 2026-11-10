//! `wf ci comment`: the run's report, on every run, most important first: the verdict with the wall time, what failed and why (read from
//! each failed job's log), the counts, a table of the jobs with a time bar, the VM's own numbers during the run (reporting-service),
//! then the cache, crate and e2e details; posted on the open PR and in the job summary, and, when a job failed, on the pushed commit.
//! Each run (and runner) keeps its own comment.
use cli_http::{Request, Response};
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &["dry-run"], positional: Positional::None, ..Spec::NONE };
const BODY_CAP: usize = 60_000;
const RESERVED: usize = 4_000;
const LINE_CAP: usize = 300;
const CAUSE_LINES: usize = 8;
const DETAIL_LINES: usize = 40;
const SKIPPED_NAMES: usize = 5;
const CAUSE: &str = r#"^##\[error\]|\bsccache: error\b|^wf: |returned HTTP \d{3}|\bexitCode"?\s*[=:]\s*"?[1-9]|^#\d+ ERROR|^ERROR: |^Error response from daemon|^test .* FAILED$|^test result: FAILED|^error(\[E\d+\])?: |^\w*Error: |^FAIL |\berror TS\d+|^NX .*\bfailed\b|^Failed tasks:$|"verdict"\s*:\s*"(fail|error)"|"ok"\s*:\s*false|panicked at|^--- script (stdout|stderr|output) ---$"#;

#[derive(Clone)]
pub(crate) struct Env {
    pub(crate) token: String,
    pub(crate) repo: String,
    pub(crate) run: String,
    pub(crate) sha: String,
    pub(crate) api: String,
    pub(crate) server: String,
    pub(crate) branch: String,
    pub(crate) runner: String,
    /// `WF_CI_LABEL`: names a second pipeline running next to the CI ("GitHub-hosted"): its own comment, its own title.
    pub(crate) label: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) results: Option<String>,
    /// Where reporting-service answers (`WF_VM_STATS_URL`), for the VM's numbers during the run.
    pub(crate) vm_url: String,
}

struct Failure {
    name: String,
    step: String,
    took: String,
    cause: Vec<String>,
    more: Vec<String>,
    url: String,
}

pub(crate) fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

impl Env {
    pub(crate) fn read() -> Result<Env, String> {
        let need = |name: &str| var(name).ok_or_else(|| format!("{name} is not set"));
        let base = |name: &str, default: &str| var(name).unwrap_or_else(|| default.to_string()).trim_end_matches('/').to_string();
        Ok(Env {
            token: need("GITHUB_TOKEN")?,
            repo: need("GITHUB_REPOSITORY")?,
            run: need("GITHUB_RUN_ID")?,
            sha: need("GITHUB_SHA")?,
            api: base("GITHUB_API_URL", "https://api.github.com"),
            server: base("GITHUB_SERVER_URL", "https://github.com"),
            branch: var("GITHUB_REF_NAME").unwrap_or_else(|| "unknown".to_string()),
            runner: var("RUNNER_NAME").unwrap_or_else(|| "unknown".to_string()),
            label: var("WF_CI_LABEL"),
            summary: var("GITHUB_STEP_SUMMARY"),
            results: var("WF_CI_RESULTS"),
            vm_url: var("WF_VM_STATS_URL").unwrap_or_else(|| wf_vmops::host_summary::REPORTING_URL.to_string()),
        })
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a valid pattern"))
}

pub(crate) fn text(value: &Value, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

pub(crate) fn cut(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        return line.to_string();
    }
    format!("{}…", line.chars().take(max).collect::<String>())
}

pub(crate) fn redact(line: &str, token: &str) -> String {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        Vec::from([
            (r"\b(?:gh[pousr]_[A-Za-z0-9]{16,}|github_pat_[A-Za-z0-9_]{16,})", "[REDACTED]"),
            (r"(?i)\b(bearer|basic|token)\s+[A-Za-z0-9._~+/=-]{20,}", "${1} [REDACTED]"),
            (r"/p/[A-Za-z0-9_-]{16,}/n/", "/p/[REDACTED]/n/"),
            (r"(?i)([?&](?:sig|signature|token|access_token|x-amz-signature|x-amz-credential|x-amz-security-token))=[^&\s)]+", "${1}=[REDACTED]"),
        ])
        .into_iter()
        .map(|(pattern, with)| (Regex::new(pattern).expect("a valid pattern"), with))
        .collect()
    });
    let mut out = if token.len() >= 8 { line.replace(token, "[REDACTED]") } else { line.to_string() };
    for (pattern, with) in patterns {
        out = pattern.replace_all(&out, *with).into_owned();
    }
    out
}

fn html(line: &str) -> String {
    line.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub(crate) fn cell(line: &str) -> String {
    html(line).replace('|', "\\|")
}

pub(crate) fn epoch(value: &Value) -> Option<i64> {
    epoch_of(value.as_str()?)
}

/// Seconds since the epoch of an ISO 8601 UTC timestamp.
pub(crate) fn epoch_of(stamp: &str) -> Option<i64> {
    let (date, time) = stamp.split_once('T')?;
    let mut date = date.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (date.next()??, date.next()??, date.next()??);
    let mut clock = time.trim_end_matches('Z').split(':').map(|part| part.split('.').next().and_then(|whole| whole.parse::<i64>().ok()));
    let (hours, minutes, seconds) = (clock.next()??, clock.next()??, clock.next()??);
    let year = if month <= 2 { year - 1 } else { year };
    let (era, day_of_year) = (year.div_euclid(400), (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1);
    let year_of_era = year - era * 400;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some((era * 146_097 + day_of_era - 719_468) * 86_400 + hours * 3600 + minutes * 60 + seconds)
}

fn took(job: &Value) -> String {
    match (epoch(&job["started_at"]), epoch(&job["completed_at"])) {
        (Some(from), Some(to)) if to >= from => match to - from {
            s if s >= 3600 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
            s if s >= 60 => format!("{}m {:02}s", s / 60, s % 60),
            s => format!("{s}s"),
        },
        _ => "-".to_string(),
    }
}

fn is_failed(job: &Value) -> bool {
    matches!(job["conclusion"].as_str(), Some("failure" | "timed_out"))
}

fn failed_step(job: &Value) -> String {
    let steps = job["steps"].as_array().map(Vec::as_slice).unwrap_or_default();
    let with = |conclusions: &[&str]| steps.iter().find(|s| s["conclusion"].as_str().is_some_and(|c| conclusions.contains(&c)));
    with(&["failure", "timed_out"]).or_else(|| with(&["cancelled"])).map_or_else(|| "-".to_string(), |s| text(s, "name"))
}

pub(crate) fn send(env: &Env, method: &str, url: &str, body: Option<&Value>) -> Result<Response, String> {
    let mut request = Request::new(method, url)
        .header("Authorization", &format!("Bearer {}", env.token))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "ci-comment")
        .timeout(Duration::from_secs(60))
        .follow_redirects();
    if let Some(body) = body {
        request = request.json(body);
    }
    request.send().map_err(|e| format!("{method} {url}: {e}"))
}

pub(crate) fn json_of(reply: Response, what: &str) -> Result<Value, String> {
    if !reply.ok() {
        let body = reply.text();
        let message = serde_json::from_str::<Value>(&body).ok().and_then(|v| v["message"].as_str().map(str::to_string)).unwrap_or(body);
        return Err(format!("{what}: HTTP {}: {}", reply.status, cut(&message, 200)));
    }
    serde_json::from_str(&reply.text()).map_err(|e| format!("{what}: {e}"))
}

pub(crate) fn list_jobs(env: &Env) -> Result<Vec<Value>, String> {
    list_jobs_of(env, &env.run)
}

pub(crate) fn list_jobs_of(env: &Env, run: &str) -> Result<Vec<Value>, String> {
    let mut jobs: Vec<Value> = Vec::new();
    for page in 1..=10 {
        let url = format!("{}/repos/{}/actions/runs/{run}/jobs?per_page=100&page={page}", env.api, env.repo);
        let body = json_of(send(env, "GET", &url, None)?, "listing the jobs")?;
        let got = body["jobs"].as_array().cloned().unwrap_or_default();
        let (empty, total) = (got.is_empty(), body["total_count"].as_u64().unwrap_or(0) as usize);
        jobs.extend(got.into_iter().map(|mut job| {
            // a matrix job that was skipped before it expanded keeps its template as its name
            let name = job["name"].as_str().map(|n| n.replace("${{ matrix.app }}", "services"));
            if let Some(name) = name {
                job["name"] = Value::String(wf_bucket::job_name::job_name(&name).to_string());
            }
            job
        }));
        if empty || jobs.len() >= total {
            break;
        }
    }
    Ok(jobs)
}

/// A docker pull's per-layer progress: hundreds of lines a deploy log carries and none of them is the failure.
fn pull_progress(line: &str) -> bool {
    ["Pulling fs layer", "Already exists", "Download complete", "Pull complete", "Verifying Checksum", "Waiting"].iter().any(|p| line.ends_with(p))
        || line.contains(" Extracting ")
        || line.contains(" Downloading [")
}

/// The job's log lines without timestamp or colour, minus GitHub's echo of each `run:` script and the group markers.
pub(crate) fn log_lines(log: &str) -> Vec<String> {
    static STAMP: OnceLock<Regex> = OnceLock::new();
    static COLOUR: OnceLock<Regex> = OnceLock::new();
    let (stamp, colour) = (re(&STAMP, r"^\x{feff}?\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z ?"), re(&COLOUR, r"\x1b\[[0-9;?]*[ -/]*[@-~]"));
    let mut echo = false;
    let mut lines = Vec::new();
    for raw in log.split('\n') {
        let line = stamp.replace(&colour.replace_all(raw, ""), "").trim().to_string();
        if line.starts_with("##[group]Run ") {
            echo = true;
        } else if line.starts_with("##[endgroup]") {
            echo = false;
        } else if !echo && !line.is_empty() && !["##[group]", "##[command]", "##[debug]"].iter().any(|p| line.starts_with(p)) && !pull_progress(&line) {
            lines.push(line);
        }
    }
    lines
}

/// The cache and compile figures, the stage summary and the per-kind progress lines (`wf: flow-tests: unit in crate-a, crate-b`) `wf`
/// prints around every run: they match a marker and are no cause.
fn is_statistics(line: &str) -> bool {
    static STAGE_LINE: OnceLock<Regex> = OnceLock::new();
    ["wf: build cache ", "wf: storage guard ", "wf: nx cost ", "wf: sccache ", "wf: compile cache ", "{\"ok\":false,\"stage\":"].iter().any(|p| line.starts_with(p)) || re(&STAGE_LINE, r"^wf: [\w-]+: [\w-]+ in ").is_match(line)
}

/// A line without the label Nx puts before every line a task prints (`crate-wf-login: `, `@backend/reporting-service: `): the marker patterns
/// are written for the tools' own lines.
fn untagged(line: &str) -> &str {
    static TAG: OnceLock<Regex> = OnceLock::new();
    match re(&TAG, r"^(?:crate-[A-Za-z0-9_-]+|@[A-Za-z0-9_/-]+):\s*").find(line) {
        Some(tag) => &line[tag.end()..],
        None => line,
    }
}

/// The lines that carry the failure, as indexes: a marker line, plus the lines that belong to it (a panic's message, a failed
/// script's stderr, the tasks Nx lists).
fn causes(lines: &[String]) -> Vec<usize> {
    static MARKERS: OnceLock<Regex> = OnceLock::new();
    let markers = re(&MARKERS, CAUSE);
    let (mut hits, mut follow, mut listing) = (Vec::new(), 0usize, false);
    for (at, line) in lines.iter().enumerate() {
        let item = listing && line.starts_with("- ");
        listing = line == "Failed tasks:" || item;
        let bare = untagged(line);
        let own = (markers.is_match(bare) && !is_statistics(bare)).then(|| {
            if bare.contains("panicked at") {
                3
            } else if bare.starts_with("--- script") {
                4
            } else if bare.starts_with("error: process didn't exit") {
                3
            } else {
                0
            }
        });
        if own.is_none() && !item && follow == 0 {
            continue;
        }
        follow = own.unwrap_or_else(|| follow.saturating_sub(1));
        hits.push(at);
    }
    hits
}

fn tail<T>(mut items: Vec<T>, keep: usize) -> Vec<T> {
    let from = items.len().saturating_sub(keep);
    items.split_off(from)
}

/// A line of Nx's list of failed tasks, or its headline: they say which tasks failed, not why.
fn is_task_listing(line: &str) -> bool {
    let bare = untagged(line);
    bare.starts_with("- ") || bare == "Failed tasks:" || bare.starts_with("NX ")
}

/// What went wrong in a job log (the lines for the table) and up to 40 lines of context before it (for the details block). The lines
/// that say why come first, a line that twelve tasks printed once with its count, and Nx's list of failed tasks as one line after.
fn analyse(log: &str, token: &str) -> (Vec<String>, Vec<String>) {
    let lines = log_lines(log);
    let hits = causes(&lines);
    let end = hits.last().map_or(lines.len(), |last| last + 1);
    let (listing, specific): (Vec<usize>, Vec<usize>) = hits.iter().copied().filter(|&at| !lines[at].starts_with("##[error]Process completed with exit code")).partition(|&at| is_task_listing(&lines[at]));
    let shape = |at: usize| cut(&redact(untagged(lines[at].strip_prefix("##[error]").unwrap_or(&lines[at])), token), LINE_CAP);
    let mut shown: Vec<usize> = Vec::new();
    let mut texts: Vec<(String, usize)> = Vec::new();
    for &at in &specific {
        let text = shape(at);
        match texts.iter_mut().find(|(t, _)| *t == text) {
            Some((_, n)) => *n += 1,
            None => {
                texts.push((text, 1));
                shown.push(at);
            }
        }
    }
    let mut cause: Vec<String> = texts.iter().map(|(t, n)| if *n > 1 { format!("{t} (x{n})") } else { t.clone() }).collect();
    cause = tail(cause, CAUSE_LINES);
    let tasks: Vec<String> = listing.iter().map(|&at| untagged(&lines[at]).trim_start_matches("- ").to_string()).filter(|l| !l.starts_with("NX ") && l != "Failed tasks:").collect();
    if !tasks.is_empty() {
        cause.push(cut(&format!("Failed tasks: {}", tasks.join(", ")), LINE_CAP));
    }
    if cause.is_empty() {
        cause = (end.saturating_sub(CAUSE_LINES)..end).map(shape).collect();
    }
    let skip: HashSet<usize> = hits.iter().copied().collect();
    let more: Vec<usize> = (0..end).filter(|at| !skip.contains(at)).collect();
    (cause, tail(more, DETAIL_LINES).into_iter().map(shape).collect())
}

pub(crate) fn read_log(env: &Env, job: &Value) -> Result<String, String> {
    let url = format!("{}/repos/{}/actions/jobs/{}/logs", env.api, env.repo, job["id"].as_u64().unwrap_or(0));
    send(env, "GET", &url, None).and_then(|r| if r.ok() { Ok(r.text()) } else { Err(format!("HTTP {}", r.status)) })
}

fn failure(env: &Env, job: &Value, log: &Result<String, String>) -> Failure {
    let (cause, more) = match log {
        Ok(log) => analyse(log, &env.token),
        Err(reason) => (vec![format!("the job log could not be read: {reason}")], Vec::new()),
    };
    Failure { name: text(job, "name"), step: failed_step(job), took: took(job), cause, more, url: text(job, "html_url") }
}

fn details(f: &Failure) -> String {
    let lines = f.more.iter().map(|l| l.replace("```", "'''")).collect::<Vec<_>>().join("\n");
    let link = if f.url.is_empty() { String::new() } else { format!("[Open the job]({})\n\n", f.url) };
    format!("\n<details><summary>{}: {} more log lines</summary>\n\n{link}```text\n{lines}\n```\n\n</details>\n", html(&f.name), f.more.len())
}

/// First line of the pull request comment: the one comment of a run, edited as its jobs go and left as its report. A later run removes it.
pub(crate) const LIVE_PREFIX: &str = "<!-- ci-live";

/// The part of the marker that tells a pipeline's comments from another's: empty for the CI itself, `:github-hosted` for one labelled so.
fn flavor(env: &Env) -> String {
    env.label.as_ref().map_or(String::new(), |l| format!(":{}", l.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().trim_matches('-')))
}

pub(crate) fn live_marker(env: &Env) -> String {
    format!("{LIVE_PREFIX}{} run:{} -->", flavor(env), env.run)
}

/// Whether a comment is one of this pipeline's (the CI keeps its older `ci-comment` format too), so a second pipeline never deletes the first's.
fn same_flavor(env: &Env, text: &str) -> bool {
    text.starts_with(&format!("{LIVE_PREFIX}{} ", flavor(env))) || (env.label.is_none() && text.starts_with("<!-- ci-comment"))
}

fn marker(env: &Env) -> String {
    format!("<!-- ci-comment run:{} runner:{} -->", env.run, env.runner)
}

/// What one job's log says about the cache and the time: the build cache's stats lines summed, Nx's replay counts, the crates'
/// `cargo-flow` and `cargo-e2e` tasks, docker's cached layers and the EAS export, upload and served seconds.
#[derive(Default)]
struct Stats {
    cache: Option<Cache>,
    nx: Option<(u64, u64)>,
    crates: Vec<(String, bool)>,
    layers: Option<(u64, u64)>,
    eas: Option<String>,
}

#[derive(Default, Clone, Copy)]
struct Cache {
    hits: u64,
    misses: u64,
    puts: u64,
    quota: u64,
    gets: u64,
    get_ms: f64,
    get_max: f64,
    put_calls: u64,
    put_ms: f64,
    put_max: f64,
}

fn cache_of(lines: &[String]) -> Option<Cache> {
    let mut sum: Option<Cache> = None;
    for line in lines {
        let Some((_, json)) = line.split_once("wf: build cache ") else { continue };
        let Ok(v) = serde_json::from_str::<Value>(json) else { continue };
        let c = sum.get_or_insert_with(Cache::default);
        let (n, l) = (|key: &str| v[key].as_u64().unwrap_or(0), &v["latencyMs"]);
        (c.hits, c.misses, c.puts, c.quota) = (c.hits + n("hits"), c.misses + n("misses"), c.puts + n("puts"), c.quota + n("quotaExceeded"));
        c.gets += l["get"]["n"].as_u64().unwrap_or(0);
        c.get_ms += l["get"]["sumMs"].as_f64().unwrap_or(0.0);
        c.get_max = c.get_max.max(l["get"]["maxMs"].as_f64().unwrap_or(0.0));
        c.put_calls += l["put"]["n"].as_u64().unwrap_or(0);
        c.put_ms += l["put"]["sumMs"].as_f64().unwrap_or(0.0);
        c.put_max = c.put_max.max(l["put"]["maxMs"].as_f64().unwrap_or(0.0));
    }
    sum
}

fn stats(log: &str) -> Stats {
    static REPLAYED: OnceLock<Regex> = OnceLock::new();
    static CRATE: OnceLock<Regex> = OnceLock::new();
    static CACHED: OnceLock<Regex> = OnceLock::new();
    static BUILT: OnceLock<Regex> = OnceLock::new();
    static EAS: OnceLock<Regex> = OnceLock::new();
    let lines = log_lines(log);
    let (replayed, krate) = (re(&REPLAYED, r"(\d+) of (\d+) tasks replayed from the cache"), re(&CRATE, r"nx run (crate-[A-Za-z0-9_-]+):cargo-(?:flow|e2e)\s*(\[[^\]]*\])?"));
    let (cached, built) = (re(&CACHED, r"^#\d+ CACHED$"), re(&BUILT, r"^#\d+ DONE "));
    let eas = re(&EAS, r#""(exportS|uploadS|servedS)"\s*:\s*([0-9.]+)"#);
    let mut out = Stats { cache: cache_of(&lines), ..Stats::default() };
    let (mut nx, mut layers, mut times) = ((0u64, 0u64, false), (0u64, 0u64), Vec::<(String, String)>::new());
    for line in &lines {
        if let Some(c) = replayed.captures(line) {
            nx = (nx.0 + c[1].parse::<u64>().unwrap_or(0), nx.1 + c[2].parse::<u64>().unwrap_or(0), true);
        }
        if let Some(c) = krate.captures(line) {
            let (name, hit) = (c[1].to_string(), c.get(2).is_some_and(|m| m.as_str().contains("cache")));
            out.crates.retain(|(n, _)| *n != name);
            out.crates.push((name, hit));
        }
        if cached.is_match(line) {
            layers.0 += 1;
            layers.1 += 1;
        } else if built.is_match(line) {
            layers.1 += 1;
        }
        for c in eas.captures_iter(line) {
            times.retain(|(k, _)| *k != c[1]);
            times.push((c[1].to_string(), c[2].to_string()));
        }
    }
    out.nx = nx.2.then_some((nx.0, nx.1));
    out.layers = (layers.1 > 0).then_some(layers);
    let find = |key: &str| times.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    out.eas = (!times.is_empty()).then(|| format!("EAS export {} s, upload {} s, served {} s", find("exportS").unwrap_or("-".into()), find("uploadS").unwrap_or("-".into()), find("servedS").unwrap_or("-".into())));
    out
}

fn mean(sum: f64, n: u64) -> String {
    if n == 0 { "-".to_string() } else { format!("{:.0} ms", sum / n as f64) }
}

fn seconds(span: i64) -> String {
    match span {
        s if s >= 3600 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
        s if s >= 60 => format!("{}m {:02}s", s / 60, s % 60),
        s => format!("{s}s"),
    }
}

fn slowest_step(job: &Value) -> Option<String> {
    let steps = job["steps"].as_array()?;
    let span = |s: &Value| Some(epoch(&s["completed_at"])? - epoch(&s["started_at"])?);
    let (step, span) = steps.iter().filter(|s| !text(s, "name").starts_with("Post ") && !text(s, "name").starts_with("Set up")).filter_map(|s| Some((s, span(s)?))).max_by_key(|(_, span)| *span)?;
    (span >= 30).then(|| format!("slowest step: {} {}", text(step, "name"), seconds(span)))
}

fn span_secs(job: &Value) -> Option<i64> {
    match (epoch(&job["started_at"]), epoch(&job["completed_at"])) {
        (Some(from), Some(to)) if to >= from => Some(to - from),
        _ => None,
    }
}

fn icon(job: &Value) -> &'static str {
    match job["conclusion"].as_str() {
        Some("success") => "✅",
        Some("failure" | "timed_out") => "❌",
        Some("skipped") => "⏭️",
        Some("cancelled") => "🚫",
        _ if job["status"] == "in_progress" => "🔄",
        _ => "⏸️",
    }
}

fn percent(part: u64, whole: u64) -> Option<u64> {
    (whole > 0).then(|| (part as f64 * 100.0 / whole as f64).round() as u64)
}

/// The notes of a job row: only what helps a reader, the full cache latencies go under "Cache details".
fn notes(job: &Value, s: &Stats) -> String {
    let mut notes: Vec<String> = Vec::new();
    notes.extend(s.cache.and_then(|c| percent(c.hits, c.hits + c.misses)).map(|p| format!("build cache {p}% hit")));
    notes.extend(s.cache.filter(|c| c.quota > 0).map(|c| format!("⚠️ Object Storage quota exceeded: {} cache write(s) refused", c.quota)));
    notes.extend(s.nx.map(|(r, t)| format!("Nx replayed {r} of {t}")));
    notes.extend(s.layers.map(|(c, t)| format!("docker layers cached {c} of {t}")));
    notes.extend(s.eas.clone());
    notes.extend(slowest_step(job));
    notes.join("; ")
}

/// One row of the jobs table: its number (the one the chart's bars carry), its state, its name with the notes under it, how long it took or has run.
fn job_row(job: &Value, s: &Stats, index: usize, now: i64) -> String {
    let name = cell(&text(job, "name"));
    let name = match text(job, "html_url").as_str() {
        "" => name,
        url => format!("[{name}]({url})"),
    };
    let skipped = job["conclusion"] == "skipped";
    let took = match job["status"].as_str() {
        Some("completed") if skipped => "-".to_string(),
        Some("completed") => took(job),
        Some("in_progress") => format!("🔄 {}", epoch(&job["started_at"]).map_or("running".to_string(), |a| format!("{} so far", seconds((now - a).max(0))))),
        _ => "⏸️ waiting".to_string(),
    };
    let step = (job["status"] == "in_progress").then(|| job["steps"].as_array().and_then(|st| st.iter().find(|x| x["status"] == "in_progress")).map(|x| text(x, "name"))).flatten();
    let mut notes = notes(job, s);
    if let Some(step) = step {
        notes = if notes.is_empty() { format!("in: {step}") } else { format!("in: {step}; {notes}") };
    }
    let name = if notes.is_empty() { name } else { format!("{name}<br><sub>{}</sub>", cell(&notes)) };
    format!("| {} {index} | {name} | {took} |\n", icon(job))
}

fn cache_details(rows: &[(String, Cache)]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n<details><summary>Cache details (hits, latency per job)</summary>\n\n| Job | Hits | Misses | Puts | Get latency | Put latency |\n|---|---|---|---|---|---|\n");
    for (name, c) in rows {
        out.push_str(&format!("| {} | {} | {} | {} | {} (max {:.0} ms) | {} (max {:.0} ms) |\n", cell(name), c.hits, c.misses, c.puts, mean(c.get_ms, c.gets), c.get_max, mean(c.put_ms, c.put_calls), c.put_max));
    }
    out.push_str("\n</details>\n");
    out
}

fn crates_table(rows: &[(String, String, bool)]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let replayed = rows.iter().filter(|(_, _, hit)| *hit).count();
    let mut out = format!("\n<details><summary>Flow tests by crate ({} ran, {replayed} replayed from the cache)</summary>\n\n| Crate | Job | Result |\n|---|---|---|\n", rows.len() - replayed);
    for (name, job, hit) in rows.iter().take(60) {
        out.push_str(&format!("| {} | {} | {} |\n", cell(name), cell(job), if *hit { "replayed from the cache" } else { "ran" }));
    }
    out.push_str("\n</details>\n");
    out
}

/// The e2e units of this commit's published runs, from the file `wf test results --json` wrote.
fn units_table(env: &Env) -> String {
    let Some(path) = &env.results else { return String::new() };
    let Ok(raw) = std::fs::read_to_string(path) else { return String::new() };
    let start = raw.find('{').unwrap_or(0);
    let Ok(doc) = serde_json::from_str::<Value>(&raw[start..]) else { return String::new() };
    let mut runs: Vec<&Value> = doc["runs"].as_array().map(|a| a.iter().filter(|r| r["sha"] == env.sha.as_str()).collect()).unwrap_or_default();
    if runs.is_empty() {
        return String::new();
    }
    runs.sort_by_key(|r| (text(r, "platform"), text(r, "test"), text(r, "unit")));
    let mut out = String::from("\n### 🧪 E2E units\n\n| Platform | Test | Unit | Verdict | Took | App start |\n|---|---|---|---|---|---|\n");
    for r in runs.iter().take(80) {
        let (took, start) = (r["durationS"].as_f64().map_or("-".to_string(), |d| format!("{d:.1} s")), r["startupS"].as_f64().map_or("-".to_string(), |d| format!("{d:.1} s ({})", text(r, "appStart"))));
        out.push_str(&format!("| {} | {} | {} | {} | {} | {} |\n", cell(&text(r, "platform")), cell(&text(r, "test")), cell(&text(r, "unit")), cell(&text(r, "verdict")), took, start));
    }
    out
}

/// `HH:MM` (UTC) of an instant in milliseconds.
fn clock(ms: i64) -> String {
    let secs = ms.div_euclid(1000).rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

fn flag(value: f64, orange: f64, red: f64) -> &'static str {
    if value >= red {
        "🔴"
    } else if value >= orange {
        "🟠"
    } else {
        "🟢"
    }
}

/// The VM's own numbers over the run, from reporting-service: what the host was doing while the jobs ran (the average and the worst
/// moment of each signal) and whether it stopped sampling. A line saying why it is missing when it cannot be read.
fn vm_section(env: &Env, jobs: &[crate::ci_insights::JobTime], from_ms: i64, to_ms: i64) -> String {
    let heading = "\n### 🖥️ VM during the run\n\n";
    let now = wf_kit::time::now_ms() as i64;
    if (now - from_ms) / 1000 > 23 * 3600 {
        return format!("{heading}*VM stats unavailable: the run is older than the stored history.*\n");
    }
    let window = "24h";
    let series = match wf_vmops::host_summary::fetch_scoped(&env.vm_url, window, Some("host")) {
        Ok(series) => series,
        Err(reason) => return format!("{heading}*VM stats unavailable: {}*\n", cell(&cut(&reason, 160))),
    };
    let points = series["points"].as_array().cloned().unwrap_or_default();
    let sum = wf_vmops::host_summary::summarize(&points, from_ms, to_ms, 180_000);
    let samples = sum["samples"].as_u64().unwrap_or(0);
    if samples == 0 {
        return format!("{heading}*No host samples were stored for the run.*\n");
    }
    let sig = |name: &str| (sum["signals"][name]["avg"].as_f64(), sum["signals"][name]["peak"].as_f64(), sum["signals"][name]["atMs"].as_i64());
    let total = sum["memoryTotal"].as_f64().unwrap_or(0.0);
    let gb = |bytes: f64| format!("{:.1} GB", bytes / 1e9);
    let mut out = format!("{heading}| | Average | Peak | At (UTC) |\n|---|---|---|---|\n");
    let mut row = |label: &str, name: &str, show: &dyn Fn(f64) -> String, mark: &dyn Fn(f64) -> &'static str| {
        if let (Some(avg), Some(peak), Some(at)) = sig(name) {
            out.push_str(&format!("| {} {label} | {} | **{}** | {} |\n", mark(peak), show(avg), show(peak), clock(at)));
        }
    };
    let pct = |v: f64| format!("{v:.0}%");
    row("CPU busy (of 2 cores)", "cpu", &pct, &|v| flag(v, 70.0, 90.0));
    row("CPU taken by the hypervisor", "cpu_steal", &pct, &|v| flag(v, 10.0, 30.0));
    row("CPU waiting on disk", "cpu_iowait", &pct, &|v| flag(v, 10.0, 30.0));
    row("CPU in the kernel (writeback, overlay, network)", "cpu_kernel", &pct, &|v| flag(v, 30.0, 50.0));
    row("Memory in use", "memory_used", &|v| if total > 0.0 { format!("{} ({:.0}%)", gb(v), v * 100.0 / total) } else { gb(v) }, &|v| if total > 0.0 { flag(v * 100.0 / total, 75.0, 90.0) } else { "🟢" });
    row("I/O wait", "psi_io", &pct, &|v| flag(v, 20.0, 50.0));
    row("Memory pressure", "psi_memory", &pct, &|v| flag(v, 1.0, 10.0));
    row("Load (1 min)", "load1", &|v| format!("{v:.1}"), &|v| flag(v, 4.0, 8.0));
    row("Swap in use", "swap_used", &gb, &|v| flag(v, 1.0, 1e9));
    row("Docker list call", "list_ms", &|v| format!("{v:.0} ms"), &|v| flag(v, 500.0, 5_000.0));
    let gaps = sum["gaps"].as_array().cloned().unwrap_or_default();
    match gaps.iter().max_by_key(|g| g["seconds"].as_i64().unwrap_or(0)) {
        None => out.push_str(&format!("\n🟢 Sampled {samples} times, no gaps: the VM kept up.\n")),
        Some(worst) => out.push_str(&format!(
            "\n⚠️ **The VM stopped sampling {} time(s)**, longest {} min at {}: it stalled.\n",
            gaps.len(),
            worst["seconds"].as_i64().unwrap_or(0) / 60,
            clock(worst["fromMs"].as_i64().unwrap_or(0))
        )),
    }
    out.push_str(&job_resources(&points, jobs));
    out.push_str(&container_table(env, from_ms, to_ms, (sig("cpu").0, sig("cpu_steal").0, sig("cpu_kernel").0)));
    out.push_str(&vm_charts(&points, from_ms, to_ms, now));
    out
}

/// What the host did during each job: the average and the worst CPU, I/O wait and memory of its window, so the job that hogged the VM is the row that stands out.
fn job_resources(points: &[Value], jobs: &[crate::ci_insights::JobTime]) -> String {
    let rows: Vec<String> = jobs
        .iter()
        .filter_map(|j| {
            let (from, to) = (j.started? * 1000, j.ended? * 1000);
            let sum = wf_vmops::host_summary::summarize(points, from, to.max(from + 1000), 180_000);
            let sig = |name: &str, key: &str| sum["signals"][name][key].as_f64();
            let total = sum["memoryTotal"].as_f64().unwrap_or(0.0);
            let (cpu, io) = (sig("cpu", "avg")?, sig("psi_io", "peak")?);
            let mem = sig("memory_used", "peak").filter(|_| total > 0.0).map_or("-".to_string(), |m| format!("{:.0}%", m * 100.0 / total));
            Some(format!("| {} | {:.0}% / {:.0}% | {}{:.0}% | {mem} |\n", cell(&j.name), cpu, sig("cpu", "peak")?, flag(io, 20.0, 50.0), io))
        })
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    format!("\n**Per job** (CPU average / peak · I/O wait peak · memory peak)\n\n| Job | CPU | I/O wait | Memory |\n|---|---|---|---|\n{}", rows.concat())
}

/// The runners' own numbers for a labelled pipeline (GitHub-hosted jobs are machines of their own, so the VM's history says nothing about
/// them): every job leaves `stats-<job>.jsonl` (one `{t, cpu, memUsedMb, memTotalMb, load1}` per sample) in `WF_CI_RUNNER_STATS_DIR`.
fn runner_section(env: &Env) -> String {
    let heading = "\n### 🖥️ Runners during the run\n\n";
    let Some(dir) = var("WF_CI_RUNNER_STATS_DIR") else { return String::new() };
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect()).unwrap_or_default();
    files.sort();
    let mut rows = Vec::new();
    for path in files {
        let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("job").trim_start_matches("stats-").to_string();
        let samples: Vec<Value> = std::fs::read_to_string(&path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        if samples.is_empty() {
            continue;
        }
        let series = |key: &str| samples.iter().filter_map(|s| s[key].as_f64()).collect::<Vec<f64>>();
        let (cpu, mem, load) = (series("cpu"), series("memUsedMb"), series("load1"));
        let total = samples.iter().filter_map(|s| s["memTotalMb"].as_f64()).fold(0.0, f64::max);
        let avg = |v: &[f64]| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
        let peak = |v: &[f64]| v.iter().cloned().fold(0.0, f64::max);
        let (mem_avg, mem_peak) = (avg(&mem), peak(&mem));
        let mem_pct = if total > 0.0 { mem_peak * 100.0 / total } else { 0.0 };
        rows.push(format!(
            "| {} {} | {:.0}% | **{:.0}%** | {:.1} GB | **{:.1} GB** ({:.0}%) | {:.1} | {} |\n",
            flag(peak(&cpu), 70.0, 90.0),
            cell(&name),
            avg(&cpu),
            peak(&cpu),
            mem_avg / 1000.0,
            mem_peak / 1000.0,
            mem_pct,
            peak(&load),
            samples.len()
        ));
    }
    if rows.is_empty() {
        return format!("{heading}*No runner samples were stored for this run.*\n");
    }
    let _ = env;
    format!("{heading}Each GitHub-hosted job is its own 2-core machine; the numbers are sampled every 5 s while the job runs.\n\n| Job | CPU busy avg | CPU busy peak | Memory avg | Memory peak | Load (1 min) peak | Samples |\n|---|---|---|---|---|---|---|\n{}", rows.concat())
}

/// The containers that used the VM during the run, biggest disk writers first: who the I/O wait and the memory belong to.
fn container_table(env: &Env, from_ms: i64, to_ms: i64, host: (Option<f64>, Option<f64>, Option<f64>)) -> String {
    use crate::ci_insights::bytes;
    let params = [("scope", "container".to_string()), ("metric", "cpu,memory,disk_read_bytes,disk_write_bytes".to_string()), ("since", (from_ms - 60_000).to_string())];
    let Ok(series) = wf_vmops::host_summary::fetch_query(&env.vm_url, "24h", &params) else { return String::new() };
    let usage = wf_vmops::host_summary::container_usage(series["points"].as_array().map(Vec::as_slice).unwrap_or_default(), from_ms - 60_000, from_ms, to_ms);
    let rows: Vec<String> = usage
        .iter()
        .take(8)
        .map(|u| {
            let n = |key: &str| u[key].as_f64().unwrap_or(0.0);
            format!("| {} | {} | {} | {:.0}% / {:.0}% | {} |\n", cell(u["container"].as_str().unwrap_or("?")), bytes(n("writtenBytes")), bytes(n("readBytes")), n("cpuAvg"), n("cpuPeak"), bytes(n("memoryPeak")))
        })
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    // a container's CPU is in cores x 100 (docker's own figure), the host's is a share of its two cores
    let containers: f64 = usage.iter().map(|u| u["cpuAvg"].as_f64().unwrap_or(0.0)).sum::<f64>() / 100.0;
    let attribution = match host {
        (Some(busy), steal, kernel) => {
            let (busy_cores, steal_cores) = (busy / 100.0 * 2.0, steal.unwrap_or(0.0) / 100.0 * 2.0);
            let left = (busy_cores - steal_cores - containers).max(0.0);
            let kernel_note = kernel.map_or(String::new(), |k| format!(" The kernel's own share of the time was {:.2} cores (writeback, overlay and network work that no container's figure holds).", k / 100.0 * 2.0));
            format!("\nAverage CPU, in cores of 2: the host was busy {busy_cores:.2} = containers {containers:.2} + the hypervisor {steal_cores:.2} + not in any container {left:.2} ({:.0}% of the busy time).{kernel_note}\n", left / busy_cores.max(0.01) * 100.0)
        }
        _ => String::new(),
    };
    format!("\n**Containers** (biggest disk writers first; CPU is cores x 100)\n\n| Container | Wrote | Read | CPU avg / peak | Memory peak |\n|---|---|---|---|---|\n{}{attribution}", rows.concat())
}

/// A Mermaid chart is drawn by GitHub from the text, in the comment and in the job summary.
fn mermaid(body: &str) -> String {
    format!("\n```mermaid\n{body}```\n")
}

/// How long each job took, one vertical bar per job in the order of the table (the bars carry the table's numbers), so the slow job is the
/// tall one; a bar chart in portrait, which a phone draws without scrolling sideways.
fn job_chart(shown: &[(&Value, Stats)]) -> String {
    let mut labels = Vec::new();
    let mut minutes = Vec::new();
    for (i, (job, _)) in shown.iter().enumerate() {
        if let Some(secs) = span_secs(job).filter(|_| job["conclusion"] != "skipped") {
            labels.push((i + 1).to_string());
            minutes.push(secs as f64 / 60.0);
        }
    }
    if minutes.len() < 2 {
        return String::new();
    }
    format!("\n### Minutes per job\n{}", bar_chart("Minutes per job (numbers as in the table)", "minutes", &labels, &minutes, 1.0, false))
}

/// A vertical bar chart of one series, its labels and the unit on the axis; the axis tops out at the next ten above the peak. With `thin`,
/// about six labels are printed (every one on a phone overlaps its neighbours) and the others get an invisible, distinct name (equal names
/// would be one category).
fn bar_chart(title: &str, unit: &str, labels: &[String], values: &[f64], floor_max: f64, thin: bool) -> String {
    let peak = values.iter().copied().fold(0.0_f64, f64::max);
    let step = if peak > 100.0 { 50.0 } else if peak > 20.0 { 10.0 } else { 5.0 };
    let top = ((peak / step).ceil() * step).max(floor_max);
    let every = if thin { labels.len().div_ceil(6).max(1) } else { 1 };
    let names = labels.iter().enumerate().map(|(i, l)| if i % every == 0 { format!("\"{l}\"") } else { format!("\"{}\"", "\u{200b}".repeat(i + 1)) }).collect::<Vec<_>>().join(", ");
    let points = values.iter().map(|v| format!("{v:.1}")).collect::<Vec<_>>().join(", ");
    mermaid(&format!("xychart-beta\n    title \"{title}\"\n    x-axis [{names}]\n    y-axis \"{unit}\" 0 --> {top:.0}\n    bar [{points}]\n"))
}

/// The VM's history as charts: I/O wait during the run and over the last day (the signal that says a stall is coming), with CPU and
/// memory under a fold, all from the stored samples; a series with fewer than two points has no chart.
fn vm_charts(points: &[Value], from_ms: i64, to_ms: i64, now_ms: i64) -> String {
    use wf_vmops::host_summary::buckets;
    let chart = |title: &str, unit: &str, metric: &str, from: i64, to: i64, peak: bool, label: &dyn Fn(i64) -> String, floor: f64, scale: f64| {
        let b = buckets(points, metric, from, to, 12);
        if b.len() < 2 {
            return String::new();
        }
        let labels: Vec<String> = b.iter().map(|b| label(b.start_ms)).collect();
        let values: Vec<f64> = b.iter().map(|b| (if peak { b.max } else { b.avg }) / scale).collect();
        bar_chart(title, unit, &labels, &values, floor, true)
    };
    let hour = |ms: i64| format!("{}h", &clock(ms)[..2]);
    let mut out = String::new();
    out.push_str(&chart("I/O wait during the run (peak per stretch)", "% of time stalled", "psi_io", from_ms, to_ms, true, &clock, 10.0, 1.0));
    out.push_str(&chart("I/O wait over the last 24 h (peak per hour)", "% of time stalled", "psi_io", now_ms - 24 * 3_600_000, now_ms, true, &hour, 10.0, 1.0));
    let mut more = String::new();
    more.push_str(&chart("CPU busy during the run (average per stretch)", "%", "cpu", from_ms, to_ms, false, &clock, 10.0, 1.0));
    more.push_str(&chart("Memory in use during the run (average per stretch)", "GB", "memory_used", from_ms, to_ms, false, &clock, 1.0, 1e9));
    more.push_str(&chart("CPU busy over the last 24 h (average per hour)", "%", "cpu", now_ms - 24 * 3_600_000, now_ms, false, &hour, 10.0, 1.0));
    more.push_str(&chart("Memory in use over the last 24 h (average per hour)", "GB", "memory_used", now_ms - 24 * 3_600_000, now_ms, false, &hour, 1.0, 1e9));
    if !more.is_empty() {
        out.push_str(&format!("\n<details><summary>More VM charts (CPU, memory)</summary>\n{more}\n</details>\n"));
    }
    out
}

/// The failed jobs first, each with the step that failed, how long it took, a link and the lines of its log that say why. Jobs that
/// failed in the same step with the same cause (six deploy legs timing out on the same call) are one block, not six.
fn failures_block(failures: &[Failure], skipped: &[String]) -> String {
    let mut groups: Vec<Vec<&Failure>> = Vec::new();
    for f in failures {
        match groups.iter_mut().find(|g| g[0].step == f.step && g[0].cause == f.cause && !f.cause.is_empty()) {
            Some(group) => group.push(f),
            None => groups.push(vec![f]),
        }
    }
    let mut out = String::new();
    for group in &groups {
        let first = group[0];
        if group.len() == 1 {
            let link = if first.url.is_empty() { String::new() } else { format!(" · [open the job]({})", first.url) };
            out.push_str(&format!("> ### ❌ {} failed\n> in step **{}** after {}{link}\n>\n", html(&first.name), html(&first.step), first.took));
        } else {
            out.push_str(&format!("> ### ❌ {} jobs failed the same way\n> in step **{}**\n>\n", group.len(), html(&first.step)));
            for f in group {
                let name = if f.url.is_empty() { html(&f.name) } else { format!("[{}]({})", html(&f.name), f.url) };
                out.push_str(&format!("> - {name} · {}\n", f.took));
            }
            out.push_str(">\n");
        }
        if first.cause.is_empty() {
            out.push_str("> *no failure line in the log*\n>\n");
        } else {
            // inline code wraps on a phone, a code block would scroll sideways past the part of the line that says what happened
            let lines = first.cause.iter().map(|line| format!("<code>{}</code>", html(line))).collect::<Vec<_>>().join("<br>");
            out.push_str(&format!("> {lines}\n>\n"));
        }
    }
    if !failures.is_empty() && !skipped.is_empty() {
        let names = skipped.iter().take(SKIPPED_NAMES).map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
        let more = if skipped.len() > SKIPPED_NAMES { format!(" and {} more", skipped.len() - SKIPPED_NAMES) } else { String::new() };
        out.push_str(&format!("> Skipped because of the failure: {names}{more}.\n"));
    }
    out.push('\n');
    out
}

fn body(env: &Env, jobs: &[Value], logs: &[Result<String, String>], failures: &[Failure], skipped: &[String]) -> String {
    let (short, base) = (env.sha.chars().take(7).collect::<String>(), format!("{}/{}", env.server, env.repo));
    let run = format!("{base}/actions/runs/{}", env.run);
    let shown: Vec<(&Value, Stats)> = jobs.iter().zip(logs).filter(|(j, _)| !is_meta(j)).map(|(j, l)| (j, l.as_ref().map(|l| stats(l)).unwrap_or_default())).collect();
    let started = shown.iter().filter_map(|(j, _)| epoch(&j["started_at"])).min();
    let ended = shown.iter().filter_map(|(j, _)| epoch(&j["completed_at"])).max();
    let now = wf_kit::time::now_ms() as i64 / 1000;
    let running = shown.iter().any(|(j, _)| j["status"] != "completed");
    let wall = started.zip(if running { Some(now) } else { ended }).filter(|(f, t)| t >= f).map(|(f, t)| seconds(t - f));
    let (verdict, mark) = match (running, failures.is_empty()) {
        (true, true) => ("running", "🟡"),
        (true, false) => ("running, a job failed", "🟠"),
        (false, true) => ("passed", "✅"),
        (false, false) => ("failed", "❌"),
    };
    let title = format!("## {mark} CI ({}) {verdict} · `{short}` · {}{}", env.label.as_deref().unwrap_or("self-hosted"), env.branch, wall.as_ref().map_or(String::new(), |w| format!(" · {w}")));
    let mut out = format!("{}\n{title}\n[Commit `{short}`]({base}/commit/{}) · [run {}]({run}) · runner `{}`\n\n", live_marker(env), env.sha, env.run, env.runner);
    out.push_str(failures_block(failures, skipped).trim_start_matches('\n'));

    let count = |pred: &dyn Fn(&Value) -> bool| shown.iter().filter(|(j, _)| pred(j)).count();
    let (passed, failed, skipped_jobs) = (count(&|j| j["conclusion"] == "success"), count(&is_failed), count(&|j| j["conclusion"] == "skipped"));
    let (hits, misses) = shown.iter().filter_map(|(_, s)| s.cache).fold((0, 0), |(h, m), c| (h + c.hits, m + c.misses));
    let layers = shown.iter().filter_map(|(_, s)| s.layers).fold((0, 0), |(c, t), l| (c + l.0, t + l.1));
    let times: Vec<crate::ci_insights::JobTime> = shown.iter().map(|(j, _)| job_time(j)).collect();
    let facts: Vec<(String, crate::ci_insights::LogFacts)> = jobs.iter().zip(logs).filter(|(j, _)| !is_meta(j)).filter_map(|(j, l)| Some((text(j, "name"), crate::ci_insights::facts(l.as_ref().ok()?)))).collect();
    let mut glance = vec![format!("**✅ {passed} passed**")];
    glance.extend((failed > 0).then(|| format!("**❌ {failed} failed**")));
    let live_jobs = count(&|j| j["status"] == "in_progress");
    glance.extend((live_jobs > 0).then(|| format!("**🔄 {live_jobs} running**")));
    let waiting = count(&|j| !matches!(j["status"].as_str(), Some("completed" | "in_progress")));
    glance.extend((waiting > 0).then(|| format!("**⏸️ {waiting} waiting**")));
    glance.extend((skipped_jobs > 0).then(|| format!("**⏭️ {skipped_jobs} skipped**")));
    glance.extend(wall.map(|w| format!("⏱️ {w}")));
    glance.extend(percent(hits, hits + misses).map(|p| format!("🗄️ build cache {p}% hit")));
    let refused: u64 = shown.iter().filter_map(|(_, s)| s.cache).map(|c| c.quota).sum();
    glance.extend((refused > 0).then(|| format!("**⚠️ Object Storage full: {refused} cache write(s) refused**")));
    glance.extend((layers.1 > 0).then(|| format!("🐳 docker layers {} of {} cached", layers.0, layers.1)));
    out.push_str(&format!("{}\n\n", glance.join(" · ")));

    out.push_str(&crate::ci_insights::where_time_went(&times, &facts));
    out.push_str("### Jobs\n\n| | Job | Took |\n|---|---|---|\n");
    let mut crates: Vec<(String, String, bool)> = Vec::new();
    let mut caches: Vec<(String, Cache)> = Vec::new();
    for (index, (job, job_stats)) in shown.iter().enumerate() {
        out.push_str(&job_row(job, job_stats, index + 1, now));
        crates.extend(job_stats.crates.iter().map(|(name, hit)| (name.clone(), text(job, "name"), *hit)));
        caches.extend(job_stats.cache.map(|c| (text(job, "name"), c)));
    }
    out.push_str(&job_chart(&shown));
    if env.label.is_some() {
        out.push_str(&runner_section(env));
    } else if let (Some(from), Some(to)) = (started, ended.or(started)) {
        out.push_str(&vm_section(env, &times, from * 1000, to.max(from) * 1000 + 60_000));
    }
    out.push_str(&crate::ci_insights::cache_section(&cache_rows(&shown, &facts), &facts));
    let finished: Vec<(String, String)> = jobs.iter().zip(logs).filter(|(j, _)| !is_meta(j)).filter_map(|(j, l)| Some((text(j, "name"), l.as_ref().ok()?.clone()))).collect();
    out.push_str(&crate::ci_cache_health::section(&crate::ci_cache_health::assess(&finished, crate::ci_cache_health::is_push_to_dev(env))));
    out.push_str(&units_table(env));
    out.push_str(&cache_details(&caches));
    out.push_str(&crate::ci_insights::cost_block(&facts));
    out.push_str(&crates_table(&crates));
    out.push_str(&crate::ci_insights::detail_blocks(&times, &facts));
    for f in failures.iter().filter(|f| !f.more.is_empty()) {
        let block = details(f);
        if out.len() + block.len() <= BODY_CAP - RESERVED {
            out.push_str(&block);
        }
    }
    if out.len() > BODY_CAP {
        let mut cut_at = BODY_CAP - RESERVED;
        while !out.is_char_boundary(cut_at) {
            cut_at -= 1;
        }
        out.truncate(cut_at);
        out.push_str("\n\n*The rest is in the run.*\n");
    }
    out
}

fn job_time(job: &Value) -> crate::ci_insights::JobTime {
    let (started, ended) = (epoch(&job["started_at"]), epoch(&job["completed_at"]));
    let mut steps: Vec<(String, i64)> = job["steps"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|s| !text(s, "name").starts_with("Post ") && !text(s, "name").starts_with("Set up") && !text(s, "name").starts_with("Complete"))
        .filter_map(|s| Some((text(s, "name"), epoch(&s["completed_at"])? - epoch(&s["started_at"])?)))
        .filter(|(_, t)| *t >= 5)
        .collect();
    steps.sort_by_key(|(_, t)| std::cmp::Reverse(*t));
    let queued = started.zip(epoch(&job["created_at"])).map(|(s, c)| (s - c).max(0));
    crate::ci_insights::JobTime { name: text(job, "name"), started, ended, queued, steps }
}

fn cache_rows(shown: &[(&Value, Stats)], facts: &[(String, crate::ci_insights::LogFacts)]) -> Vec<crate::ci_insights::CacheFacts> {
    shown
        .iter()
        .map(|(j, s)| {
            let name = text(j, "name");
            let sccache = facts.iter().find(|(n, _)| *n == name).and_then(|(_, f)| f.sccache);
            crate::ci_insights::CacheFacts {
                build: s.cache.map(|c| (c.hits, c.misses, c.puts, if c.gets == 0 { 0.0 } else { c.get_ms / c.gets as f64 })),
                nx: s.nx,
                layers: s.layers,
                sccache,
                name,
            }
        })
        .collect()
}

/// The jobs skipped from the first failure on: one skipped earlier was the plan's choice, not the failure's.
fn skipped_after(jobs: &[Value], failed: &[&Value]) -> Vec<String> {
    let first = failed.iter().filter_map(|j| epoch(&j["completed_at"])).min();
    let after = |job: &Value| match (first, epoch(&job["completed_at"])) {
        (Some(first), Some(at)) => at >= first,
        _ => true,
    };
    jobs.iter().filter(|j| j["conclusion"] == "skipped" && after(j)).map(|j| text(j, "name")).collect()
}

/// The id of the comment under `list` (an API path) that carries `marker`, if any.
fn find_comment(env: &Env, list: &str, marker: &str) -> Result<Option<u64>, String> {
    for page in 1..=5 {
        let url = format!("{}{list}?per_page=100&page={page}", env.api);
        let comments = json_of(send(env, "GET", &url, None)?, "listing the comments")?;
        let comments = comments.as_array().cloned().unwrap_or_default();
        if let Some(found) = comments.iter().find(|c| c["body"].as_str().is_some_and(|b| b.contains(marker))) {
            return Ok(found["id"].as_u64());
        }
        if comments.len() < 100 {
            break;
        }
    }
    Ok(None)
}

/// Creates the comment under `list`, or updates the one this run and runner already made (never another run's).
fn post_comment(env: &Env, list: &str, on_commit: bool, body: &str) -> Result<(&'static str, Value), String> {
    let (method, url, action) = match find_comment(env, list, &marker(env))? {
        Some(id) => ("PATCH", format!("{}/repos/{}/{}comments/{id}", env.api, env.repo, if on_commit { "" } else { "issues/" }), "updated"),
        None => ("POST", format!("{}{list}", env.api), "created"),
    };
    let posted = json_of(send(env, method, &url, Some(&json!({ "body": body })))?, "posting the comment")?;
    Ok((action, posted["id"].clone()))
}

pub(crate) fn open_pr(env: &Env) -> Result<Option<u64>, String> {
    let owner = env.repo.split('/').next().unwrap_or_default();
    let url = format!("{}/repos/{}/pulls?head={owner}:{}&state=open&per_page=1", env.api, env.repo, env.branch);
    Ok(json_of(send(env, "GET", &url, None)?, "finding the pull request")?.as_array().and_then(|a| a.first()).and_then(|p| p["number"].as_u64()))
}

/// The jobs that are not part of what the report is about: the report itself and the budget that runs after it.
pub(crate) fn is_meta(job: &Value) -> bool {
    matches!(wf_bucket::job_name::job_name(&text(job, "name")), "report" | "budget")
}

/// A finished job's log, kept on the runner for the run so each update of the comment does not download every earlier log again.
pub(crate) fn cached_log(env: &Env, job: &Value) -> Result<String, String> {
    let dir = var("WF_CI_LOG_CACHE").map(std::path::PathBuf::from).or_else(|| var("HOME").map(|h| std::path::PathBuf::from(h).join(".cache/wf/ci-logs")));
    let file = dir.as_ref().map(|d| d.join(&env.run).join(format!("{}.log", job["id"].as_u64().unwrap_or(0))));
    if let Some(text) = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok()) {
        return Ok(text);
    }
    let log = read_log(env, job)?;
    if let Some(file) = &file {
        let _ = file.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(file, &log);
        if let Some(dir) = &dir {
            // a run's logs are only wanted while it runs: the older runs' go
            for old in std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy() != env.run) {
                if old.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a > Duration::from_secs(86_400)) {
                    let _ = std::fs::remove_dir_all(old.path());
                }
            }
        }
    }
    Ok(log)
}

/// What the report says about the run now: its comment text, and which jobs failed.
pub(crate) struct Report {
    pub body: String,
    pub failed: Vec<String>,
}

/// The report of the run as it stands: the jobs so far, the logs of the finished ones, and the VM's numbers.
pub(crate) fn report(env: &Env) -> Result<Report, String> {
    let jobs = list_jobs(env)?;
    let failed: Vec<&Value> = jobs.iter().filter(|j| is_failed(j) && !is_meta(j)).collect();
    let logs: Vec<Result<String, String>> = jobs.iter().map(|j| if matches!(j["conclusion"].as_str(), Some("success" | "failure" | "timed_out")) && !is_meta(j) { cached_log(env, j) } else { Err("not run".to_string()) }).collect();
    let failures: Vec<Failure> = jobs.iter().zip(&logs).filter(|(j, _)| is_failed(j) && !is_meta(j)).map(|(j, log)| failure(env, j, log)).collect();
    let body = redact(&body(env, &jobs, &logs, &failures, &skipped_after(&jobs, &failed)), &env.token);
    Ok(Report { body, failed: failed.iter().map(|j| text(j, "name")).collect() })
}

/// The pull request's comments that belong to a CI run (this format or the one before it) with their ids and bodies.
fn ci_comments(env: &Env, pr: u64) -> Result<Vec<(u64, String)>, String> {
    let mut found = Vec::new();
    for page in 1..=10 {
        let url = format!("{}/repos/{}/issues/{pr}/comments?per_page=100&page={page}", env.api, env.repo);
        let comments = json_of(send(env, "GET", &url, None)?, "listing the comments")?;
        let comments = comments.as_array().cloned().unwrap_or_default();
        found.extend(comments.iter().filter_map(|c| {
            let body = c["body"].as_str()?;
            (body.starts_with(LIVE_PREFIX) || body.starts_with("<!-- ci-comment")).then(|| (c["id"].as_u64().unwrap_or(0), body.to_string()))
        }));
        if comments.len() < 100 {
            break;
        }
    }
    Ok(found)
}

/// Makes this run's comment the only CI comment of the pull request, and the last one: the other runs' are deleted, this run's is edited in
/// place, or created when the run has none yet.
pub(crate) fn sync_live(env: &Env, pr: u64, body: &str) -> Result<(&'static str, Value), String> {
    let mine_marker = live_marker(env);
    let existing = ci_comments(env, pr)?;
    let mut mine = None;
    for (id, text) in existing.iter().filter(|c| same_flavor(env, &c.1)) {
        if text.contains(&mine_marker) {
            mine = Some(*id);
        } else {
            send(env, "DELETE", &format!("{}/repos/{}/issues/comments/{id}", env.api, env.repo), None)?;
        }
    }
    match mine {
        Some(id) => {
            json_of(send(env, "PATCH", &format!("{}/repos/{}/issues/comments/{id}", env.api, env.repo), Some(&json!({ "body": body })))?, "updating the comment")?;
            Ok(("updated", json!(id)))
        }
        None => {
            let posted = json_of(send(env, "POST", &format!("{}/repos/{}/issues/{pr}/comments", env.api, env.repo), Some(&json!({ "body": body })))?, "posting the comment")?;
            Ok(("created", posted["id"].clone()))
        }
    }
}

fn attempt(dry_run: bool, names: &mut Vec<String>) -> Result<Value, String> {
    let env = Env::read()?;
    let Report { body, failed } = report(&env)?;
    *names = failed.clone();
    if dry_run {
        return Ok(json!({ "ok": true, "posted": false, "dryRun": true, "failedJobs": names, "body": body }));
    }
    let mut out = json!({ "ok": true, "posted": false, "failedJobs": names });
    if let Some(path) = &env.summary {
        use std::io::Write;
        let wrote = std::fs::OpenOptions::new().append(true).create(true).open(path).and_then(|mut f| writeln!(f, "{body}"));
        out["summary"] = json!(wrote.is_ok());
    }
    if !failed.is_empty() {
        // the commit's own comment keeps the report of a red run; its marker is the older per-run one
        let for_commit = body.replacen(&live_marker(&env), &marker(&env), 1);
        let (action, id) = post_comment(&env, &format!("/repos/{}/commits/{}/comments", env.repo, env.sha), true, &for_commit)?;
        (out["posted"], out["action"], out["commentId"]) = (json!(true), json!(action), id);
    }
    match open_pr(&env).and_then(|pr| pr.map(|n| sync_live(&env, n, &body).map(|(action, id)| json!({ "number": n, "action": action, "commentId": id }))).transpose()) {
        Ok(pr) => out["prComment"] = pr.unwrap_or(Value::Null),
        Err(reason) => out["prComment"] = json!({ "error": redact(&reason, &env.token) }),
    }
    Ok(out)
}

/// `wf ci comment [--dry-run]`. A failure to comment is reported and never fails the step: exit 0 either way.
pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    match verb.as_str() {
        "audit" => return crate::ci_audit::run(rest),
        "cache-health" => return crate::ci_cache_health::run(rest),
        "preflight" => return crate::ci_preflight::run(rest),
        "live" => return crate::ci_live::run(rest),
        "tidy" => return crate::ci_tidy::run(rest),
        _ => {}
    }
    if verb != "comment" {
        return None;
    }
    let dry_run = parse_spec(rest, &SPEC)?.flag("dry-run");
    let mut names = Vec::new();
    let out = attempt(dry_run, &mut names).unwrap_or_else(|reason| {
        let reason = redact(&reason, &var("GITHUB_TOKEN").unwrap_or_default());
        eprintln!("wf ci comment: {reason}");
        json!({ "ok": false, "posted": false, "failedJobs": names, "reason": reason })
    });
    println!("{out}");
    Some(0)
}
