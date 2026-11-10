//! `wf mimic crawl`: the completeness guard over an endpoint inventory and its corpus. The reference is a `cat` of a report file the test
//! writes (the driver's crawl verb is the Node side), the catalog and the pin are files beside it; every expectation is derived from the
//! ids of the catalog under test, and the committed crawls of the real apps are checked offline and against planted lags.
use crate::support::*;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn endpoint(id: &str, classes: &[&str], skipped: Option<&str>) -> Value {
    let counts: serde_json::Map<String, Value> = classes.iter().map(|c| (c.to_string(), json!(1))).collect();
    let mut e = json!({"id": id, "kind": "rpc", "name": id.trim_start_matches("rpc:"), "transport": "rpc", "streaming": false, "users": ["mobile"], "sources": ["schema", "catalog"],
        "input": if classes.contains(&"valid") { "schema" } else { "empty" }, "classes": counts});
    if let Some(why) = skipped {
        e["skipped"] = json!(why);
    }
    e
}

fn case(id: &str, row: &str, class: &str) -> Value {
    json!({"id": format!("{id}-{class}"), "row": row, "op": "script", "input": {"requests": [{"method": id}]}, "tags": [format!("class:{class}")]})
}

/// A report whose endpoints are the catalog's ids: each asked with a valid and an invalid case, except the last, which is skipped.
fn report(ids: &[&str], commit: &str) -> Value {
    let (last, asked) = ids.split_last().unwrap();
    let mut endpoints: Vec<Value> = asked.iter().map(|id| endpoint(id, &["valid", "invalid"], None)).collect();
    endpoints.push(endpoint(last, &[], Some("replaces the installed binary")));
    let cases: Vec<Value> = asked.iter().flat_map(|id| [case(id, id, "valid"), case(id, id, "invalid")]).collect();
    json!({"schema": "mimic-crawl-report/1", "app": "toy", "pin": {"version": "1", "commit": commit},
        "sources": [{"name": "schema", "available": true, "found": ids.len()}], "endpoints": endpoints, "cli_commands": [], "cases": cases, "blind": []})
}

struct Crawl {
    dir: tempfile::TempDir,
}

impl Crawl {
    fn new(ids: &[&str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let c = Self { dir };
        c.write("crawl.toml", "app = \"toy\"\ncommand = [\"cat\", \"report.json\"]\ncatalog = \"catalog.json\"\npin = \"pin.json\"\ncommit_pointer = \"/commit\"\n");
        c.write("pin.json", r#"{"commit":"c0ffee"}"#);
        c.set_catalog(ids);
        c.set_report(&report(ids, "c0ffee"));
        c
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.path(name), text).unwrap();
    }

    fn set_catalog(&self, ids: &[&str]) {
        let rows: Vec<Value> = ids.iter().map(|i| json!({"id": i})).collect();
        self.write("catalog.json", &json!({"rows": rows}).to_string());
    }

    fn set_report(&self, r: &Value) {
        self.write("report.json", &r.to_string());
    }

    fn run(&self, flags: &[&str]) -> Run {
        let d = self.dir.path().display().to_string();
        let mut args = vec!["crawl", d.as_str()];
        args.extend(flags);
        mimic(&args, &[])
    }
}

fn problems(r: &Run) -> Vec<(String, String)> {
    r.json["sets"][0]["problems"].as_array().unwrap().iter().map(|p| (p["code"].as_str().unwrap().to_string(), p["id"].as_str().unwrap().to_string())).collect()
}

const IDS: [&str; 4] = ["rpc:a.one", "rpc:a.two", "rpc:b.three", "rpc:c.four"];

#[test]
fn a_crawl_writes_inventory_corpus_and_coverage_and_a_second_one_changes_nothing() {
    // Given a reference whose crawl covers every row of the catalog
    let c = Crawl::new(&IDS);
    // When it is crawled
    let first = c.run(&[]);
    // Then the three files are written and the summary counts the endpoints, cases and the skipped one
    assert_eq!((first.code, first.json["ok"].clone()), (0, json!(true)), "{}", first.json);
    let summary = &first.json["sets"][0]["summary"];
    assert_eq!((summary["endpoints"].as_u64(), summary["cases"].as_u64(), summary["skipped"].as_u64()), (Some(IDS.len() as u64), Some(2 * (IDS.len() as u64 - 1)), Some(1)));
    assert_eq!(first.json["sets"][0]["wrote"].as_array().unwrap().len(), 3);
    for f in ["inventory.json", "corpus.json", "coverage.json"] {
        assert!(c.path(f).is_file(), "{f}");
    }
    // And the skipped endpoint is a row of the coverage with its reason, the others cases
    let coverage: Value = serde_json::from_str(&std::fs::read_to_string(c.path("coverage.json")).unwrap()).unwrap();
    let rows = coverage["rows"].as_array().unwrap();
    assert_eq!(rows.iter().filter(|r| r["status"] == "skipped").map(|r| r["row"].as_str().unwrap()).collect::<Vec<_>>(), [*IDS.last().unwrap()]);
    // When it is crawled again, Then nothing is written
    assert_eq!(c.run(&[]).json["sets"][0]["wrote"], json!([]));
}

#[test]
fn the_committed_files_are_checked_offline_without_the_reference() {
    // Given a crawl written once, and then the report (the reference's source) gone
    let c = Crawl::new(&IDS);
    assert_eq!(c.run(&[]).code, 0);
    std::fs::remove_file(c.path("report.json")).unwrap();
    // When --check runs, Then it passes from the committed files and the catalog alone
    let r = c.run(&["--check"]);
    assert_eq!((r.code, r.json["sets"][0]["mode"].clone()), (0, json!("check")), "{}", r.json);
    // And --check --fresh needs the reference and fails naming why
    let fresh = c.run(&["--check", "--fresh"]);
    assert_eq!(fresh.code, 1, "{}", fresh.json);
    assert!(fresh.json["sets"][0]["error"].as_str().unwrap().contains("exited"), "{}", fresh.json);
}

#[test]
fn a_row_the_pinned_source_lists_and_the_crawl_lacks_fails_naming_it_and_writes_nothing() {
    // Given a catalog with one row more than the crawl found
    let c = Crawl::new(&IDS);
    let mut longer = IDS.to_vec();
    longer.push("rpc:z.new");
    c.set_catalog(&longer);
    // When crawled, Then it fails with lags-source for exactly that row and the files are not written
    let r = c.run(&[]);
    assert_eq!(r.code, 1, "{}", r.json);
    assert_eq!(problems(&r), [("lags-source".to_string(), "rpc:z.new".to_string())]);
    assert!(!c.path("corpus.json").exists());
}

#[test]
fn an_endpoint_without_cases_or_a_skip_reason_and_a_schema_endpoint_without_an_invalid_input_fail() {
    // Given a report where the first endpoint lost both its cases and the second its invalid one
    let c = Crawl::new(&IDS);
    let mut r = report(&IDS, "c0ffee");
    let cases = r["cases"].as_array_mut().unwrap();
    cases.retain(|k| k["row"] != IDS[0] && k["id"] != format!("{}-invalid", IDS[1]));
    c.set_report(&r);
    // When crawled, Then the first is uncovered and the second misses its class (its inventory also still claims the dropped case)
    let got = problems(&c.run(&[]));
    assert!(got.contains(&("uncovered".to_string(), IDS[0].to_string())), "{got:?}");
    assert!(got.contains(&("missing-class".to_string(), IDS[1].to_string())), "{got:?}");
    assert!(got.contains(&("classes-disagree".to_string(), IDS[1].to_string())), "{got:?}");
}

#[test]
fn a_case_for_no_endpoint_and_a_crawl_at_another_commit_than_the_pin_fail() {
    // Given a case for a row that is not an endpoint and a pin file that moved on
    let c = Crawl::new(&IDS);
    let mut r = report(&IDS, "c0ffee");
    r["cases"].as_array_mut().unwrap().push(case("ghost", "rpc:ghost", "valid"));
    c.set_report(&r);
    c.write("pin.json", r#"{"commit":"beef"}"#);
    // When crawled, Then both are named
    let got = problems(&c.run(&[]));
    assert!(got.contains(&("orphan-case".to_string(), "ghost-valid".to_string())), "{got:?}");
    assert!(got.iter().any(|(code, id)| code == "stale-pin" && id == "c0ffee"), "{got:?}");
}

#[test]
fn a_hand_edited_committed_file_is_stale_and_a_new_crawl_of_a_moved_source_is_not_the_committed_one() {
    // Given a crawl written once
    let c = Crawl::new(&IDS);
    assert_eq!(c.run(&[]).code, 0);
    // When coverage.json is edited by hand, Then --check names it
    let text = std::fs::read_to_string(c.path("coverage.json")).unwrap();
    c.write("coverage.json", &text.replace("\"case\"", "\"skipped\""));
    assert!(problems(&c.run(&["--check"])).contains(&("stale-output".to_string(), "coverage.json".to_string())));
    // And when the source gains an endpoint (catalog and crawl both), --check --fresh names the files that differ
    assert_eq!(c.run(&[]).code, 0);
    let mut more = IDS.to_vec();
    more.insert(1, "rpc:a.added");
    c.set_catalog(&more);
    c.set_report(&report(&more, "c0ffee"));
    let fresh = c.run(&["--check", "--fresh"]);
    assert_eq!(fresh.code, 1, "{}", fresh.json);
    assert!(problems(&fresh).iter().any(|(code, id)| code == "stale-output" && id == "inventory.json"), "{}", fresh.json);
}

fn committed_crawls() -> Vec<PathBuf> {
    let found = mimic(&["crawl", "--all", "--check", "--root", root().to_str().unwrap()], &[]);
    found.json["sets"].as_array().unwrap().iter().map(|s| PathBuf::from(s["dir"].as_str().unwrap())).collect()
}

#[test]
fn the_committed_crawls_cover_every_row_of_their_catalog_and_every_endpoint_has_a_user_and_cases() {
    // Given every crawl directory of the repository (found by its crawl.toml)
    let dirs = committed_crawls();
    assert!(dirs.len() >= 2, "{dirs:?}");
    for dir in dirs {
        // When its committed files are checked against the committed catalog and pin
        let r = mimic(&["crawl", "--check", dir.to_str().unwrap()], &[]);
        // Then the guard finds nothing
        assert_eq!((r.code, r.json["sets"][0]["problems"].clone()), (0, json!([])), "{}: {}", dir.display(), r.json);
    }
}

#[test]
fn a_catalog_row_added_to_a_committed_crawls_catalog_is_caught_as_a_lag() {
    // Given each committed crawl copied with its catalog gaining one row the crawl cannot have
    for dir in committed_crawls() {
        let copy = tempfile::tempdir().unwrap();
        for f in ["inventory.json", "corpus.json", "coverage.json"] {
            std::fs::copy(dir.join(f), copy.path().join(f)).unwrap();
        }
        let toml: toml::Value = std::fs::read_to_string(dir.join("crawl.toml")).unwrap().parse().unwrap();
        let abs = |key: &str| dir.join(toml[key].as_str().unwrap()).canonicalize().unwrap();
        let mut catalog: Value = serde_json::from_str(&std::fs::read_to_string(abs("catalog")).unwrap()).unwrap();
        catalog["rows"].as_array_mut().unwrap().push(json!({"id": "rpc:planted.lag"}));
        std::fs::write(copy.path().join("catalog.json"), catalog.to_string()).unwrap();
        std::fs::write(copy.path().join("crawl.toml"), format!("app = {:?}\ncommand = [\"true\"]\ncatalog = \"catalog.json\"\n", toml["app"].as_str().unwrap())).unwrap();
        // When the copy is checked, Then exactly the planted row is reported
        let r = mimic(&["crawl", "--check", copy.path().to_str().unwrap()], &[]);
        assert_eq!(r.code, 1, "{}", r.json);
        assert_eq!(problems(&r), [("lags-source".to_string(), "rpc:planted.lag".to_string())], "{}", dir.display());
    }
}
