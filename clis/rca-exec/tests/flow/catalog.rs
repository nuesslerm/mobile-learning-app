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
fn given_skill_directories_the_caller_names_when_scanned_then_only_those_are_read_and_the_ones_that_exist_are_reported() {
    // Given a root with a skill below .trae/skills and an empty .kiro/skills, and a home with one below .config/opencode/skills
    let ((_r, root), (_h, home)) = (dir(), dir());
    skill(&root, ".trae/skills", "fmt", "---\nname: fmt\n---\n");
    std::fs::create_dir_all(root.join(".kiro/skills")).unwrap();
    skill(&root, ".claude/skills", "ignored", "---\nname: ignored\n---\n");
    skill(&home, ".config/opencode/skills", "lint", "---\nname: lint\n---\n");
    let ex = LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home));
    // When the caller names the directories of the root and of the home
    let args = json!({"root": ".", "home": true, "dirs": [".trae/skills", ".kiro/skills", ".grok/skills"], "home_dirs": [".config/opencode/skills"]});
    let result = call(&ex, "skill_scan", args).unwrap();
    // Then only the named directories are read, in their order
    let found: Vec<(&str, &str)> = result["skills"].as_array().unwrap().iter().map(|s| (s["name"].as_str().unwrap(), s["dir"].as_str().unwrap())).collect();
    assert_eq!(found, [("fmt", ".trae/skills"), ("lint", ".config/opencode/skills")]);
    // And the directories that exist are reported, an empty one too, a missing one not
    assert_eq!(result["present"], json!([{"scope": "workspace", "dir": ".trae/skills"}, {"scope": "workspace", "dir": ".kiro/skills"}, {"scope": "home", "dir": ".config/opencode/skills"}]));
    // And a directory that climbs out or is absolute is refused before anything is read
    for bad in ["../x", "/etc", "~/x", "a//b"] {
        assert_eq!(call(&ex, "skill_scan", json!({"root": ".", "dirs": [bad]})).unwrap_err().code, Code::InvalidArgs, "{bad}");
    }
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

#[test]
fn given_a_claude_projects_directory_when_a_file_is_read_in_its_scope_then_only_files_below_it_are_read_and_a_link_out_is_refused() {
    // Given a home whose .claude/projects holds a script and a link to a file outside it, and a file outside
    let ((_r, root), (_h, home), (_o, outside)) = (dir(), dir(), dir());
    let projects = home.join(".claude/projects/p");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(projects.join("flow.js"), "export const a = 1").unwrap();
    std::fs::write(outside.join("secret.js"), "x").unwrap();
    symlink(outside.join("secret.js"), projects.join("linked.js")).unwrap();
    let ex = LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home));
    let read = |path: &std::path::Path| call(&ex, "read_chunk", json!({"path": path.to_str().unwrap(), "scope": "claudeProjects"}));
    // When the script is read, a link out, a file outside, and the directory itself
    let script = read(&projects.join("flow.js")).unwrap();
    let linked = read(&projects.join("linked.js")).unwrap_err();
    let elsewhere = read(&outside.join("secret.js")).unwrap_err();
    let dir_read = read(&projects).unwrap_err();
    // Then the script is its bytes, the link and the file outside are outside the root, and a directory is no file
    assert_eq!((script["bytes"].clone(), script["eof"].clone()), (json!(18), json!(true)));
    assert_eq!((linked.code, elsewhere.code, dir_read.code), (Code::PathOutsideRoot, Code::PathOutsideRoot, Code::NotAFile));
    // And without the scope the same absolute path is outside the workspace
    assert_eq!(call(&ex, "read_chunk", json!({"path": projects.join("flow.js").to_str().unwrap()})).unwrap_err().code, Code::PathOutsideRoot);
}

fn login_cli(bin: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = r#"#!/bin/sh
case "$1 $2" in
  "auth login")
    echo "Open https://example.test/oauth to sign in"
    echo "Paste code:"
    read code
    if [ "$code" = "bad" ]; then echo "Invalid code" >&2; exit 1; fi
    mkdir -p "$HOME/.claude"
    printf '{"t":"%s"}' "$code" > "$HOME/.claude/.credentials.json"
    echo "Logged in"
    ;;
  "auth logout") rm -f "$HOME/.claude/.credentials.json" ;;
  *) echo "claude 1.0" ;;
esac
"#;
    let path = bin.join("claude");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The login's status once it has ended, polled for at most five seconds.
fn ended(ex: &LocalExecutor, flow: &str) -> Value {
    for _ in 0..250 {
        let state = call(ex, "login_status", json!({"flow": flow})).unwrap();
        if state["ended"] == true {
            return state;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the login {flow} did not end");
}

#[test]
fn given_an_agent_cli_with_a_login_when_it_is_started_answered_and_ended_then_the_executors_home_holds_the_login_and_a_wrong_code_fails_with_the_cli_s_words() {
    // Given an executor whose home is empty and which has a claude that prompts for a code and writes its credential file
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    login_cli(&bin);
    let ex = LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home)).with_cli_path(bin.to_str().unwrap());
    // When a login is started and its output read
    let flow = call(&ex, "login_start", json!({"program": "claude"})).unwrap()["flow"].as_str().unwrap().to_string();
    let mut printed = String::new();
    for _ in 0..250 {
        let state = call(&ex, "login_status", json!({"flow": flow, "after": printed.len()})).unwrap();
        printed.push_str(state["output"].as_str().unwrap());
        if printed.contains("Paste code:") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Then the address and the prompt are in it and the CLI holds no login yet
    assert!(printed.contains("https://example.test/oauth") && printed.contains("Paste code:"), "{printed}");
    assert_eq!(call(&ex, "login_status", json!({"flow": flow})).unwrap()["signed_in"], false);
    // And when the code is sent, the login ends well and the credential file is in the home
    call(&ex, "login_send", json!({"flow": flow, "text": "abc"})).unwrap();
    let done = ended(&ex, &flow);
    assert_eq!((done["ok"].clone(), done["signed_in"].clone()), (json!(true), json!(true)), "{done}");
    assert_eq!(std::fs::read_to_string(home.join(".claude/.credentials.json")).unwrap(), r#"{"t":"abc"}"#);
    // And an ended login takes no more input, a status of the whole output starts at 0 and a logout takes the login away
    assert_eq!(call(&ex, "login_send", json!({"flow": flow, "text": "x"})).unwrap_err().code, Code::NotFound);
    assert!(call(&ex, "login_status", json!({"flow": flow})).unwrap()["output"].as_str().unwrap().contains("Logged in"));
    assert_eq!(call(&ex, "logout", json!({"program": "claude"})).unwrap()["signed_in"], false);
    // And a wrong code ends the login as failed with the CLI's words
    let second = call(&ex, "login_start", json!({"program": "claude"})).unwrap()["flow"].as_str().unwrap().to_string();
    call(&ex, "login_send", json!({"flow": second, "text": "bad"})).unwrap();
    let failed = ended(&ex, &second);
    assert_eq!((failed["ok"].clone(), failed["signed_in"].clone()), (json!(false), json!(false)), "{failed}");
    assert!(failed["error"].as_str().unwrap().contains("Invalid code"), "{failed}");
}

#[test]
fn given_logins_the_caller_gets_wrong_when_asked_then_each_is_refused_and_a_running_login_is_stopped_and_forgotten_by_a_cancel() {
    // Given an executor with a claude and no codex
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    login_cli(&bin);
    let ex = LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home)).with_cli_path(bin.to_str().unwrap());
    // When a program that is no agent CLI, one that is missing, an unknown flow and a program-less request are asked for
    let shell = call(&ex, "login_start", json!({"program": "sh"})).unwrap_err();
    let missing = call(&ex, "login_start", json!({"program": "codex"})).unwrap_err();
    let unknown = call(&ex, "login_status", json!({"flow": "login-99"})).unwrap_err();
    let none = call(&ex, "login_start", json!({})).unwrap_err();
    // Then each is refused with its own words
    assert_eq!((shell.code, missing.code, unknown.code, none.code), (Code::InvalidArgs, Code::NotFound, Code::NotFound, Code::InvalidArgs));
    assert!(missing.message.contains("codex is not installed here"), "{missing:?}");
    // And a running login is stopped by a cancel, which forgets it, and a second cancel finds none
    let flow = call(&ex, "login_start", json!({"program": "claude"})).unwrap()["flow"].as_str().unwrap().to_string();
    assert_eq!(call(&ex, "login_cancel", json!({"flow": flow})).unwrap()["cancelled"], true);
    assert_eq!(call(&ex, "login_status", json!({"flow": flow})).unwrap_err().code, Code::NotFound);
    assert_eq!(call(&ex, "login_cancel", json!({"flow": flow})).unwrap()["cancelled"], false);
}

#[test]
fn given_an_agent_cli_when_the_executors_home_holds_its_credential_file_then_the_detection_says_it_is_signed_in_and_a_tool_or_a_missing_cli_says_nothing() {
    // Given an executor with a claude and a git, and a home with no login
    let ((_r, root), (_h, home), (_b, bin)) = (dir(), dir(), dir());
    login_cli(&bin);
    for tool in ["git"] {
        let path = bin.join(tool);
        std::fs::write(&path, "#!/bin/sh\necho git 2.43\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let ex = LocalExecutor::new(&root).unwrap().with_skill_home(Some(&home)).with_cli_path(bin.to_str().unwrap());
    let detect = || call(&ex, "cli_detect", json!({"programs": ["claude", "codex", "git"]})).unwrap()["clis"].clone();
    // When it is detected before and after the credential file is written
    let before = detect();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
    let after = detect();
    // Then claude is signed out and then signed in, and codex (missing) and git (a tool) carry no such field
    assert_eq!((before[0]["signed_in"].clone(), after[0]["signed_in"].clone()), (json!(false), json!(true)), "{before} {after}");
    assert!(after[1].get("signed_in").is_none() && after[2].get("signed_in").is_none(), "{after}");
}
