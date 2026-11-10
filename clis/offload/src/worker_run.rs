//! `wf offload plan <slug>` / `wf offload worker --plan <slug>`: the git-state worker. Units from `docs/plans/<slug>/units.jsonl` are
//! claimed through lease refs on the remote, implemented in a throwaway worktree by the remote-chat-agent's `rca run`,
//! gated with the unit's own command, committed with a `Unit:` trailer and pushed to the land ref. Concurrency adapts
//! to memory, pressure and disk (AIMD), and a SIGTERM or SIGINT lets running units release their leases first.
use wf_kit::proc::hostname;
use wf_kit::js;
use crate::worker_plan::{id_from_ref, ls_remote_refs, parse_lease, Git, Io, Lease as PlanLease, Unit, LEASE_TTL_MS};
use rca_core::time::{iso, parse as parse_time};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

const HEARTBEAT_MS: i64 = 60_000;
const TICK_MS: i64 = 15_000;
const LAUNCH_SPACING_MS: i64 = 60_000;
const OUTPUT_CAP: usize = 256 * 1024;
const FAILED_OUTPUT_MAX_BYTES: usize = 8 * 1024;
const COOLDOWN_MS: i64 = 300_000;
const PROBE_INTERVAL_MS: i64 = 60_000;
const WARM_AGE_MS: i64 = 120_000;
const PROBE_FREE_MB: i64 = 3072;
const LOW_FREE_MB: i64 = 1536;
const PROBE_FREE_DISK_MB: i64 = 2048;
const LOW_FREE_DISK_MB: i64 = 1024;
const PSI_PROBE_MAX: f64 = 10.0;
const PSI_DECREASE: f64 = 20.0;
const MAX_SAFE: i64 = 9_007_199_254_740_991;

static SIGNALED: AtomicBool = AtomicBool::new(false);
static STDERR_TAIL: Mutex<String> = Mutex::new(String::new());

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now_ms() -> i64 {
    wf_kit::time::now_ms()
}

/// One JSON line on stderr, also kept for the failure message wf repeats.
fn emit_line(event: &str, meta: Vec<(&str, Value)>) {
    let mut m = Map::new();
    m.insert("at".into(), json!(iso(now_ms())));
    m.insert("event".into(), json!(event));
    for (k, v) in meta {
        m.insert(k.to_string(), v);
    }
    let line = format!("{}\n", Value::Object(m));
    eprint!("{line}");
    let mut tail = lock(&STDERR_TAIL);
    tail.push_str(&line);
    *tail = js::tail(&tail, 4096);
}

/* ---------- domain: leases, aimd, scheduler ---------- */

#[derive(Clone)]
struct Lease {
    worker: String,
    attempt: i64,
    model: String,
    started_at: String,
    heartbeat_at: String,
}

impl Lease {
    fn serialize(&self) -> String {
        json!({ "worker": self.worker, "attempt": self.attempt, "model": self.model, "startedAt": self.started_at, "heartbeatAt": self.heartbeat_at }).to_string()
    }
    fn expired(&self, now: i64) -> bool {
        parse_time(&self.heartbeat_at).is_none_or(|b| now - b > LEASE_TTL_MS)
    }
}

fn is_expired(l: &PlanLease, now: i64) -> bool {
    parse_time(&l.heartbeat_at).is_none_or(|b| now - b > LEASE_TTL_MS)
}

#[derive(Clone)]
struct Controller {
    limit: i64,
    ceiling: i64,
    clean_streak: i64,
    cooldown_until: Option<i64>,
    last_increase_at: Option<i64>,
}

enum Signal {
    Tick { free_mb: i64, psi: Option<f64>, free_disk_mb: Option<i64>, waiting_on_limit: bool, youngest_ms: Option<i64> },
    Finish { exit_code: i32 },
    Lost,
}

struct Change {
    from: i64,
    to: i64,
    reason: &'static str,
}

fn in_cooldown(c: &Controller, now: i64) -> bool {
    c.cooldown_until.is_some_and(|u| now < u)
}

fn decrease(mut next: Controller, now: i64, reason: &'static str) -> (Controller, Option<Change>) {
    let from = next.limit;
    let was_cooling = in_cooldown(&next, now);
    next.cooldown_until = Some(now + COOLDOWN_MS);
    if was_cooling {
        return (next, None);
    }
    let to = 1.max(next.limit / 2);
    next.limit = to;
    next.clean_streak = 0;
    let change = (from != to).then_some(Change { from, to, reason });
    (next, change)
}

fn observe(c: &Controller, s: &Signal, now: i64) -> (Controller, Option<Change>) {
    let mut next = c.clone();
    match s {
        Signal::Lost => decrease(next, now, "lost-unit"),
        Signal::Finish { exit_code } => {
            if *exit_code == 137 {
                return decrease(next, now, "oom-kill");
            }
            next.clean_streak += 1;
            (next, None)
        }
        Signal::Tick { free_mb, psi, free_disk_mb, waiting_on_limit, youngest_ms } => {
            if *free_mb < LOW_FREE_MB {
                return decrease(next, now, "low-memory");
            }
            if psi.is_some_and(|p| p > PSI_DECREASE) {
                return decrease(next, now, "memory-pressure");
            }
            if free_disk_mb.is_some_and(|d| d < LOW_FREE_DISK_MB) {
                return decrease(next, now, "low-disk");
            }
            let probe = *waiting_on_limit
                && *free_mb >= PROBE_FREE_MB
                && psi.unwrap_or(0.0) <= PSI_PROBE_MAX
                && free_disk_mb.unwrap_or(MAX_SAFE) >= PROBE_FREE_DISK_MB
                && !in_cooldown(&next, now)
                && next.last_increase_at.is_none_or(|t| now - t >= PROBE_INTERVAL_MS)
                && youngest_ms.is_none_or(|a| a >= WARM_AGE_MS)
                && next.limit < next.ceiling;
            if !probe {
                return (next, None);
            }
            let from = next.limit;
            next.limit = from + 1;
            next.last_increase_at = Some(now);
            let to = next.limit;
            (next, Some(Change { from, to, reason: "probe-up" }))
        }
    }
}

fn launch_slots(c: &Controller, running: i64, last_launch_at: Option<i64>, now: i64, spacing: i64) -> bool {
    if in_cooldown(c, now) || running >= c.limit {
        return false;
    }
    !last_launch_at.is_some_and(|t| now - t < spacing)
}

struct Caps {
    vm: bool,
    deployed: bool,
}

fn needs_satisfied(unit: &Unit, caps: &Caps) -> bool {
    match unit.needs.as_str() {
        "any" => true,
        "deployed" => caps.vm && caps.deployed,
        _ => false,
    }
}

fn runnable<'a>(units: &'a [Unit], done: &HashSet<String>, held: &HashSet<String>, caps: &Caps, free: usize, skip: &HashSet<String>) -> Vec<&'a Unit> {
    if free == 0 {
        return vec![];
    }
    let by_id: HashMap<&str, &Unit> = units.iter().map(|u| (u.id.as_str(), u)).collect();
    let mut busy: HashSet<&str> = HashSet::new();
    for id in held {
        if let Some(u) = by_id.get(id.as_str()) {
            busy.extend(u.files.iter().map(String::as_str));
        }
    }
    let mut out: Vec<&Unit> = vec![];
    for u in units {
        if out.len() >= free {
            break;
        }
        if done.contains(&u.id) || held.contains(&u.id) || skip.contains(&u.id) {
            continue;
        }
        if !u.deps.iter().all(|d| done.contains(d)) || !needs_satisfied(u, caps) {
            continue;
        }
        if u.files.iter().any(|f| busy.contains(f.as_str())) {
            continue;
        }
        busy.extend(u.files.iter().map(String::as_str));
        out.push(u);
    }
    out
}

/* ---------- process runner ---------- */

struct CmdResult {
    code: i32,
    output: String,
    stdout: String,
}

fn keep(buf: &mut String, add: &str) {
    buf.push_str(add);
    if buf.len() > OUTPUT_CAP * 2 {
        *buf = js::tail(buf, OUTPUT_CAP);
    }
}

fn run_cmd(program: &str, args: &[String], cwd: &Path, abort: &AtomicBool, timeout_ms: u64) -> CmdResult {
    let spawned = Command::new(program).args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0).spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => return CmdResult { code: 1, output: format!("\n{e}"), stdout: String::new() },
    };
    let shared = Arc::new(Mutex::new((String::new(), String::new())));
    let mut readers = vec![];
    for (is_out, pipe) in [(true, child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>)), (false, child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>))] {
        let Some(mut pipe) = pipe else { continue };
        let shared = Arc::clone(&shared);
        readers.push(thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let text = String::from_utf8_lossy(&buf[..n]).into_owned();
                let mut g = lock(&shared);
                keep(&mut g.0, &text);
                if is_out {
                    keep(&mut g.1, &text);
                }
            }
        }));
    }
    let started = Instant::now();
    let mut killed = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if !killed && (abort.load(Ordering::SeqCst) || started.elapsed() > Duration::from_millis(timeout_ms)) {
            // SIGTERM to the child's whole process group (it leads its own), so what it spawned goes down with it.
            let _ = nix::sys::signal::killpg(nix::unistd::Pid::from_raw(child.id() as i32), nix::sys::signal::Signal::SIGTERM);
            killed = true;
        }
        thread::sleep(Duration::from_millis(5));
    };
    for r in readers {
        let _ = r.join();
    }
    let g = lock(&shared);
    let code = status.map_or(1, |s| s.signal().map_or_else(|| s.code().unwrap_or(1), |sig| 128 + sig));
    CmdResult { code, output: js::tail(&g.0, OUTPUT_CAP), stdout: js::tail(&g.1, OUTPUT_CAP) }
}

/* ---------- git: leases and workspace ---------- */

struct GitCtx {
    repo: PathBuf,
    land_ref: String,
    identity: Mutex<Option<Vec<String>>>,
}

impl GitCtx {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let (code, out, err) = Git { cwd: &self.repo }.raw(args);
        if code != 0 {
            return Err(format!("git {} exited {code}: {}", args.join(" "), js::trim(&err)));
        }
        Ok(out)
    }
    fn run_in(&self, cwd: &Path, args: &[&str]) -> Result<String, String> {
        let (code, out, err) = Git { cwd }.raw(args);
        if code != 0 {
            return Err(format!("git {} exited {code}: {}", args.join(" "), js::trim(&err)));
        }
        Ok(out)
    }
}

const LEASE_IDENTITY: [&str; 4] = ["-c", "user.name=wf-lease", "-c", "user.email=wf-lease@localhost"];

fn rejected_re() -> Regex {
    Regex::new(r"(?i)\[rejected\]|\[remote rejected\]|already exists|stale info|fetch first|non-fast-forward|failed to (lock|update)").expect("re")
}

fn lease_ref(slug: &str, id: &str) -> String {
    format!("refs/leases/{slug}/{id}")
}

fn failed_ref(slug: &str, id: &str) -> String {
    format!("refs/failed/{slug}/{id}")
}

impl GitCtx {
    fn empty_commit(&self, message: &str) -> Result<String, String> {
        let tree = js::trim(&self.run(&["hash-object", "-t", "tree", "-w", "/dev/null"])?).to_string();
        let mut args: Vec<&str> = LEASE_IDENTITY.to_vec();
        args.extend(["commit-tree", &tree, "-m", message]);
        Ok(js::trim(&self.run(&args)?).to_string())
    }

    fn push(&self, args: &[&str], sha: &str) -> Result<String, String> {
        let mut full = vec!["push", "--porcelain"];
        full.extend_from_slice(args);
        let (code, out, err) = Git { cwd: &self.repo }.raw(&full);
        if code == 0 { Ok(sha.to_string()) } else { Err(js::trim(&format!("{out}{err}")).to_string()) }
    }

    fn claim(&self, slug: &str, id: &str, lease: &Lease) -> Result<String, (&'static str, String)> {
        let sha = self.empty_commit(&lease.serialize()).map_err(|e| ("error", e))?;
        let spec = format!("{sha}:{}", lease_ref(slug, id));
        self.push(&["origin", &spec], &sha).map_err(|detail| (if rejected_re().is_match(&detail) { "taken" } else { "error" }, detail))
    }

    /// Ok(sha) on success; Err((lost, detail)) otherwise.
    fn renew(&self, slug: &str, id: &str, previous: &str, lease: &Lease) -> Result<String, (bool, String)> {
        let r = lease_ref(slug, id);
        let sha = self.empty_commit(&lease.serialize()).map_err(|e| (false, e))?;
        let flag = format!("--force-with-lease={r}:{previous}");
        let spec = format!("{sha}:{r}");
        self.push(&[&flag, "origin", &spec], &sha).map_err(|detail| (Regex::new(r"(?i)rejected|stale info").expect("re").is_match(&detail), detail))
    }

    fn release(&self, slug: &str, id: &str, sha: &str) -> Result<String, String> {
        let r = lease_ref(slug, id);
        let flag = format!("--force-with-lease={r}:{sha}");
        let spec = format!(":{r}");
        self.push(&[&flag, "origin", &spec], sha)
    }

    fn list_refs(&self, prefix: &str) -> Result<Vec<(String, String)>, String> {
        let out = self.run(&["ls-remote", "origin", &format!("{prefix}/*")])?;
        Ok(ls_remote_refs(&out).into_iter().filter_map(|(sha, r)| id_from_ref(&r, prefix).map(|id| (id, sha))).collect())
    }

    fn list(&self, slug: &str) -> Result<Vec<(String, String, Option<PlanLease>)>, String> {
        let prefix = format!("refs/leases/{slug}");
        let refs = self.list_refs(&prefix)?;
        if refs.is_empty() {
            return Ok(vec![]);
        }
        self.run(&["fetch", "--no-tags", "--prune", "origin", &format!("+{prefix}/*:refs/wf/leases/{slug}/*")])?;
        refs.into_iter().map(|(id, sha)| Ok((id, sha.clone(), parse_lease(&self.run(&["log", "-1", "--format=%B", &sha])?)))).collect()
    }

    fn expire_stale(&self, slug: &str, now: i64) -> Result<Vec<String>, String> {
        let mut out = vec![];
        for (id, sha, lease) in self.list(slug)? {
            if lease.as_ref().is_none_or(|l| is_expired(l, now)) && self.release(slug, &id, &sha).is_ok() {
                out.push(id);
            }
        }
        Ok(out)
    }

    fn mark_failed(&self, slug: &str, id: &str, output: &str) -> Result<String, String> {
        let truncated = truncate_failed(output);
        let sha = self.empty_commit(if truncated.is_empty() { "(no output)" } else { &truncated })?;
        let spec = format!("{sha}:{}", failed_ref(slug, id));
        self.push(&["--force", "origin", &spec], &sha)
    }

    fn failed(&self, slug: &str) -> Result<HashSet<String>, String> {
        Ok(self.list_refs(&format!("refs/failed/{slug}"))?.into_iter().map(|(id, _)| id).collect())
    }

    fn done(&self, slug: &str) -> Result<HashSet<String>, String> {
        let dev_ref = format!("origin/{}", self.land_ref);
        let (code, out, err) = Git { cwd: &self.repo }.raw(&["log", &dev_ref, "--format=%(trailers:key=Unit,valueonly)"]);
        if code != 0 {
            let lower = err.to_lowercase();
            if lower.contains("unknown revision") || lower.contains("bad revision") || lower.contains("ambiguous argument") {
                return Ok(HashSet::new());
            }
            return Err(format!("git log {dev_ref} exited {code}: {}", js::trim(&err)));
        }
        let want = format!("{slug}/");
        Ok(out.split('\n').map(js::trim).filter(|v| v.starts_with(&want) && v.len() > want.len()).map(|v| v[want.len()..].to_string()).collect())
    }

    fn fetch_base(&self) -> Result<Option<String>, String> {
        let base = "dev";
        let land = self.land_ref.as_str();
        let heads = self.run(&["ls-remote", "--heads", "origin", base, land])?;
        let has = |r: &str| heads.contains(&format!("refs/heads/{r}"));
        let mut refs: Vec<&str> = vec![];
        for r in [base, land] {
            if !refs.contains(&r) && has(r) {
                refs.push(r);
            }
        }
        if refs.is_empty() {
            return Ok(None);
        }
        let specs: Vec<String> = refs.iter().map(|r| format!("+refs/heads/{r}:refs/remotes/origin/{r}")).collect();
        let mut args = vec!["fetch", "--no-tags", "--no-write-fetch-head", "origin"];
        args.extend(specs.iter().map(String::as_str));
        self.run(&args)?;
        let from = if has(land) { land } else { base };
        Ok(Some(js::trim(&self.run(&["rev-parse", "--verify", &format!("refs/remotes/origin/{from}")])?).to_string()))
    }

    fn ident(&self) -> Vec<String> {
        let mut g = lock(&self.identity);
        g.get_or_insert_with(|| {
            let (code, out, _) = Git { cwd: &self.repo }.raw(&["config", "user.email"]);
            if code == 0 && !js::trim(&out).is_empty() {
                vec![]
            } else {
                ["-c", "user.name=wf offload plan", "-c", "user.email=wf-work@localhost"].map(String::from).to_vec()
            }
        })
        .clone()
    }

    fn add_worktree(&self, path: &Path, sha: &str) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        self.run(&["worktree", "add", "--detach", "--quiet", &path.to_string_lossy(), sha]).map(|_| ())
    }

    fn remove_worktree(&self, path: &Path) -> Result<(), String> {
        let p = path.to_string_lossy();
        let _ = Git { cwd: &self.repo }.raw(&["worktree", "remove", "--force", &p]);
        let _ = std::fs::remove_dir_all(path);
        let _ = Git { cwd: &self.repo }.raw(&["worktree", "prune"]);
        Ok(())
    }

    fn has_changes(&self, cwd: &Path) -> Result<bool, String> {
        Ok(!js::trim(&self.run_in(cwd, &["status", "--porcelain"])?).is_empty())
    }

    fn commit(&self, cwd: &Path, message: &str) -> Result<(), String> {
        self.run_in(cwd, &["add", "-A"])?;
        let ident = self.ident();
        let mut args: Vec<&str> = ident.iter().map(String::as_str).collect();
        args.extend(["commit", "-q", "-m", message]);
        self.run_in(cwd, &args)?;
        self.run_in(cwd, &["rev-parse", "HEAD"]).map(|_| ())
    }

    fn rebase(&self, cwd: &Path, onto: &str) -> Result<bool, String> {
        let ident = self.ident();
        let mut args: Vec<&str> = ident.iter().map(String::as_str).collect();
        args.extend(["rebase", onto]);
        let (code, _, _) = Git { cwd }.raw(&args);
        if code == 0 {
            return Ok(true);
        }
        let _ = Git { cwd }.raw(&["rebase", "--abort"]);
        Ok(false)
    }

    fn changed_files(&self, from: &str, to: &str, files: &[String]) -> Result<Vec<String>, String> {
        let mut args = vec!["diff", "--name-only", from, to, "--"];
        args.extend(files.iter().map(String::as_str));
        Ok(self.run(&args)?.split('\n').filter(|l| !l.is_empty()).map(str::to_string).collect())
    }

    fn land_push(&self, cwd: &Path) -> bool {
        let (code, _, _) = Git { cwd }.raw(&["push", "--porcelain", "origin", &format!("HEAD:refs/heads/{}", self.land_ref)]);
        code == 0
    }
}

fn truncate_failed(output: &str) -> String {
    let bytes = output.as_bytes();
    if bytes.len() <= FAILED_OUTPUT_MAX_BYTES {
        return output.to_string();
    }
    let s = String::from_utf8_lossy(&bytes[bytes.len() - FAILED_OUTPUT_MAX_BYTES..]).into_owned();
    s.trim_start_matches('\u{FFFD}').to_string()
}

/* ---------- implementer, install, gate, sysinfo ---------- */

fn build_prompt(unit: &Unit, feedback: Option<&str>) -> String {
    let retry = match feedback {
        Some(f) if !f.is_empty() => format!("\n\nPrevious attempt failed. Fix this:\n{}", js::tail(f, 4000)),
        _ => String::new(),
    };
    format!("{}\n\nFILES: {}\nVERIFY: {}{retry}", unit.brief, unit.files.join(", "), unit.gate)
}

struct Implemented {
    ok: bool,
    model: String,
    detail: String,
    exit_code: i32,
}

/// The model a finished `wf offload ask` names on its stderr meta line (`[served by <model> | ...]`).
fn served_by(output: &str) -> Option<String> {
    let at = output.rfind("served by ")? + "served by ".len();
    output[at..].split(|c: char| c.is_whitespace() || c == ']' || c == '|').next().filter(|m| !m.is_empty()).map(str::to_string)
}

/// One unit's implementer: `wf offload ask` on the rca service, in the unit's worktree, as a child of this process so an abort or
/// the timeout takes it down with the group.
fn implement(repo_dir: &Path, unit: &Unit, cwd: &Path, feedback: Option<&str>, abort: &AtomicBool) -> Implemented {
    let exe = std::env::current_exe().map_or_else(|_| "wf".to_string(), |p| p.to_string_lossy().into_owned());
    let args: Vec<String> = ["offload", "ask", "--prompt"]
        .iter()
        .map(|s| s.to_string())
        .chain([build_prompt(unit, feedback), "--cwd".into(), cwd.to_string_lossy().into_owned(), "--caller".into(), "worker".into(), "--auto".into(), "--shape".into(), "implementer:unit".into()])
        .collect();
    // The service has no checkout: the unit's existing files go along, and what the run changed comes back into the worktree.
    let existing: Vec<&str> = unit.files.iter().map(String::as_str).filter(|f| cwd.join(f).is_file()).collect();
    let args: Vec<String> = if existing.is_empty() { args } else { args.into_iter().chain(["--files".to_string(), existing.join(",")]).collect() };
    let r = run_cmd(&exe, &args, repo_dir, abort, 45 * 60_000);
    let ok = r.code == 0;
    let model = served_by(&r.output).unwrap_or_else(|| "dispatch".into());
    let detail = js::tail(if ok { &r.stdout } else { &r.output }, 4000);
    Implemented { ok, model, detail, exit_code: r.code }
}

fn gate(cmd: &str, cwd: &Path, abort: &AtomicBool) -> CmdResult {
    run_cmd("sh", &["-c".to_string(), cmd.to_string()], cwd, abort, 20 * 60_000)
}

struct Sysinfo {
    free_mb: i64,
    psi: Option<f64>,
    free_disk_mb: Option<i64>,
}

fn sysinfo(dir: &Path) -> Sysinfo {
    let mem = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let psi_text = std::fs::read_to_string("/proc/pressure/memory").unwrap_or_default();
    let free_mb = Regex::new(r"(?m)^MemAvailable:\s+(\d+)\s+kB").ok().and_then(|re| re.captures(&mem)).and_then(|c| c[1].parse::<i64>().ok()).map_or(MAX_SAFE, |kb| kb / 1024);
    let psi = Regex::new(r"(?m)^some\s.*\bavg10=([\d.]+)").ok().and_then(|re| re.captures(&psi_text)).and_then(|c| c[1].parse::<f64>().ok());
    let df = Command::new("df").args(["-Pk"]).arg(dir).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let free_disk_mb = js::trim(&df).split('\n').nth(1).and_then(|row| js::trim(row).split_whitespace().nth(3).map(str::to_string)).and_then(|kb| kb.parse::<i64>().ok()).map(|kb| kb / 1024);
    Sysinfo { free_mb, psi, free_disk_mb }
}

/* ---------- the worker ---------- */

struct Opts {
    slug: String,
    units: Vec<Unit>,
    vm: bool,
    worker: String,
    worktree_dir: PathBuf,
    once: bool,
}

struct Settings {
    tick_ms: i64,
    heartbeat_ms: i64,
    launch_spacing_ms: i64,
    gate_attempts: u32,
    land_retries: u32,
    max_local_releases: u32,
}

struct RunInner {
    lease: Lease,
    lease_sha: String,
    model: String,
    committed: bool,
    lost: bool,
}

struct Running {
    unit: Unit,
    started_at: i64,
    inner: Mutex<RunInner>,
    abort: AtomicBool,
    lease_ops: Mutex<()>,
}

struct St {
    running: HashMap<String, Arc<Running>>,
    local_releases: HashMap<String, u32>,
    landed: Vec<String>,
    failed: Vec<String>,
    released: Vec<String>,
    controller: Controller,
    last_launch_at: Option<i64>,
}

struct Run {
    opts: Opts,
    settings: Settings,
    git: GitCtx,
    repo_dir: PathBuf,
    stop: AtomicBool,
    repo_lock: Mutex<()>,
    st: Mutex<St>,
}

#[derive(PartialEq)]
enum Outcome {
    Landed,
    Failed,
    Released,
    Lost,
    Stopped,
}

enum PipeErr {
    Stopped,
    Fail(String),
}

impl From<String> for PipeErr {
    fn from(s: String) -> Self {
        PipeErr::Fail(s)
    }
}

impl Run {
    fn emit_unit(&self, event: &str, unit: &Running, extra: Vec<(&str, Value)>) {
        let model = lock(&unit.inner).model.clone();
        let mut meta = vec![
            ("slug", json!(self.opts.slug)),
            ("id", json!(unit.unit.id)),
            ("worker", json!(self.opts.worker)),
            ("model", json!(model)),
            ("ms", json!(now_ms() - unit.started_at)),
        ];
        meta.extend(extra);
        emit_line(event, meta);
    }

    fn feed(&self, signal: &Signal) {
        let (change, now) = {
            let mut st = lock(&self.st);
            let now = now_ms();
            let (c, change) = observe(&st.controller, signal, now);
            st.controller = c;
            (change, now)
        };
        let _ = now;
        if let Some(c) = change {
            emit_line("worker.aimd.changed", vec![("slug", json!(self.opts.slug)), ("worker", json!(self.opts.worker)), ("from", json!(c.from)), ("to", json!(c.to)), ("reason", json!(c.reason))]);
        }
    }

    fn release_lease(&self, unit: &Running) -> Result<String, String> {
        let _g = lock(&unit.lease_ops);
        let sha = lock(&unit.inner).lease_sha.clone();
        self.git.release(&self.opts.slug, &unit.unit.id, &sha)
    }

    fn throw_if_stopped(&self, unit: &Running) -> Result<(), PipeErr> {
        if unit.abort.load(Ordering::SeqCst) { Err(PipeErr::Stopped) } else { Ok(()) }
    }

    fn worktree_path(&self, unit: &Running) -> PathBuf {
        let name = format!("{}--{}--{}", self.opts.slug, unit.unit.id, unit.started_at);
        let clean: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') { c } else { '_' }).collect();
        self.opts.worktree_dir.join(clean)
    }

    fn implement_until_gated(&self, unit: &Running, path: &Path) -> Result<Result<(), (i32, String)>, PipeErr> {
        let mut feedback: Option<String> = None;
        let mut output = String::new();
        let mut exit_code = 1;
        for _attempt in 1..=self.settings.gate_attempts {
            self.throw_if_stopped(unit)?;
            let imp = implement(&self.repo_dir, &unit.unit, path, feedback.as_deref(), &unit.abort);
            if !imp.model.is_empty() {
                lock(&unit.inner).model = imp.model.clone();
            }
            self.throw_if_stopped(unit)?;
            if !imp.ok {
                output = format!("implementer failed: {}", imp.detail);
                exit_code = imp.exit_code;
                feedback = Some(output.clone());
                continue;
            }
            if !self.git.has_changes(path)? {
                output = "implementer wrote nothing".into();
                feedback = Some(format!("{output}; make the edit now"));
                continue;
            }
            let g = gate(&unit.unit.gate, path, &unit.abort);
            self.throw_if_stopped(unit)?;
            exit_code = g.code;
            if g.code == 0 {
                return Ok(Ok(()));
            }
            output = g.output.clone();
            feedback = Some(g.output);
        }
        Ok(Err((exit_code, output)))
    }

    fn land_unit(&self, unit: &Running, path: &Path, start_base: &str) -> Result<bool, PipeErr> {
        let mut base = start_base.to_string();
        for _ in 0..=self.settings.land_retries {
            let head = {
                let _g = lock(&self.repo_lock);
                self.git.fetch_base()?
            };
            if let Some(head) = head.filter(|h| *h != base) {
                if !self.git.rebase(path, &head)? {
                    return Ok(false);
                }
                let touched = self.git.changed_files(&base, &head, &unit.unit.files)?;
                base = head;
                if !touched.is_empty() && gate(&unit.unit.gate, path, &unit.abort).code != 0 {
                    return Ok(false);
                }
            }
            if self.git.land_push(path) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn pipeline_body(&self, unit: &Running, path: &Path, added: &mut bool) -> Result<Outcome, PipeErr> {
        let base = {
            let _g = lock(&self.repo_lock);
            let base = self.git.fetch_base()?.ok_or_else(|| PipeErr::Fail("no base ref on the remote".into()))?;
            self.git.add_worktree(path, &base)?;
            base
        };
        *added = true;
        self.throw_if_stopped(unit)?;
        let install = run_cmd("pnpm", &["install".into(), "--prefer-offline".into(), "--frozen-lockfile".into()], path, &unit.abort, 15 * 60_000);
        self.throw_if_stopped(unit)?;
        if install.code != 0 {
            self.emit_unit("worker.unit.released", unit, vec![("reason", json!("install-failed"))]);
            let _ = self.release_lease(unit);
            return Ok(Outcome::Released);
        }
        if let Err((exit_code, output)) = self.implement_until_gated(unit, path)? {
            self.feed(&Signal::Finish { exit_code });
            let _ = self.git.mark_failed(&self.opts.slug, &unit.unit.id, &output);
            let _ = self.release_lease(unit);
            self.emit_unit("worker.unit.failed", unit, vec![("exitCode", json!(exit_code))]);
            return Ok(Outcome::Failed);
        }
        self.emit_unit("worker.unit.gated", unit, vec![]);
        if lock(&unit.inner).lost {
            return Ok(Outcome::Lost);
        }
        self.git.commit(path, &format!("{}\n\nUnit: {}/{}", unit.unit.title, self.opts.slug, unit.unit.id))?;
        lock(&unit.inner).committed = true;
        let landed = self.land_unit(unit, path, &base)?;
        self.feed(&Signal::Finish { exit_code: 0 });
        let _ = self.release_lease(unit);
        if landed {
            self.emit_unit("worker.unit.landed", unit, vec![]);
            return Ok(Outcome::Landed);
        }
        self.emit_unit("worker.unit.released", unit, vec![("reason", json!("land-conflict"))]);
        Ok(Outcome::Released)
    }

    fn pipeline(&self, unit: &Running) -> Outcome {
        let path = self.worktree_path(unit);
        let mut added = false;
        let result = self.pipeline_body(unit, &path, &mut added);
        let outcome = match result {
            Ok(o) => o,
            Err(e) => {
                if lock(&unit.inner).lost {
                    Outcome::Lost
                } else {
                    let _ = self.release_lease(unit);
                    if matches!(e, PipeErr::Stopped) || self.stop.load(Ordering::SeqCst) {
                        self.emit_unit("worker.unit.released", unit, vec![("reason", json!("stopping"))]);
                        Outcome::Stopped
                    } else {
                        let PipeErr::Fail(msg) = e else { unreachable!() };
                        self.emit_unit("worker.unit.released", unit, vec![("reason", json!("error")), ("error", json!(msg))]);
                        Outcome::Released
                    }
                }
            }
        };
        if added {
            let removed = {
                let _g = lock(&self.repo_lock);
                self.git.remove_worktree(&path)
            };
            if let Err(e) = removed {
                emit_line("worker.error", vec![("slug", json!(self.opts.slug)), ("id", json!(unit.unit.id)), ("op", json!("worktree-remove")), ("error", json!(e))]);
            }
        }
        outcome
    }

    fn record(&self, id: &str, outcome: &Outcome) {
        let mut st = lock(&self.st);
        st.running.remove(id);
        match outcome {
            Outcome::Landed => st.landed.push(id.to_string()),
            Outcome::Failed => st.failed.push(id.to_string()),
            Outcome::Released => {
                st.released.push(id.to_string());
                *st.local_releases.entry(id.to_string()).or_insert(0) += 1;
            }
            _ => {}
        }
    }

    fn renew_lease(&self, unit: &Running, now: i64) {
        let (lease, sha, lost) = {
            let g = lock(&unit.inner);
            (g.lease.clone(), g.lease_sha.clone(), g.lost)
        };
        if lost || now - parse_time(&lease.heartbeat_at).unwrap_or(0) < self.settings.heartbeat_ms {
            return;
        }
        let mut next = lease.clone();
        next.heartbeat_at = iso(now);
        let renewed = {
            let _g = lock(&unit.lease_ops);
            self.git.renew(&self.opts.slug, &unit.unit.id, &sha, &next)
        };
        match renewed {
            Ok(new_sha) => {
                let mut g = lock(&unit.inner);
                g.lease = next;
                g.lease_sha = new_sha;
            }
            Err((was_lost, detail)) => {
                if !was_lost && !lease.expired(now) {
                    return;
                }
                let committed = {
                    let mut g = lock(&unit.inner);
                    g.lost = true;
                    g.committed
                };
                if !committed {
                    unit.abort.store(true, Ordering::SeqCst);
                }
                self.feed(&Signal::Lost);
                self.emit_unit("worker.unit.lost", unit, vec![("detail", json!(detail))]);
            }
        }
    }

    fn renew_leases(&self, now: i64) {
        let units: Vec<Arc<Running>> = lock(&self.st).running.values().cloned().collect();
        thread::scope(|s| {
            for u in &units {
                s.spawn(|| self.renew_lease(u, now));
            }
        });
    }

    fn on_stop_signal(&self) {
        if self.stop.swap(true, Ordering::SeqCst) {
            return;
        }
        emit_line("worker.stopping", vec![("slug", json!(self.opts.slug)), ("worker", json!(self.opts.worker))]);
        for u in lock(&self.st).running.values() {
            if !lock(&u.inner).committed {
                u.abort.store(true, Ordering::SeqCst);
            }
        }
    }

    fn poll_signal(&self) {
        if SIGNALED.load(Ordering::SeqCst) {
            self.on_stop_signal();
        }
    }

    /// Sleeps `ms`, waking early once the worker is told to stop.
    fn nap(&self, ms: i64) {
        let end = Instant::now() + Duration::from_millis(ms.max(0) as u64);
        while Instant::now() < end {
            self.poll_signal();
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn caps(&self) -> Caps {
        Caps { vm: self.opts.vm, deployed: self.opts.vm }
    }

    fn tick(self: &Arc<Self>) -> Result<(bool, Vec<thread::JoinHandle<()>>), String> {
        let now = now_ms();
        self.renew_leases(now);
        let slug = &self.opts.slug;
        let (expired, held, failed, done) = {
            let _g = lock(&self.repo_lock);
            let expired = self.git.expire_stale(slug, now)?;
            let held: HashSet<String> = self.git.list(slug)?.into_iter().map(|(id, _, _)| id).collect();
            let failed = self.git.failed(slug)?;
            self.git.fetch_base()?;
            let done = self.git.done(slug)?;
            (expired, held, failed, done)
        };
        for id in &expired {
            emit_line("worker.unit.expired", vec![("slug", json!(slug)), ("id", json!(id)), ("worker", json!(self.opts.worker)), ("model", json!("")), ("ms", json!(0))]);
        }
        let (skip, running_len, ages, controller, last_launch) = {
            let st = lock(&self.st);
            let mut skip = failed.clone();
            skip.extend(st.running.keys().cloned());
            for (id, n) in &st.local_releases {
                if *n >= self.settings.max_local_releases {
                    skip.insert(id.clone());
                }
            }
            let ages: Vec<i64> = st.running.values().map(|r| now - r.started_at).collect();
            (skip, st.running.len() as i64, ages, st.controller.clone(), st.last_launch_at)
        };
        let caps = self.caps();
        let waiting = runnable(&self.opts.units, &done, &held, &caps, usize::MAX, &skip).len();
        if self.opts.once && running_len == 0 && waiting == 0 {
            return Ok((false, vec![]));
        }
        let si = sysinfo(&self.opts.worktree_dir);
        self.feed(&Signal::Tick {
            free_mb: si.free_mb,
            psi: si.psi,
            free_disk_mb: si.free_disk_mb,
            waiting_on_limit: waiting > 0 && running_len >= controller.limit,
            youngest_ms: ages.iter().copied().min(),
        });
        let controller = lock(&self.st).controller.clone();
        let mut handles = vec![];
        if launch_slots(&controller, running_len, last_launch, now, self.settings.launch_spacing_ms) {
            let pick: Vec<Unit> = runnable(&self.opts.units, &done, &held, &caps, 1, &skip).into_iter().cloned().collect();
            handles = self.claim_and_start(pick);
        }
        Ok((true, handles))
    }

    fn claim_and_start(self: &Arc<Self>, units: Vec<Unit>) -> Vec<thread::JoinHandle<()>> {
        let mut handles = vec![];
        for unit in units {
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let at = iso(now_ms());
            let lease = Lease { worker: self.opts.worker.clone(), attempt: 1, model: "rca".into(), started_at: at.clone(), heartbeat_at: at };
            match self.git.claim(&self.opts.slug, &unit.id, &lease) {
                Err((reason, _)) => {
                    emit_line("worker.claim.lost", vec![("slug", json!(self.opts.slug)), ("id", json!(unit.id)), ("worker", json!(self.opts.worker)), ("reason", json!(reason))]);
                }
                Ok(sha) => {
                    lock(&self.st).last_launch_at = Some(now_ms());
                    let running = Arc::new(Running {
                        unit: unit.clone(),
                        started_at: now_ms(),
                        inner: Mutex::new(RunInner { model: lease.model.clone(), lease, lease_sha: sha, committed: false, lost: false }),
                        abort: AtomicBool::new(false),
                        lease_ops: Mutex::new(()),
                    });
                    lock(&self.st).running.insert(unit.id.clone(), Arc::clone(&running));
                    self.emit_unit("worker.unit.claimed", &running, vec![]);
                    let me = Arc::clone(self);
                    handles.push(thread::spawn(move || {
                        let outcome = me.pipeline(&running);
                        me.record(&running.unit.id, &outcome);
                    }));
                }
            }
        }
        handles
    }

    fn schedule_until_stopped(self: &Arc<Self>, handles: &mut Vec<thread::JoinHandle<()>>) {
        while !self.stop.load(Ordering::SeqCst) {
            self.poll_signal();
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            match self.tick() {
                Ok((false, _)) => return,
                Ok((true, h)) => handles.extend(h),
                Err(error) => emit_line("worker.error", vec![("slug", json!(self.opts.slug)), ("worker", json!(self.opts.worker)), ("op", json!("tick")), ("error", json!(error))]),
            }
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            self.nap(self.settings.tick_ms);
        }
    }

    fn drain(&self, handles: Vec<thread::JoinHandle<()>>) {
        let mut last = now_ms();
        while !lock(&self.st).running.is_empty() {
            self.poll_signal();
            thread::sleep(Duration::from_millis(5));
            if !self.stop.load(Ordering::SeqCst) && now_ms() - last >= self.settings.tick_ms {
                self.renew_leases(now_ms());
                last = now_ms();
            }
        }
        for h in handles {
            let _ = h.join();
        }
    }
}

fn env_ms(name: &str) -> Option<i64> {
    let raw = std::env::var(name).ok().filter(|s| !s.is_empty())?;
    let n: f64 = raw.trim().parse().ok()?;
    (n.is_finite() && n >= 0.0).then_some(n as i64)
}

/// `path.resolve`: absolute, with `.` and `..` folded away.
fn resolve(p: &str, cwd: &Path) -> PathBuf {
    let joined = if Path::new(p).is_absolute() { PathBuf::from(p) } else { cwd.join(p) };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn is_inside(child: &Path, parent: &Path) -> bool {
    child.starts_with(parent)
}

pub(crate) struct RunArgs {
    pub(crate) worktree_dir: Option<String>,
    pub(crate) once: bool,
}

/// Runs the plan to completion or until stopped; returns the exit code.
pub(crate) fn run(slug: &str, units: Vec<Unit>, repo_dir: PathBuf, site: &str, max: Option<&str>, land_ref: &str, args: RunArgs) -> i32 {
    let cwd = std::env::current_dir().unwrap_or_default();
    let default_dir = PathBuf::from(std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".into())).join("wf-worktrees").join(slug);
    let worktree_dir = args.worktree_dir.as_deref().map_or_else(|| resolve(&default_dir.to_string_lossy(), &cwd), |d| resolve(d, &cwd));
    if is_inside(&worktree_dir, &repo_dir) {
        let mut io = Io { stderr: String::new() };
        io.err(&format!("--worktree-dir must be outside the repo: {}\n", worktree_dir.display()));
        return io.finish(2);
    }
    let vm = site == "vm";
    let mut controller = Controller { limit: 2, ceiling: if vm { 4 } else { 6 }, clean_streak: 0, cooldown_until: None, last_increase_at: None };
    if let Some(m) = max.and_then(|m| m.trim().parse::<f64>().ok()) {
        let m = 1.max(m as i64);
        controller.ceiling = controller.ceiling.min(m);
        controller.limit = controller.limit.min(m);
    }
    let worker = format!("{site}:{}:{}", hostname(), std::process::id());
    let run = Arc::new(Run {
        git: GitCtx { repo: repo_dir.clone(), land_ref: land_ref.to_string(), identity: Mutex::new(None) },
        repo_dir,
        settings: Settings {
            tick_ms: env_ms("WF_WORKER_TICK_MS").unwrap_or(TICK_MS),
            heartbeat_ms: env_ms("WF_WORKER_HEARTBEAT_MS").unwrap_or(HEARTBEAT_MS),
            launch_spacing_ms: env_ms("WF_WORKER_LAUNCH_SPACING_MS").unwrap_or(LAUNCH_SPACING_MS),
            gate_attempts: 2,
            land_retries: 3,
            max_local_releases: 3,
        },
        opts: Opts { slug: slug.to_string(), units, vm, worker, worktree_dir, once: args.once },
        stop: AtomicBool::new(false),
        repo_lock: Mutex::new(()),
        st: Mutex::new(St { running: HashMap::new(), local_releases: HashMap::new(), landed: vec![], failed: vec![], released: vec![], controller, last_launch_at: None }),
    });
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        // SAFETY: the action touches an atomic and may call `_exit`, both async-signal-safe: a second signal ends the process at once, like the TS host.
        let _ = unsafe {
            signal_hook::low_level::register(signal, move || {
                if SIGNALED.swap(true, Ordering::SeqCst) {
                    signal_hook::low_level::exit(128 + signal);
                }
            })
        };
    }
    let mut handles = vec![];
    run.schedule_until_stopped(&mut handles);
    run.drain(handles);
    let code = if run.stop.load(Ordering::SeqCst) { 143 } else { 0 };
    let st = lock(&run.st);
    let list = |v: &Vec<String>| format!("{} [{}]", v.len(), v.join(" "));
    println!("landed {}, failed {}, released {}", list(&st.landed), list(&st.failed), list(&st.released));
    if code != 0 {
        let tail = lock(&STDERR_TAIL).clone();
        let tail = if tail.is_empty() { format!("worker command failed with exit code {code}") } else { tail };
        wf_kit::wf_err!("wf: {tail}");
    }
    code
}
