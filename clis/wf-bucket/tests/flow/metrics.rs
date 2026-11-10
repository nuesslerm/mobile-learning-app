//! What the cache layers report: a structured event per operation on stderr and gauges to a metrics source, entered through the
//! `wf-bucket` binary's argv. The bucket is a loopback fake behind a PAR URL, the metrics source a loopback double of Better Stack's
//! OTLP ingest, `pnpm` a stub.
use crate::support::otlp::{gauges, Gauge};
use crate::support::{create_stub, last_json, run_wf, MockReply, MockServer, RunOpts, RunResult};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const TOKEN: &str = "PAR-TOKEN-SECRET";

/// A bucket whose every answer is `status`.
fn bucket(status: u16) -> (MockServer, String) {
    let server = MockServer::start(None);
    server.queue(vec![MockReply::text(status, "")], true);
    let url = format!("http://127.0.0.1:{}/p/{TOKEN}/n/ns/b/build-cache/o/", server.port);
    (server, url)
}

/// The run's environment: both links, and the metrics double as the source gauges go to.
fn opts(bucket_url: &str, source: &MockServer) -> RunOpts {
    RunOpts::default().env("BUILD_CACHE_READ_PAR_URL", bucket_url).env("BUILD_CACHE_WRITE_PAR_URL", bucket_url).env("WF_CI_METRICS_BASE", &format!("http://127.0.0.1:{}", source.port))
}

fn pnpm_with_store(dir: &std::path::Path) -> std::path::PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    create_stub(&bin.join("pnpm"), &format!("echo {}/store/v11", dir.display()));
    bin
}

fn event(r: &RunResult, name: &str) -> Option<String> {
    r.stderr.lines().find(|l| l.contains("source=ci-cache") && l.contains(&format!("msg=\"{name}\""))).map(str::to_string)
}

fn has(seen: &[Gauge], metric: &str, scope: &str, name: &str) -> Option<f64> {
    seen.iter().find(|g| g.metric == metric && g.scope == scope && g.name == name).map(|g| g.value)
}

#[test]
fn a_restore_against_an_unreachable_store_reports_the_status_and_class_and_the_run_goes_cold_not_red() {
    // Given a bucket that answers every request with HTTP 503, a metrics source, and an empty pnpm store to fill
    let (_bucket, url) = bucket(503);
    let source = MockServer::start(None);
    let dir = tempfile::tempdir().unwrap();
    let bin = pnpm_with_store(dir.path());

    // When the pnpm store is restored
    let r = run_wf(&["cache", "restore", "pnpm"], &opts(&url, &source).bin_dir(&bin));

    // Then the run is cold, not red, and the verb's JSON carries the store's HTTP status and class
    assert_eq!(r.exit_code, 0, "{r:?}");
    let out = last_json(&r);
    assert_eq!((out["ok"].clone(), out["restored"].clone()), (json!(true), json!(false)), "{out}");
    assert_eq!(out["storeError"], json!({ "class": "unreachable", "status": 503 }), "{out}");

    // And one loud event says the same with its timing, and carries no link
    let line = event(&r, "cache.restore").unwrap_or_else(|| panic!("no cache.restore event: {r:?}"));
    assert!(line.contains("level=warn") && line.contains("error_class=unreachable") && line.contains("error_status=503") && line.contains("kind=pnpm"), "{line}");
    assert!(!format!("{}{}", r.stdout, r.stderr).contains(TOKEN), "{r:?}");

    // And the source received the step's seconds and the run's degradation
    let seen = gauges(&source.requests());
    assert!(has(&seen, "ci.cache.step_seconds", "pnpm", "restore").is_some(), "{seen:?}");
    assert_eq!(has(&seen, "ci.cache.degraded", "pnpm", "unreachable"), Some(1.0), "{seen:?}");
}

#[test]
fn a_store_that_refuses_a_request_is_told_apart_from_one_that_is_down() {
    // Given a bucket that refuses every request with HTTP 400, which is no outage and no credential problem
    let (_bucket, url) = bucket(400);
    let source = MockServer::start(None);
    let dir = tempfile::tempdir().unwrap();
    let bin = pnpm_with_store(dir.path());

    // When the pnpm store is restored
    let r = run_wf(&["cache", "restore", "pnpm"], &opts(&url, &source).bin_dir(&bin));

    // Then the class is a rejection with its status, and the breaker stays closed (nothing is reported as degraded)
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert_eq!(last_json(&r)["storeError"], json!({ "class": "rejected", "status": 400 }), "{r:?}");
    let seen = gauges(&source.requests());
    assert!(!seen.iter().any(|g| g.metric == "ci.cache.degraded"), "{seen:?}");
}

#[test]
fn a_run_on_a_dirty_tree_says_loudly_which_paths_made_it_read_only() {
    // Given a git repository with an untracked file, a bucket that has nothing, and a metrics source
    let (_bucket, url) = bucket(404);
    let source = MockServer::start(None);
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| assert!(std::process::Command::new("git").args(args).current_dir(repo.path()).status().unwrap().success());
    git(&["init", "-q"]);
    std::fs::write(repo.path().join("scratch.txt"), "x").unwrap();

    // When a command runs under the Nx shim there
    let r = run_wf(&["cache", "nx", "--", "true"], &opts(&url, &source).cwd(repo.path()));

    // Then the run reads only, and a warn event names the reason and the dirty path
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(r.stderr.contains("wf: build cache reads only for this run"), "{r:?}");
    let line = event(&r, "cache.write.refused").unwrap_or_else(|| panic!("no refusal event: {r:?}"));
    assert!(line.contains("level=warn") && line.contains("reason=dirty") && line.contains("scratch.txt") && line.contains("changed=1"), "{line}");

    // And the shim's run is one event and its gauges: requests by result, and how long the shim ran
    assert!(event(&r, "cache.run").is_some_and(|l| l.contains("kind=nx")), "{r:?}");
    let seen = gauges(&source.requests());
    assert!(has(&seen, "ci.cache.requests", "nx", "hit").is_some() && has(&seen, "ci.cache.step_seconds", "nx", "run").is_some(), "{seen:?}");
}

#[test]
fn a_metrics_source_that_hangs_or_is_gone_costs_the_run_two_seconds_and_nothing_else() {
    // Given a cold bucket and a metrics source that takes ten seconds to answer
    let (_bucket, url) = bucket(503);
    let slow = MockServer::start(None);
    slow.delay(Duration::from_secs(10));
    let dir = tempfile::tempdir().unwrap();
    let bin = pnpm_with_store(dir.path());
    let started = Instant::now();

    // When the pnpm store is restored
    let r = run_wf(&["cache", "restore", "pnpm"], &opts(&url, &slow).bin_dir(&bin));

    // Then the verb still succeeds, says the gauges were not sent, and did not wait for the source
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(event(&r, "ci.metrics.timeout").is_some_and(|l| l.contains("level=warn")), "{r:?}");
    assert!(started.elapsed() < Duration::from_secs(8), "waited {:?}", started.elapsed());

    // And a source that is not listening at all changes nothing about the verb's answer
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let gone = run_wf(&["cache", "restore", "pnpm"], &RunOpts::default().env("BUILD_CACHE_READ_PAR_URL", &url).env("WF_CI_METRICS_BASE", &format!("http://127.0.0.1:{closed}")).bin_dir(&bin));
    assert_eq!((gone.exit_code, last_json(&gone)["ok"].clone()), (0, Value::Bool(true)), "{gone:?}");
}

#[test]
fn a_run_with_the_shared_cache_switched_off_says_so_in_a_line_and_a_warn_event() {
    // Given an environment that switches the shared build cache off, and an empty pnpm store to fill
    let (bucket, url) = bucket(200);
    let source = MockServer::start(None);
    let dir = tempfile::tempdir().unwrap();
    let bin = pnpm_with_store(dir.path());

    // When the pnpm store is restored
    let r = run_wf(&["cache", "restore", "pnpm"], &opts(&url, &source).env("WF_BLOB_CACHE", "off").bin_dir(&bin));

    // Then the run is cold, not red, one stderr line says the cache is disabled and why, and a warn event carries the reason
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(r.stderr.contains("wf: build cache disabled for this run, WF_BLOB_CACHE=off"), "{}", r.stderr);
    let line = event(&r, "cache.shim.disabled").unwrap_or_else(|| panic!("no cache.shim.disabled event: {r:?}"));
    assert!(line.contains("level=warn") && line.contains("reason=WF_BLOB_CACHE=off"), "{line}");

    // And the store was never asked
    assert!(bucket.requests().is_empty());
}
