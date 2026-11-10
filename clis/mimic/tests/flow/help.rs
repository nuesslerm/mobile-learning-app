use crate::support::*;
use wf_mimic::HELP;

/// The words of a help row after `mimic`, plus `--help`.
fn words(path: &str) -> Vec<&str> {
    path.split(' ').skip(1).chain(["--help"]).collect()
}

fn mimic_args(args: &[&str]) -> Run {
    // help is plain text, not JSON: run it directly
    let beside = std::path::Path::new(env!("CARGO_BIN_EXE_wf-mimic")).parent().unwrap().display().to_string();
    let bin = std::env::var_os("WF_BIN").map_or_else(|| std::path::PathBuf::from(env!("CARGO_BIN_EXE_wf-mimic")), std::path::PathBuf::from);
    let out = std::process::Command::new(bin).arg("mimic").args(args).env("PATH", format!("{beside}:{}", std::env::var("PATH").unwrap_or_default())).stdin(std::process::Stdio::null()).output().unwrap();
    Run { code: out.status.code().unwrap_or(-1), json: Value::String(String::from_utf8_lossy(&out.stdout).into_owned()), stderr: String::from_utf8_lossy(&out.stderr).into_owned() }
}

#[test]
fn every_command_prints_its_committed_help_and_a_bare_group_prints_it_to_stderr() {
    for (path, bare, text) in HELP {
        // When the help of a command is asked for
        let r = mimic_args(&words(path));
        // Then the committed text comes out on stdout with exit 0
        assert_eq!((r.code, r.json.as_str().unwrap()), (0, *text), "wf {path}");
        assert!(text.starts_with(&format!("Usage: wf {path}")), "{path}");
        // When a bare group runs without a sub-word
        if *bare {
            let r = mimic_args(&[]);
            // Then its help goes to stderr with exit 1
            assert_eq!((r.code, r.stderr.as_str()), (1, *text), "bare {path}");
        }
    }
}

#[test]
fn the_reference_the_owner_reads_carries_every_help_text() {
    let doc = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/wf-cli.md")).expect("docs/reference/wf-cli.md");
    for (path, _, text) in HELP {
        assert!(doc.contains(text.trim_end()), "docs/reference/wf-cli.md lacks the help of wf {path}");
        assert!(doc.lines().any(|l| l.starts_with("### ") && l.ends_with(&format!("`wf {path}`"))), "no section for wf {path}");
    }
}

/// The top-level field names of the `{...}` that follows `Output (schema <schema>):` in a help text, and which are optional (`name?`).
fn documented(text: &str) -> (String, Vec<(String, bool)>) {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let start = flat.find("Output (schema ").expect("an Output block") + "Output (schema ".len();
    let schema = flat[start..].split(')').next().unwrap().to_string();
    let open = start + flat[start..].find('{').unwrap();
    let (mut depth, mut item, mut items) = (0, String::new(), Vec::new());
    for c in flat[open..].chars() {
        match c {
            '{' | '[' => {
                depth += 1;
                if depth > 1 {
                    item.push(c);
                }
            }
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                item.push(c);
            }
            ',' if depth == 1 => items.push(std::mem::take(&mut item)),
            _ => item.push(c),
        }
    }
    items.push(item);
    let names = items.iter().map(|i| i.trim().split([':', ' ']).next().unwrap().to_string()).map(|n| (n.trim_end_matches('?').to_string(), n.ends_with('?'))).collect();
    (schema, names)
}

#[test]
fn the_json_of_every_verb_has_the_fields_its_help_documents() {
    // Given a recorded set and the output of each verb on it
    let w = World::new();
    let record = w.record();
    let other = World::new();
    other.record();
    let outputs = [
        ("mimic record", record),
        ("mimic replay", w.run("replay", &["--candidate", "extra"])),
        ("mimic diff", mimic(&["diff", &w.arg(), &other.arg()], &[])),
        ("mimic verify", w.run("verify", &[])),
    ];
    for (path, run) in outputs {
        // When the documented fields of the verb's help are compared with what it printed
        let text = HELP.iter().find(|(p, _, _)| *p == path).unwrap().2;
        let (schema, fields) = documented(text);
        // Then the schema is the one it says, and every field not marked optional is there
        assert_eq!(run.json["schema"], json!(schema), "{path}");
        assert!(fields.len() >= 5, "{path}: {fields:?}");
        for (name, optional) in fields {
            assert!(optional || run.json.get(&name).is_some(), "{path}: documented field {name} is missing from {}", run.json);
        }
    }
}

#[test]
fn a_failure_carries_the_error_the_evidence_the_next_step_and_exit_two() {
    let w = World::new();
    let r = w.run("replay", &["--candidate", "same"]);
    assert_eq!(r.code, 2);
    for field in ["ok", "error", "next", "evidence", "warnings", "schema"] {
        assert!(r.json.get(field).is_some(), "{field}: {}", r.json);
    }
}
