//! The read-only side of the git-state worker: `wf offload plan <slug> --check`, `wf offload plan --status <slug>` and
//! `wf offload worker check|status --plan <slug>`, and running a plan (`wf offload plan <slug>`).
use wf_kit::args::parse;
use wf_kit::js;
use wf_kit::time::now_ms;
use rca_core::time::parse as parse_time;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const USAGE_FILE: &str = include_str!("worker_usage.txt");
pub(crate) const LEASE_TTL_MS: i64 = 180_000;
const TIERS: &[&str] = &["unit", "contract", "flow", "integration", "e2e", "visual"];
const NEEDS: &[&str] = &["any", "deployed"];
const MAX_FILES: usize = 3;
pub(crate) const STDERR_TAIL: usize = 4096;

pub(crate) fn usage() -> &'static str {
    USAGE_FILE.trim_end_matches('\n')
}

/// The worker writes its report to stderr as it goes; wf then repeats the tail as the failure message.
pub(crate) struct Io {
    pub(crate) stderr: String,
}

impl Io {
    pub(crate) fn err(&mut self, s: &str) {
        eprint!("{s}");
        self.stderr.push_str(s);
    }
    pub(crate) fn finish(self, code: i32) -> i32 {
        if code != 0 {
            let tail = if self.stderr.is_empty() { format!("worker command failed with exit code {code}") } else { js::tail(&self.stderr, STDERR_TAIL) };
            wf_kit::wf_err!("wf: {tail}");
        }
        code
    }
}

/* ---------- units ---------- */

#[derive(Clone)]
pub(crate) struct Unit {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) tier: String,
    pub(crate) files: Vec<String>,
    pub(crate) brief: String,
    pub(crate) deps: Vec<String>,
    pub(crate) gate: String,
    pub(crate) needs: String,
}

pub(crate) struct UnitError {
    pub(crate) line: Option<usize>,
    pub(crate) reason: String,
}

fn nonblank(o: &serde_json::Map<String, Value>, k: &str) -> Option<String> {
    o.get(k).and_then(Value::as_str).filter(|s| !js::trim(s).is_empty()).map(str::to_string)
}

fn str_arr(o: &serde_json::Map<String, Value>, k: &str) -> Option<Vec<String>> {
    match o.get(k) {
        None => Some(vec![]),
        Some(Value::Array(a)) => a.iter().map(|x| x.as_str().filter(|s| !js::trim(s).is_empty()).map(str::to_string)).collect(),
        Some(_) => None,
    }
}

fn parse_line(o: &serde_json::Map<String, Value>) -> Result<Unit, String> {
    let id = nonblank(o, "id").ok_or("missing id")?;
    let files = str_arr(o, "files").ok_or_else(|| format!("{id}: files must be an array of non-empty strings"))?;
    if files.is_empty() || files.len() > MAX_FILES {
        return Err(format!("{id}: files must list 1-{MAX_FILES} paths (got {})", files.len()));
    }
    if files.iter().collect::<HashSet<_>>().len() != files.len() {
        return Err(format!("{id}: duplicate entry in files"));
    }
    let deps = str_arr(o, "deps").ok_or_else(|| format!("{id}: deps must be an array of unit ids"))?;
    let brief = nonblank(o, "brief").ok_or_else(|| format!("{id}: missing brief"))?;
    let gate = nonblank(o, "gate").ok_or_else(|| format!("{id}: missing gate"))?;
    let tier = nonblank(o, "tier").filter(|t| TIERS.contains(&t.as_str())).ok_or_else(|| format!("{id}: missing or unknown tier (allowed: {})", TIERS.join(", ")))?;
    let needs = if o.contains_key("needs") { nonblank(o, "needs") } else { Some("any".to_string()) };
    if !needs.as_deref().is_some_and(|n| NEEDS.contains(&n)) {
        return Err(format!("{id}: unknown needs (allowed: {})", NEEDS.join(", ")));
    }
    let title = nonblank(o, "title").unwrap_or_else(|| id.clone());
    Ok(Unit { id, title, tier, files, brief, deps, gate, needs: needs.unwrap_or_default() })
}

fn find_cycle(units: &[Unit]) -> Option<Vec<String>> {
    fn visit(u: &Unit, by_id: &HashMap<&str, &Unit>, state: &mut HashMap<String, bool>, trail: &mut Vec<String>) -> Option<Vec<String>> {
        match state.get(&u.id) {
            Some(true) => return None,
            Some(false) => {
                let at = trail.iter().position(|t| *t == u.id).unwrap_or(0);
                let mut cycle: Vec<String> = trail[at..].to_vec();
                cycle.push(u.id.clone());
                return Some(cycle);
            }
            None => {}
        }
        state.insert(u.id.clone(), false);
        trail.push(u.id.clone());
        for d in &u.deps {
            if let Some(dep) = by_id.get(d.as_str()) {
                if let Some(c) = visit(dep, by_id, state, trail) {
                    return Some(c);
                }
            }
        }
        trail.pop();
        state.insert(u.id.clone(), true);
        None
    }
    let by_id: HashMap<&str, &Unit> = units.iter().map(|u| (u.id.as_str(), u)).collect();
    let mut state = HashMap::new();
    for u in units {
        if let Some(c) = visit(u, &by_id, &mut state, &mut Vec::new()) {
            return Some(c);
        }
    }
    None
}

pub(crate) fn parse_units(text: &str) -> (Vec<Unit>, Vec<UnitError>) {
    let mut units: Vec<Unit> = Vec::new();
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    for (i, raw) in text.split('\n').enumerate() {
        let line = js::trim(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let value: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                errors.push(UnitError { line: Some(i + 1), reason: format!("not JSON: {e}") });
                continue;
            }
        };
        let Value::Object(obj) = value else {
            errors.push(UnitError { line: Some(i + 1), reason: "not an object".into() });
            continue;
        };
        match parse_line(&obj) {
            Err(reason) => errors.push(UnitError { line: Some(i + 1), reason }),
            Ok(u) if seen.contains(&u.id) => errors.push(UnitError { line: Some(i + 1), reason: format!("duplicate id {}", u.id) }),
            Ok(u) => {
                seen.insert(u.id.clone());
                units.push(u);
            }
        }
    }
    if errors.is_empty() {
        let ids: HashSet<&str> = units.iter().map(|u| u.id.as_str()).collect();
        for u in &units {
            for d in &u.deps {
                if !ids.contains(d.as_str()) {
                    errors.push(UnitError { line: None, reason: format!("{}: depends on unknown unit {d}", u.id) });
                }
            }
        }
        if let Some(cycle) = find_cycle(&units) {
            errors.push(UnitError { line: None, reason: format!("cycle: {}", cycle.join(" -> ")) });
        }
    }
    if units.is_empty() && errors.is_empty() {
        errors.push(UnitError { line: None, reason: "no units".into() });
    }
    (units, errors)
}

fn topo_order(units: &[Unit]) -> Vec<&Unit> {
    let mut done: HashSet<&str> = HashSet::new();
    let mut out: Vec<&Unit> = Vec::new();
    while out.len() < units.len() {
        let ready: Vec<&Unit> = units.iter().filter(|u| !done.contains(u.id.as_str()) && u.deps.iter().all(|d| done.contains(d.as_str()))).collect();
        if ready.is_empty() {
            break;
        }
        done.extend(ready.iter().map(|u| u.id.as_str()));
        out.extend(ready);
    }
    out
}

fn format_check(units: &[Unit]) -> String {
    let rows: Vec<String> = topo_order(units).iter().map(|u| format!("{}  {}  {}  gate: {}", u.id, u.tier, u.files.join(","), u.gate)).collect();
    format!("{}\nok: {} unit(s)\n", rows.join("\n"), units.len())
}

/* ---------- git ---------- */

pub(crate) struct Git<'a> {
    pub(crate) cwd: &'a Path,
}

impl Git<'_> {
    pub(crate) fn raw(&self, args: &[&str]) -> (i32, String, String) {
        match Command::new("git").args(args).current_dir(self.cwd).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null()).output() {
            Ok(o) => (o.status.code().unwrap_or(1), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned()),
            Err(e) => (1, String::new(), e.to_string()),
        }
    }
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let (code, out, err) = self.raw(args);
        if code != 0 {
            return Err(format!("git {} exited {code}: {}", args.join(" "), js::trim(&err)));
        }
        Ok(out)
    }
}

pub(crate) fn ls_remote_refs(out: &str) -> Vec<(String, String)> {
    let sha = Regex::new(r"^[0-9a-f]{40,64}$").expect("sha");
    out.split('\n')
        .filter_map(|l| {
            let parts: Vec<&str> = js::trim(l).split_whitespace().collect();
            (parts.len() == 2 && sha.is_match(parts[0])).then(|| (parts[0].to_string(), parts[1].to_string()))
        })
        .collect()
}

pub(crate) fn id_from_ref(r: &str, prefix: &str) -> Option<String> {
    let id = r.strip_prefix(&format!("{prefix}/"))?;
    (!id.is_empty() && !id.contains('/')).then(|| id.to_string())
}

pub(crate) struct Lease {
    pub(crate) worker: String,
    pub(crate) started_at: String,
    pub(crate) heartbeat_at: String,
}

pub(crate) fn parse_lease(message: &str) -> Option<Lease> {
    let v: Value = serde_json::from_str(js::trim(message)).ok()?;
    let o = v.as_object()?;
    let worker = o.get("worker")?.as_str().filter(|s| !s.is_empty())?;
    let attempt = o.get("attempt")?.as_f64()?;
    if attempt.fract() != 0.0 || attempt < 1.0 {
        return None;
    }
    o.get("model")?.as_str()?;
    let iso = |k: &str| o.get(k)?.as_str().filter(|s| parse_time(s).is_some()).map(str::to_string);
    Some(Lease { worker: worker.to_string(), started_at: iso("startedAt")?, heartbeat_at: iso("heartbeatAt")? })
}

enum State {
    Done,
    Leased { worker: String, age_ms: i64, stale: bool },
    Failed,
    Runnable,
    Blocked(Vec<String>),
}

fn plan_status(git: &Git<'_>, slug: &str, units: &[Unit], land_ref: &str, now: i64) -> Result<Vec<(String, State)>, String> {
    // Leases held on the remote, with the lease each commit message carries.
    let prefix = format!("refs/leases/{slug}");
    let listed = ls_remote_refs(&git.run(&["ls-remote", "origin", &format!("{prefix}/*")])?);
    let held_refs: Vec<(String, String)> = listed.into_iter().filter_map(|(sha, r)| id_from_ref(&r, &prefix).map(|id| (id, sha))).collect();
    let mut held: HashMap<String, Option<Lease>> = HashMap::new();
    if !held_refs.is_empty() {
        git.run(&["fetch", "--no-tags", "--prune", "origin", &format!("+{prefix}/*:refs/wf/leases/{slug}/*")])?;
        for (id, sha) in &held_refs {
            held.insert(id.clone(), parse_lease(&git.run(&["log", "-1", "--format=%B", sha])?));
        }
    }
    let failed_prefix = format!("refs/failed/{slug}");
    let failed: HashSet<String> =
        ls_remote_refs(&git.run(&["ls-remote", "origin", &format!("{failed_prefix}/*")])?).into_iter().filter_map(|(_, r)| id_from_ref(&r, &failed_prefix)).collect();
    // Base fetch: dev and the land ref.
    let heads = git.run(&["ls-remote", "--heads", "origin", "dev", land_ref])?;
    let has = |r: &str| heads.contains(&format!("refs/heads/{r}"));
    let mut refs: Vec<&str> = vec!["dev"];
    if land_ref != "dev" {
        refs.push(land_ref);
    }
    refs.retain(|r| has(r));
    if !refs.is_empty() {
        let specs: Vec<String> = refs.iter().map(|r| format!("+refs/heads/{r}:refs/remotes/origin/{r}")).collect();
        let mut args = vec!["fetch", "--no-tags", "--no-write-fetch-head", "origin"];
        args.extend(specs.iter().map(String::as_str));
        git.run(&args)?;
        let from = if has(land_ref) { land_ref } else { "dev" };
        git.run(&["rev-parse", "--verify", &format!("refs/remotes/origin/{from}")])?;
    }
    // Landed units: `Unit:` trailers on the land ref.
    let dev_ref = format!("origin/{land_ref}");
    let (code, out, err) = git.raw(&["log", &dev_ref, "--format=%(trailers:key=Unit,valueonly)"]);
    let done: HashSet<String> = if code != 0 {
        let lower = err.to_lowercase();
        if lower.contains("unknown revision") || lower.contains("bad revision") || lower.contains("ambiguous argument") {
            HashSet::new()
        } else {
            return Err(format!("git log {dev_ref} exited {code}: {}", js::trim(&err)));
        }
    } else {
        let want = format!("{slug}/");
        out.split('\n').map(js::trim).filter(|v| v.starts_with(&want) && v.len() > want.len()).map(|v| v[want.len()..].to_string()).collect()
    };
    Ok(units
        .iter()
        .map(|u| {
            let state = if done.contains(&u.id) {
                State::Done
            } else if let Some(lease) = held.get(&u.id) {
                match lease {
                    Some(l) => {
                        let beat = parse_time(&l.heartbeat_at);
                        State::Leased {
                            worker: l.worker.clone(),
                            age_ms: now - parse_time(&l.started_at).unwrap_or(now),
                            stale: beat.is_none_or(|b| now - b > LEASE_TTL_MS),
                        }
                    }
                    None => State::Leased { worker: "unknown".into(), age_ms: 0, stale: true },
                }
            } else if failed.contains(&u.id) {
                State::Failed
            } else {
                let waiting: Vec<String> = u.deps.iter().filter(|d| !done.contains(*d)).cloned().collect();
                if waiting.is_empty() { State::Runnable } else { State::Blocked(waiting) }
            };
            (u.id.clone(), state)
        })
        .collect())
}

fn fmt_age(ms: i64) -> String {
    let s = (((ms as f64) / 1000.0) + 0.5).floor().max(0.0) as i64;
    if s < 120 { format!("{s}s") } else { format!("{}m", ((s as f64 / 60.0) + 0.5).floor() as i64) }
}

fn format_status(rows: &[(String, State)]) -> String {
    let name = |s: &State| match s {
        State::Done => "done",
        State::Leased { .. } => "leased",
        State::Failed => "failed",
        State::Runnable => "runnable",
        State::Blocked(_) => "blocked",
    };
    let width = rows.iter().map(|(id, _)| id.encode_utf16().count()).max().unwrap_or(0).max(2);
    let pad = |s: &str, w: usize| format!("{s}{}", " ".repeat(w.saturating_sub(s.encode_utf16().count())));
    let mut counts: Vec<(&str, usize)> = Vec::new();
    let lines: Vec<String> = rows
        .iter()
        .map(|(id, state)| {
            match counts.iter_mut().find(|(k, _)| *k == name(state)) {
                Some(c) => c.1 += 1,
                None => counts.push((name(state), 1)),
            }
            let detail = match state {
                State::Leased { worker, age_ms, stale } => format!("by {worker}, {}{}", fmt_age(*age_ms), if *stale { " (stale)" } else { "" }),
                State::Blocked(w) => format!("waiting on {}", w.join(", ")),
                _ => String::new(),
            };
            format!("{}  {}  {detail}", pad(id, width), pad(name(state), 8)).trim_end().to_string()
        })
        .collect();
    let summary: Vec<String> = counts.iter().map(|(k, v)| format!("{v} {k}")).collect();
    format!("{}\n{}\n", lines.join("\n"), summary.join(", "))
}

/* ---------- entry points ---------- */

enum Cmd {
    Check,
    Status,
    Run(crate::worker_run::RunArgs),
}

fn repo_root(cwd: &Path) -> Result<PathBuf, String> {
    let (code, out, _) = Git { cwd }.raw(&["rev-parse", "--show-toplevel"]);
    if code != 0 {
        return Err(format!("not a git checkout: {}", cwd.display()));
    }
    Ok(PathBuf::from(js::trim(&out)))
}

fn execute(cmd: Cmd, slug: &str, site: &str, max: Option<&str>, land_ref: &str, repo: Option<&str>) -> i32 {
    let mut io = Io { stderr: String::new() };
    let u = usage();
    if slug.is_empty() || !slug.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        io.err(&format!("--plan <slug> is required\n{u}\n"));
        return io.finish(2);
    }
    if site != "sandbox" && site != "vm" {
        io.err("--site must be sandbox or vm\n");
        return io.finish(2);
    }
    if let Some(m) = max {
        if m.trim().parse::<f64>().ok().is_none_or(|n| n.fract() != 0.0 || n < 1.0) {
            io.err("--max must be a positive integer\n");
            return io.finish(2);
        }
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    let start = repo.map_or(cwd.clone(), |r| if Path::new(r).is_absolute() { PathBuf::from(r) } else { cwd.join(r) });
    let repo_dir = match repo_root(&start) {
        Ok(r) => r,
        Err(m) => {
            wf_kit::wf_err!("wf: {m}");
            return 1;
        }
    };
    let units_path = repo_dir.join("docs").join("plans").join(slug).join("units.jsonl");
    let Ok(text) = std::fs::read_to_string(&units_path) else {
        io.err(&format!("no units file: {}\n", units_path.display()));
        return io.finish(2);
    };
    let (units, errors) = parse_units(&text);
    if !errors.is_empty() {
        let lines: Vec<String> = errors.iter().map(|e| format!("  {}: {}", e.line.map_or("-".to_string(), |l| l.to_string()), e.reason)).collect();
        io.err(&format!("{}:\n{}\n", units_path.display(), lines.join("\n")));
        return io.finish(2);
    }
    match cmd {
        Cmd::Run(run_args) => crate::worker_run::run(slug, units, repo_dir, site, max, land_ref, run_args),
        Cmd::Check => {
            print!("{}", format_check(&units));
            0
        }
        Cmd::Status => match plan_status(&Git { cwd: &repo_dir }, slug, &units, land_ref, now_ms()) {
            Ok(rows) => {
                print!("{}", format_status(&rows));
                0
            }
            Err(m) => {
                wf_kit::wf_err!("wf: {m}");
                1
            }
        },
    }
}

/// `wf offload plan <slug>` and `wf offload plan <slug> --check`.
pub fn run_work(args: &[String]) -> Option<i32> {
    let p = parse(args, &["max", "site", "land-ref", "worktree-dir"], &["check", "once"])?;
    if p.positional.len() != 1 {
        return None;
    }
    if p.flags.iter().any(|f| f == "check") {
        return Some(execute(Cmd::Check, &p.positional[0], "sandbox", None, "dev", None));
    }
    let run = crate::worker_run::RunArgs { worktree_dir: p.opts.get("worktree-dir").cloned(), once: p.flags.iter().any(|f| f == "once") };
    Some(execute(
        Cmd::Run(run),
        &p.positional[0],
        p.opts.get("site").map_or("sandbox", String::as_str),
        p.opts.get("max").map(String::as_str),
        p.opts.get("land-ref").map_or("dev", String::as_str),
        None,
    ))
}

/// `wf offload plan --status <slug> [--land-ref <ref>]`.
pub fn run_status(args: &[String]) -> Option<i32> {
    let p = parse(args, &["land-ref"], &[])?;
    if p.positional.len() != 1 {
        return None;
    }
    let land = p.opts.get("land-ref").map_or("dev", String::as_str);
    Some(execute(Cmd::Status, &p.positional[0], "sandbox", None, land, None))
}

/// `wf offload worker check|status --plan <slug> ...`.
pub fn run_worker(args: &[String]) -> Option<i32> {
    let p = parse(args, &["plan", "max", "site", "land-ref", "worktree-dir", "repo"], &["once", "help"])?;
    if p.flags.iter().any(|f| f == "help") || p.positional.len() > 1 {
        return None;
    }
    let (cmd, slug) = match p.positional.first().map(String::as_str) {
        Some("check") => (Cmd::Check, p.opts.get("plan")),
        Some("status") => (Cmd::Status, p.opts.get("plan")),
        None | Some("run") => (Cmd::Run(crate::worker_run::RunArgs { worktree_dir: p.opts.get("worktree-dir").cloned(), once: p.flags.iter().any(|f| f == "once") }), p.opts.get("plan")),
        _ => return None,
    };
    Some(execute(
        cmd,
        slug.map_or("", String::as_str),
        p.opts.get("site").map_or("sandbox", String::as_str),
        p.opts.get("max").map(String::as_str),
        p.opts.get("land-ref").map_or("dev", String::as_str),
        p.opts.get("repo").map(String::as_str),
    ))
}
