//! `wf parity coverage --by-oracle` over fixture trees: each class decided from the cassettes, the known gaps and the decisions, and the
//! host state of a behaviour test read from its source.
use super::support::*;
use std::path::Path;

const CASSETTES: &str = "clis/t3-serve/cassettes";
const CHAIN: &str = "clis/t3-serve/tests/integration";

fn put(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn row(id: &str, suite: &str, test: &str) -> Value {
    json!({"id": id, "state": "served", "evidence": {"suite": suite, "test": test}})
}

fn interactions(rows: &[(&str, &str)]) -> String {
    rows.iter().map(|(id, row)| json!({"id": id, "row": row}).to_string() + "\n").collect()
}

/// A tree whose table serves six rows (four by behaviour tests, two by a replay) plus a pending one, with a recorded set, an authored set,
/// a gap file naming one breaking and one drift interaction, and two behaviour test sources.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let table = json!({"rows": [
        row("rpc:a.clean", "t3-serve-integration", "alpha::reads_it"),
        row("rpc:b.drift", "wf-mimic-t3-recorded", "set-recorded::i-drift"),
        row("rpc:c.breaks", "t3-serve-integration", "alpha::breaks_it"),
        row("rpc:d.unrecorded", "t3-serve-integration", "beta::only_authored"),
        row("rpc:e.decided", "t3-serve-integration", "beta::pty"),
        row("rpc:f.listed", "t3-serve-integration", "beta::listed_test"),
        {"id": "rpc:g.pending", "state": "pending"},
    ]});
    put(root, "docs/plans/rca-parity/coverage/t3.json", &table.to_string());
    put(root, "docs/plans/rca-parity/coverage/t3-decisions.json", &json!({"decisions": {"rpc:e.decided": {"unrecordable": "a pty's output"}}, "unrecordableTests": {"beta::listed*": "a restart"}}).to_string());
    put(root, &format!("{CASSETTES}/set-recorded/cassette.json"), &json!({"provenance": {"kind": "recorded"}}).to_string());
    put(
        root,
        &format!("{CASSETTES}/set-recorded/interactions.jsonl"),
        &interactions(&[("i-clean", "rpc:a.clean"), ("i-drift", "rpc:b.drift"), ("i-break", "rpc:c.breaks"), ("i-break-too", "rpc:c.breaks"), ("i-decided", "rpc:e.decided"), ("i-listed", "rpc:f.listed")]),
    );
    put(
        root,
        &format!("{CASSETTES}/set-recorded/known-gaps.json"),
        &json!({"groups": [{"class": "x", "severity": "drift", "diffs": [], "ids": ["i-drift"]}, {"class": "y", "severity": "breaking", "diffs": [], "ids": ["i-break"]}]}).to_string(),
    );
    put(root, &format!("{CASSETTES}/set-authored/cassette.json"), &json!({"provenance": {"kind": "authored"}}).to_string());
    put(root, &format!("{CASSETTES}/set-authored/interactions.jsonl"), &interactions(&[("a1", "rpc:d.unrecorded")]));
    put(
        root,
        &format!("{CHAIN}/alpha.rs"),
        "// Script::of(vec![]) in a comment is no turn\n#[test]\nfn reads_it() {\n    let chain = Chain::start_hosted();\n    chain.cli.script(\"claude\", &Script::of(vec![Turn::replaying(&lines)]));\n    chain.restart_serve();\n    let other = Chain::start_with_env(None, &[(\"K\", \"v\")]);\n    assert_eq!(none[\"result\"][\"restart\"], true);\n}\n#[test]\nfn breaks_it() {}\n",
    );
    put(root, &format!("{CHAIN}/beta.rs"), "#[test]\nfn only_authored() {}\n#[test]\nfn pty() {}\n#[test]\nfn listed_test() {}\n");
    dir
}

fn by_oracle(root: &Path, args: &[&str]) -> Run {
    let mut all = vec!["--app", "t3", "--by-oracle"];
    all.extend_from_slice(args);
    coverage(root, &all)
}

fn classes(md: &str) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    let mut class = String::new();
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ").filter(|h| h.starts_with('(')) {
            class = h[1..2].to_string();
        } else if let Some(id) = line.strip_prefix("| `rpc:").filter(|_| !class.is_empty()) {
            out.insert(format!("rpc:{}", id.split('`').next().unwrap()), class.clone());
        }
    }
    out
}

#[test]
fn each_served_row_gets_the_class_its_recording_and_decisions_decide() {
    // Given six served rows, a recorded set with a drift and a breaking gap, an authored set, and two decisions that name a reason
    let dir = tree();
    // When the table is built by oracle
    let run = by_oracle(dir.path(), &[]);
    // Then clean and drift-only rows are (a), a breaking gap is (b), a row only an authored set has is (c), and a named reason is (d)
    assert_eq!(run.code, 0, "{}", run.stdout);
    assert_eq!(run.json["served"], 6);
    assert_eq!(run.json["counts"], json!({"a": 2, "b": 1, "c": 1, "d": 2}));
    assert_eq!(run.json["chainCounts"], json!({"a": 1, "b": 1, "c": 1, "d": 2}));
    assert_eq!(run.json["recordedSets"], json!(["set-recorded"]));
    let md = std::fs::read_to_string(dir.path().join("docs/plans/rca-parity/coverage/t3-oracle.md")).unwrap();
    let by = classes(&md);
    let class_of = |id: &str| by[id].as_str();
    assert_eq!((class_of("rpc:a.clean"), class_of("rpc:b.drift"), class_of("rpc:c.breaks"), class_of("rpc:d.unrecorded"), class_of("rpc:e.decided"), class_of("rpc:f.listed")), ("a", "a", "b", "c", "d", "d"));
    assert!(!by.contains_key("rpc:g.pending"));
    // And the reasons and the recording show in the detail
    assert!(md.contains("a pty's output") && md.contains("a restart"), "{md}");
    assert!(md.contains("set-recorded x2; 2 gap-listed, 1 breaking") || md.contains("set-recorded x2; 1 gap-listed, 1 breaking"), "{md}");
}

#[test]
fn a_chain_tests_host_state_is_read_from_its_source_and_comments_do_not_count() {
    // Given a behaviour test that scripts a turn, starts the gh double and restarts the server, with a needle in a comment and a JSON key named restart
    let dir = tree();
    // When the table is built
    let run = by_oracle(dir.path(), &[]);
    // Then it lists that file's host state once per use, and a file that sets up none lists none
    let tests = run.json["chainTests"].as_array().unwrap();
    let find = |file: &str| tests.iter().find(|t| t["file"] == file).unwrap().clone();
    assert_eq!(find("alpha")["hostState"], json!({"scripted agent turn": 3, "gh double": 1, "restart": 1, "service env": 1}));
    assert_eq!(find("alpha")["tests"], 2);
    assert_eq!(find("alpha")["rows"], 2);
    assert_eq!(find("beta")["hostState"], json!({}));
    let md = std::fs::read_to_string(dir.path().join("docs/plans/rca-parity/coverage/t3-oracle.md")).unwrap();
    assert!(md.contains("| `alpha` | 2 | 2 | 3 | 1 | 1 | none | 1 |"), "{md}");
}

#[test]
fn the_check_passes_on_what_a_write_made_and_fails_naming_the_table_once_it_is_edited() {
    // Given a written table
    let dir = tree();
    assert_eq!(by_oracle(dir.path(), &[]).code, 0);
    // When it is checked, Then nothing is stale and nothing is written
    let ok = by_oracle(dir.path(), &["--check"]);
    assert_eq!((ok.code, ok.json["problems"].clone()), (0, json!([])), "{}", ok.stdout);
    // And a second write changes nothing
    assert_eq!(by_oracle(dir.path(), &[]).json["wrote"], json!([]));
    // When a recorded interaction of a clean row gains a breaking gap, Then the check fails with the table stale
    put(dir.path(), &format!("{CASSETTES}/set-recorded/known-gaps.json"), &json!({"groups": [{"class": "y", "severity": "breaking", "diffs": [], "ids": ["i-break", "i-clean"]}]}).to_string());
    let stale = by_oracle(dir.path(), &["--check"]);
    assert_eq!(stale.code, 1);
    assert_eq!(problems(&stale), vec![("stale".to_string(), vec!["docs/plans/rca-parity/coverage/t3-oracle.md".to_string()])]);
    // And the write that follows moves rpc:a.clean to (b)
    by_oracle(dir.path(), &[]);
    let md = std::fs::read_to_string(dir.path().join("docs/plans/rca-parity/coverage/t3-oracle.md")).unwrap();
    assert_eq!(classes(&md)["rpc:a.clean"], "b");
}

#[test]
fn evidence_naming_a_chain_test_whose_source_is_gone_fails_the_check_and_other_flags_are_refused() {
    // Given a table whose evidence names a behaviour test file that does not exist
    let dir = tree();
    std::fs::remove_file(dir.path().join(format!("{CHAIN}/beta.rs"))).unwrap();
    // When it is checked, Then the file is named
    let run = by_oracle(dir.path(), &["--check"]);
    assert!(problems(&run).contains(&("chain-source-missing".to_string(), vec!["beta".to_string()])), "{}", run.stdout);
    // And a flag of the other mode is refused
    let refused = by_oracle(dir.path(), &["--traces", "x"]);
    assert_eq!(refused.code, 2);
    assert!(refused.json["error"].as_str().unwrap().contains("--by-oracle takes only"));
}

#[test]
fn orca_reads_its_own_sets_and_chain_tests() {
    // Given an Orca tree with one row served by a behaviour test and a recorded set that labels it
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    put(root, "docs/plans/rca-parity/coverage/orca.json", &json!({"rows": [row("rpc:files.stat", "orca-serve-integration", "web::serves_it")]}).to_string());
    put(root, "clis/orca-serve/cassettes/orca-recorded/cassette.json", &json!({"provenance": {"kind": "recorded"}}).to_string());
    put(root, "clis/orca-serve/cassettes/orca-recorded/interactions.jsonl", &interactions(&[("files.stat", "rpc:files.stat")]));
    put(root, "clis/orca-serve/tests/integration/web.rs", "#[test]\nfn serves_it() {}\n");
    // When the table is built for Orca
    let run = coverage(root, &["--app", "orca", "--by-oracle"]);
    // Then the row is (a), served by the behaviour test, and the md is the Orca one
    assert_eq!((run.code, run.json["counts"].clone()), (0, json!({"a": 1, "b": 0, "c": 0, "d": 0})), "{}", run.stdout);
    assert!(std::fs::read_to_string(root.join("docs/plans/rca-parity/coverage/orca-oracle.md")).unwrap().contains("| `rpc:files.stat` | chain `web` |"));
}
