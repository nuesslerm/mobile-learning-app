//! `wf ci preflight`: the first job of a CI run, which stops the run in seconds, with one verdict naming what is wrong and what to do,
//! when anything the run needs from start to finish is not live and valid: a clean workspace, the pinned toolchain, the environment,
//! the secrets, the build cache (links resolve, a probe object is written and read back), the ledger database, the services the run
//! deploys through and reads from, the runner's disk and memory, and the GitHub token. Found at minute zero, none of them
//! costs a run that discovers it at minute twenty (a refused cache write once cost every job of a run its cache).
use crate::ci_comment::{json_of, send, var, Env};
use blob_cache::adapters::OciParStore;
use blob_cache::domain::{Budget, Kind};
use blob_cache::ports::BlobStore;
use blob_cache::{Cache, Writes};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { values: &["skip"], flags: &[], positional: Positional::None, ..Spec::NONE };
const HEALTH_TIMEOUT: Duration = Duration::from_secs(8);
const RUNNER_COMPOSE: &str = "infra/oracle-cloud/ci-runner/docker-compose.prod.yml";
const MIN_DISK_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const MIN_MEMORY_KB: u64 = 1024 * 1024;
const MIN_API_CALLS: i64 = 200;

pub struct Check {
    pub name: String,
    /// A failed check that is not required is a warning: the run goes on.
    pub required: bool,
    pub ok: bool,
    pub detail: String,
    pub next: String,
}

fn pass(name: impl Into<String>, detail: impl Into<String>) -> Check {
    Check { name: name.into(), required: true, ok: true, detail: detail.into(), next: String::new() }
}

fn fail(name: impl Into<String>, detail: impl Into<String>, next: impl Into<String>) -> Check {
    Check { name: name.into(), required: true, ok: false, detail: detail.into(), next: next.into() }
}

fn warn(mut check: Check) -> Check {
    check.required = false;
    check
}

/// The workspace has no change the build cache's write guard would refuse.
fn workspace() -> Check {
    let root = wf_kit::root::repo_root();
    let Some(status) = wf_kit::proc::git_out(&root, &["status", "--porcelain=v1", "-uall"]) else { return fail("workspace", "git status failed", "check the checkout") };
    let files: Vec<&str> = status.lines().map(|l| l.get(3..).unwrap_or(l)).collect();
    if files.is_empty() {
        return pass("workspace", "clean");
    }
    fail("workspace", format!("{} file(s) differ from the commit: {}", files.len(), files.iter().take(5).copied().collect::<Vec<_>>().join(", ")), "git clean -fd on the runner (the build cache refuses writes from a dirty tree)")
}

fn toolchain() -> Check {
    let pins = wf_bucket::pins::pins(&wf_kit::root::repo_root());
    let wrong: Vec<String> = pins.iter().filter(|p| !p.ok()).map(|p| format!("{} {} (pinned {})", p.tool, if p.actual.is_empty() { "missing" } else { &p.actual }, p.pinned)).collect();
    if wrong.is_empty() { pass("toolchain", format!("{} pinned tools match", pins.len())) } else { fail("toolchain", wrong.join(", "), "install the versions mise.toml pins: Nx hashes them, so other versions never share a cache entry") }
}

fn environment(env: &[&str]) -> Check {
    let missing: Vec<&&str> = env.iter().filter(|k| var(k).is_none()).collect();
    if missing.is_empty() { pass("env", format!("{} variables set", env.len())) } else { fail("env", format!("not set: {}", missing.iter().map(|k| **k).collect::<Vec<_>>().join(", ")), "the workflow's env block names them") }
}

fn secrets() -> Check {
    match secrets::verify_for("ci", &[]) {
        Ok(()) => pass("secrets", "every secret of the ci group is present and usable"),
        Err(why) => fail("secrets", why.lines().take(6).collect::<Vec<_>>().join("; "), "wf secret verify --for ci; the vault read token or the secret itself is bad"),
    }
}

/// The build cache's two links resolve, and a probe object is written and read back through the write link.
fn build_cache() -> Check {
    if var("WF_BLOB_CACHE").as_deref() == Some("off") {
        return pass("build-cache", "switched off (WF_BLOB_CACHE=off)");
    }
    if let Some(why) = secrets::write_urls_refused() {
        return read_only_build_cache(&why);
    }
    let links = secrets::load_values(&["BUILD_CACHE_READ_PAR_URL", "BUILD_CACHE_WRITE_PAR_URL"]);
    let link = |k: &str| links.get(k).cloned().flatten();
    let (Some(read), Some(write)) = (link("BUILD_CACHE_READ_PAR_URL"), link("BUILD_CACHE_WRITE_PAR_URL")) else {
        return fail("build-cache", "a build cache link is not in the environment or the vault", "wf secret get BUILD_CACHE_READ_PAR_URL BUILD_CACHE_WRITE_PAR_URL; mint a new link if it expired");
    };
    let cache = Cache::open(OciParStore::new(&read, Some(&write)), Budget::committed().clone(), Writes::Allowed);
    let name = format!("preflight-{}", var("GITHUB_RUN_ID").unwrap_or_else(|| "local".into()));
    let Ok(key) = cache.key(Kind::Probe, &name) else { return fail("build-cache", "the probe key was refused", "a bug in the cache's key rules") };
    let store = OciParStore::new(&read, Some(&write));
    let body = format!("preflight {}", wf_kit::time::now_ms());
    if let Err(why) = store.put(&key, body.as_bytes(), false) {
        return fail("build-cache", format!("writing the probe failed: {why}"), "the write link is expired, revoked or the bucket is unreachable: mint a new BUILD_CACHE_WRITE_PAR_URL");
    }
    match store.get(&key) {
        Ok(Some(back)) if back == body.as_bytes() => pass("build-cache", "a probe object was written and read back"),
        Ok(_) => fail("build-cache", "the probe was written but not read back as written", "the read link points at another bucket or prefix than the write link"),
        Err(why) => fail("build-cache", format!("reading the probe failed: {why}"), "the read link is expired or revoked: mint a new BUILD_CACHE_READ_PAR_URL"),
    }
}

/// An event that may not resolve the write URL reads only, so the probe is a read of an object that need not exist.
fn read_only_build_cache(why: &str) -> Check {
    let Some(read) = secrets::load_values(&["BUILD_CACHE_READ_PAR_URL"]).get("BUILD_CACHE_READ_PAR_URL").cloned().flatten() else {
        return fail("build-cache", "the build cache read link is not in the environment or the vault", "set BUILD_CACHE_READ_PAR_URL or mint a new link if it expired");
    };
    let cache = Cache::open(OciParStore::new(&read, None), Budget::committed().clone(), Writes::ReadOnly(why.to_string()));
    let Ok(key) = cache.key(Kind::Probe, "preflight-read") else { return fail("build-cache", "the probe key was refused", "a bug in the cache's key rules") };
    match OciParStore::new(&read, None).get(&key) {
        Ok(_) => pass("build-cache", format!("the read link answers; {why}, so nothing was written")),
        Err(e) => fail("build-cache", format!("reading through the read link failed: {e}"), "the read link is expired or revoked: mint a new BUILD_CACHE_READ_PAR_URL"),
    }
}

fn ledger() -> Check {
    match crate::ci_ledger::open().and_then(|l| l.ensure()) {
        Ok(()) => pass("ledger", "the database answers and its tables exist"),
        Err(why) => warn(fail("ledger", crate::ci_comment::cut(&why, 200), "HEALTH_REPORTS_ORDS_* in the vault, or the autonomous database is down: budget checks of tasks are skipped meanwhile")),
    }
}

/// `name=url` pairs: what the run reads from or deploys through, over the tailnet (`WF_PREFLIGHT_HEALTH` replaces the defaults).
fn health_targets(env: &Env) -> Vec<(String, String, bool)> {
    if let Some(list) = var("WF_PREFLIGHT_HEALTH") {
        return list.split(',').filter_map(|p| p.split_once('=')).map(|(n, u)| (n.to_string(), u.to_string(), true)).collect();
    }
    vec![
        ("reporting-service".into(), format!("{}/health", env.vm_url), true),
        ("remote-chat-agent".into(), wf_vmops::services::health_url("remote-chat-agent").unwrap_or_default().to_string(), false),
    ]
}

fn services(env: &Env) -> Vec<Check> {
    let mut out = Vec::new();
    if var("WF_PREFLIGHT_HEALTH").is_none() {
        out.push(match wf_vmops::vmops::send_routed("GET", "/health", None, HEALTH_TIMEOUT) {
            Ok(reply) if (200..300).contains(&reply.status) => pass("vm-ops-api", "answers a signed /health"),
            Ok(reply) => fail("vm-ops-api", format!("answered HTTP {}", reply.status), "wf vm ops status; every deploy leg goes through it"),
            Err(why) => fail("vm-ops-api", crate::ci_comment::cut(&why, 200), "the tailnet or the VM is down; every deploy leg goes through vm-ops-api (wf vm net up)"),
        });
    }
    for (name, url, required) in health_targets(env) {
        let check = match cli_http::Request::get(&url).via(wf_vmops::vmops::tailnet_via()).timeout(HEALTH_TIMEOUT).send() {
            Ok(r) if r.ok() => pass(name.clone(), "answers /health"),
            Ok(r) => fail(name.clone(), format!("answered HTTP {}", r.status), format!("wf vm logs {name}")),
            Err(why) => fail(name.clone(), crate::ci_comment::cut(&why, 200), "the service or the tailnet is down"),
        };
        out.push(if required { check } else { warn(check) });
    }
    out
}

fn available_kb(meminfo: &str) -> Option<u64> {
    meminfo.lines().find_map(|l| l.strip_prefix("MemAvailable:")).and_then(|v| v.split_whitespace().next()).and_then(|n| n.parse().ok())
}

fn free_bytes(path: &Path) -> Option<u64> {
    let c = std::ffi::CString::new(path.to_str()?).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0).then(|| st.f_bavail as u64 * st.f_frsize)
}

fn runner() -> Vec<Check> {
    let mut out = Vec::new();
    let root = wf_kit::root::repo_root();
    out.push(match free_bytes(&root) {
        Some(b) if b >= MIN_DISK_BYTES => pass("runner-disk", format!("{} GB free", b / 1_000_000_000)),
        Some(b) => fail("runner-disk", format!("{} GB free, {} needed", b / 1_000_000_000, MIN_DISK_BYTES / 1_000_000_000), "free space on the runner volume (old build targets): a full disk fails builds with bus errors"),
        None => fail("runner-disk", "statvfs failed", "check the mount"),
    });
    out.push(match std::fs::read_to_string("/proc/meminfo").ok().as_deref().and_then(available_kb) {
        Some(kb) if kb >= MIN_MEMORY_KB => pass("runner-memory", format!("{} GB available", kb / 1_000_000)),
        Some(kb) => fail("runner-memory", format!("{} MB available, {} MB needed", kb / 1000, MIN_MEMORY_KB / 1000), "something else holds the VM's memory: wf vm ops host"),
        None => warn(fail("runner-memory", "/proc/meminfo has no MemAvailable", "")),
    });
    out
}

fn github(env: &Env) -> Check {
    let url = format!("{}/rate_limit", env.api);
    match send(env, "GET", &url, None).and_then(|r| json_of(r, "reading the rate limit")) {
        Ok(v) => match v["resources"]["core"]["remaining"].as_i64() {
            Some(left) if left >= MIN_API_CALLS => pass("github", format!("token valid, {left} API calls left")),
            Some(left) => fail("github", format!("{left} API calls left, {MIN_API_CALLS} needed"), "wait for the rate limit window to reset"),
            None => pass("github", "token valid"),
        },
        Err(why) => fail("github", crate::ci_comment::cut(&why, 200), "GITHUB_TOKEN is missing, expired or lacks actions: read"),
    }
}

/// The runner version compose pins, against GitHub's latest: a runner left behind restarts in a loop trying to update, and the jobs it then runs
/// store no log (31 Oct 00:12 UTC: pinned 2.337.0, GitHub on 2.338.x). A warning, never a stop: GitHub allows a few releases of lag.
fn runner_version(env: &Env) -> Check {
    let pinned = std::fs::read_to_string(wf_kit::root::repo_root().join(RUNNER_COMPOSE)).ok().and_then(|text| {
        let tag = text.split("github-runner:").nth(1)?.split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?.to_string();
        let parts: Vec<u64> = tag.split('.').filter_map(|p| p.parse().ok()).collect();
        (parts.len() == 3).then_some((tag, parts))
    });
    let Some((tag, pinned)) = pinned else { return warn(fail("runner-version", format!("no runner version is pinned in {RUNNER_COMPOSE}"), "pin myoung34/github-runner:<version>-ubuntu-noble there")) };
    let url = format!("{}/repos/actions/runner/releases/latest", env.api);
    let latest = send(env, "GET", &url, None).and_then(|r| json_of(r, "reading the latest runner release")).ok().and_then(|v| {
        let name = v["tag_name"].as_str()?.trim_start_matches('v').to_string();
        let parts: Vec<u64> = name.split('.').filter_map(|p| p.parse().ok()).collect();
        (parts.len() == 3).then_some((name, parts))
    });
    match latest {
        Some((name, latest)) if latest[0] > pinned[0] || (latest[0] == pinned[0] && latest[1] > pinned[1]) => warn(fail(
            "runner-version",
            format!("the runner is pinned at {tag}, GitHub's latest is {name}"),
            format!("set the gh-runner image in {RUNNER_COMPOSE} to {name} and redeploy ci-runner by hand (wf ship ci-runner --ref dev); a runner left behind stores no job logs"),
        )),
        Some((name, _)) => pass("runner-version", format!("pinned {tag}, latest {name}")),
        None => warn(fail("runner-version", format!("pinned {tag}; GitHub's latest runner release could not be read"), "nothing to do unless it persists: the check reads the latest release of actions/runner")),
    }
}

type Probe<'a> = Box<dyn FnOnce() -> Vec<Check> + Send + 'a>;

fn run_checks(skip: &[&str]) -> Result<Vec<Check>, String> {
    let env = Env::read()?;
    let probes: Vec<(&str, Probe)> = vec![
        ("workspace", Box::new(|| vec![workspace()])),
        ("toolchain", Box::new(|| vec![toolchain()])),
        ("env", Box::new(|| vec![environment(&["GITHUB_TOKEN", "GITHUB_REPOSITORY", "GITHUB_RUN_ID", "GITHUB_SHA"])])),
        ("secrets", Box::new(|| vec![secrets()])),
        ("build-cache", Box::new(|| vec![build_cache()])),
        ("ledger", Box::new(|| vec![ledger()])),
        ("services", Box::new(|| services(&env))),
        ("runner", Box::new(runner)),
        ("github", Box::new(|| vec![github(&env)])),
        ("runner-version", Box::new(|| vec![runner_version(&env)])),
    ];
    let mut checks: Vec<Check> = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = probes.into_iter().filter(|(name, _)| !skip.contains(name)).map(|(name, f)| (name, scope.spawn(f))).collect();
        for (name, handle) in handles {
            checks.extend(handle.join().unwrap_or_else(|_| vec![fail("preflight", format!("the {name} check crashed"), "a bug in wf ci preflight")]));
        }
    });
    Ok(checks)
}

fn block(checks: &[Check]) -> String {
    let rows: String = checks.iter().map(|c| format!("| {} | {} | {}{} |\n", if c.ok { "✅" } else if c.required { "❌" } else { "⚠️" }, c.name, crate::ci_comment::cell(&c.detail), if c.ok || c.next.is_empty() { String::new() } else { format!("<br><sub>{}</sub>", crate::ci_comment::cell(&c.next)) })).collect();
    format!("\n### 🛫 Preflight\n\n| | Check | Result |\n|---|---|---|\n{rows}")
}

/// `wf ci preflight [--skip a,b]`: exit 1 when a required check failed.
pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let skip: Vec<&str> = p.val("skip").map(|s| s.split(',').collect()).unwrap_or_default();
    let checks = match run_checks(&skip) {
        Ok(c) => c,
        Err(why) => {
            println!("{}", json!({ "ok": false, "reason": why }));
            return Some(1);
        }
    };
    let failed: Vec<&Check> = checks.iter().filter(|c| !c.ok && c.required).collect();
    let list: Vec<Value> = checks.iter().map(|c| json!({ "check": c.name.as_str(), "ok": c.ok, "required": c.required, "detail": c.detail, "next": c.next })).collect();
    let report = block(&checks);
    if let Some(path) = var("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let _ = std::fs::OpenOptions::new().append(true).create(true).open(path).and_then(|mut f| writeln!(f, "{report}"));
    }
    println!("{}", json!({ "ok": failed.is_empty(), "failed": failed.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), "checks": list }));
    Some(i32::from(!failed.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_meminfo_when_read_then_the_available_kilobytes_come_out() {
        assert_eq!(available_kb("MemTotal: 100 kB\nMemAvailable:    2048 kB\n"), Some(2048));
        assert_eq!(available_kb("MemTotal: 100 kB\n"), None);
    }

    #[test]
    fn given_a_directory_when_asked_for_free_space_then_a_number_comes_back() {
        assert!(free_bytes(Path::new("/tmp")).is_some_and(|b| b > 0));
    }
}
