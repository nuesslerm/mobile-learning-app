//! `cli_detect` and `skill_scan` on real directories and real processes: which programs the executor runs and how, and which skill files it
//! reads and which it never does. The recorded exchanges of both actions are replayed in `contracts.rs`.
use exec_proto::{Code, ExecError};
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::execute;
use serde_json::{json, Value};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn call(ex: &LocalExecutor, tool: &str, args: Value) -> Result<Value, ExecError> {
    execute(ex, "c1", tool, &args, &mut |_, _| {}, &mut |_| {})
}

fn dir() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let p = std::fs::canonicalize(d.path()).unwrap();
    (d, p)
}

fn program(bin: &Path, name: &str, body: &str, mode: u32) {
    let path = bin.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn skill(base: &Path, dir: &str, name: &str, text: &str) {
    let at = base.join(dir).join(name);
    std::fs::create_dir_all(&at).unwrap();
    std::fs::write(at.join("SKILL.md"), text).unwrap();
}

fn found(result: &Value, program: &str) -> Value {
    result["clis"].as_array().unwrap().iter().find(|c| c["program"] == program).unwrap().clone()
}

#[test]
fn given_agent_clis_that_answer_fail_hang_or_are_not_runnable_when_detected_then_each_is_reported_as_it_behaved() {
    // Given a path with a CLI that prints its version, one that prints it on stderr, one that fails, one that hangs; and a file that is not executable
    let ((_r, root), (_b, bin)) = (dir(), dir());
    let log = bin.join("argv.log");
    program(&bin, "claude", &format!("printf '%s|' \"$#\" \"$1\" >> {}\nprintf '2.1.7 (Claude Code)\\nsecond line\\n'", log.display()), 0o755);
    program(&bin, "codex", "echo 'codex-cli 0.9.1' >&2", 0o755);
    program(&bin, "opencode", "echo broken >&2\nexit 3", 0o755);
    let ex = LocalExecutor::new(&root).unwrap().with_cli_path(&format!("{}:/bin:/usr/bin", bin.display())).with_version_timeout(Duration::from_millis(400));
    // When all three are detected
    let all = call(&ex, "cli_detect", json!({"programs": ["claude", "codex", "opencode"]})).unwrap();
    // Then the first answers with its first line, the second with what it wrote to stderr, the third as exited with its code and no version
    assert_eq!(found(&all, "claude")["version"], "2.1.7 (Claude Code)");
    assert_eq!(found(&all, "claude")["path"], bin.join("claude").to_str().unwrap());
    assert_eq!(found(&all, "codex")["version"], "codex-cli 0.9.1");
    assert_eq!((found(&all, "opencode")["installed"].clone(), found(&all, "opencode")["error"].clone(), found(&all, "opencode").get("version").cloned()), (json!(true), json!("exited with 3"), None));
    // And the CLI was started once with exactly the one argument `--version`
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "1|--version|");
    // When a program hangs
    program(&bin, "codex", "sleep 30", 0o755);
    let started = Instant::now();
    let hung = call(&ex, "cli_detect", json!({"programs": ["codex"]})).unwrap();
    // Then it is reported as timed out, installed and without a version, long before it would have ended
    assert_eq!((found(&hung, "codex")["installed"].clone(), found(&hung, "codex")["error"].clone(), found(&hung, "codex").get("version").cloned()), (json!(true), json!("timed out"), None));
    assert!(started.elapsed() < Duration::from_secs(10), "{:?}", started.elapsed());
    // When the file is not executable
    program(&bin, "codex", "echo 1", 0o644);
    // Then it is not installed
    assert_eq!(found(&call(&ex, "cli_detect", json!({"programs": ["codex"]})).unwrap(), "codex"), json!({"program": "codex", "installed": false}));
}

#[test]
fn given_a_program_name_that_is_not_an_agent_cli_when_detected_then_it_is_refused_and_nothing_is_started() {
    // Given a path holding a program that records every start, and one called by a made-up name
    let ((_r, root), (_b, bin)) = (dir(), dir());
    let marker = bin.join("started");
    for name in ["claude", "rm", "--version"] {
        program(&bin, name, &format!("echo {name} >> {}", marker.display()), 0o755);
    }
    let ex = LocalExecutor::new(&root).unwrap().with_cli_path(bin.to_str().unwrap());
    // When names that are an option, a path to the real CLI and a made-up program are detected
    let refused: Vec<Code> = ["--version", "./claude", "rm", "claude --help"].iter().map(|name| call(&ex, "cli_detect", json!({"programs": [name]})).unwrap_err().code).collect();
    // Then each is INVALID_ARGS and no program was started
    assert!(refused.iter().all(|c| *c == Code::InvalidArgs), "{refused:?}");
    assert!(!marker.exists());
}

#[test]
fn given_a_relative_entry_on_the_path_when_detected_then_a_program_of_the_working_directory_is_never_found() {
    // Given a root holding a program called claude and a path that names only the current directory and an empty one
    let ((_r, root), (_e, empty)) = (dir(), dir());
    program(&root, "claude", "echo planted", 0o755);
    let ex = LocalExecutor::new(&root).unwrap().with_cli_path(&format!(".::{}", empty.display()));
    // When it is detected, Then it is not installed
    assert_eq!(found(&call(&ex, "cli_detect", json!({"programs": ["claude"]})).unwrap(), "claude"), json!({"program": "claude", "installed": false}));
}

#[test]
fn given_skill_directories_that_link_out_when_scanned_then_only_skills_inside_the_root_and_the_home_are_read() {
    // Given a root and a home with a skill each, a skill directory linked to a skill outside both, and a whole skills directory linked outside
    let ((_r, root), (_h, home), (_o, outside)) = (dir(), dir(), dir());
    skill(&root, ".claude/skills", "inside", "---\nname: inside\ndescription: stays\n---\n");
    skill(&outside, "", "stolen", "---\nname: stolen\ndescription: secret\n---\n");
    skill(&outside, "skills", "leak", "---\nname: leak\ndescription: secret\n---\n");
    symlink(outside.join("stolen"), root.join(".claude/skills/linked")).unwrap();
    symlink(&outside, root.join(".agents")).unwrap();
    skill(&home, ".claude/skills", "mine", "---\nname: mine\n---\n");
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    symlink(&outside, home.join(".codex/skills")).unwrap();
    let ex = LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home));
    // When both are scanned
    let result = call(&ex, "skill_scan", json!({"root": ".", "home": true})).unwrap();
    // Then only the two inside are listed, and the home is named below ~ and not by where it is
    let names: Vec<&str> = result["skills"].as_array().unwrap().iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["inside", "mine"]);
    assert!(!result.to_string().contains(home.to_str().unwrap()) && !result.to_string().contains("secret"), "{result}");
    // And a root that is a link out of the workspace is refused
    symlink(&outside, root.join("out")).unwrap();
    assert_eq!(call(&ex, "skill_scan", json!({"root": "out"})).unwrap_err().code, Code::PathOutsideRoot);
}

#[test]
fn given_more_skills_than_the_most_when_scanned_then_the_most_come_back_in_order_and_the_answer_says_it_stopped() {
    // Given one skill more than the contract's most, named so their order is the number's
    let ((_r, root), most) = (dir(), exec_fs::SKILLS_MAX);
    for n in 0..=most {
        skill(&root, ".claude/skills", &format!("s{n:04}"), &format!("---\nname: n{n}\n---\n"));
    }
    let ex = LocalExecutor::new(&root).unwrap();
    // When scanned
    let result = call(&ex, "skill_scan", json!({"root": "."})).unwrap();
    // Then the first `most` are listed and the answer is marked truncated
    let skills = result["skills"].as_array().unwrap();
    assert_eq!((skills.len(), result["truncated"].clone(), skills[most - 1]["name"].clone()), (most, json!(true), json!(format!("n{}", most - 1))));
}

#[test]
fn given_a_skill_with_no_front_matter_or_a_huge_file_when_scanned_then_the_directory_names_it_and_only_the_top_of_the_file_is_read() {
    // Given a skill that starts with text and one whose front matter is followed by a file larger than the read limit
    let ((_r, root), big) = (dir(), "x".repeat(exec_fs::SKILL_READ_MAX as usize * 2));
    skill(&root, ".claude/skills", "plain", "just text\n");
    skill(&root, ".claude/skills", "large", &format!("---\nname: large\ndescription: ok\n---\n{big}"));
    let ex = LocalExecutor::new(&root).unwrap();
    // When scanned
    let skills = call(&ex, "skill_scan", json!({"root": "."})).unwrap()["skills"].clone();
    // Then the one without front matter is named by its directory and the large one is read from its top
    assert_eq!((skills[1]["name"].clone(), skills[1].get("description").cloned()), (json!("plain"), None));
    assert_eq!((skills[0]["name"].clone(), skills[0]["description"].clone()), (json!("large"), json!("ok")));
}

#[test]
fn given_the_scripted_clis_on_the_executors_path_when_detected_then_each_reports_the_version_it_is_scripted_with_and_no_turn_is_spent() {
    // Given the scripted claude and codex with versions and a turn each, and no opencode on the path
    let (_r, root) = dir();
    let cli = exec_testkit::ScriptedCli::install();
    let turn = || exec_testkit::Script::of(vec![exec_testkit::Turn::default().then_line("a turn")]);
    cli.script("claude", &turn().printing_version("2.1.7 (Claude Code)"));
    cli.script("codex", &turn().printing_version("codex-cli 0.9.1"));
    let only = tempfile::tempdir().unwrap();
    for name in ["claude", "codex"] {
        symlink(cli.dir().join(name), only.path().join(name)).unwrap();
    }
    let ex = LocalExecutor::new(&root).unwrap().with_cli_path(only.path().to_str().unwrap());
    // When all three are detected
    let result = call(&ex, "cli_detect", json!({"programs": ["claude", "codex", "opencode"]})).unwrap();
    // Then the two report their versions, the third is absent, and the scripts' logs hold no start
    assert_eq!((found(&result, "claude")["version"].clone(), found(&result, "codex")["version"].clone(), found(&result, "opencode")["installed"].clone()), (json!("2.1.7 (Claude Code)"), json!("codex-cli 0.9.1"), json!(false)));
    assert!(cli.invocations("claude").is_empty() && cli.invocations("codex").is_empty());
}
