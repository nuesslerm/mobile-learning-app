//! `wf cache` and the loopback shim that gives Nx the shared build cache (plan `docs/plans/blob-cache/plan.md`). The shim runs where
//! Nx runs: `wf test run --stage` and `wf check --staged` start it, hand Nx its address and a per-run token, and stop it when Nx
//! is done. Whether a run may write is `write_allowed`, the one decision for the sandbox, the pre-commit gate and CI.
mod admin;
mod guard;
mod metro;
mod pnpm;
mod wrap;

pub use guard::ensure_oci_config;

use wf_kit::args::parse_spec;
use wf_kit::proc::git_out;
use blob_cache::adapters::OciParStore;
use blob_cache::domain::{self, Budget, Generation, Kind, Pins, Refusal, WriteFacts};
use blob_cache::domain::Stats;
use blob_cache::{Cache, NxServer, SccacheServer, Writes};
use crate::ci_metrics::{self, Gauges};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Instant;

const READ_PAR: &str = "BUILD_CACHE_READ_PAR_URL";
const WRITE_PAR: &str = "BUILD_CACHE_WRITE_PAR_URL";

/// What counts as a change when the guard asks whether the tree is clean.
#[derive(Clone, Copy)]
pub enum Scope {
    /// Every path git reports: a stage tests the tree as it is.
    Tree,
    /// The pre-commit gate: what is staged is what the commit will hold, so only an unstaged or an untracked file counts.
    Staged,
    /// The pre-push gate of a push to `dev`: the tree must be clean, and HEAD is about to be on origin/dev, so it counts as being there.
    Pushed,
}

fn facts(root: &Path, scope: Scope) -> WriteFacts {
    let changed = match git_out(root, &["status", "--porcelain=v1", "-uall"]) {
        Some(status) => status
            .lines()
            .filter(|line| matches!(scope, Scope::Tree | Scope::Pushed) || line.as_bytes().get(1).is_some_and(|worktree| *worktree != b' '))
            .map(|line| line.get(3..).unwrap_or(line).to_string())
            .collect(),
        None => vec!["git status failed".to_string()],
    };
    let ancestor = if matches!(scope, Scope::Pushed) { Some(true) } else { Command::new("git")
        .args(["merge-base", "--is-ancestor", "HEAD", "origin/dev"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .and_then(|status| match status.code() {
            Some(0) => Some(true),
            Some(1) => Some(false),
            _ => None,
        }) };
    let tools = crate::pins::pins(root);
    let tool = |name: &str| tools.iter().find(|p| p.tool == name);
    let pinned = |name: &str| tool(name).map(|p| p.pinned.clone());
    let found = |name: &str| tool(name).map(|p| p.actual.clone()).filter(|version| !version.is_empty());
    WriteFacts { changed, head_on_origin_dev: ancestor, node: found("node"), pnpm: found("pnpm"), pins: Pins { node: pinned("node"), pnpm: pinned("pnpm") } }
}

/// Whether this run may write to the shared cache: a clean checkout of a commit on `origin/dev` (as the local ref knows it, so no
/// fetch) running the node and pnpm `mise.toml` pins. A refusal says why and the run reads only.
fn write_allowed(root: &Path, scope: Scope) -> Result<(), Refusal> {
    domain::write_allowed(&facts(root, scope))
}

/// What this run may write: `write_allowed`, and when it says no, the one line the gate reads and one loud event with the reason and,
/// for a dirty tree, the paths that made it so.
fn writes_for(root: &Path, scope: Scope, step: &str) -> Writes {
    match write_allowed(root, scope) {
        Ok(()) => Writes::Allowed,
        Err(refusal) => {
            eprintln!("wf: build cache reads only for this run, {refusal}");
            let mut fields = vec![("step", step.to_string()), ("reason", refusal.reason().to_string())];
            match &refusal {
                Refusal::Dirty { files } => {
                    fields.push(("changed", files.len().to_string()));
                    fields.push(("paths", files.iter().take(20).cloned().collect::<Vec<_>>().join(";")));
                }
                other => fields.push(("detail", other.to_string())),
            }
            ci_metrics::event("warn", "cache.write.refused", &fields);
            Writes::ReadOnly(refusal.to_string())
        }
    }
}

/// One event for what a run did against the store, and a loud one when writes were wanted and did not land.
pub(crate) fn store_events(kind: &str, stats: &Stats) {
    let failure = |fields: &mut Vec<(&str, String)>| {
        if let Some(failure) = stats.last_failure {
            fields.push(("error_class", failure.class.to_string()));
            fields.push(("error_status", failure.status.map_or_else(String::new, |s| s.to_string())));
        }
    };
    let mut fields = vec![
        ("kind", kind.to_string()),
        ("hits", stats.hits.to_string()),
        ("misses", stats.misses.to_string()),
        ("puts", stats.puts.to_string()),
        ("skipped", stats.skipped.to_string()),
        ("requests", stats.requests.to_string()),
        ("bytes", stats.bytes.to_string()),
        ("degraded", stats.degraded.map_or("no", |d| d.as_str()).to_string()),
    ];
    failure(&mut fields);
    let missing = stats.seeded.is_some_and(|s| s.present < s.checked);
    if let Some(seeded) = stats.seeded {
        fields.push(("seeded_checked", seeded.checked.to_string()));
        fields.push(("seeded_present", seeded.present.to_string()));
    }
    ci_metrics::event(if missing { "warn" } else { "info" }, "cache.run", &fields);
    if stats.dropped_puts > 0 {
        let mut fields = vec![("kind", kind.to_string()), ("dropped", stats.dropped_puts.to_string()), ("quota_exceeded", stats.quota_exceeded.to_string())];
        failure(&mut fields);
        ci_metrics::event("warn", "cache.put.dropped", &fields);
    }
}

/// The shim of one `wf` run: a loopback server answering Nx from the bucket. It stops with the value and prints the run's stats.
pub struct Shim {
    server: Option<NxServer>,
    cache: Arc<Cache<OciParStore>>,
    compiles: Option<Compiles>,
    started: Instant,
}

/// Which layers a shim serves.
#[derive(Clone, Copy)]
pub struct Layers {
    pub nx: bool,
    pub sccache: bool,
}

/// The sccache layer of a shim: its WebDAV server, the cache behind it (its own meter, so a compile cache's many small calls do not
/// spend an Nx run's budget) and the port of the sccache server this run owns, which it stops to read the compiler's own counts.
struct Compiles {
    server: SccacheServer,
    cache: Arc<Cache<OciParStore>>,
    port: u16,
}

/// A free loopback port for the sccache server of this run, so a server left by an earlier run, which would keep that run's endpoint,
/// is never reused.
fn free_port() -> Option<u16> {
    std::net::TcpListener::bind("127.0.0.1:0").ok()?.local_addr().ok().map(|a| a.port())
}

fn sccache_on_path() -> bool {
    Command::new("sccache").arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Why the kill switch is on, if it is: the budget file's `enabled` or the `WF_BLOB_CACHE=off` of the environment.
pub(crate) fn switched_off(budget: &Budget) -> Option<&'static str> {
    if !budget.enabled {
        Some("budget.json enabled is false")
    } else if std::env::var("WF_BLOB_CACHE").is_ok_and(|v| v == "off") {
        Some("WF_BLOB_CACHE=off")
    } else {
        None
    }
}

/// A run with no shared cache says so, in a line and a warn event, so a disabled cache is never a quiet cold run.
pub(crate) fn disabled<T>(why: &str) -> Option<T> {
    eprintln!("wf: build cache disabled for this run, {why}; every layer runs cold");
    ci_metrics::event("warn", "cache.shim.disabled", &[("reason", why.to_string())]);
    None
}

impl Shim {
    /// `None` is a run on Nx's local cache alone: the kill switch is off, or a PAR link does not resolve (the environment, then
    /// the vault), or the server cannot listen.
    pub fn start(root: &Path, scope: Scope) -> Option<Shim> {
        Shim::start_with(root, scope, Layers { nx: true, sccache: true })
    }

    /// `start` for the layers a caller names: `wf cache nx -- <cmd>` serves Nx only, `wf cache sccache -- <cmd>` compiles only.
    pub fn start_with(root: &Path, scope: Scope, layers: Layers) -> Option<Shim> {
        let budget = Budget::committed();
        if let Some(why) = switched_off(budget) {
            return disabled(why);
        }
        let mut writes = writes_for(root, scope, "shim");
        if let (Writes::Allowed, Some(why)) = (&writes, secrets::write_urls_refused()) {
            writes = Writes::ReadOnly(why);
        }
        let wanted: &[&str] = if matches!(writes, Writes::Allowed) { &[READ_PAR, WRITE_PAR] } else { &[READ_PAR] };
        let links = secrets::load_values(wanted);
        let link = |key: &str| links.get(key).cloned().flatten();
        let Some(read) = link(READ_PAR) else { return disabled("no link") };
        let write = match matches!(writes, Writes::Allowed).then(|| link(WRITE_PAR)) {
            Some(None) => return disabled("no link"),
            other => other.flatten(),
        };
        let compile_writes = writes.clone();
        let cache = Arc::new(Cache::open(OciParStore::new(&read, write.as_deref()), budget.clone(), writes));
        // the compile cache shares the generation the first cache read, so a shim reads the pointer once, not once per layer
        let compiles = if layers.sccache { compiles(&read, write.as_deref(), budget, &compile_writes, cache.generation()) } else { None };
        if !layers.nx {
            return Some(Shim { server: None, cache, compiles, started: Instant::now() });
        }
        match NxServer::start(cache.clone()) {
            Ok(server) => Some(Shim { server: Some(server), cache, compiles, started: Instant::now() }),
            Err(error) => {
                eprintln!("wf: build cache not started, {error}");
                None
            }
        }
    }

    /// What the Nx child needs: the server, its token, and the loopback kept out of the proxy the sandbox sets; and, where sccache runs,
    /// the compiler wrapper and its endpoint (`CARGO_INCREMENTAL=0` because sccache refuses incremental compiles).
    pub(crate) fn env(&self) -> Vec<(&'static str, String)> {
        let proxy_bypass = std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy")).ok();
        let mut env = self.server.as_ref().map(|server| server.env(proxy_bypass.as_deref())).unwrap_or_default();
        if let Some(compiles) = &self.compiles {
            env.extend([
                ("RUSTC_WRAPPER", "sccache".to_string()),
                ("CARGO_INCREMENTAL", "0".to_string()),
                ("SCCACHE_WEBDAV_ENDPOINT", compiles.server.url()),
                ("SCCACHE_WEBDAV_TOKEN", compiles.server.token().to_string()),
                ("SCCACHE_SERVER_PORT", compiles.port.to_string()),
            ]);
        }
        env
    }
}

impl Drop for Shim {
    fn drop(&mut self) {
        let nx = self.server.take().map(NxServer::stop).is_some();
        let seconds = self.started.elapsed().as_secs_f64();
        self.cache.verify_written();
        let stats = self.cache.stats();
        let json = stats.to_json();
        eprintln!("wf: build cache {json}");
        crate::cache_runs::record(&json);
        let mut gauges = Gauges::new();
        if nx {
            store_events("nx", &stats);
            gauges.store("nx", &stats).set("ci.cache.step_seconds", "nx", "run", seconds);
        }
        if let Some(compiles) = self.compiles.take() {
            let counts = stop_sccache(&compiles);
            let served = compiles.server.served();
            compiles.cache.verify_written();
            let stats = compiles.cache.stats();
            eprintln!("wf: compile cache {}", serde_json::json!({ "gets": served.gets, "puts": served.puts, "store": stats.to_json() }));
            compiles.server.stop();
            store_events("sccache", &stats);
            gauges.store("sccache", &stats).set("ci.cache.step_seconds", "sccache", "run", seconds);
            for (result, n) in counts {
                gauges.set("ci.cache.requests", "sccache-compiler", result, n);
            }
        }
        gauges.send();
    }
}

/// The sccache layer, when `sccache` is installed and a port is free; otherwise compiles stay uncached and one line says why.
fn compiles(read: &str, write: Option<&str>, budget: &Budget, writes: &Writes, generation: Generation) -> Option<Compiles> {
    if !sccache_on_path() {
        eprintln!("wf: compile cache not used, sccache is not on PATH");
        return None;
    }
    let cache = Arc::new(Cache::with_generation(OciParStore::new(read, write), budget.for_kind_run(Kind::Sccache), writes.clone(), generation));
    let server = SccacheServer::start(cache.clone()).ok()?;
    let compiles = Compiles { server, cache, port: free_port()? };
    start_sccache(&compiles);
    Some(compiles)
}

/// Starts the sccache server before any compile does. Left to the first `sccache rustc` it is started by whichever of a dozen
/// parallel crates asks first, and the others give up after five seconds of refused connections when the VM is busy (12 flow
/// tasks failed at once on 30 Oct); started here, with the same settings, every compile finds it running.
fn start_sccache(compiles: &Compiles) {
    let started = Command::new("sccache")
        .arg("--start-server")
        .env("SCCACHE_SERVER_PORT", compiles.port.to_string())
        .env("SCCACHE_WEBDAV_ENDPOINT", compiles.server.url())
        .env("SCCACHE_WEBDAV_TOKEN", compiles.server.token())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !started.is_ok_and(|status| status.success()) {
        eprintln!("wf: the compile cache server did not start; the first compile will try to");
    }
}

/// Stops the sccache server of this run and prints its own hit and miss counts, the numbers a cold-versus-warm comparison needs.
fn stop_sccache(compiles: &Compiles) -> Vec<(&'static str, f64)> {
    let port = compiles.port.to_string();
    let stats = Command::new("sccache").arg("--show-stats").env("SCCACHE_SERVER_PORT", &port).env("SCCACHE_WEBDAV_ENDPOINT", compiles.server.url()).stdin(Stdio::null()).output();
    let mut counts = Vec::new();
    if let Ok(out) = stats {
        let text = String::from_utf8_lossy(&out.stdout);
        let pick = |label: &str| text.lines().find(|l| l.starts_with(label)).and_then(|l| l.split_whitespace().last()).unwrap_or("0").to_string();
        let (hits, misses, errors) = (pick("Cache hits  "), pick("Cache misses  "), pick("Cache write errors"));
        eprintln!("wf: sccache {}", serde_json::json!({ "hits": hits, "misses": misses, "writeErrors": errors }));
        counts = vec![("hit", hits.parse().unwrap_or(0.0)), ("miss", misses.parse().unwrap_or(0.0)), ("write-error", errors.parse().unwrap_or(0.0))];
    }
    let _ = Command::new("sccache").arg("--stop-server").env("SCCACHE_SERVER_PORT", &port).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    counts
}

/// The env a run gives Nx: the shim's, or nothing.
pub fn env_of(shim: &Option<Shim>) -> Vec<(&'static str, String)> {
    shim.as_ref().map(Shim::env).unwrap_or_default()
}

/// The kind a `restore` or `save` names: its first word that is not an option.
fn kind_of(rest: &[String]) -> Option<&str> {
    rest.iter().map(String::as_str).find(|a| !a.starts_with('-'))
}

/// `wf cache <verb>`.
pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    match verb.as_str() {
        "status" => admin::status(&parse_spec(rest, &admin::STATUS)?),
        "guard" => guard::run(&parse_spec(rest, &guard::GUARD)?),
        "gc" => admin::gc(&parse_spec(rest, &admin::GC)?),
        "purge" => admin::purge(&parse_spec(rest, &admin::PURGE)?),
        "restore" if kind_of(rest) == Some("metro") => metro::restore(&parse_spec(rest, &pnpm::SPEC)?),
        "save" if kind_of(rest) == Some("metro") => metro::save(&parse_spec(rest, &pnpm::SPEC)?),
        "restore" => pnpm::restore(&parse_spec(rest, &pnpm::SPEC)?),
        "save" => pnpm::save(&parse_spec(rest, &pnpm::SPEC)?),
        "nx" => wrap::run("nx", rest, Layers { nx: true, sccache: false }),
        "sccache" => wrap::run("sccache", rest, Layers { nx: false, sccache: true }),
        _ => None,
    }
}
