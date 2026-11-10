use crate::support::*;
use std::path::Path;

fn check<'a>(r: &'a Run, name: &str) -> &'a Value {
    r.json["sets"][0]["checks"].as_array().unwrap_or_else(|| panic!("no checks: {}", r.json)).iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("no check {name}: {}", r.json))
}

#[test]
fn a_fresh_set_verifies_offline_and_says_what_it_was_checked_against() {
    let w = World::new();
    w.record();
    let r = w.run("verify", &[]);
    assert_eq!((r.code, r.json["ok"].clone(), r.json["sets"][0]["kind"].clone()), (0, json!(true), json!("recorded")), "{}", r.json);
    for name in ["pin_hash", "rules_hash", "corpus_hash"] {
        assert_eq!(check(&r, name)["status"], "ok", "{name}");
    }
}

#[test]
fn moving_the_pin_the_rules_or_the_corpus_makes_the_set_stale_with_the_command_that_fixes_it() {
    let more_rules = format!("{},{{\"kind\":\"drop-header\",\"names\":[\"x-extra\"]}}]}}", RULES.trim_end().strip_suffix("]}").unwrap());
    for (what, file, change, stale) in [
        ("the pin moves", "pin.json", r#"{"version":"toy-2.0"}"#, "pin_hash"),
        ("a rule is added", "rules.json", more_rules.as_str(), "rules_hash"),
        ("a case is added", "corpus.json", &crate::support::corpus(&[json!({"kind": "corpus", "name": "more", "cases": [{"id": "more-1", "input": {}}]})]).to_string(), "corpus_hash"),
    ] {
        // Given a recorded set
        let w = World::new();
        w.record();
        // When one of the files it was recorded from changes
        w.write(file, change);
        let r = w.run("verify", &[]);
        // Then that check alone is stale, the set is not ok and the next step is a re-record of this set
        assert_eq!((r.code, r.json["ok"].clone()), (1, json!(false)), "{what}: {}", r.json);
        for name in ["pin_hash", "rules_hash", "corpus_hash"] {
            let want = if name == stale { "stale" } else { "ok" };
            assert_eq!(check(&r, name)["status"], want, "{what}: {name}: {}", r.json);
        }
        assert!(r.json["next"].as_str().unwrap().starts_with(&format!("wf mimic record {}", w.arg())), "{what}: {}", r.json["next"]);
        // When it is re-recorded as the next step says
        w.record();
        // Then it verifies again
        assert_eq!(w.run("verify", &[]).code, 0, "{what}");
    }
}

#[test]
fn a_cassette_edited_by_hand_is_refused_as_edited() {
    let w = World::new();
    w.record();
    let edited = w.read("interactions.jsonl").replacen("\"x\"", "\"y\"", 1);
    w.write("interactions.jsonl", &edited);
    let r = w.run("verify", &[]);
    assert_eq!((r.code, r.json["ok"].clone()), (1, json!(false)), "{}", r.json);
    assert!(r.json["sets"][0]["error"].as_str().unwrap().contains("edited by hand"), "{}", r.json);
    // and replay refuses it the same way, rather than diffing against what nobody recorded
    assert_eq!(w.run("replay", &["--candidate", "same"]).code, 2);
}

#[test]
fn all_finds_every_set_under_the_root_and_skips_what_is_vendored() {
    // Given three sets, one of them stale, and a fourth inside node_modules
    let w = World::new();
    w.record();
    let root = w.dir.path();
    let copy = |to: &std::path::Path| {
        std::fs::create_dir_all(to).unwrap();
        for f in ["cassette.json", "interactions.jsonl", "rules.json", "corpus.json", "mimic.toml", "pin.json"] {
            std::fs::copy(w.path(f), to.join(f)).unwrap();
        }
    };
    copy(&root.join("nested/deep/second"));
    copy(&root.join("node_modules/vendored"));
    std::fs::write(root.join("nested/deep/second/pin.json"), r#"{"version":"other"}"#).unwrap();
    // When every set under the root is verified
    let r = mimic(&["verify", "--all", "--root", &root.display().to_string()], &[]);
    // Then the two sets are found, the vendored one is not, and only the stale one fails
    let sets = r.json["sets"].as_array().unwrap();
    let verdicts: Vec<(&str, bool)> = sets.iter().map(|s| (s["set"].as_str().unwrap(), s["ok"].as_bool().unwrap())).collect();
    assert_eq!((r.code, verdicts), (1, vec![("second", false), ("toy-set", true)]), "{}", r.json);
    // When there is no set at all
    let empty = tempfile::tempdir().unwrap();
    let r = mimic(&["verify", "--all", "--root", &empty.path().display().to_string()], &[]);
    // Then it is a usage error, not a pass
    assert_eq!((r.code, r.json["ok"].clone()), (2, json!(false)), "{}", r.json);
}

#[test]
fn live_verification_re_records_the_reference_and_catches_a_reference_that_moved() {
    // Given a recorded set
    let w = World::new();
    w.record();
    // When it is verified live against the same reference
    let r = w.run("verify", &["--live"]);
    // Then the re-recording has no diff
    assert_eq!((r.code, r.json["sets"][0]["live"]["severity"].clone()), (0, json!("none")), "{}", r.json);
    // When the reference behaves differently though the pin file is unchanged
    let toml = w.read("mimic.toml").replacen(&format!("command = [{:?}]", toy().display().to_string()), &format!("command = [{:?}, \"--tweak\", \"drop-key\"]", toy().display().to_string()), 1);
    w.write("mimic.toml", &toml);
    let r = w.run("verify", &["--live"]);
    // Then offline checks pass and the live one fails with the diff
    assert_eq!(r.code, 1, "{}", r.json);
    assert_eq!((check(&r, "pin_hash")["status"].clone(), r.json["sets"][0]["live"]["severity"].clone()), (json!("ok"), json!("breaking")));
    assert_eq!(w.run("verify", &[]).code, 0);
}

#[test]
fn a_set_without_its_mimic_toml_is_checked_for_integrity_only_and_says_so() {
    let w = World::new();
    w.record();
    std::fs::remove_file(w.path("mimic.toml")).unwrap();
    let r = w.run("verify", &[]);
    assert_eq!(r.code, 0, "{}", r.json);
    assert!(r.json["sets"][0]["warnings"][0].as_str().unwrap().contains("no mimic.toml"), "{}", r.json);
}

fn broken_tomls() -> Vec<(&'static str, String)> {
    let w = World::new();
    vec![("bad TOML", "[reference\ncommand = ".to_string()), ("an unknown key", format!("{}\nsurprise = true\n", w.read("mimic.toml")))]
}

#[test]
fn a_mimic_toml_that_exists_but_does_not_load_fails_verify_with_its_error_and_a_next_step() {
    for (what, toml) in broken_tomls() {
        // Given a recorded set whose pin has moved since, and whose mimic.toml is then broken
        let w = World::new();
        w.record();
        w.write("pin.json", r#"{"version":"toy-2.0"}"#);
        w.write("mimic.toml", &toml);
        // When it is verified
        let r = w.run("verify", &[]);
        // Then the set fails with the load error rather than passing as if it had no config
        let set = &r.json["sets"][0];
        assert_eq!((r.code, set["ok"].clone()), (1, json!(false)), "{what}: {}", r.json);
        assert!(set["error"].as_str().unwrap().contains("mimic.toml"), "{what}: {}", r.json);
        assert!(set["next"].as_str().unwrap().contains("mimic.toml"), "{what}: {}", r.json);
    }
}

#[test]
fn a_mimic_toml_that_does_not_load_fails_diff_instead_of_diffing_with_defaults() {
    for (what, toml) in broken_tomls() {
        // Given two recorded sets, the first with a broken mimic.toml
        let (a, b) = (World::new(), World::new());
        a.record();
        b.record();
        a.write("mimic.toml", &toml);
        // When they are diffed
        let r = mimic(&["diff", &a.arg(), &b.arg()], &[]);
        // Then the diff cannot run and names the file
        assert_eq!(r.code, 2, "{what}: {}", r.json);
        assert!(r.json["error"].as_str().unwrap().contains("mimic.toml"), "{what}: {}", r.json);
    }
}

#[test]
fn a_set_without_a_mimic_toml_still_diffs_with_the_defaults() {
    // Given two recorded sets, the first without its mimic.toml
    let (a, b) = (World::new(), World::new());
    a.record();
    b.record();
    std::fs::remove_file(a.path("mimic.toml")).unwrap();
    // When they are diffed
    let r = mimic(&["diff", &a.arg(), &b.arg()], &[]);
    // Then the diff runs
    assert_eq!((r.code, r.json["severity"].clone()), (0, json!("none")), "{}", r.json);
}

#[test]
fn a_set_pinned_to_a_directory_of_the_repository_is_stale_once_a_file_of_it_changes_and_current_again_when_it_is_reverted() {
    // Given a set recorded against the git tree of a directory (the reference's own code), not a pin file
    let w = World::new();
    let code = w.dir.path().join("code");
    std::fs::create_dir_all(code.join("src")).unwrap();
    std::fs::write(code.join("src/rule.ts"), "export const rule = 1\n").unwrap();
    assert!(std::process::Command::new("git").args(["init", "--quiet"]).arg(w.dir.path()).status().unwrap().success());
    let toml = w.read("mimic.toml").replace("pin = \"pin.json\"\nversion_pointer = \"/version\"\n", "pin_tree = \"../code\"\nversion = \"1\"\n");
    w.write("mimic.toml", &toml);
    let recorded = w.record();
    let pin = recorded.json["provenance"]["target"]["pin_hash"].as_str().unwrap().to_string();
    assert!(pin.starts_with("git-tree:"), "{}", recorded.json);

    // When nothing changed, Then the pin is current
    let fresh = w.run("verify", &[]);
    assert_eq!((fresh.code, check(&fresh, "pin_hash")["status"].clone()), (0, json!("ok")), "{}", fresh.json);

    // When a file of the directory changes (not committed, not staged), Then the pin is stale and verify names the command that fixes it
    std::fs::write(code.join("src/rule.ts"), "export const rule = 2\n").unwrap();
    let stale = w.run("verify", &[]);
    assert_eq!((stale.code, check(&stale, "pin_hash")["status"].clone()), (1, json!("stale")), "{}", stale.json);
    assert_ne!(check(&stale, "pin_hash")["current"], json!(pin));
    assert!(stale.json["next"].as_str().unwrap().contains("wf mimic record"), "{}", stale.json);

    // And putting it back makes the recording current again
    std::fs::write(code.join("src/rule.ts"), "export const rule = 1\n").unwrap();
    let back = w.run("verify", &[]);
    assert_eq!((back.code, check(&back, "pin_hash")["status"].clone()), (0, json!("ok")), "{}", back.json);
}

#[test]
fn a_reference_with_both_a_pin_file_and_a_pinned_directory_is_refused_by_name() {
    let w = World::new();
    w.record();
    let toml = w.read("mimic.toml").replace("pin = \"pin.json\"", "pin = \"pin.json\"\npin_tree = \".\"");
    w.write("mimic.toml", &toml);
    let r = w.run("verify", &[]);
    assert!(r.code != 0 && r.json["sets"][0]["error"].as_str().unwrap().contains("pin_tree"), "{}", r.json);
}

#[test]
fn all_live_skips_an_imported_set_with_no_corpus_but_a_named_one_is_an_error() {
    // Given a recorded set and an imported set (no reference, no corpus) under one root
    let w = World::new();
    w.record();
    let root = w.dir.path();
    let imported = root.join("imported");
    std::fs::create_dir_all(&imported).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libraries/agent-api/exec-hosted/tests/cassettes/exec-hosted");
    for e in std::fs::read_dir(source).unwrap().flatten() {
        std::fs::copy(e.path(), imported.join(e.file_name())).unwrap();
    }
    // When every set is verified live
    let r = mimic(&["verify", "--all", "--live", "--root", &root.display().to_string()], &[]);
    // Then the imported set is skipped with a reason and nothing fails because of it
    let sets = r.json["sets"].as_array().unwrap();
    let skipped = sets.iter().find(|s| s["set"] == "imported").and_then(|s| s["live"]["skipped"].as_str()).unwrap_or_default();
    assert!(skipped.contains("no corpus"), "{}", r.json);
    assert_eq!((r.code, &r.json["ok"]), (0, &json!(true)), "{}", r.json);
    // When that set is named
    let r = mimic(&["verify", &imported.display().to_string(), "--live"], &[]);
    // Then the missing corpus is an error
    assert!(r.code != 0 && r.json["sets"][0]["error"].as_str().unwrap_or_default().contains("needs a corpus"), "{}", r.json);
}
