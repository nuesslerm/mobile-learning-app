use super::support::*;

const OLD: i64 = 90;

/// Four versions of fixture-core in the bucket (and the index), 0.2.0 pinned on origin/dev, 0.3.0 young, 0.4.0 the latest.
fn stocked() -> Repo {
    let repo = Repo::new(&libraries_json("mla", "", ""));
    let index: String = ["0.1.0", "0.2.0", "0.3.0", "0.4.0"].iter().map(|v| format!("{{\"name\":\"fixture-core\",\"vers\":\"{v}\",\"deps\":[],\"cksum\":\"x\",\"features\":{{}},\"yanked\":false}}\n")).collect();
    repo.bucket.put("libs/index/fi/xt/fixture-core", index.as_bytes());
    for (version, age) in [("0.1.0", OLD), ("0.2.0", OLD), ("0.3.0", 5), ("0.4.0", OLD)] {
        let name = format!("libs/crates/fixture-core/{version}.crate");
        repo.bucket.put(&name, version.as_bytes());
        repo.bucket.age(&name, age);
    }
    repo.write("services/a/Cargo.lock", &lock("fixture-core", "0.2.0", true));
    repo.commit("service a pins 0.2.0");
    repo.push();
    repo
}

fn versions(repo: &Repo) -> Vec<String> {
    repo.bucket.names().iter().filter_map(|n| n.strip_prefix("libs/crates/fixture-core/")?.strip_suffix(".crate").map(str::to_string)).collect()
}

#[test]
fn a_dry_run_prints_what_would_go_and_deletes_nothing() {
    // Given old, pinned, young and latest versions in the bucket
    let repo = stocked();

    // When prune runs as a dry run
    let r = repo.lib(&["prune", "--dry-run"]);

    // Then only the old, unpinned, not-latest version is listed, with why each other one stays, and the bucket is as it was
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = last_json(&r);
    assert_eq!((out["dryRun"].clone(), out["wouldDelete"].as_array().unwrap().iter().map(|d| d["version"].as_str().unwrap()).collect::<Vec<_>>()), (true.into(), vec!["0.1.0"]));
    assert!(out.get("deleted").is_none());
    let kept: Vec<(&str, &str)> = out["kept"].as_array().unwrap().iter().map(|k| (k["version"].as_str().unwrap(), k["reason"].as_str().unwrap())).collect();
    assert_eq!(kept, [("0.2.0", "pinned"), ("0.3.0", "grace"), ("0.4.0", "latest")]);
    assert_eq!(versions(&repo).len(), 4);
}

#[test]
fn prune_deletes_only_the_unpinned_old_version_and_yanks_it_so_no_new_resolution_picks_it() {
    // Given the same bucket, and a lockfile pinning 0.1.0 that is committed here but not on origin/dev
    let repo = stocked();
    repo.write("services/b/Cargo.lock", &lock("fixture-core", "0.1.0", true));
    repo.commit("service b pins 0.1.0, not pushed");

    // When prune runs
    let r = repo.lib(&["prune"]);

    // Then 0.1.0 is deleted (dev does not pin it), nothing else is, and the index marks it yanked
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let out = last_json(&r);
    assert_eq!(out["deleted"].as_array().unwrap().iter().map(|d| d["version"].as_str().unwrap()).collect::<Vec<_>>(), ["0.1.0"]);
    assert_eq!(versions(&repo), ["0.2.0", "0.3.0", "0.4.0"]);
    let index = repo.bucket.text("libs/index/fi/xt/fixture-core").unwrap();
    let yanked: Vec<(String, bool)> = index.lines().map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()).map(|v| (v["vers"].as_str().unwrap().to_string(), v["yanked"].as_bool().unwrap())).collect();
    assert_eq!(repo.proxy.fetch("/index/fi/xt/fixture-core"), (200, index.clone().into_bytes()));
    assert_eq!(yanked, [("0.1.0".into(), true), ("0.2.0".into(), false), ("0.3.0".into(), false), ("0.4.0".into(), false)]);
}

#[test]
fn a_version_pinned_on_origin_dev_and_the_latest_survive_however_old_and_however_short_the_grace() {
    // Given the stocked bucket
    let repo = stocked();

    // When prune runs with no grace at all
    let r = repo.lib(&["prune", "--grace-days", "0"]);

    // Then the young one goes too, but never the pinned 0.2.0 or the latest 0.4.0
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(versions(&repo), ["0.2.0", "0.4.0"]);
}

#[test]
fn prune_that_cannot_read_origin_dev_deletes_nothing_and_says_what_to_do() {
    // Given the stocked bucket and a repository whose origin is gone
    let repo = stocked();
    git(&repo.work(), &["remote", "set-url", "origin", "/nonexistent/origin.git"]);

    // When prune runs
    let r = repo.lib(&["prune"]);

    // Then it refuses, because a stale origin/dev could hide a pin, and the bucket is untouched
    assert_eq!(r.exit_code, 1);
    let out = last_json(&r);
    assert!(out["ok"] == false && out["next"].as_str().unwrap().contains("--no-fetch"), "{out}");
    assert_eq!(versions(&repo).len(), 4);
}

#[test]
fn a_library_on_a_registry_that_cannot_delete_is_listed_as_skipped_never_pruned() {
    // Given a library whose registry is crates.io
    let repo = Repo::new(&libraries_json("crates-io", "", ""));
    repo.push();

    // When prune runs as a dry run
    let r = repo.lib(&["prune", "--dry-run"]);

    // Then it is skipped with the reason
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let skipped = &last_json(&r)["skipped"][0];
    assert_eq!((skipped["library"].as_str(), skipped["registry"].as_str()), (Some("fixture-core"), Some("crates-io")));
    assert!(skipped["why"].as_str().unwrap().contains("cannot delete"));
}
