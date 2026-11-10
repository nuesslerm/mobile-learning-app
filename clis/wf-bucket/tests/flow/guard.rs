//! `wf cache guard`: the eviction policy through the real entry point over a stub `oci` that holds a planted tenancy (`--local`, as
//! agent-shell runs it), and the relay a CI runner does through a fake vm-ops-api. Nothing reaches a cloud.
use crate::support::{create_stub, iso_from_ms, last_json, now_ms, run_wf, RunOpts, RunResult};
use blob_cache::domain::Budget;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use wf_vmops::testing::{answers, FakeVm};

const CACHE: &str = "mobile-learning-build-cache";
const EVIDENCE: &str = "mobile-learning-verification-artifacts";
const GB: u64 = 1_000_000_000;

const GOLDENS: &str = "mobile-learning-golden-screenshots";
const TFSTATE: &str = "mobile-learning-tfstate";

const OCI: &str = r#"here="$(dirname "$0")"
echo "$*" >> "$here/calls.log"
bucket="$(echo "$*" | sed -n 's/.*--bucket-name \([^ ]*\).*/\1/p')"
name="$(echo "$*" | sed -n 's/.*--object-name \([^ ]*\).*/\1/p')"
case "$*" in
  "os ns get") echo '{"data":"ns1"}' ;;
  "os bucket get "*) cat "$here/bucket-$bucket.json" ;;
  "os bucket list "*) cat "$here/buckets.json" ;;
  "os object list "*) [ ! -f "$here/list-delay" ] || sleep "$(cat "$here/list-delay")"; cat "$here/objects-$bucket.json" ;;
  "os object get "*)
    got="$(echo "$*" | sed -n 's/.*--name \([^ ]*\).*/\1/p')"
    file="$here/get-$(echo "$got" | tr / _)"
    if [ -f "$file" ]; then cat "$file"; else echo "ServiceError: ObjectNotFound" >&2; exit 1; fi ;;
  "os object delete "*)
    [ ! -f "$here/delete-delay" ] || sleep "$(cat "$here/delete-delay")"
    if grep -qxF -- "$name" "$here/changed.txt" 2>/dev/null; then echo '{"code": "IfMatchFailed", "status": 412}' >&2; exit 1; fi
    if grep -qxF -- "$name" "$here/failing.txt" 2>/dev/null; then echo 'ServiceError: 500 internal error' >&2; exit 1; fi
    echo "${bucket#mobile-learning-}:$name" >> "$here/removed.log" ;;
  *) echo "unexpected oci call: $*" >&2; exit 64 ;;
esac"#;

type Planted = Vec<(String, u64, f64)>;

struct Tenancy {
    dir: tempfile::TempDir,
    planted: RefCell<BTreeMap<String, Planted>>,
}

impl Tenancy {
    fn new() -> Tenancy {
        let dir = tempfile::tempdir().expect("a temp dir");
        create_stub(&dir.path().join("oci"), OCI);
        Tenancy { dir, planted: RefCell::new(BTreeMap::new()) }
    }

    fn write(&self, name: &str, value: &Value) {
        self.put(name, &value.to_string());
    }

    fn put(&self, name: &str, text: &str) {
        std::fs::write(self.dir.path().join(name), text).expect("a fixture");
    }

    /// The four buckets, `total` bytes in all: what the build cache and the evidence bucket list is what they hold (a protected filler
    /// object makes up the difference), the other two hold 100 MB each by the store's own figure.
    fn used(&self, total: u64) {
        self.sized(total, [CACHE, EVIDENCE].map(|b| (b, None)));
    }

    /// As `used`, with the build cache and evidence buckets' own size figures given: they lag the listings after a delete.
    fn sized(&self, total: u64, figures: [(&str, Option<u64>); 2]) {
        let mut planted = self.planted.borrow_mut();
        for objects in planted.values_mut() {
            objects.retain(|o| o.0 != "verification/verify/filler");
        }
        let listed: u64 = [CACHE, EVIDENCE].iter().map(|b| planted.get(*b).map_or(0, |objects| objects.iter().map(|o| o.1).sum::<u64>())).sum();
        let filler = total.checked_sub(200_000_000 + listed).expect("the planted objects fit in the total");
        planted.entry(EVIDENCE.to_string()).or_default().push(("verification/verify/filler".into(), filler, 40.0));
        let now = now_ms();
        for (bucket, objects) in planted.iter() {
            let data: Vec<Value> = objects.iter().map(|(name, size, days)| json!({ "name": name, "size": size, "time-created": iso_from_ms(now - (*days * 86_400_000.0) as i128), "etag": format!("etag-{name}") })).collect();
            self.write(&format!("objects-{bucket}.json"), &json!({ "data": data }));
        }
        let figure = |name: &str| figures.iter().find(|(b, _)| *b == name).and_then(|(_, bytes)| *bytes).unwrap_or(100_000_000);
        for name in [CACHE, EVIDENCE, GOLDENS, TFSTATE] {
            self.write(&format!("bucket-{name}.json"), &json!({ "data": { "approximate-size": figure(name), "compartment-id": "ocid1.compartment.oc1..test", "name": name } }));
        }
        let all: Vec<Value> = [CACHE, EVIDENCE, GOLDENS, TFSTATE].iter().map(|name| json!({ "name": name })).collect();
        self.write("buckets.json", &json!({ "data": all }));
    }

    /// `(name, bytes, days old)`.
    fn objects(&self, bucket: &str, listed: &[(&str, u64, f64)]) {
        self.planted.borrow_mut().insert(bucket.to_string(), listed.iter().map(|(name, size, days)| (name.to_string(), *size, *days)).collect());
    }

    fn pointer(&self, group: &str) {
        self.put("get-verification_eas-updates_latest-dev.json", &json!({ "published": true, "branch": "dev", "group": group }).to_string());
    }

    fn plant(&self) {
        self.objects(CACHE, &[
            ("sccache/g0/s1", GB, 10.0),
            ("sccache/g0/s2", GB, 5.0),
            ("sccache/g0/fresh", GB, 0.5),
            ("metro/g0/app-build.tar.gz", GB / 2, 4.0),
            ("probe/g0/p1", GB / 10, 3.0),
            ("nx/g0/old", GB, 12.0),
            ("nx/g0/recent", GB, 2.0),
            ("nx/g0/today", GB, 0.2),
            ("pnpm/g0/store-v11.tar.gz", GB, 20.0),
            ("pnpm/g0/store-v11.json", 100, 20.0),
            ("pnpm/g1/store-v11.tar.gz", GB, 3.0),
            ("pnpm/g1/store-v11.json", 100, 3.0),
            ("meta/generation", 1, 30.0),
            ("libs/index/cr/at/crate-a", GB, 40.0),
        ]);
        self.objects(EVIDENCE, &[
            ("verification/test-runs/r1/bundle.json", GB, 10.0),
            ("verification/test-runs/r2/bundle.json", GB, 2.0),
            ("verification/test-runs/index.json", GB, 30.0),
            ("verification/eas-updates/g1/summary.json", GB, 6.0),
            ("verification/eas-updates/g2/summary.json", 1000, 9.0),
            ("verification/eas-updates/g2/maps/app.map", 1000, 9.0),
            ("verification/eas-updates/g3/summary.json", 1000, 2.0),
            ("verification/eas-updates/latest-dev.json", 100, 20.0),
            ("verification/device-lab/run-1/video.mp4", GB, 5.0),
            ("verification/device-lab/run-2/video.mp4", 1000, 1.5),
            ("verification/device-lab/index.json", 100, 30.0),
            ("verification/device-lab", 100, 30.0),
            ("verification/verify/leetcode/shot.png", GB, 40.0),
        ]);
        self.objects(GOLDENS, &[("goldens/home.png", 1000, 90.0), ("goldens/index.json", 100, 90.0)]);
        self.objects(TFSTATE, &[("terraform.tfstate", 100_000, 90.0), ("terraform.tfstate.backup", 100_000, 90.0)]);
        self.pointer("g2");
    }

    fn wf(&self, args: &[&str]) -> RunResult {
        run_wf(args, &RunOpts::default().bin_dir(self.dir.path()))
    }

    fn wf_env(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let opts = env.iter().fold(RunOpts::default().bin_dir(self.dir.path()), |opts, (k, v)| opts.env(k, v));
        run_wf(args, &opts)
    }

    fn forget_calls(&self) {
        for file in ["calls.log", "removed.log"] {
            let _ = std::fs::remove_file(self.dir.path().join(file));
        }
    }

    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("calls.log")).unwrap_or_default().lines().map(String::from).collect()
    }

    /// What the stub removed, as `<bucket>:<name>`.
    fn deleted(&self) -> BTreeSet<String> {
        std::fs::read_to_string(self.dir.path().join("removed.log")).unwrap_or_default().lines().map(String::from).collect()
    }
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|name| name.to_string()).collect()
}

fn sizes(names: &[&str], table: &[(&str, u64)]) -> u64 {
    names.iter().map(|n| table.iter().find(|(t, _)| n.ends_with(t)).map_or(0, |(_, bytes)| *bytes)).sum()
}

#[test]
fn over_the_high_water_mark_the_guard_deletes_cheapest_first_and_stops_at_the_low_water_mark_and_never_touches_what_it_must_keep() {
    // Given a tenancy planted with every class of object, old and young, and a cap read from the committed budget
    let cap = Budget::committed().tenancy_stored_bytes;
    assert_eq!(cap, 20 * GB);
    let tenancy = Tenancy::new();
    tenancy.plant();
    // The cheapest-first order the policy promises, with each object's size in bytes
    let order: [(&str, u64); 9] = [
        ("build-cache:sccache/g0/s1", GB),
        ("build-cache:sccache/g0/s2", GB),
        ("build-cache:metro/g0/app-build.tar.gz", GB / 2),
        ("build-cache:probe/g0/p1", GB / 10),
        ("build-cache:nx/g0/old", GB),
        ("build-cache:nx/g0/recent", GB),
        ("build-cache:pnpm/g0/store-v11.json", 100),
        ("build-cache:pnpm/g0/store-v11.tar.gz", GB),
        ("verification-artifacts:verification/test-runs/r1/bundle.json", GB),
    ];
    let kept = [
        "build-cache:sccache/g0/fresh", "build-cache:nx/g0/today", "build-cache:pnpm/g1/store-v11.tar.gz", "build-cache:pnpm/g1/store-v11.json",
        "build-cache:meta/generation", "build-cache:libs/index/cr/at/crate-a", "verification-artifacts:verification/test-runs/r2/bundle.json",
        "verification-artifacts:verification/test-runs/index.json", "verification-artifacts:verification/eas-updates/latest-dev.json", "verification-artifacts:verification/verify/leetcode/shot.png",
        "verification-artifacts:verification/eas-updates/g2/summary.json", "verification-artifacts:verification/eas-updates/g2/maps/app.map", "verification-artifacts:verification/eas-updates/g3/summary.json",
        "verification-artifacts:verification/device-lab/run-2/video.mp4", "verification-artifacts:verification/device-lab/index.json", "verification-artifacts:verification/device-lab",
        "golden-screenshots:goldens/home.png", "golden-screenshots:goldens/index.json", "tfstate:terraform.tfstate", "tfstate:terraform.tfstate.backup",
    ];
    let low = cap / 100 * 70;

    // When usage is just over the high-water mark (85 percent), then well over it, each time asking for the least that fits
    for (used, objects) in [(17_500_000_000u64, 5usize), (19_900_000_000, 9)] {
        tenancy.used(used);
        tenancy.forget_calls();
        let r = tenancy.wf(&["cache", "guard", "--local"]);

        // Then exactly the first objects of the order go, the fewest that bring usage under the low-water mark
        assert_eq!(r.exit_code, 0, "{r:?}");
        assert_eq!(r.stdout.lines().count(), 1, "one line on stdout: {r:?}");
        let freed: u64 = order[..objects].iter().map(|(_, bytes)| bytes).sum();
        assert!(used - freed <= low && used - (freed - order[objects - 1].1) > low, "the case wants the shortest prefix: {used} {freed}");
        assert_eq!(tenancy.deleted(), order[..objects].iter().map(|(name, _)| name.to_string()).collect::<BTreeSet<_>>(), "{used}: {r:?}");
        let line = last_json(&r);
        assert_eq!((line["event"].clone(), line["action"].clone(), line["ok"].clone()), (json!("storage-guard"), json!("evicted"), json!(true)), "{line}");
        assert_eq!((line["usedBytes"].clone(), line["capBytes"].clone(), line["deletedObjects"].clone(), line["freedBytes"].clone(), line["usedBytesAfter"].clone()), (json!(used), json!(cap), json!(objects), json!(freed), json!(used - freed)));
        for object in kept {
            assert!(!tenancy.deleted().contains(object), "{object} must stay");
        }
        // And each delete names the version it was listed with, so an object rewritten since is not deleted
        assert!(tenancy.calls().iter().any(|call| call.starts_with("os object delete ") && call.contains("--object-name sccache/g0/s1 ") && call.contains("--if-match etag-sccache/g0/s1")), "{:?}", tenancy.calls());
    }

    // And the line carries each bucket's bytes (what its listing adds up to) and the layers of the two buckets it can evict from
    let line = last_json(&tenancy.wf(&["cache", "guard", "--local"]));
    assert_eq!(line["buckets"][CACHE], json!(9_600_000_201u64));
    assert_eq!(line["layers"][CACHE]["sccache"], json!(3 * GB));
    assert_eq!(line["layers"][EVIDENCE]["test-runs"], json!(3 * GB));
    assert_eq!(line["evicted"]["sccache"], json!({ "objects": 2, "bytes": 2 * GB }));
}

#[test]
fn when_everything_that_may_go_is_not_enough_the_guard_deletes_all_of_it_and_says_it_fell_short_and_never_lists_the_buckets_it_may_not_touch() {
    // Given a tenancy far over the cap that holds only 8.6 GB of evictable data, old objects in the terraform state and goldens
    // buckets with listings a careless guard could delete from, and an update group an old pointer still names
    let tenancy = Tenancy::new();
    tenancy.plant();
    tenancy.used(25 * GB);

    // When the guard runs
    let r = tenancy.wf(&["cache", "guard", "--local"]);

    // Then every evictable object is deleted, none of the protected ones, and the action is short, still with exit 0
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert_eq!(
        tenancy.deleted(),
        set(&[
            "build-cache:sccache/g0/s1", "build-cache:sccache/g0/s2", "build-cache:metro/g0/app-build.tar.gz", "build-cache:probe/g0/p1", "build-cache:nx/g0/old", "build-cache:nx/g0/recent",
            "build-cache:pnpm/g0/store-v11.json", "build-cache:pnpm/g0/store-v11.tar.gz", "verification-artifacts:verification/test-runs/r1/bundle.json",
            "verification-artifacts:verification/eas-updates/g1/summary.json", "verification-artifacts:verification/device-lab/run-1/video.mp4",
        ])
    );
    let touched_others = tenancy.calls().iter().any(|call| call.starts_with("os object") && (call.contains(&format!("--bucket-name {TFSTATE}")) || call.contains(&format!("--bucket-name {GOLDENS}"))));
    assert!(!touched_others, "no object of tfstate or goldens is listed or deleted: {:?}", tenancy.calls());
    let line = last_json(&r);
    assert_eq!((line["action"].clone(), line["deletedObjects"].clone(), line["skipCode"].clone()), (json!("short"), json!(11), Value::Null), "{line}");
}

#[test]
fn an_update_group_the_pointer_names_is_kept_however_old_and_when_a_pointer_cannot_be_read_no_update_group_goes() {
    // Given a quiet branch: its pointer names g2, published 9 days ago, and g1 is an older group nothing names
    let tenancy = Tenancy::new();
    tenancy.plant();
    tenancy.used(25 * GB);

    // When the guard has to free everything it can
    let r = tenancy.wf(&["cache", "guard", "--local"]);

    // Then g1 goes and every object of g2 stays
    assert_eq!(r.exit_code, 0, "{r:?}");
    let gone = tenancy.deleted();
    assert!(gone.contains("verification-artifacts:verification/eas-updates/g1/summary.json"), "{gone:?}");
    assert!(!gone.iter().any(|name| name.contains("eas-updates/g2/") || name.contains("latest-dev")), "{gone:?}");

    // And when the pointer is not a readable update summary, no group is a candidate, the rest still is, and the line says why
    tenancy.forget_calls();
    tenancy.put("get-verification_eas-updates_latest-dev.json", "<html>bad gateway</html>");
    let blind = tenancy.wf(&["cache", "guard", "--local"]);
    assert_eq!(blind.exit_code, 0, "{blind:?}");
    let gone = tenancy.deleted();
    assert!(!gone.iter().any(|name| name.contains("eas-updates/")), "{gone:?}");
    assert!(gone.contains("verification-artifacts:verification/device-lab/run-1/video.mp4"), "{gone:?}");
    assert_eq!(last_json(&blind)["skipCode"], json!("pointer-unreadable"), "{blind:?}");
}

#[test]
fn a_dry_run_reports_what_it_would_delete_and_deletes_nothing() {
    // Given a tenancy well over the high-water mark
    let tenancy = Tenancy::new();
    tenancy.plant();
    tenancy.used(19_900_000_000);

    // When the guard runs with --dry-run
    let r = tenancy.wf(&["cache", "guard", "--local", "--dry-run"]);

    // Then no delete reaches the store and the line lists the nine objects a real run deletes, with their bytes
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(tenancy.calls().iter().all(|call| !call.starts_with("os object delete ")), "{:?}", tenancy.calls());
    assert_eq!(tenancy.deleted(), set(&[]));
    let line = last_json(&r);
    assert_eq!((line["action"].clone(), line["deletedObjects"].clone(), line["usedBytes"].clone()), (json!("would-evict"), json!(0), json!(19_900_000_000u64)), "{line}");
    assert_eq!((line["wouldDelete"]["objects"].clone(), line["wouldDelete"]["bytes"].clone()), (json!(9), json!(6 * GB + GB / 2 + GB / 10 + 100)), "{line}");
    assert_eq!(line["wouldDelete"]["listed"][0], json!({ "bucket": CACHE, "name": "sccache/g0/s1", "bytes": GB, "class": "sccache" }));

    // And under the high-water mark a dry run says there is nothing to do
    tenancy.used(16 * GB);
    assert_eq!(last_json(&tenancy.wf(&["cache", "guard", "--local", "--dry-run"]))["action"], json!("none"));
}

#[test]
fn usage_is_what_the_listings_add_up_to_so_a_stale_size_figure_after_a_delete_does_not_evict_again() {
    // Given the build cache and evidence listings add up to 16 GB, below the high-water mark, while the stores' own size figures
    // still show the state before the last eviction
    let tenancy = Tenancy::new();
    tenancy.plant();
    tenancy.sized(16 * GB, [(CACHE, Some(19 * GB)), (EVIDENCE, Some(GB))]);

    // When the guard runs
    let r = tenancy.wf(&["cache", "guard", "--local"]);

    // Then it measures 16 GB, deletes nothing, and reports the bucket sizes it counted
    assert_eq!(r.exit_code, 0, "{r:?}");
    let line = last_json(&r);
    assert_eq!((line["action"].clone(), line["usedBytes"].clone(), line["deletedObjects"].clone()), (json!("none"), json!(16 * GB), json!(0)), "{line}");
    assert_eq!(tenancy.deleted(), set(&[]));
    assert_eq!(line["buckets"][GOLDENS], json!(100_000_000));
    assert_ne!(line["buckets"][CACHE], json!(19 * GB));
}

#[test]
fn a_delete_that_fails_half_way_still_counts_what_was_deleted_and_an_object_rewritten_since_the_listing_is_left_alone() {
    // Given usage over the high-water mark, and a store that fails one delete (nx/g0/old) and refuses another because it changed
    let tenancy = Tenancy::new();
    tenancy.plant();
    tenancy.used(19_900_000_000);
    tenancy.put("failing.txt", "nx/g0/old\n");

    // When the guard runs
    let r = tenancy.wf(&["cache", "guard", "--local"]);

    // Then exactly the objects the store removed are counted, with their bytes, the line says the delete failed, and nothing after it ran
    assert_eq!(r.exit_code, 0, "{r:?}");
    let removed = tenancy.deleted();
    assert_eq!(removed.len(), 7, "{removed:?}");
    assert!(!removed.contains("build-cache:nx/g0/old") && !removed.contains("verification-artifacts:verification/test-runs/r1/bundle.json"), "{removed:?}");
    let line = last_json(&r);
    let names: Vec<&str> = removed.iter().map(String::as_str).collect();
    let freed = sizes(&names, &[("s1", GB), ("s2", GB), ("app-build.tar.gz", GB / 2), ("p1", GB / 10), ("recent", GB), ("store-v11.json", 100), ("store-v11.tar.gz", GB)]);
    assert_eq!((line["deletedObjects"].clone(), line["freedBytes"].clone(), line["skipCode"].clone(), line["action"].clone()), (json!(7), json!(freed), json!("delete-failed"), json!("short")), "{line}");

    // And with no failure but one object rewritten after the listing, it is not deleted, not counted, and is reported as changed
    tenancy.forget_calls();
    tenancy.put("failing.txt", "");
    tenancy.put("changed.txt", "sccache/g0/s1\n");
    let r = tenancy.wf(&["cache", "guard", "--local"]);
    let line = last_json(&r);
    assert!(!tenancy.deleted().contains("build-cache:sccache/g0/s1") && tenancy.deleted().len() == 8, "{:?}", tenancy.deleted());
    assert_eq!((line["deletedObjects"].clone(), line["changedSinceListing"].clone(), line["skipCode"].clone()), (json!(8), json!(1), Value::Null), "{line}");
}

#[test]
fn when_the_time_limit_passes_between_batches_the_guard_stops_deleting_and_reports_short_with_what_it_did() {
    // Given a store whose deletes take 5 s each and a limit of 7.5 s (2.5 s of room either side, so a loaded runner does not tip it): the build cache's batch fits, the evidence batch cannot
    let tenancy = Tenancy::new();
    tenancy.plant();
    tenancy.used(25 * GB);
    tenancy.put("delete-delay", "5");

    // When the guard runs
    let r = tenancy.wf_env(&["cache", "guard", "--local"], &[("WF_GUARD_LOCAL_LIMIT_MS", "7500")]);

    // Then the cache objects are deleted, no evidence is, and the one line says short because of the time limit
    assert_eq!(r.exit_code, 0, "{r:?}");
    let removed = tenancy.deleted();
    assert!(removed.len() == 8 && removed.iter().all(|name| name.starts_with("build-cache:")), "{removed:?}");
    let line = last_json(&r);
    assert_eq!((line["action"].clone(), line["skipCode"].clone(), line["deletedObjects"].clone()), (json!("short"), json!("time-limit"), json!(8)), "{line}");
}

#[test]
fn a_listing_that_outlasts_the_time_limit_is_a_skip_and_not_a_hang() {
    // Given a store whose object listing takes 5 s and a limit of 1 s
    let tenancy = Tenancy::new();
    tenancy.plant();
    tenancy.used(25 * GB);
    tenancy.put("list-delay", "5");
    let started = std::time::Instant::now();

    // When the guard runs
    let r = tenancy.wf_env(&["cache", "guard", "--local"], &[("WF_GUARD_LOCAL_LIMIT_MS", "1000")]);

    // Then it answers within a few seconds, deleted nothing, and says it could not list
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(4), "{:?}", started.elapsed());
    assert_eq!(tenancy.deleted(), set(&[]));
    assert_eq!(last_json(&r)["skipCode"], json!("listing-failed"), "{r:?}");
}

#[test]
fn under_the_high_water_mark_the_guard_measures_and_deletes_nothing() {
    // Given a tenancy at 16 GB of 20, below the 17 GB high-water mark
    let tenancy = Tenancy::new();
    tenancy.plant();
    tenancy.used(16 * GB);

    // When the guard runs
    let r = tenancy.wf(&["cache", "guard", "--local"]);

    // Then it reports usage against the cap and calls no delete
    assert_eq!(r.exit_code, 0, "{r:?}");
    let line = last_json(&r);
    assert_eq!((line["action"].clone(), line["usedBytes"].clone(), line["capBytes"].clone(), line["deletedObjects"].clone(), line["freedBytes"].clone()), (json!("none"), json!(16 * GB), json!(20 * GB), json!(0), json!(0)));
    assert_eq!(tenancy.deleted(), set(&[]));
}

#[test]
fn when_the_object_store_cannot_be_reached_the_guard_still_prints_one_line_with_a_reason_code_and_exits_0() {
    // Given an oci CLI that is refused, and one with no config that the vault cannot supply
    for (said, code, reason) in [("ServiceError: 503 service unavailable", "unmeasurable", "503 service unavailable"), ("ConfigFileNotFound: Could not find config file at /root/.oci/config", "config-missing", "do not resolve from the vault")] {
        let dir = tempfile::tempdir().expect("a temp dir");
        create_stub(&dir.path().join("oci"), &format!("echo '{said}' >&2\nexit 1"));

        // When the guard runs
        let r = run_wf(&["cache", "guard", "--local"], &RunOpts::default().bin_dir(dir.path()));

        // Then the line says it skipped, with the code and why, and nothing was measured
        assert_eq!((r.exit_code, r.stdout.lines().count()), (0, 1), "{said}: {r:?}");
        let line = last_json(&r);
        assert_eq!((line["event"].clone(), line["action"].clone(), line["ok"].clone(), line["skipCode"].clone()), (json!("storage-guard"), json!("skipped"), json!(false), json!(code)), "{line}");
        assert!(line["skip"].as_str().is_some_and(|skip| skip.contains(reason)), "{line}");
        assert_eq!((line["usedBytes"].clone(), line["deletedObjects"].clone()), (json!(0), json!(0)));
    }
}

#[test]
fn a_skip_reason_never_carries_key_material_or_a_passphrase_from_the_oci_cli() {
    // Given an oci CLI whose error text echoes a private key block and the passphrase of its config
    let dir = tempfile::tempdir().expect("a temp dir");
    let said = "InvalidKeyFile: could not load\n-----BEGIN RSA PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7\n-----END RSA PRIVATE KEY-----\npass_phrase = hunter2secret\nstatus 401";
    create_stub(&dir.path().join("oci"), &format!("printf '%s\\n' '{said}' >&2\nexit 1"));

    // When the guard runs
    let r = run_wf(&["cache", "guard", "--local"], &RunOpts::default().bin_dir(dir.path()));

    // Then the line keeps the cause and none of the secrets, on stdout or stderr
    let line = last_json(&r);
    let skip = line["skip"].as_str().unwrap_or_default();
    assert!(skip.contains("InvalidKeyFile") && skip.contains("401"), "{line}");
    for secret in ["MIIEvQ", "BEGIN RSA", "hunter2secret"] {
        assert!(!r.stdout.contains(secret) && !r.stderr.contains(secret), "{secret}: {r:?}");
    }
}

#[test]
fn on_the_ci_runner_the_guard_runs_on_agent_shell_through_vm_ops_api_and_returns_the_line_it_printed() {
    // Given agent-shell, which prints the guard's line among other output
    let vm = FakeVm::start();
    let job = answers::job_id(1);
    let printed = json!({ "event": "storage-guard", "ok": true, "action": "evicted", "usedBytes": 18 * GB, "usedBytesAfter": 13 * GB, "capBytes": 20 * GB, "deletedObjects": 4, "freedBytes": 5 * GB });
    let output = format!("wf: some stderr noise\n{printed}\n");
    vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(0), &output))]);

    // When the runner, which holds only the vm-ops-api signing seed, runs the guard
    let r = run_wf(&["cache", "guard"], &vm.opts());

    // Then one signed exec job starts `wf cache guard --local` on agent-shell and its line is the answer
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert_eq!(last_json(&r), printed);
    let requests = vm.requests();
    assert_eq!((requests[0].method.as_str(), requests[0].url.as_str()), ("POST", "/services/agent-shell/exec-script"));
    vm.assert_signed(&requests[0], "POST", "/services/agent-shell/exec-script", &requests[0].body);
    assert!(requests[0].body.contains("cache guard --local"), "{}", requests[0].body);
    assert!(!r.stdout.contains("OCI") && !r.stderr.contains("OCI_"), "the runner touches no OCI credential: {r:?}");
}

#[test]
fn on_the_ci_runner_an_unreachable_or_older_agent_shell_is_a_skip_and_never_a_failure() {
    // Given agent-shell down (503), then agent-shell whose wf has no `cache guard` (an exit code and no line)
    let vm = FakeVm::start();
    vm.replies(&[answers::reply(503, &answers::agent_shell_not_running())]);

    // When the runner runs the guard
    let down = run_wf(&["cache", "guard"], &vm.opts());

    // Then the line says skipped with the cause, and the exit code is 0
    assert_eq!(down.exit_code, 0, "{down:?}");
    let line = last_json(&down);
    assert_eq!((line["action"].clone(), line["ok"].clone()), (json!("skipped"), json!(false)));
    assert!(line["skip"].as_str().is_some_and(|skip| skip.contains("agent-shell container is not running")), "{line}");

    let job = answers::job_id(2);
    vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(2), "error: unrecognized subcommand 'guard'\n"))]);
    let old = run_wf(&["cache", "guard"], &vm.opts());
    assert_eq!(old.exit_code, 0, "{old:?}");
    let line = last_json(&old);
    assert_eq!(line["action"], json!("skipped"));
    assert!(line["skip"].as_str().is_some_and(|skip| skip.contains("printed no guard result (exit 2)") && skip.contains("unrecognized subcommand")), "{line}");
}

#[test]
fn on_the_ci_runner_an_agent_shell_job_that_never_finishes_is_cancelled_and_is_a_skip_within_the_limit() {
    // Given agent-shell whose job starts and then stays running, and a relay limit of 1.5 s
    let vm = FakeVm::start();
    let job = answers::job_id(3);
    vm.replies_then_last(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, None, ""))]);
    let started = std::time::Instant::now();

    // When the runner runs the guard
    let r = run_wf(&["cache", "guard", "--dry-run"], &vm.opts().env("WF_GUARD_RELAY_LIMIT_MS", "1500"));

    // Then the remote job is cancelled, the line is a skip that says it did not finish, and the exit code is 0 within a few seconds
    assert_eq!(r.exit_code, 0, "{r:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(8), "{:?}", started.elapsed());
    let line = last_json(&r);
    assert_eq!((line["action"].clone(), line["skipCode"].clone()), (json!("skipped"), json!("relay-timeout")), "{line}");
    assert!(line["skip"].as_str().is_some_and(|skip| skip.contains("did not finish")), "{line}");
    let requests = vm.requests();
    assert!(requests.iter().any(|q| q.method == "POST" && q.url == format!("/services/agent-shell/exec-script/{job}/cancel")), "{:?}", requests.iter().map(|q| format!("{} {}", q.method, q.url)).collect::<Vec<_>>());
    assert!(requests[0].body.contains("cache guard --local --dry-run"), "{}", requests[0].body);
}
