//! The cache against a directory store and against a loopback bucket behind a PAR URL, by one contract; plus the write guard,
//! the kinds table and the purity of the domain.
#[path = "flow/app_build.rs"]
mod app_build;
#[path = "flow/par_fake.rs"]
mod par_fake;
#[path = "flow/pnpm.rs"]
mod pnpm;

use blob_cache::adapters::{DirStore, OciParStore};
use blob_cache::domain::{write_allowed, Budget, Degraded, Generation, Key, Kind, Pins, Refusal, WriteFacts};
use blob_cache::ports::{BlobStore, PutOutcome, StoreError};
use blob_cache::{Cache, Put, Skip, Writes};
use cli_http::Via;
use par_fake::{closed_url, ParFake, TOKEN};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

struct Counting<S> {
    inner: S,
    calls: AtomicU64,
}

impl<S: BlobStore> Counting<S> {
    fn wrap(inner: S) -> Arc<Counting<S>> {
        Arc::new(Counting { inner, calls: AtomicU64::new(0) })
    }

    fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl<S: BlobStore> BlobStore for Counting<S> {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.get(key)
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.put(key, body, if_absent)
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.inner.exists(key)
    }

    fn requests_per_put(&self, if_absent: bool) -> u64 {
        self.inner.requests_per_put(if_absent)
    }
}

/// The committed budget with some numbers changed.
fn budget(change: impl FnOnce(&mut serde_json::Value)) -> Budget {
    let mut json: serde_json::Value = serde_json::from_str(include_str!("../data/budget.json")).unwrap();
    change(&mut json);
    Budget::parse(&json.to_string()).unwrap()
}

fn key(kind: Kind, generation: u64, name: &str) -> Key {
    Key::new(kind, Generation(generation), name).unwrap()
}

/// What every `BlobStore` must do. `broken` is a store of the same adapter that cannot reach what it stores in.
fn contract<S: BlobStore>(healthy: S, broken: S) {
    let store = Counting::wrap(healthy);
    let broken = Counting::wrap(broken);
    let committed = Budget::committed().clone();
    let a = key(Kind::Probe, 0, "contract-a");

    // Given an empty store, When a key is read, Then a miss is None and not an error
    assert_eq!(store.get(&a), Ok(None));
    assert_eq!(store.exists(&a), Ok(false));

    // When the same key is put twice if absent, Then the second says Exists and the first body stays
    assert_eq!(store.put(&a, b"first", true), Ok(PutOutcome::Stored));
    assert_eq!(store.put(&a, b"second", true), Ok(PutOutcome::Exists));
    assert_eq!((store.get(&a), store.exists(&a)), (Ok(Some(b"first".to_vec())), Ok(true)));
    // and a put that is not if-absent overwrites in place
    assert_eq!(store.put(&a, b"third", false), Ok(PutOutcome::Stored));
    assert_eq!(store.get(&a), Ok(Some(b"third".to_vec())));

    // When the generation is bumped, Then readers move to a new empty namespace and nothing is deleted
    let mut cache = Cache::open(store.clone(), committed.clone(), Writes::Allowed);
    let old = cache.key(Kind::Nx, "abc123").unwrap();
    assert_eq!((cache.generation(), cache.put(&old, b"result", true)), (Generation(0), Put::Stored));
    assert_eq!(cache.get(&old), Some(b"result".to_vec()));
    assert_eq!(cache.purge(), Ok(Generation(1)));
    let reopened = Cache::open(store.clone(), committed.clone(), Writes::Allowed);
    let moved = reopened.key(Kind::Nx, "abc123").unwrap();
    assert_eq!((reopened.generation(), moved.as_str(), reopened.get(&moved)), (Generation(1), "nx/g1/abc123", None));
    assert_eq!(store.get(&old), Ok(Some(b"result".to_vec())));

    // When the guard refused writes, Then a put sends nothing
    let read_only = Cache::with_generation(store.clone(), committed.clone(), Writes::ReadOnly("dirty: x".into()), Generation(1));
    let before = store.calls();
    assert_eq!(read_only.put(&moved, b"nope", true), Put::Skipped(Skip::ReadOnly));
    assert_eq!(store.calls(), before);

    // When the store cannot be reached, Then a read is cold, the breaker opens, and nothing more is sent
    let down = Cache::with_generation(broken.clone(), committed.clone(), Writes::Allowed, Generation(0));
    let lost = down.key(Kind::Nx, "abc123").unwrap();
    assert!(matches!(broken.get(&lost), Err(StoreError::Unreachable(_))));
    assert_eq!((down.get(&lost), down.degraded()), (None, Some(Degraded::Unreachable)));
    assert_eq!(down.put(&lost, b"x", true), Put::Skipped(Skip::Degraded(Degraded::Unreachable)));
    assert_eq!(broken.calls(), 2, "one direct call and the one that opened the breaker");
    assert_eq!(Cache::open(broken.clone(), committed.clone(), Writes::Allowed).degraded(), Some(Degraded::Unreachable));

    // When a run spends its requests, Then the next call is cold and the budget is named
    let tight = Cache::with_generation(store.clone(), budget(|b| b["requestsPerRun"] = 3.into()), Writes::Allowed, Generation(1));
    let before = store.calls();
    assert_eq!((tight.get(&moved), tight.get(&moved), tight.get(&moved)), (None, None, None));
    assert_eq!((tight.get(&moved), tight.degraded()), (None, Some(Degraded::Budget)));
    assert_eq!(store.calls() - before, 3);
    assert_eq!(tight.stats().to_json()["degraded"], "budget");

    // When a run would pass its byte limit, Then that put is cold; an entry over its kind's cap is dropped alone
    let small = budget(|b| {
        b["bytesPerRun"] = 10.into();
        b["kinds"]["probe"]["maxEntryBytes"] = 6.into();
    });
    let bytes = Cache::with_generation(store.clone(), small, Writes::Allowed, Generation(1));
    assert_eq!(bytes.put(&key(Kind::Probe, 1, "big"), b"1234567", true), Put::Skipped(Skip::TooLarge));
    assert_eq!(bytes.put(&key(Kind::Probe, 1, "one"), b"123456", true), Put::Stored);
    assert_eq!(bytes.put(&key(Kind::Probe, 1, "two"), b"123456", true), Put::Skipped(Skip::Degraded(Degraded::Budget)));
    assert_eq!(bytes.stats().to_json()["puts"], 1);

    // When the kill switch is off, Then nothing is sent
    let off = Cache::with_generation(store.clone(), budget(|b| b["enabled"] = false.into()), Writes::Allowed, Generation(1));
    let before = store.calls();
    assert_eq!((off.get(&moved), off.put(&moved, b"x", true)), (None, Put::Skipped(Skip::Disabled)));
    assert_eq!(store.calls(), before);
}

#[test]
fn given_a_directory_store_when_the_contract_runs_then_every_case_holds() {
    // Given a directory, and a path under a plain file as the store that cannot be reached
    let dir = std::env::temp_dir().join(format!("blob-cache-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plain-file"), b"x").unwrap();

    // When the contract runs against it
    contract(DirStore::new(dir.join("store")), DirStore::new(dir.join("plain-file")));

    // Then two writers racing for one key are told apart, one Stored and one Exists
    let race = Arc::new(DirStore::new(dir.join("race")));
    let k = key(Kind::Nx, 0, "contested");
    let outcomes: Vec<_> = (0..8).map(|i| {
        let (race, k) = (race.clone(), k.clone());
        std::thread::spawn(move || race.put(&k, format!("writer {i}").as_bytes(), true).unwrap())
    }).collect::<Vec<_>>().into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|o| **o == PutOutcome::Stored).count(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn given_a_loopback_bucket_when_the_contract_runs_over_a_par_url_then_every_case_holds_and_no_link_leaks() {
    for conditional in [false, true] {
        // Given a bucket behind a PAR URL, with the write link the same and a link that nothing listens on
        let fake = ParFake::start();
        let store = |url: &str| OciParStore::new(url, Some(url)).via(Via::Direct).conditional_put(conditional);

        // When one put if absent is sent, Then a conditional write is one PUT with if-none-match and the other checks with a HEAD first
        assert_eq!(store(&fake.url).put(&key(Kind::Probe, 0, "one"), b"x", true), Ok(PutOutcome::Stored));
        let sent: Vec<_> = fake.seen().iter().map(|s| (s.method.as_str().to_owned(), s.if_none_match)).collect();
        let sent: Vec<_> = sent.iter().map(|(m, c)| (m.as_str(), *c)).collect();
        let expected = if conditional { vec![("PUT", true)] } else { vec![("HEAD", false), ("PUT", false)] };
        assert_eq!(sent, expected);

        // And the contract holds against it
        contract(store(&fake.url), store(&closed_url()));
    }

    // When the bucket answers 403, 503 and 404 to the same key
    let fake = ParFake::start();
    let store = OciParStore::new(&fake.url, Some(&fake.url)).via(Via::Direct);
    let k = key(Kind::Nx, 0, "status");
    fake.answer_all_with(Some(403));
    let credential = (store.get(&k), store.put(&k, b"x", false));
    fake.answer_all_with(Some(503));
    let server_error = store.get(&k);
    fake.answer_all_with(Some(404));
    let missing = store.get(&k);

    // Then a credential error, an unreachable store and a miss, and a cache over the 403 goes cold for the credential
    assert!(matches!(credential, (Err(StoreError::Credential(_)), Err(StoreError::Credential(_)))));
    assert!(matches!(server_error, Err(StoreError::Unreachable(_))));
    assert_eq!(missing, Ok(None));
    fake.answer_all_with(Some(403));
    let cache = Cache::with_generation(store, Budget::committed().clone(), Writes::Allowed, Generation(0));
    assert_eq!((cache.get(&k), cache.degraded()), (None, Some(Degraded::Credential)));

    // When the bucket fails one request in transit of the store's reads and writes (a 503, then served normally), Then the request is asked again and succeeds
    let fake = ParFake::start();
    let store = OciParStore::new(&fake.url, Some(&fake.url)).via(Via::Direct);
    let k = key(Kind::Nx, 0, "blip");
    assert_eq!(store.put(&k, b"kept", false), Ok(PutOutcome::Stored));
    fake.fail_next(1, 503);
    let read = store.get(&k);
    fake.fail_next(2, 429);
    let written = store.put(&k, b"again", false);
    // And a store that keeps failing is still reported unreachable after the last retry
    fake.answer_all_with(Some(503));
    let down = store.get(&k);
    assert_eq!((read, written), (Ok(Some(b"kept".to_vec())), Ok(PutOutcome::Stored)));
    assert!(matches!(down, Err(StoreError::Unreachable(_))));

    // And no error text carries the token of the link
    let unreachable = OciParStore::new(&closed_url(), None).via(Via::Direct).get(&k).unwrap_err();
    let rejected = OciParStore::new(&closed_url(), None).put(&k, b"x", false).unwrap_err();
    assert!(!format!("{unreachable} {rejected}").contains(TOKEN), "{unreachable} {rejected}");
}

const REFUSAL_CHILD: &str = "BLOB_CACHE_REFUSAL_CHILD";

/// Two puts against a bucket that refuses each as `mode` says, in a child of this test binary so its stderr can be read.
fn two_refused_puts_in_a_child(mode: &str) -> (String, serde_json::Value) {
    let me = std::env::current_exe().expect("this test binary");
    let out = std::process::Command::new(me).args(["--exact", "when_two_puts_are_refused_the_quota_warning_is_one_stderr_line_only_for_the_quota", "--nocapture", "--test-threads=1"]).env(REFUSAL_CHILD, mode).output().expect("the child runs");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let stats = stderr.lines().find_map(|l| l.strip_prefix("STATS ")).and_then(|l| serde_json::from_str(l).ok()).unwrap_or_else(|| panic!("the child printed its stats: {stderr}"));
    (stderr, stats)
}

#[test]
fn when_two_puts_are_refused_the_quota_warning_is_one_stderr_line_only_for_the_quota() {
    if let Ok(mode) = std::env::var(REFUSAL_CHILD) {
        let fake = ParFake::start();
        match mode.as_str() {
            "quota" => fake.answer_all_with(Some(400)),
            "bare-400" => fake.answer_all_with_body(400, ""),
            "other-400" => fake.answer_all_with_body(400, r#"{"code":"InvalidParameter"}"#),
            "403" => fake.answer_all_with_body(403, r#"{"code":"NotAuthorized"}"#),
            _ => fake.answer_all_with_body(503, ""),
        }
        let cache = Cache::with_generation(OciParStore::new(&fake.url, Some(&fake.url)).via(Via::Direct), Budget::committed().clone(), Writes::Allowed, Generation(0));
        cache.put(&key(Kind::Nx, 0, "a"), b"x", false);
        cache.put(&key(Kind::Nx, 0, "b"), b"y", false);
        eprintln!("STATS {}", cache.stats().to_json());
        return;
    }
    // Given a bucket that refuses two writes with the StorageQuotaExceeded body, then the same writes refused for other reasons
    // When the writes are made
    let (stderr, stats) = two_refused_puts_in_a_child("quota");
    // Then both are counted and exactly one line says the tenancy is full
    assert_eq!(stats["quotaExceeded"], serde_json::json!(2), "{stderr}");
    assert_eq!(stderr.lines().filter(|l| l.starts_with("wf: build cache storage quota exceeded")).count(), 1, "{stderr}");
    // And a bare 400, a 400 for another cause, a 403 and a 503 count none and print no such line
    for mode in ["bare-400", "other-400", "403", "503"] {
        let (stderr, stats) = two_refused_puts_in_a_child(mode);
        assert_eq!(stats["quotaExceeded"], serde_json::json!(0), "{mode}: {stderr}");
        assert!(!stderr.contains("quota exceeded"), "{mode}: {stderr}");
    }
}

#[test]
fn given_a_full_tenancy_when_a_put_is_rejected_with_storage_quota_exceeded_then_the_run_counts_it_apart_from_other_refusals() {
    // Given a bucket whose tenancy quota is spent, so every PUT answers HTTP 400 with the StorageQuotaExceeded body
    let fake = ParFake::start();
    fake.answer_all_with(Some(400));
    let cache = Cache::with_generation(OciParStore::new(&fake.url, Some(&fake.url)).via(Via::Direct), Budget::committed().clone(), Writes::Allowed, Generation(0));

    // When two entries are put
    let outcomes = [cache.put(&key(Kind::Nx, 0, "a"), b"x", false), cache.put(&key(Kind::Nx, 0, "b"), b"y", false)];

    // Then both are refused without cooling the cache, and the run's stats say how many hit the quota
    assert!(outcomes.iter().all(|put| matches!(put, Put::Skipped(Skip::Rejected))), "{outcomes:?}");
    let stats = cache.stats().to_json();
    assert_eq!((stats["quotaExceeded"].clone(), stats["puts"].clone(), cache.degraded()), (serde_json::json!(2), serde_json::json!(0), None));

    // And the stats carry the distinct class and the HTTP status of the refusal, and the two writes that did not land
    assert_eq!(stats["lastFailure"], serde_json::json!({ "class": "quota-exceeded", "status": 400 }));
    assert_eq!(stats["droppedPuts"], serde_json::json!(2));

    // And an ordinary refusal of another status is not counted as the quota, but is a dropped put with its own status
    fake.answer_all_with(Some(409));
    cache.put(&key(Kind::Nx, 0, "c"), b"z", false);
    let after = cache.stats().to_json();
    assert_eq!(after["quotaExceeded"], serde_json::json!(2));
    assert_eq!((after["lastFailure"].clone(), after["droppedPuts"].clone()), (serde_json::json!({ "class": "rejected", "status": 409 }), serde_json::json!(3)));
}

#[test]
fn given_a_read_only_run_when_a_put_is_wanted_then_it_is_not_counted_as_a_dropped_write() {
    // Given a run that may only read (a dirty tree, which the guard reports by its own event)
    let fake = ParFake::start();
    let cache = Cache::with_generation(OciParStore::new(&fake.url, None).via(Via::Direct), Budget::committed().clone(), Writes::ReadOnly("dirty".into()), Generation(0));

    // When a put is wanted, Then nothing is sent and no write is counted as dropped
    assert!(matches!(cache.put(&key(Kind::Nx, 0, "a"), b"x", false), Put::Skipped(Skip::ReadOnly)));
    assert_eq!(cache.stats().to_json()["droppedPuts"], serde_json::json!(0));
    assert!(cache.stats().last_failure.is_none());
}

#[test]
fn given_a_run_that_spent_its_request_budget_when_its_writes_are_verified_then_nothing_is_reported_lost_and_an_oversized_entry_is_no_dropped_write() {
    // Given a run whose budget allows exactly the two puts it makes, and an entry cap of 4 bytes for the third
    let dir = std::env::temp_dir().join(format!("blob-cache-verify-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cache = Cache::with_generation(
        DirStore::new(dir.clone()),
        budget(|b| {
            b["requestsPerRun"] = serde_json::json!(2);
            b["kinds"]["nx"]["maxEntryBytes"] = serde_json::json!(4);
        }),
        Writes::Allowed,
        Generation(0),
    );
    assert!(matches!(cache.put(&key(Kind::Nx, 0, "a"), b"x", false), Put::Stored));
    assert!(matches!(cache.put(&key(Kind::Nx, 0, "b"), b"y", false), Put::Stored));
    assert!(matches!(cache.put(&key(Kind::Nx, 0, "big"), b"too large", false), Put::Skipped(Skip::TooLarge)));

    // When the writes are verified
    let seeded = cache.verify_written().unwrap();

    // Then the store is asked past the budget and holds both, and the oversized entry is a deliberate skip, not a dropped write
    assert_eq!((seeded.checked, seeded.present), (2, 2));
    assert_eq!(cache.stats().to_json()["droppedPuts"], serde_json::json!(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn given_the_facts_of_a_checkout_when_writing_is_decided_then_only_a_clean_trunk_commit_on_the_pinned_tools_may_write() {
    // Given a clean checkout of a commit on origin/dev, on the pinned node and pnpm
    let pins = Pins { node: Some("22.22.0".into()), pnpm: Some("11.5.2".into()) };
    let clean = WriteFacts { changed: vec![], head_on_origin_dev: Some(true), node: Some("v22.22.0".into()), pnpm: Some("11.5.2".into()), pins };
    let with = |change: &dyn Fn(&mut WriteFacts)| {
        let mut facts = clean.clone();
        change(&mut facts);
        write_allowed(&facts)
    };

    // When each fact is changed in turn, Then the first refusal names its reason, and a v prefix on a version is not a difference
    assert_eq!(write_allowed(&clean), Ok(()));
    let dirty = with(&|f| f.changed = vec!["?? scratch.txt".into()]).unwrap_err();
    assert_eq!((dirty.reason(), dirty.to_string()), ("dirty", "dirty: the work tree has changes: ?? scratch.txt".to_string()));
    assert_eq!(with(&|f| f.head_on_origin_dev = Some(false)), Err(Refusal::OffDev));
    assert_eq!(with(&|f| f.head_on_origin_dev = None), Err(Refusal::OffDev));
    assert_eq!(with(&|f| f.node = Some("24.13.1".into())).unwrap_err().to_string(), "tools-unpinned: node 24.13.1 is not the pinned 22.22.0");
    assert_eq!(with(&|f| f.pnpm = None).unwrap_err().to_string(), "tools-unpinned: pnpm is not installed");
    assert_eq!(with(&|f| f.pins.node = None).unwrap_err().to_string(), "tools-unpinned: mise.toml pins no node");
    assert_eq!(with(&|f| { f.changed = vec!["M a".into()]; f.head_on_origin_dev = Some(false) }).unwrap_err().reason(), "dirty");
}

#[test]
fn given_the_kinds_when_keys_and_budget_rows_are_built_then_they_follow_the_plan_and_the_json() {
    // Given the committed budget, When a key is built for each kind, Then it is <kind>/g<generation>/<name>
    assert_eq!(key(Kind::Nx, 3, "9f2c").as_str(), "nx/g3/9f2c");
    assert_eq!(key(Kind::Pnpm, 0, "store-v11.tar.gz").as_str(), "pnpm/g0/store-v11.tar.gz");
    assert_eq!(Key::generation_pointer().as_str(), "meta/generation");
    for bad in ["", ".hidden", "..", "a/b", "a b", &"x".repeat(201)] {
        assert!(Key::new(Kind::Nx, Generation(0), bad).is_err(), "{bad:?} is not a blob name");
    }

    // Then the expiry days are the lifecycle rules of the plan and every kind has its prefix round trip and a row
    assert_eq!(Kind::ALL.map(|k| (k.prefix(), k.ttl_days())), [("nx", 14), ("pnpm", 60), ("probe", 1), ("sccache", 7), ("metro", 14)]);
    assert!(Kind::ALL.iter().all(|k| Kind::from_prefix(k.prefix()) == Some(*k) && Budget::committed().max_entry_bytes(*k) > 0));
    assert_eq!(Budget::committed().requests_per_month, 50_000);
    let text = include_str!("../data/budget.json");
    assert!(Budget::parse(&text.replace("\"probe\"", "\"wf\"")).is_err(), "a row for a kind that does not exist, and none for probe");

    // And the generation pointer: absent or unreadable is 0, a number is itself, and a purge makes it one higher
    assert_eq!([None, Some(&b"garbage"[..]), Some(&b" 7\n"[..])].map(Generation::parse), [Generation(0), Generation(0), Generation(7)]);
    assert_eq!(Generation(7).next(), Generation(8));
}

#[test]
fn given_the_domain_sources_when_scanned_then_none_reaches_the_file_system_network_process_clock_or_http_client() {
    // Given every source file of src/domain
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/domain");
    let sources: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();

    // When each code line is searched for an outside effect
    let forbidden = ["std::fs", "std::net", "std::process", "std::env", "std::io", "Command", "cli_http", "SystemTime", "Instant", "crate::ports", "crate::adapters"];
    let hits: Vec<String> = sources.iter().flat_map(|path| {
        let text = std::fs::read_to_string(path).unwrap();
        text.lines().filter(|l| !l.trim_start().starts_with("//")).flat_map(|l| forbidden.iter().filter(move |f| l.contains(*f)).map(|f| format!("{}: {f}", path.display()))).collect::<Vec<_>>()
    }).collect();

    // Then there is something scanned and nothing found
    assert!(sources.len() >= 6, "{sources:?}");
    assert!(hits.is_empty(), "{hits:?}");
}

struct Slow<S>(S, std::time::Duration);

impl<S: BlobStore> BlobStore for Slow<S> {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        std::thread::sleep(self.1);
        self.0.get(key)
    }
    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        std::thread::sleep(self.1);
        self.0.put(key, body, if_absent)
    }
    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        std::thread::sleep(self.1);
        self.0.exists(key)
    }
}

#[test]
fn given_a_slow_store_when_the_run_reads_writes_and_checks_then_each_operation_reports_how_long_it_took() {
    // Given a store that answers each call after 20 ms
    let dir = std::env::temp_dir().join(format!("blob-cache-latency-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let delay = std::time::Duration::from_millis(20);
    let cache = Cache::with_generation(Slow(DirStore::new(dir.join("store")), delay), Budget::committed().clone(), Writes::Allowed, Generation(0));
    let k = key(Kind::Nx, 0, "timed");

    // When the run misses, writes, hits and checks existence once each
    assert!(cache.get(&k).is_none());
    assert_eq!(cache.put(&k, b"entry", true), Put::Stored);
    assert!(cache.get(&k).is_some());
    assert_eq!(cache.exists(&k), Ok(true));

    // Then every operation was counted and none took less than the store's delay
    let stats = cache.stats();
    assert_eq!((stats.get.n, stats.put.n, stats.exists.n), (2, 1, 1));
    assert!([stats.get.max_us, stats.put.max_us, stats.exists.max_us].iter().all(|us| *us >= 20_000), "{stats:?}");
    // And the report carries them in milliseconds
    let report = stats.to_json();
    assert_eq!(report["latencyMs"]["get"]["n"], 2);
    assert!(report["latencyMs"]["get"]["sumMs"].as_f64().unwrap() >= 40.0, "{report}");
    let _ = std::fs::remove_dir_all(&dir);
}
