use crate::support::*;
use std::path::Path;

/// A fixture tree: `files` are (relative path, text); the rules file sits at its root.
fn tree(rules: &Value, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fixture-provenance.json"), rules.to_string()).unwrap();
    for (rel, text) in files {
        let path = dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn scan(dir: &Path, extra: &[&str]) -> Run {
    let d = dir.display().to_string();
    let mut args = vec!["verify", "--fixtures", d.as_str()];
    args.extend_from_slice(extra);
    mimic(&args, &[])
}

fn rule(glob: &str, kind: &str) -> Value {
    json!({"match": glob, "kind": kind, "reason": "because", "row": "rpc:echo"})
}

fn rules(list: &[Value]) -> Value {
    json!({"schema": "mimic-fixture-rules/1", "include": ["*/tests/fixtures/**"], "rules": list})
}

const WITH_VOLATILE: &str = r#"{"method":"m","exchanges":[{"name":"a","request":{},"response":{},"volatile":["/x","/y"]},{"name":"b","request":{},"response":{}}]}"#;

#[test]
fn given_files_of_each_kind_when_scanned_then_each_is_classified_by_its_rule_and_counted_per_crate() {
    // Given a tree whose crates are covered by one rule each, and a file outside the include globs
    let dir = tree(
        &rules(&[rule("authored-crate/**", "authored"), rule("mirror-crate/**", "mirrors-reference"), rule("cli-crate/**", "cli")]),
        &[
            ("authored-crate/tests/fixtures/a.json", WITH_VOLATILE),
            ("authored-crate/tests/fixtures/sub/b.json", r#"{"method":"m","exchanges":[]}"#),
            ("mirror-crate/tests/fixtures/c.json", "{}"),
            ("cli-crate/tests/fixtures/d.jsonl", "{}\n"),
            ("authored-crate/src/lib.rs", "not a fixture"),
        ],
    );
    // When the tree is scanned with the file list asked for
    let r = scan(dir.path(), &["--files"]);
    // Then each file has the kind of its rule, the source file is not scanned and the counts add up per crate
    assert_eq!((r.code, r.json["ok"].clone(), r.json["schema"].clone()), (0, json!(true), json!("mimic-fixtures/1")), "{}", r.json);
    let kind = |path: &str| r.json["files"].as_array().unwrap().iter().find(|f| f["path"] == path).unwrap_or_else(|| panic!("{path} not listed: {}", r.json))["kind"].clone();
    assert_eq!(kind("authored-crate/tests/fixtures/a.json"), "authored");
    assert_eq!(kind("mirror-crate/tests/fixtures/c.json"), "mirrors-reference");
    assert_eq!(kind("cli-crate/tests/fixtures/d.jsonl"), "cli");
    assert_eq!(r.json["files"].as_array().unwrap().len(), 4, "{}", r.json);
    assert_eq!(r.json["total"], json!({"files": 4, "authored": 2, "mirrors_reference": 1, "cli": 1, "unclassified": 0, "with_volatile": 1, "exchanges": 2}), "{}", r.json);
    assert_eq!(r.json["groups"]["authored-crate"]["authored"], 2);
    assert_eq!(r.json["groups"]["mirror-crate"]["mirrors_reference"], 1);
    let volatile = r.json["files"].as_array().unwrap().iter().find(|f| f["path"] == "authored-crate/tests/fixtures/a.json").unwrap()["volatile_pointers"].clone();
    assert_eq!(volatile, 2);
}

#[test]
fn given_a_file_no_rule_matches_when_scanned_then_it_fails_naming_the_file_and_the_rule_to_add() {
    // Given a crate with no rule
    let dir = tree(&rules(&[rule("known/**", "authored")]), &[("known/tests/fixtures/a.json", "{}"), ("new-crate/tests/fixtures/b.json", "{}")]);
    // When scanned
    let r = scan(dir.path(), &[]);
    // Then the scan fails (exit 1) with the file and a next step that names the rules file
    assert_eq!((r.code, r.json["ok"].clone()), (1, json!(false)), "{}", r.json);
    assert_eq!(r.json["unclassified"], json!(["new-crate/tests/fixtures/b.json"]));
    let next = r.json["next"].as_str().unwrap();
    assert!(next.contains("new-crate/tests/fixtures/b.json") && next.contains("fixture-provenance.json"), "{next}");
    // When the rule is added as the next step says
    std::fs::write(dir.path().join("fixture-provenance.json"), rules(&[rule("known/**", "authored"), rule("new-crate/**", "cli")]).to_string()).unwrap();
    // Then the same tree scans clean
    let r = scan(dir.path(), &[]);
    assert_eq!((r.code, r.json["ok"].clone()), (0, json!(true)), "{}", r.json);
}

#[test]
fn given_two_rules_for_one_file_when_scanned_then_the_first_decides_and_a_rule_matching_nothing_is_a_warning() {
    let dir = tree(&rules(&[rule("a/**", "cli"), rule("**", "authored"), rule("gone/**", "authored")]), &[("a/tests/fixtures/x.json", "{}")]);
    let r = scan(dir.path(), &["--files"]);
    assert_eq!((r.code, r.json["files"][0]["kind"].clone()), (0, json!("cli")), "{}", r.json);
    // The catch-all never matched, the stale rule is named in the warnings
    assert_eq!(r.json["unused_rules"], json!(["**", "gone/**"]));
    assert_eq!(r.json["warnings"].as_array().unwrap().len(), 2, "{}", r.json);
}

#[test]
fn given_rules_that_break_the_contract_when_scanned_then_exit_two_names_the_fix() {
    for (what, doc, needle) in [
        ("a mirror without the row it is re-recorded against", rules(&[json!({"match": "**", "kind": "mirrors-reference", "reason": "r"})]), "row"),
        ("a rule without a reason", rules(&[json!({"match": "**", "kind": "authored", "reason": " "})]), "reason"),
        ("another schema", json!({"schema": "other/9", "include": ["**"], "rules": []}), "schema"),
        ("an unknown kind", rules(&[json!({"match": "**", "kind": "golden", "reason": "r"})]), "golden"),
    ] {
        let dir = tree(&doc, &[("a/tests/fixtures/x.json", "{}")]);
        let r = scan(dir.path(), &[]);
        assert_eq!(r.code, 2, "{what}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains(needle), "{what}: {}", r.json);
        assert!(r.json["next"].as_str().unwrap().contains("fixture-provenance.json"), "{what}: {}", r.json);
    }
}

#[test]
fn given_a_tree_without_rules_when_scanned_then_exit_two_names_the_file_to_write() {
    let dir = tempfile::tempdir().unwrap();
    let r = scan(dir.path(), &[]);
    assert_eq!(r.code, 2, "{}", r.json);
    assert!(r.json["next"].as_str().unwrap().contains("fixture-provenance.json"), "{}", r.json);
}

/// Every file under `<crate>/tests/fixtures`, counted on the disk without the scan.
fn fixture_files(dir: &Path) -> usize {
    let mut n = 0;
    for krate in std::fs::read_dir(dir).unwrap().flatten().filter(|e| e.path().is_dir()) {
        let mut stack = vec![krate.path().join("tests/fixtures")];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else {
                    n += 1;
                }
            }
        }
    }
    n
}

#[test]
fn the_contract_fixtures_of_agent_api_are_all_classified_by_the_checked_in_rules() {
    // Given the real tree and its checked-in rules
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libraries/agent-api");
    // When it is scanned
    let r = scan(&dir, &[]);
    // Then no file is unclassified and the scan counted every fixture file the disk holds
    assert_eq!((r.code, r.json["ok"].clone()), (0, json!(true)), "{}", r.json);
    let total = &r.json["total"];
    assert_eq!(total["files"], json!(fixture_files(&dir)), "{total}");
    assert_eq!(total["authored"].as_u64().unwrap() + total["mirrors_reference"].as_u64().unwrap() + total["cli"].as_u64().unwrap(), total["files"].as_u64().unwrap());
    assert!(total["with_volatile"].as_u64().unwrap() > 0, "{total}");
}

#[test]
fn the_fixtures_output_has_the_fields_its_help_documents() {
    let dir = tree(&rules(&[rule("**", "authored")]), &[("a/tests/fixtures/x.json", WITH_VOLATILE)]);
    let r = scan(dir.path(), &["--files"]);
    let help = wf_mimic::HELP.iter().find(|(p, _, _)| *p == "mimic verify").unwrap().2;
    let block = &help[help.find("Provenance scan").unwrap()..];
    for field in ["ok", "dir", "rules", "total", "groups", "unclassified", "unused_rules", "files", "warnings", "next"] {
        assert!(block.contains(field) && r.json.get(field).is_some(), "{field}: {}", r.json);
    }
    for count in ["files", "authored", "mirrors_reference", "cli", "unclassified", "with_volatile", "exchanges"] {
        assert!(block.contains(count) && r.json["total"].get(count).is_some(), "{count}");
    }
}
