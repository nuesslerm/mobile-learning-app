//! The pnpm store as one cache object, over a directory bucket and the real `tar`: restored only into an empty store, saved only
//! after a green install of a lockfile the bucket has not seen and only when the write guard allows it.
use blob_cache::adapters::{DirStore, TarGz};
use blob_cache::domain::pnpm::{Layout, Sidecar};
use blob_cache::domain::{Budget, Generation, Key, Kind};
use blob_cache::pnpm::{restore, save, Outcome, Reason, SaveFacts};
use blob_cache::ports::{BlobStore, Packer, PutOutcome, StoreError};
use blob_cache::{Cache, Writes};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A directory bucket that remembers every request and the names written.
pub(super) struct Bucket {
    inner: DirStore,
    requests: AtomicU64,
    written: Mutex<Vec<String>>,
}

impl Bucket {
    pub(super) fn at(root: &Path) -> Arc<Bucket> {
        Arc::new(Bucket { inner: DirStore::new(root), requests: AtomicU64::new(0), written: Mutex::new(Vec::new()) })
    }

    pub(super) fn requests(&self) -> u64 {
        self.requests.load(Ordering::SeqCst)
    }

    pub(super) fn written(&self) -> Vec<String> {
        self.written.lock().unwrap().clone()
    }

    fn count(&self) {
        self.requests.fetch_add(1, Ordering::SeqCst);
    }
}

impl BlobStore for Bucket {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        self.count();
        self.inner.get(key)
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.count();
        self.written.lock().unwrap().push(key.to_string());
        self.inner.put(key, body, if_absent)
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        self.count();
        self.inner.exists(key)
    }

    fn get_file(&self, key: &Key, dest: &Path) -> Result<bool, StoreError> {
        self.count();
        self.inner.get_file(key, dest)
    }

    fn put_file(&self, key: &Key, source: &Path, if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.count();
        self.written.lock().unwrap().push(key.to_string());
        self.inner.put_file(key, source, if_absent)
    }
}

pub(super) struct Scratch(PathBuf);

impl Scratch {
    pub(super) fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("blob-cache-pnpm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    pub(super) fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A store of three content-addressed files, one of them incompressible, so the archive has a size.
pub(super) fn fill_store(store: &Path, salt: u8) {
    let mut noise = Vec::new();
    let mut state = 0x2545F491u32 ^ u32::from(salt);
    for _ in 0..24_000 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        noise.push((state >> 24) as u8);
    }
    for (name, body) in [("index/ab/cd-index.json", br#"{"name":"left-pad"}"#.to_vec()), ("files/00/aabbcc", noise), ("files/ff/exec-sha512", vec![salt; 40])] {
        let path = store.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
}

pub(super) fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, into);
            } else {
                into.insert(path.strip_prefix(root).unwrap().display().to_string(), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(dir, dir, &mut files);
    files
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    names
}

fn layout() -> Layout {
    Layout::parse("v11").unwrap()
}

fn cache(bucket: &Arc<Bucket>, writes: Writes) -> Cache<Arc<Bucket>> {
    Cache::with_generation(bucket.clone(), Budget::committed().for_one_entry_of(Kind::Pnpm), writes, Generation(0))
}

fn archive_key() -> Key {
    Key::new(Kind::Pnpm, Generation(0), "store-v11.tar.gz").unwrap()
}

fn sidecar_key() -> Key {
    Key::new(Kind::Pnpm, Generation(0), "store-v11.json").unwrap()
}

#[test]
fn given_a_bucket_holding_the_store_when_restore_runs_then_only_an_empty_local_store_is_filled_and_nothing_failing_is_red() {
    // Given an object in the bucket made from a populated store, and the committed names for it
    let scratch = Scratch::new("restore");
    let seed = scratch.path("seed/v11");
    fill_store(&seed, 1);
    let packed = scratch.path("seed.tar.gz");
    TarGz.pack(&seed, &packed).unwrap();
    let bucket = Bucket::at(&scratch.path("bucket"));
    bucket.inner.put_file(&archive_key(), &packed, false).unwrap();
    bucket.requests.store(0, Ordering::SeqCst);
    let want = snapshot(&seed);
    let run = |store: &Path, bucket: &Arc<Bucket>| restore(&cache(bucket, Writes::ReadOnly("restore reads only".into())), &TarGz, store, &layout());

    // When the local store already has content
    let held = scratch.path("held/v11");
    fill_store(&held, 9);
    let before = snapshot(&held);
    let outcome = run(&held, &bucket);
    // Then nothing is asked of the bucket and nothing local changes
    assert_eq!((outcome, bucket.requests(), snapshot(&held)), (Outcome::Nothing(Reason::StoreNotEmpty), 0, before));

    // When the local store does not exist, and again when it exists and is empty
    for (name, create) in [("absent", false), ("empty", true)] {
        let store = scratch.path(&format!("{name}/v11"));
        if create {
            std::fs::create_dir_all(&store).unwrap();
        }
        let outcome = run(&store, &bucket);
        // Then it holds the saved files exactly, and no scratch is left beside it
        assert!(matches!(outcome, Outcome::Done { bytes } if bytes > 0), "{name}: {outcome:?}");
        assert_eq!(snapshot(&store), want, "{name}");
        assert_eq!(names_in(store.parent().unwrap()), ["v11"], "{name}");
    }

    // When the bucket has no object, and when it cannot be reached
    let missing = scratch.path("missing/v11");
    let outcome = run(&missing, &Bucket::at(&scratch.path("empty-bucket")));
    std::fs::write(scratch.path("plain-file"), b"x").unwrap();
    let down = scratch.path("down/v11");
    let unreachable = run(&down, &Bucket::at(&scratch.path("plain-file/bucket")));
    // Then each is a cold run with its reason, the store stays empty and no scratch is left
    assert_eq!(outcome, Outcome::Nothing(Reason::NoObject));
    assert_eq!(unreachable, Outcome::Nothing(Reason::Cold("unreachable")));
    assert_eq!((names_in(&missing), names_in(&down)), (Vec::<String>::new(), Vec::<String>::new()));
    assert_eq!((names_in(missing.parent().unwrap()), names_in(down.parent().unwrap())), (Vec::<String>::new(), Vec::<String>::new()));
}

#[test]
fn given_an_installed_store_when_save_runs_then_it_writes_one_object_only_after_a_green_install_of_a_new_lockfile_and_only_if_the_guard_allows() {
    // Given a populated store, and a bucket that records what is asked of it
    let scratch = Scratch::new("save");
    let store = scratch.path("pnpm/v11");
    fill_store(&store, 1);
    let bucket = Bucket::at(&scratch.path("bucket"));
    let facts = |lockfile: &'static str, installed: Option<&'static str>| (lockfile, installed);
    let run = |cache: &Cache<Arc<Bucket>>, (lockfile, installed): (&str, Option<&str>)| {
        let layout = layout();
        save(cache, &TarGz, &SaveFacts { store: &store, layout: &layout, lockfile_sha256: lockfile, installed_lockfile_sha256: installed })
    };
    let objects = || names_in(&scratch.path("bucket/pnpm/g0"));

    // When the guard refuses the write
    let refused = run(&cache(&bucket, Writes::ReadOnly("dirty: the work tree has changes: a.txt".into())), facts("aaa", Some("aaa")));
    // Then nothing is sent and nothing is stored
    assert_eq!((refused, bucket.requests(), objects()), (Outcome::Nothing(Reason::GuardRefused("dirty: the work tree has changes: a.txt".into())), 0, Vec::<String>::new()));

    // When the install on disk is not the lockfile's (a failed one leaves the old tree, or none)
    let stale = run(&cache(&bucket, Writes::Allowed), facts("aaa", Some("zzz")));
    let none = run(&cache(&bucket, Writes::Allowed), facts("aaa", None));
    // Then nothing is sent either
    assert_eq!((stale, none, bucket.requests()), (Outcome::Nothing(Reason::InstallNotCurrent), Outcome::Nothing(Reason::InstallNotCurrent), 0));

    // When a run's byte limit is below the archive, as an Nx run's is, and when the budget is the one a single pnpm entry gets
    let tight = Budget::parse(&include_str!("../../data/budget.json").replace("268435456", "1000")).unwrap();
    let small = Cache::with_generation(bucket.clone(), tight.clone(), Writes::Allowed, Generation(0));
    let refused_by_budget = run(&small, facts("aaa", Some("aaa")));
    let first = run(&Cache::with_generation(bucket.clone(), tight.for_one_entry_of(Kind::Pnpm), Writes::Allowed, Generation(0)), facts("aaa", Some("aaa")));
    // Then the plain limit makes it cold and the pnpm one writes the archive, then the sidecar naming the lockfile and the size
    assert_eq!(refused_by_budget, Outcome::Nothing(Reason::Cold("budget")));
    let Outcome::Done { bytes } = first else { panic!("{first:?}") };
    assert_eq!(objects(), ["store-v11.json", "store-v11.tar.gz"]);
    assert_eq!(bucket.written(), ["probe/g0/pnpm-save", "probe/g0/pnpm-save", "pnpm/g0/store-v11.tar.gz", "pnpm/g0/store-v11.json"]);
    let recorded = Sidecar::parse(&bucket.inner.get(&sidecar_key()).unwrap().unwrap()).unwrap();
    assert_eq!(recorded, Sidecar { lockfile_sha256: "aaa".into(), bytes });
    assert_eq!(std::fs::metadata(scratch.path("bucket/pnpm/g0/store-v11.tar.gz")).unwrap().len(), bytes);
    assert_eq!(names_in(scratch.path("pnpm").as_path()), ["v11"], "no scratch is left beside the store");

    // When the same lockfile saves again
    let writes = bucket.written().len();
    let again = run(&cache(&bucket, Writes::Allowed), facts("aaa", Some("aaa")));
    // Then it only looks: the sidecar and the object, and nothing is written
    assert_eq!((again, bucket.written().len()), (Outcome::Nothing(Reason::LockfileUnchanged), writes));

    // When the object has expired but its sidecar has not
    std::fs::remove_file(scratch.path("bucket/pnpm/g0/store-v11.tar.gz")).unwrap();
    let healed = run(&cache(&bucket, Writes::Allowed), facts("aaa", Some("aaa")));
    // Then the object is written again
    assert!(matches!(healed, Outcome::Done { .. }), "{healed:?}");
    assert_eq!(objects(), ["store-v11.json", "store-v11.tar.gz"]);

    // When the lockfile changes and the store with it
    std::fs::remove_dir_all(&store).unwrap();
    fill_store(&store, 2);
    let before = std::fs::read(scratch.path("bucket/pnpm/g0/store-v11.tar.gz")).unwrap();
    let changed = run(&cache(&bucket, Writes::Allowed), facts("bbb", Some("bbb")));
    // Then the same one object is overwritten in place and the sidecar follows the lockfile
    assert!(matches!(changed, Outcome::Done { .. }), "{changed:?}");
    assert_eq!(objects(), ["store-v11.json", "store-v11.tar.gz"]);
    assert_ne!(std::fs::read(scratch.path("bucket/pnpm/g0/store-v11.tar.gz")).unwrap(), before);
    assert_eq!(Sidecar::parse(&bucket.inner.get(&sidecar_key()).unwrap().unwrap()).unwrap().lockfile_sha256, "bbb");
}

/// A store that takes reads and refuses every write the way a full tenancy does.
struct Full(Arc<Bucket>);

impl BlobStore for Full {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        self.0.get(key)
    }

    fn put(&self, _key: &Key, _body: &[u8], _if_absent: bool) -> Result<PutOutcome, StoreError> {
        Err(StoreError::Rejected("HTTP 400 StorageQuotaExceeded".into()))
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        self.0.exists(key)
    }
}

/// A packer that fails the test when asked: the point of the checks before packing is that it is never reached.
struct NeverPacks;

impl Packer for NeverPacks {
    fn pack(&self, _dir: &Path, _archive: &Path) -> Result<(), String> {
        panic!("the store was packed although the write would be refused")
    }

    fn unpack(&self, _archive: &Path, _dir: &Path) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn given_a_store_the_bucket_would_refuse_when_save_runs_then_it_is_not_packed_and_the_reason_is_the_full_storage_or_the_size() {
    // Given an installed store and a bucket whose tenancy is full
    let scratch = Scratch::new("refused");
    let store = scratch.path("pnpm/v11");
    fill_store(&store, 1);
    let layout = layout();
    let facts = SaveFacts { store: &store, layout: &layout, lockfile_sha256: "aaa", installed_lockfile_sha256: Some("aaa") };
    let full = Cache::with_generation(Full(Bucket::at(&scratch.path("bucket"))), Budget::committed().for_one_entry_of(Kind::Pnpm), Writes::Allowed, Generation(0));

    // When the save runs
    let outcome = save(&full, &NeverPacks, &facts);

    // Then the probe write told it so before any packing, and the full storage is counted
    assert_eq!(outcome, Outcome::Nothing(Reason::StorageFull));
    assert_eq!(full.stats().quota_exceeded, 1);

    // When the entry cap is far below what the store holds
    let tiny = Budget::parse(&include_str!("../../data/budget.json").replace("1073741824", "1")).unwrap().for_one_entry_of(Kind::Pnpm);
    let roomy = Bucket::at(&scratch.path("roomy"));
    let over = save(&Cache::with_generation(roomy.clone(), tiny, Writes::Allowed, Generation(0)), &NeverPacks, &facts);

    // Then the size check stops it before packing, and nothing is written
    assert_eq!((over, roomy.written()), (Outcome::Nothing(Reason::TooLarge), Vec::<String>::new()));
}

#[test]
fn given_writes_a_run_made_when_they_are_looked_up_then_the_count_present_says_whether_the_store_holds_them() {
    // Given a cache that stored three entries and a store that then lost one
    let scratch = Scratch::new("seeded");
    let bucket = Bucket::at(&scratch.path("bucket"));
    let cache = cache(&bucket, Writes::Allowed);
    let keys: Vec<Key> = ["a", "b", "c"].iter().map(|n| Key::new(Kind::Nx, Generation(0), n).unwrap()).collect();
    for key in &keys {
        cache.put(key, b"result", true);
    }
    assert_eq!(cache.verify_written().map(|s| (s.checked, s.present)), Some((3, 3)));
    std::fs::remove_file(scratch.path("bucket/nx/g0/b")).unwrap();

    // When the run asks for them again
    let seeded = cache.verify_written();

    // Then it reports 2 of 3, and the stats carry it
    assert_eq!(seeded.map(|s| (s.checked, s.present)), Some((3, 2)));
    assert_eq!(cache.stats().to_json()["seeded"], serde_json::json!({ "checked": 3, "present": 2 }));

    // When a run wrote nothing
    let idle = blob_cache_idle(&scratch);

    // Then there is nothing to ask for
    assert_eq!(idle, None);
}

fn blob_cache_idle(scratch: &Scratch) -> Option<blob_cache::domain::Seeded> {
    cache(&Bucket::at(&scratch.path("idle")), Writes::Allowed).verify_written()
}
