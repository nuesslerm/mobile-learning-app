//! The operator verbs of `wf cache` (`status`, `gc`, `purge`): through the built binary, over a stub `oci` first on PATH and a
//! stateful loopback bucket behind a PAR URL. No cloud is reached.
use crate::support::{create_stub, last_json, run_wf, RunOpts, RunResult};
use blob_cache::adapters::OciParStore;
use blob_cache::domain::{Budget, Generation, Key, Kind};
use blob_cache::{Cache, Writes};
use cli_http::Via;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

const OCI: &str = r#"here="$(dirname "$0")"
echo "$*" >> "$here/calls.log"
case "$*" in
  "os ns get") echo '{"data":"ns1"}' ;;
  "os bucket get "*) cat "$here/bucket.json" ;;
  "monitoring metric-data summarize-metrics-data "*) cat "$here/metrics.json" ;;
  "os object list "*) cat "$here/objects.json" ;;
  "os object delete "*) ;;
  *) echo "unexpected oci call: $*" >&2; exit 64 ;;
esac"#;

struct Oci {
    dir: tempfile::TempDir,
}

impl Oci {
    fn new() -> Oci {
        let dir = tempfile::tempdir().expect("a temp dir");
        create_stub(&dir.path().join("oci"), OCI);
        Oci { dir }
    }

    fn write(&self, name: &str, value: &Value) {
        std::fs::write(self.dir.path().join(name), value.to_string()).expect("a fixture");
    }

    fn bucket(&self, bytes: u64, objects: u64) {
        let data = json!({ "approximate-size": bytes, "approximate-count": objects, "compartment-id": "ocid1.compartment.oc1..test", "name": "mobile-learning-build-cache" });
        self.write("bucket.json", &json!({ "data": data }));
    }

    fn requests(&self, per_day: &[u64]) {
        let points: Vec<Value> = per_day.iter().enumerate().map(|(day, n)| json!({ "timestamp": format!("2026-10-{:02}T00:00:00+00:00", day + 1), "value": *n as f64 })).collect();
        self.write("metrics.json", &json!({ "data": [{ "name": "AllRequests", "aggregated-datapoints": points }] }));
    }

    fn objects(&self, listed: &[(&str, u64, &str)]) {
        let data: Vec<Value> = listed.iter().map(|(name, size, created)| json!({ "name": name, "size": size, "time-created": created })).collect();
        self.write("objects.json", &json!({ "data": data }));
    }

    fn wf(&self, args: &[&str]) -> RunResult {
        run_wf(args, &RunOpts::default().bin_dir(self.dir.path()))
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("calls.log")).unwrap_or_default().lines().map(String::from).collect()
    }

    fn forget_calls(&self) {
        let _ = std::fs::remove_file(self.dir.path().join("calls.log"));
    }

    fn deleted(&self) -> BTreeSet<String> {
        self.calls().iter().filter(|call| call.starts_with("os object delete ")).filter_map(|call| call.split(" --object-name ").nth(1)?.split(' ').next().map(String::from)).collect()
    }
}

fn names(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|name| name.to_string()).collect()
}

#[test]
fn status_reports_bytes_and_monthly_requests_against_the_caps_and_exits_1_over_either() {
    // Given the bucket's size and the daily request counts of this month, in and over each cap of budget.json
    let budget = Budget::committed();
    let (bytes_cap, requests_cap) = (budget.stored_bytes, budget.requests_per_month);
    let oci = Oci::new();
    let cases: [(u64, Vec<u64>, i32, Vec<&str>); 5] = [
        (1_000_000, vec![100, 200], 0, vec![]),
        (bytes_cap, vec![requests_cap - 1, 1], 0, vec![]),
        (bytes_cap + 1, vec![5], 1, vec!["storedBytes"]),
        (1_000, vec![requests_cap - 10, 20], 1, vec!["requestsPerMonth"]),
        (bytes_cap + 1, vec![requests_cap + 1], 1, vec!["storedBytes", "requestsPerMonth"]),
    ];
    // Each layer's row comes from one listing, by prefix; the generation pointer belongs to none
    let layers = json!({
        "nx": { "bytes": 150, "objects": 2, "ttlDays": 14 },
        "pnpm": { "bytes": 0, "objects": 0, "ttlDays": 60 },
        "probe": { "bytes": 0, "objects": 0, "ttlDays": 1 },
        "sccache": { "bytes": 400, "objects": 1, "ttlDays": 7 },
        "metro": { "bytes": 2000, "objects": 1, "ttlDays": 14 },
    });
    for (stored, per_day, code, over) in cases {
        oci.bucket(stored, 7);
        oci.requests(&per_day);
        oci.objects(&[("nx/g0/a", 100, "2026-10-01T00:00:00+00:00"), ("nx/g0/b", 50, "2026-10-02T00:00:00+00:00"), ("sccache/g0/c", 400, "2026-10-03T00:00:00+00:00"), ("metro/g0/app-build.tar.gz", 2000, "2026-10-04T00:00:00+00:00"), ("meta/generation", 1, "2026-10-01T00:00:00+00:00")]);

        // When the operator asks where the cache stands
        let r = oci.wf(&["cache", "status", "--json"]);

        // Then one JSON object gives both numbers beside their caps, and the exit code says whether either is over
        assert_eq!(r.exit_code, code, "stored {stored}, requests {per_day:?}: {r:?}");
        assert_eq!(r.stdout.lines().count(), 1, "one object on stdout: {r:?}");
        let out = last_json(&r);
        let expected = json!({ "ok": code == 0, "storedBytes": stored, "storedBytesCap": bytes_cap, "objects": 7, "requestsThisMonth": per_day.iter().sum::<u64>(), "requestsPerMonthCap": requests_cap, "layers": layers, "over": over });
        assert_eq!(out, expected, "{r:?}");
    }

    // And the numbers come from the dedicated bucket: its size fields, and this month's requests from the first of the month
    let calls = oci.calls().join("\n");
    assert!(calls.contains("--fields approximateSize --fields approximateCount"), "{calls}");
    let query = calls.lines().find(|call| call.starts_with("monitoring ")).expect("a metrics query");
    assert!(query.contains(r#"{resourceDisplayName = "mobile-learning-build-cache"}"#), "{query}");
    let start = query.split(" --start-time ").nth(1).and_then(|rest| rest.split(' ').next()).expect("a start time");
    assert!(start.len() == 20 && start.ends_with("-01T00:00:00Z"), "the window opens on the first of the month: {start}");
}

#[test]
fn gc_evicts_the_oldest_objects_first_until_under_the_target_and_never_the_generation_pointer() {
    // Given a bucket of four entries listed out of order (two creation times carry a zone offset) and the generation pointer, the
    // oldest object of all
    let oci = Oci::new();
    oci.bucket(1001, 5);
    oci.objects(&[
        ("nx/g0/d", 100, "2026-10-13T00:00:00.000000+00:00"),
        ("meta/generation", 1, "2026-10-01T00:00:00+00:00"),
        ("nx/g0/c", 200, "2026-10-11T00:30:00.250000+00:00"),
        ("pnpm/g0/store-v11.tar.gz", 300, "2026-10-11T00:00:00Z"),
        ("nx/g0/a", 400, "2026-10-11T01:00:00+05:00"),
    ]);

    // When gc is asked for nothing, or for the empty prefix that names every object, it is refused before any oci call
    for args in [&["cache", "gc"][..], &["cache", "gc", "--prefix", ""]] {
        assert_eq!(oci.wf(args).exit_code, 2, "{args:?}");
    }
    assert_eq!(oci.calls(), Vec::<String>::new());

    // And --status only reads
    let status = oci.wf(&["cache", "gc", "--status"]);
    assert_eq!((status.exit_code, last_json(&status)), (0, json!({ "ok": true, "storedBytes": 1001, "objects": 5 })), "{status:?}");
    assert_eq!(oci.deleted(), names(&[]));

    // When it evicts down to 450 bytes
    let evicted = oci.wf(&["cache", "gc", "--evict-to", "450"]);

    // Then it deletes the oldest first until the rest fits: a (10 Oct 20:00 UTC) and the pnpm store (11 Oct 00:00), not c or d
    assert_eq!(evicted.exit_code, 0, "{evicted:?}");
    assert_eq!(last_json(&evicted), json!({ "ok": true, "deleted": 2, "freedBytes": 700, "storedBytes": 301 }));
    assert_eq!(oci.deleted(), names(&["nx/g0/a", "pnpm/g0/store-v11.tar.gz"]));

    // When the target is unreachable by deletion, everything but the generation pointer goes and the verb says it fell short
    oci.forget_calls();
    let all = oci.wf(&["cache", "gc", "--evict-to", "0"]);
    assert_eq!((all.exit_code, last_json(&all)), (1, json!({ "ok": false, "deleted": 4, "freedBytes": 1000, "storedBytes": 1 })), "{all:?}");
    assert_eq!(oci.deleted(), names(&["nx/g0/a", "nx/g0/c", "nx/g0/d", "pnpm/g0/store-v11.tar.gz"]));

    // And --prefix deletes exactly what it names
    oci.forget_calls();
    let prefixed = oci.wf(&["cache", "gc", "--prefix", "nx/g0/"]);
    assert_eq!((prefixed.exit_code, last_json(&prefixed)), (0, json!({ "ok": true, "deleted": 3, "freedBytes": 700, "storedBytes": 301 })), "{prefixed:?}");
    assert_eq!(oci.deleted(), names(&["nx/g0/a", "nx/g0/c", "nx/g0/d"]));
}

const TOKEN: &str = "PAR-TOKEN-SECRET";

/// The bucket as a PAR URL sees it, on loopback, with state: `/p/<token>/n/<namespace>/b/<bucket>/o/<object name>`.
struct Bucket {
    url: String,
    objects: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Bucket {
    fn start() -> Bucket {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("a loopback port");
        let prefix = format!("/p/{TOKEN}/n/ns/b/build-cache/o/");
        let url = format!("http://127.0.0.1:{}{prefix}", server.server_addr().to_ip().expect("an ip address").port());
        let bucket = Bucket { url, objects: Arc::default(), seen: Arc::default() };
        let (held, log) = (bucket.objects.clone(), bucket.seen.clone());
        std::thread::spawn(move || {
            for mut request in server.incoming_requests() {
                let mut body = Vec::new();
                let _ = request.as_reader().read_to_end(&mut body);
                let method = request.method().as_str().to_string();
                log.lock().unwrap().push(format!("{method} {}", request.url()));
                let name = request.url().strip_prefix(prefix.as_str()).unwrap_or_default().to_string();
                let found = held.lock().unwrap().get(&name).cloned();
                let reply = match (method.as_str(), found) {
                    ("GET" | "HEAD", Some(bytes)) => tiny_http::Response::from_data(bytes),
                    ("GET" | "HEAD", None) => tiny_http::Response::from_data(Vec::new()).with_status_code(404),
                    ("PUT", _) => {
                        held.lock().unwrap().insert(name, body);
                        tiny_http::Response::from_data(Vec::new())
                    }
                    _ => tiny_http::Response::from_data(Vec::new()).with_status_code(405),
                };
                let _ = request.respond(reply);
            }
        });
        bucket
    }

    fn put(&self, name: &str, body: &str) {
        self.objects.lock().unwrap().insert(name.to_string(), body.as_bytes().to_vec());
    }

    fn holds(&self, name: &str) -> Option<String> {
        self.objects.lock().unwrap().get(name).map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }
}

fn with_links(url: &str) -> RunOpts {
    RunOpts::default().env("BUILD_CACHE_READ_PAR_URL", url).env("BUILD_CACHE_WRITE_PAR_URL", url)
}

#[test]
fn purge_moves_the_next_reader_to_a_new_empty_namespace_with_no_list_and_no_delete() {
    // Given a bucket at generation 3 that holds a task result
    let bucket = Bucket::start();
    bucket.put("meta/generation", "3");
    bucket.put("nx/g3/abc", "task output");

    // When the operator purges nx
    let r = run_wf(&["cache", "purge", "nx"], &with_links(&bucket.url));

    // Then the generation pointer is the next number, and the old entry is left to the lifecycle rule
    assert_eq!((r.exit_code, last_json(&r)), (0, json!({ "ok": true, "kind": "nx", "generation": 4 })), "{r:?}");
    assert_eq!(bucket.holds("meta/generation").as_deref(), Some("4"));
    assert_eq!(bucket.holds("nx/g3/abc").as_deref(), Some("task output"));
    let seen = bucket.seen.lock().unwrap().clone();
    let prefix = format!("/p/{TOKEN}/n/ns/b/build-cache/o/");
    assert!(seen.iter().all(|call| (call.starts_with("GET ") || call.starts_with("PUT ")) && call.split(' ').nth(1).and_then(|url| url.strip_prefix(prefix.as_str())) == Some("meta/generation")), "only the pointer is read and written: {seen:?}");

    // And the next shim, opening the same bucket, reads and writes under generation 4, where nothing is stored yet
    let next = Cache::open(OciParStore::new(&bucket.url, None).via(Via::Direct), Budget::committed().clone(), Writes::ReadOnly("test".into()));
    assert_eq!(next.generation(), Generation(4));
    assert_eq!(next.key(Kind::Nx, "abc").unwrap().as_str(), "nx/g4/abc");
    assert_eq!(next.get(&next.key(Kind::Nx, "abc").unwrap()), None);
    assert_eq!(next.get(&Key::new(Kind::Nx, Generation(3), "abc").unwrap()).as_deref(), Some(&b"task output"[..]));

    // And a purge that cannot reach the bucket fails with a reason and prints no link
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let down = run_wf(&["cache", "purge", "nx"], &with_links(&format!("http://127.0.0.1:{closed}/p/{TOKEN}/n/ns/b/build-cache/o/")));
    assert_eq!(down.exit_code, 1, "{down:?}");
    assert_eq!(last_json(&down)["ok"], false);
    assert!(!format!("{}{}{}{}", r.stdout, r.stderr, down.stdout, down.stderr).contains(TOKEN), "no verb prints a link: {r:?} {down:?}");
}

#[test]
fn nx_runs_a_command_with_the_nx_server_in_its_environment_and_returns_its_exit_code() {
    // Given a bucket behind the two PAR links
    let bucket = Bucket::start();

    // When a command that prints the Nx server address and exits 7 runs under `wf cache nx`
    let r = run_wf(&["cache", "nx", "--", "sh", "-c", "echo $NX_SELF_HOSTED_REMOTE_CACHE_SERVER; exit 7"], &with_links(&bucket.url));

    // Then the command saw a loopback server, its exit code is the verb's, and the run ends with the build cache line
    assert_eq!(r.exit_code, 7, "{r:?}");
    assert!(r.stdout.trim().starts_with("http://127.0.0.1:"), "{r:?}");
    assert!(r.stderr.contains("wf: build cache {"), "{r:?}");
    assert!(!format!("{}{}", r.stdout, r.stderr).contains(TOKEN), "no verb prints a link: {r:?}");
}

#[test]
fn sccache_runs_a_command_behind_the_compiler_wrapper_and_reports_the_compilers_own_counts() {
    // Given a bucket and an `sccache` on PATH that answers its version and its statistics
    let bucket = Bucket::start();
    let bin = tempfile::tempdir().expect("a temp dir");
    create_stub(
        &bin.path().join("sccache"),
        r#"case "$1" in
  --version) echo "sccache 0.10.0" ;;
  --show-stats) printf 'Cache hits                            3\nCache misses                          1\nCache write errors                    0\n' ;;
  *) ;;
esac"#,
    );

    // When a command that prints the wrapper variables runs under `wf cache sccache`
    let r = run_wf(&["cache", "sccache", "--", "sh", "-c", "echo $RUSTC_WRAPPER $CARGO_INCREMENTAL $SCCACHE_WEBDAV_ENDPOINT"], &with_links(&bucket.url).bin_dir(bin.path()));

    // Then it saw sccache as the wrapper, incremental compiles off and a loopback endpoint, and the run printed the compiler's counts
    assert_eq!(r.exit_code, 0, "{r:?}");
    let seen: Vec<&str> = r.stdout.split_whitespace().collect();
    assert_eq!(&seen[..2], ["sccache", "0"], "{r:?}");
    assert!(seen[2].starts_with("http://127.0.0.1:"), "{r:?}");
    assert!(r.stderr.contains("wf: sccache {\"hits\":\"3\",\"misses\":\"1\",\"writeErrors\":\"0\"}"), "{r:?}");
    assert!(r.stderr.contains("wf: compile cache {"), "{r:?}");
}

#[test]
fn the_compile_cache_server_is_already_running_when_the_command_makes_its_first_compile() {
    // Given an `sccache` that records, when asked to start its server, the port it was given
    let bucket = Bucket::start();
    let bin = tempfile::tempdir().expect("a temp dir");
    let marker = bin.path().join("started");
    create_stub(
        &bin.path().join("sccache"),
        &format!(
            r#"case "$1" in
  --version) echo "sccache 0.10.0" ;;
  --start-server) echo "$SCCACHE_SERVER_PORT" > {} ;;
  --show-stats) printf 'Cache hits                            0\nCache misses                          0\nCache write errors                    0\n' ;;
  *) ;;
esac"#,
            marker.display()
        ),
    );

    // When a command that reads what the server was started with, and the port it is told to use, runs under `wf cache sccache`
    let script = format!("cat {} 2>/dev/null; echo $SCCACHE_SERVER_PORT", marker.display());
    let r = run_wf(&["cache", "sccache", "--", "sh", "-c", &script], &with_links(&bucket.url).bin_dir(bin.path()));

    // Then the server had been started, with that very port, before the command began
    assert_eq!(r.exit_code, 0, "{r:?}");
    let ports: Vec<&str> = r.stdout.split_whitespace().collect();
    assert_eq!(ports.len(), 2, "the server was started before the command ran: {r:?}");
    assert_eq!(ports[0], ports[1], "{r:?}");
}

#[test]
fn a_cache_wrapper_without_a_command_is_a_usage_error() {
    // When either wrapper verb runs with nothing to run
    let nx = run_wf(&["cache", "nx"], &RunOpts::default());
    let sccache = run_wf(&["cache", "sccache", "--"], &RunOpts::default());

    // Then each says what it needs and exits 2
    assert_eq!((nx.exit_code, sccache.exit_code), (2, 2), "{nx:?} {sccache:?}");
    assert!(nx.stderr.contains("a command to run") && sccache.stderr.contains("a command to run"), "{nx:?} {sccache:?}");
}

#[test]
fn a_job_other_than_the_one_that_saves_the_pnpm_store_skips_the_pack() {
    // Given a CI run whose flow-tests job is the one that saves the pnpm store, and another job of the same run
    let started = std::time::Instant::now();
    let opts = RunOpts::default().env("WF_PNPM_SAVE_JOB", "flow-tests").env("GITHUB_JOB", "integration-tests");

    // When that other job asks to save the store
    let r = run_wf(&["cache", "save", "pnpm"], &opts);

    // Then it saves nothing, says another job does, exits 0 and does not pack anything
    let json: Value = last_json(&r);
    assert_eq!((r.exit_code, json["saved"].clone(), json["ok"].clone()), (0, json!(false), json!(true)), "{r:?}");
    assert!(json["reason"].as_str().is_some_and(|why| why.contains("another job saves")), "{json}");
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "it packed the store");
}

#[test]
fn a_job_named_behind_the_callers_job_is_still_compared_by_its_own_name_when_the_pnpm_store_saver_is_chosen() {
    // Given the saver set by name and a sharded job of another stage, both named behind the caller's job
    let opts = RunOpts::default().env("WF_PNPM_SAVE_JOB", "run / flow-tests (1/4)").env("GITHUB_JOB", "run / integration-tests (2/4)");

    // When that job asks to save the store
    let r = run_wf(&["cache", "save", "pnpm"], &opts);

    // Then it is another job: nothing is saved
    let json: Value = last_json(&r);
    assert_eq!((r.exit_code, json["saved"].clone()), (0, json!(false)), "{r:?}");
    assert!(json["reason"].as_str().is_some_and(|why| why.contains("another job saves")), "{json}");
}
