//! The app build state as one restore-latest object: restored into an empty directory only, saved only when the guard allows, the
//! directory has content and its fingerprint is not the saved one, and a bucket that holds nothing is a cold start, never a failure.
use super::pnpm::{fill_store, snapshot, Bucket, Scratch};
use blob_cache::adapters::TarGz;
use blob_cache::app_build::{restore, save, SaveFacts};
use blob_cache::domain::{Budget, Generation, Key, Kind};
use blob_cache::pnpm::{Outcome, Reason};
use blob_cache::{Cache, Writes};
use std::sync::Arc;

fn cache(bucket: &Arc<Bucket>, writes: Writes) -> Cache<Arc<Bucket>> {
    Cache::with_generation(bucket.clone(), Budget::committed().for_one_entry_of(Kind::Metro), writes, Generation(0))
}

fn key(name: &str) -> Key {
    Key::new(Kind::Metro, Generation(0), name).unwrap()
}

#[test]
fn given_a_saved_build_state_when_restore_runs_then_only_an_empty_directory_is_filled_and_a_missing_object_is_cold() {
    // Given a bucket that holds a build directory saved from another checkout
    let scratch = Scratch::new("app-build-restore");
    let bucket = Bucket::at(&scratch.path("bucket"));
    let (source, target) = (scratch.path("a/app-build"), scratch.path("b/app-build"));
    fill_store(&source, 1);
    let saved = save(&cache(&bucket, Writes::Allowed), &TarGz, &SaveFacts { dir: &source, fingerprint: "f1" });
    assert!(matches!(saved, Outcome::Done { bytes } if bytes > 0), "{saved:?}");
    assert_eq!(bucket.written(), ["metro/g0/app-build.tar.gz", "metro/g0/app-build.json"]);

    // When restore runs into a directory that has nothing, then into one that has content
    let filled = restore(&cache(&bucket, Writes::ReadOnly("restore reads only".into())), &TarGz, &target);
    let before = snapshot(&target);
    let again = restore(&cache(&bucket, Writes::ReadOnly("restore reads only".into())), &TarGz, &target);

    // Then the first fills it with the saved files, the second does nothing and asks the bucket nothing
    assert!(matches!(filled, Outcome::Done { bytes } if bytes > 0), "{filled:?}");
    assert_eq!(snapshot(&target), snapshot(&source));
    let requests = bucket.requests();
    assert_eq!(again, Outcome::Nothing(Reason::DirNotEmpty));
    assert_eq!(snapshot(&target), before);
    assert_eq!(bucket.requests(), requests);

    // And an empty bucket is a cold start for an empty directory, not a failure
    let empty = Bucket::at(&scratch.path("empty-bucket"));
    let cold = restore(&cache(&empty, Writes::ReadOnly("restore reads only".into())), &TarGz, &scratch.path("c/app-build"));
    assert_eq!(cold, Outcome::Nothing(Reason::NoObject));
}

#[test]
fn given_a_build_directory_when_save_runs_then_it_uploads_only_a_changed_fingerprint_and_only_when_the_guard_allows() {
    // Given a build directory with content and an empty bucket
    let scratch = Scratch::new("app-build-save");
    let bucket = Bucket::at(&scratch.path("bucket"));
    let dir = scratch.path("app-build");
    fill_store(&dir, 2);

    // When save runs under a refusing guard, then twice under an allowing one with the same fingerprint, then with a new one
    let refused = save(&cache(&bucket, Writes::ReadOnly("dirty".into())), &TarGz, &SaveFacts { dir: &dir, fingerprint: "f1" });
    assert_eq!(refused, Outcome::Nothing(Reason::GuardRefused("dirty".into())));
    assert_eq!(bucket.requests(), 0);
    let first = save(&cache(&bucket, Writes::Allowed), &TarGz, &SaveFacts { dir: &dir, fingerprint: "f1" });
    let writes = bucket.written().len();
    let same = save(&cache(&bucket, Writes::Allowed), &TarGz, &SaveFacts { dir: &dir, fingerprint: "f1" });
    let changed = save(&cache(&bucket, Writes::Allowed), &TarGz, &SaveFacts { dir: &dir, fingerprint: "f2" });

    // Then only the first and the changed fingerprint wrote, the unchanged one says so, and an empty directory has nothing to save
    assert!(matches!(first, Outcome::Done { .. }), "{first:?}");
    assert_eq!(same, Outcome::Nothing(Reason::Unchanged));
    assert_eq!(bucket.written().len(), writes + 2, "the sidecar and the archive of the changed fingerprint");
    assert!(matches!(changed, Outcome::Done { .. }), "{changed:?}");
    let none = save(&cache(&bucket, Writes::Allowed), &TarGz, &SaveFacts { dir: &scratch.path("nothing"), fingerprint: "f3" });
    assert_eq!(none, Outcome::Nothing(Reason::StoreEmpty));
    assert!(bucket.written().iter().all(|k| key("app-build.tar.gz").as_str() == k || key("app-build.json").as_str() == k));
}
