//! The hosted family through the real adapter: `gh` and `glab` are the replay double of exec-testkit on the executor's PATH, so every verb runs a
//! real process with the argv, stdin, environment and directory the executor chooses, and prints what the real CLIs print (written from their
//! documented JSON and text output). No account, token or network is involved.
use exec_hosted::testing::{fixtures, Exchange, Fixture};
use exec_hosted::{ACTIONS, AUTH_LOGIN_CANCEL, AUTH_LOGIN_START, AUTH_LOGIN_WAIT, OUTPUT_CAP, PR_COMMENT, PR_DIFF, PR_LABELS, PR_LIST, PR_VIEW, REPO_CLONE, REPO_PUBLISH};
use exec_proto::{Code, ExecResult, Stream};
use exec_testkit::{HostedClis, Rule, TempRepo};
use rca_exec::adapters::local_executor::LocalExecutor;
use rca_exec::domain::{execute, ToolExecutor};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const LOGIN_ACTIONS: [&str; 3] = [AUTH_LOGIN_START, AUTH_LOGIN_WAIT, AUTH_LOGIN_CANCEL];

struct Bench {
    root: PathBuf,
    _dir: tempfile::TempDir,
    clis: HostedClis,
    ex: LocalExecutor,
}

fn bench() -> Bench {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let clis = HostedClis::install();
    let ex = LocalExecutor::new(&root).unwrap().with_hosted_path(clis.path().to_str().unwrap());
    Bench { root, _dir: dir, clis, ex }
}

fn call(ex: &LocalExecutor, id: &str, tool: &str, args: Value) -> ExecResult<Value> {
    execute(ex, id, tool, &args, &mut |_: Stream, _: &str| {}, &mut |_| {})
}

fn rule(program: &str, args: &[&str]) -> Rule {
    Rule { program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), ..Rule::default() }
}

/// `{root}` of a fixture is the executor's root, whose path differs per run.
fn with_root(value: &Value, root: &Path) -> Value {
    serde_json::from_str(&value.to_string().replace("{root}", &root.display().to_string().replace('\\', "\\\\"))).unwrap()
}

fn without_root(value: &Value, root: &Path) -> Value {
    serde_json::from_str(&value.to_string().replace(&root.display().to_string(), "{root}")).unwrap()
}

fn frame(id: &str, outcome: ExecResult<Value>) -> Value {
    match outcome {
        Ok(result) => json!({"id": id, "ok": true, "result": result}),
        Err(e) => json!({"id": id, "ok": false, "error": {"code": e.code.as_str(), "message": e.message}}),
    }
}

/// Runs one recorded exchange on a fresh root; returns the executor's response frame and the argv of every start the double logged.
fn replay(exchange: &Exchange) -> (Value, Vec<Value>, Vec<bool>) {
    let b = bench();
    for (path, content) in &exchange.files {
        let at = b.root.join(path);
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, content).unwrap();
    }
    let rules: Vec<Value> = exchange.cli.iter().map(|r| with_root(r, &b.root)).collect();
    b.clis.replay(&rules, &[]);
    let request = &exchange.request;
    let outcome = call(&b.ex, request["id"].as_str().unwrap(), request["tool"].as_str().unwrap(), request["args"].clone());
    let starts: Vec<Value> = b.clis.argvs().iter().map(|a| without_root(&json!(a), &b.root)).collect();
    let matched = ["gh", "glab"].iter().flat_map(|p| b.clis.starts(p)).map(|s| s.matched).collect();
    (frame(request["id"].as_str().unwrap(), outcome), starts, matched)
}

#[test]
fn given_every_recorded_exchange_when_replayed_through_the_executor_then_the_response_and_the_programs_it_started_are_the_recorded_ones() {
    let all: Vec<Fixture> = fixtures();
    assert!(!all.is_empty());
    for fixture in &all {
        for exchange in &fixture.exchanges {
            // Given an exchange: the files of the root, what the double prints, the request (an interaction of the exec-hosted cassette set)
            // When the executor runs it
            let (response, starts, matched) = replay(exchange);
            // Then every program it started was one a rule answers
            assert!(matched.iter().all(|m| *m), "{} / {}: a start matched no rule: {starts:?}", fixture.action, exchange.name);
            // And the response and the argv of every start are the recorded ones
            assert_eq!(response, exchange.response, "{} / {}", fixture.action, exchange.name);
            assert_eq!(starts, exchange.starts.iter().map(|a| json!(a)).collect::<Vec<_>>(), "{} / {}", fixture.action, exchange.name);
        }
    }
}

#[test]
fn given_the_actions_of_the_contract_when_the_fixtures_are_read_then_each_action_is_answered_for_both_providers_and_every_response_is_shaped() {
    let all = fixtures();
    // Publishing to GitLab ends with a real `git push`: its own test below.
    let github_only = [REPO_PUBLISH];
    for action in ACTIONS.iter().filter(|a| !LOGIN_ACTIONS.contains(&a.name)) {
        let fixture = all.iter().find(|f| f.action == action.name).unwrap_or_else(|| panic!("{} has no fixture", action.name));
        for exchange in &fixture.exchanges {
            assert_eq!(exchange.request["tool"], action.name, "{}", exchange.name);
        }
        for provider in ["github", "gitlab"].iter().filter(|p| !(github_only.contains(&action.name) && **p == "gitlab")) {
            let answered = fixture.exchanges.iter().any(|e| e.request["args"]["provider"] == *provider && e.response["ok"] == true);
            assert!(answered, "{} has no succeeding {provider} exchange", action.name);
        }
        // And a refusal is a code the contract names, with a message
        for exchange in fixture.exchanges.iter().filter(|e| e.response["ok"] == false) {
            let code = exchange.response["error"]["code"].as_str().unwrap();
            assert!(Code::parse(code).is_some() && !exchange.response["error"]["message"].as_str().unwrap().is_empty(), "{}: {code}", exchange.name);
        }
    }
}

/// The request of the first succeeding exchange of `action` for `provider`: arguments that the executor accepts.
fn baseline(action: &str, provider: &str) -> Value {
    fixtures().iter().find(|f| f.action == action).and_then(|f| f.exchanges.iter().find(|e| e.request["args"]["provider"] == provider && e.response["ok"] == true)).unwrap_or_else(|| panic!("no {provider} exchange of {action}")).request["args"].clone()
}

fn no_start(b: &Bench) {
    assert!(b.clis.argvs().is_empty(), "a CLI was started: {:?}", b.clis.argvs());
}

const FREE_TEXT: [&str; 4] = ["title", "body", "description", "search"];
const LABEL_KEYS: [&str; 3] = ["label", "labels", "add"];

#[test]
fn given_every_argument_of_every_action_when_it_carries_text_a_cli_or_shell_could_misread_then_the_request_is_refused_and_no_program_starts() {
    for action in ACTIONS.iter().filter(|a| !LOGIN_ACTIONS.contains(&a.name)) {
        for provider in ["github", "gitlab"] {
            if action.name == REPO_PUBLISH && provider == "gitlab" {
                continue;
            }
            for property in action.properties {
                // Given a request that is accepted, with one argument replaced by hostile text of the kind that argument holds
                let hostile: Vec<Value> = match property.kind {
                    "integer" => vec![json!(-1), json!("7; rm -rf /"), json!(1.5), json!(true)],
                    "boolean" => continue,
                    _ if property.name == "dest" || property.name == "dir" => vec![json!("-x"), json!("a\u{0}b")],
                    _ if FREE_TEXT.contains(&property.name) && property.name == "body" => vec![json!("a\u{0}b")],
                    _ if FREE_TEXT.contains(&property.name) => vec![json!("a\nb"), json!("a\u{0}b"), json!("")],
                    _ if LABEL_KEYS.contains(&property.name) || (property.name == "remove" && action.name == PR_LABELS) => vec![json!("-x"), json!("--web"), json!("a\nb"), json!("a,b")],
                    _ => ["-x", "--web", "--repo=evil/x", "a b", "a;b", "a$(id)", "a`id`", "a|b", "a&b", "a>b", "a\nb", "../x", ""].iter().map(|s| json!(s)).collect(),
                };
                for text in hostile {
                    let b = bench();
                    b.clis.rules(&[], &[]);
                    let mut args = baseline(action.name, provider);
                    args[property.name] = if property.kind == "array" { json!([text]) } else { text.clone() };
                    // When the executor is asked
                    let refused = call(&b.ex, "x", action.name, args).unwrap_err();
                    // Then it is refused as invalid arguments naming the argument, and nothing was started
                    assert_eq!(refused.code, Code::InvalidArgs, "{} {} {text:?}: {refused:?}", action.name, property.name);
                    assert!(refused.message.contains(&format!("`{}`", property.name)), "{} {} {text:?}: {refused:?}", action.name, property.name);
                    no_start(&b);
                }
            }
        }
    }
}

#[test]
fn given_free_text_that_starts_like_an_option_when_a_request_carries_it_then_it_reaches_the_cli_only_as_the_value_of_a_flag_or_on_stdin() {
    // Given every action's accepted request, with each free-text argument set to something a CLI would read as flags
    let sentinel = "--sentinel-flag value";
    for action in ACTIONS.iter().filter(|a| !LOGIN_ACTIONS.contains(&a.name)) {
        for provider in ["github", "gitlab"] {
            if action.name == REPO_PUBLISH && provider == "gitlab" {
                continue;
            }
            let b = bench();
            b.clis.rules(&[], &[]);
            let mut args = baseline(action.name, provider);
            for key in FREE_TEXT.iter().filter(|k| action.properties.iter().any(|p| p.name == **k)) {
                args[*key] = json!(sentinel);
            }
            // When the executor runs it (the double has no rule, so it answers every start with a failure we ignore)
            let _ = call(&b.ex, "x", action.name, args);
            // Then in every argv an option holds the text only after its `=` (`--title=`, `-f=body=`), and the text is never an argument of its own
            for argv in b.clis.argvs() {
                for arg in &argv[1..] {
                    if let Some(at) = arg.find("sentinel") {
                        let before = &arg[..at];
                        assert!(!arg.starts_with('-') || before.contains('='), "{} {provider}: `{arg}` in {argv:?}", action.name);
                    }
                }
            }
        }
    }
}

#[test]
fn given_a_verb_that_is_no_action_when_requested_then_it_is_unknown_and_no_program_starts() {
    let b = bench();
    b.clis.rules(&[], &[]);
    for tool in ["hosted_api", "hosted_pr_exec", "hosted_", "hosted_pr_list --web", "gh", "glab", "hosted_auth_token", "HOSTED_PR_LIST"] {
        let refused = call(&b.ex, "x", tool, json!({"provider": "github", "args": ["api", "/user"]})).unwrap_err();
        assert_eq!(refused.code, Code::UnknownTool, "{tool}");
    }
    no_start(&b);
}

#[test]
fn given_a_body_when_a_comment_is_made_then_it_travels_on_stdin_unchanged_even_at_the_size_limit_and_never_in_argv() {
    let b = bench();
    let body = format!("line one\n- not a flag\n{}", "x".repeat(exec_hosted::BODY_MAX - 40));
    b.clis.rules(&[Rule { stdin: Some(body.clone()), stdout: "https://github.com/octo/hello/pull/12#issuecomment-1\n".into(), ..rule("gh", &["pr", "comment", "12", "--repo=octo/hello", "--body-file=-"]) }], &[]);
    let out = call(&b.ex, "c", PR_COMMENT, json!({"provider": "github", "repo": "octo/hello", "number": 12, "body": body})).unwrap();
    assert_eq!(out["url"], "https://github.com/octo/hello/pull/12#issuecomment-1");
    let start = &b.clis.starts("gh")[0];
    assert!(start.argv.iter().all(|a| !a.contains("line one")) && start.stdin == body);
}

#[test]
fn given_a_cli_that_prints_more_than_the_cap_when_asked_then_a_diff_is_cut_and_says_so_and_a_json_answer_is_an_error() {
    let b = bench();
    let flood = rca_exec_flood(3);
    b.clis.rules(
        &[
            Rule { flood: Some(flood.clone()), ..rule("gh", &["pr", "diff", "1", "--repo=octo/hello", "--color=never"]) },
            Rule { flood: Some(flood), ..rule("gh", &["pr", "view", "1", "--repo=octo/hello", &format!("--json={}", pr_fields())]) },
        ],
        &[],
    );
    // When a diff of 3 MiB is asked for
    let started = Instant::now();
    let diff = call(&b.ex, "d", PR_DIFF, json!({"provider": "github", "repo": "octo/hello", "number": 1})).unwrap();
    // Then it is cut at the cap and says so, and the executor did not read it all
    assert_eq!(diff["truncated"], true);
    assert!(diff["diff"].as_str().unwrap().len() <= OUTPUT_CAP);
    // And a JSON answer cut at the cap is refused, not parsed
    let cut = call(&b.ex, "v", PR_VIEW, json!({"provider": "github", "repo": "octo/hello", "number": 1})).unwrap_err();
    assert!(cut.message.contains("more than the output cap"), "{cut:?}");
    assert!(started.elapsed() < Duration::from_secs(30));
}

fn rca_exec_flood(mib: u64) -> exec_testkit::hosted::Flood {
    exec_testkit::hosted::Flood { text: "+".repeat(1024) + "\n", times: mib * 1024 * 1024 / 1025 }
}

fn pr_fields() -> String {
    rca_exec::hosted::plan::PR_FIELDS.to_string()
}

#[test]
fn given_a_cli_that_never_ends_when_the_request_has_a_timeout_then_it_is_stopped_with_an_error_naming_the_time() {
    let b = bench();
    b.clis.rules(&[Rule { hang: true, stdout: "working\n".into(), ..rule("gh", &["pr", "list", "--repo=octo/hello", "--state=open", "--limit=30", &format!("--json={}", rca_exec::hosted::plan::PR_LIST_FIELDS)]) }], &[]);
    let started = Instant::now();
    let refused = call(&b.ex, "t", PR_LIST, json!({"provider": "github", "repo": "octo/hello", "timeout_ms": 400})).unwrap_err();
    assert_eq!(refused.code, Code::Io);
    assert!(refused.message.contains("did not finish within 400 ms"), "{refused:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn given_a_machine_without_the_cli_when_a_verb_runs_then_it_says_so_and_a_status_question_is_answered_not_failed() {
    // Given a PATH with no gh and no glab
    let empty = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let ex = LocalExecutor::new(dir.path()).unwrap().with_hosted_path(empty.path().to_str().unwrap());
    for (provider, program) in [("github", "gh"), ("gitlab", "glab")] {
        // When a verb is asked, Then it is NOT_FOUND naming the program
        let refused = call(&ex, "m", PR_LIST, json!({"provider": provider, "repo": "octo/hello"})).unwrap_err();
        assert_eq!(refused.code, Code::NotFound);
        assert!(refused.message.contains(&format!("{program} is not installed")), "{refused:?}");
        // And the status of auth is a result: not installed, not signed in
        let status = call(&ex, "s", "hosted_auth_status", json!({"provider": provider})).unwrap();
        assert_eq!((status["installed"].clone(), status["signed_in"].clone(), status["account"].clone()), (json!(false), json!(false), Value::Null));
    }
}

#[test]
fn given_an_executor_environment_with_tokens_when_a_cli_is_started_then_it_sees_its_own_config_directory_and_the_instance_and_no_token() {
    let b = bench();
    // Given the executor's own sign-in in a config directory, and tokens in its environment
    std::env::set_var("GH_CONFIG_DIR", "/executor/own/gh");
    std::env::set_var("GH_TOKEN", "must-not-pass");
    std::env::set_var("GITHUB_TOKEN", "must-not-pass");
    std::env::set_var("GITLAB_TOKEN", "must-not-pass");
    std::env::set_var("RCA_SERVICE_SECRET", "must-not-pass");
    b.clis.rules(&[Rule { stdout: "[]".into(), ..rule("gh", &["label", "list", "--repo=octo/hello", "--limit=30", "--json=name,color,description"]) }], &["GH_CONFIG_DIR", "GH_TOKEN", "GITHUB_TOKEN", "GITLAB_TOKEN", "RCA_SERVICE_SECRET", "GH_HOST", "GH_PROMPT_DISABLED"]);
    // When a verb is asked for a self-hosted instance
    call(&b.ex, "e", "hosted_repo_labels", json!({"provider": "github", "repo": "octo/hello", "host": "git.example.com"})).unwrap();
    // Then the CLI saw the config directory, the instance and no secret, in the workspace root
    let start = &b.clis.starts("gh")[0];
    assert_eq!(start.env["GH_CONFIG_DIR"].as_deref(), Some("/executor/own/gh"));
    assert_eq!(start.env["GH_HOST"].as_deref(), Some("git.example.com"));
    assert_eq!(start.env["GH_PROMPT_DISABLED"].as_deref(), Some("1"));
    for secret in ["GH_TOKEN", "GITHUB_TOKEN", "GITLAB_TOKEN", "RCA_SERVICE_SECRET"] {
        assert_eq!(start.env[secret], None, "{secret} reached the CLI");
    }
    assert_eq!(Path::new(&start.cwd), b.root);
}

#[test]
fn given_a_clone_or_a_publish_when_its_path_leaves_the_root_or_is_unusable_then_it_is_refused_before_any_program_starts() {
    let b = bench();
    b.clis.rules(&[], &[]);
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), b.root.join("link")).unwrap();
    std::fs::create_dir_all(b.root.join("taken")).unwrap();
    std::fs::write(b.root.join("taken/f"), "x").unwrap();
    std::fs::create_dir_all(b.root.join("plain")).unwrap();
    let repo = json!({"provider": "github", "repo": "octo/hello"});
    let with = |key: &str, value: &str| {
        let mut args = repo.clone();
        args[key] = json!(value);
        args
    };
    // When paths that leave the root are named, Then each is PATH_OUTSIDE_ROOT
    for path in ["../elsewhere", "/etc/hello", "link/inside", &outside.path().display().to_string(), "a/../../b"] {
        assert_eq!(call(&b.ex, "c", REPO_CLONE, with("dest", path)).unwrap_err().code, Code::PathOutsideRoot, "clone {path}");
        assert_eq!(call(&b.ex, "p", REPO_PUBLISH, with("dir", path)).unwrap_err().code, Code::PathOutsideRoot, "publish {path}");
    }
    // And a destination that holds files, and a directory that is no repository, are refused with their own codes
    assert_eq!(call(&b.ex, "c", REPO_CLONE, with("dest", "taken")).unwrap_err().code, Code::AlreadyExists);
    assert_eq!(call(&b.ex, "p", REPO_PUBLISH, with("dir", "plain")).unwrap_err().code, Code::NotFound);
    no_start(&b);
}

#[test]
fn given_a_worktree_base_when_a_clone_names_a_directory_under_it_then_it_is_cloned_there_and_the_path_is_reported_relative_to_the_root_or_absolute() {
    let b = bench();
    let base = tempfile::tempdir().unwrap();
    let base_path = std::fs::canonicalize(base.path()).unwrap();
    let ex = LocalExecutor::new(&b.root).unwrap().with_worktree_base(&base_path).unwrap().with_hosted_path(b.clis.path().to_str().unwrap());
    let dest = base_path.join("hello");
    b.clis.rules(&[rule("gh", &["repo", "clone", "octo/hello", &dest.display().to_string()])], &[]);
    let out = call(&ex, "c", REPO_CLONE, json!({"provider": "github", "repo": "octo/hello", "dest": dest.display().to_string()})).unwrap();
    assert_eq!(out["repo"], "octo/hello");
    assert_eq!(out["path"], dest.display().to_string());
}

#[test]
fn given_a_clone_in_progress_when_its_request_is_cancelled_then_the_cli_is_stopped_and_the_request_ends_cancelled() {
    let b = bench();
    let dest = b.root.join("big");
    b.clis.rules(&[Rule { hang: true, mkdir: Some(dest.display().to_string()), ..rule("gh", &["repo", "clone", "octo/big", &dest.display().to_string()]) }], &[]);
    let ex = Arc::new(b.ex);
    let running = std::thread::spawn({
        let ex = ex.clone();
        move || call(&ex, "clone-1", REPO_CLONE, json!({"provider": "github", "repo": "octo/big", "dest": "big"}))
    });
    // When the clone has started and its request is cancelled
    let deadline = Instant::now() + Duration::from_secs(10);
    while b.clis.starts("gh").is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(ex.cancel("clone-1"));
    // Then the request ends with the cancellation, and there is nothing left to cancel
    let ended = running.join().unwrap().unwrap_err();
    assert!(ended.message.contains("was cancelled"), "{ended:?}");
    assert!(!ex.cancel("clone-1"));
    // And the half-cloned directory the CLI made is gone, so a retry can clone there again
    assert!(!dest.exists(), "{} was left behind", dest.display());
}

#[test]
fn given_a_machine_that_is_not_signed_in_or_a_repository_that_does_not_exist_when_a_verb_runs_then_the_refusal_says_which() {
    let b = bench();
    let list = |provider: &str| json!({"provider": provider, "repo": if provider == "github" { "octo/hello" } else { "grp/proj" }});
    let json_arg = format!("--json={}", rca_exec::hosted::plan::PR_LIST_FIELDS);
    b.clis.rules(
        &[
            Rule { stderr: "To get started with GitHub CLI, please run:  gh auth login\n".into(), exit_code: 4, ..rule("gh", &["pr", "list", "--repo=octo/hello", "--state=open", "--limit=30", &json_arg]) },
            Rule { stderr: "{\"message\":\"404 Project Not Found\"}\n".into(), exit_code: 1, ..rule("glab", &["api", "projects/grp%2Fproj/merge_requests?state=opened&per_page=30&order_by=updated_at&sort=desc"]) },
        ],
        &[],
    );
    let signed_out = call(&b.ex, "a", PR_LIST, list("github")).unwrap_err();
    assert!(signed_out.message.starts_with("not signed in to github.com"), "{signed_out:?}");
    let missing = call(&b.ex, "b", PR_LIST, list("gitlab")).unwrap_err();
    assert_eq!(missing.code, Code::NotFound, "{missing:?}");
}

fn login_rules(program: &str, args: &[&str], stderr: &str, sleep_ms: u64, hang: bool, exit_code: i32) -> Rule {
    Rule { stderr: stderr.into(), sleep_ms, hang, exit_code, ..rule(program, args) }
}

const GH_LOGIN: [&str; 5] = ["auth", "login", "--web", "--hostname=github.com", "--git-protocol=https"];
const GH_PROMPT: &str = "! First copy your one-time code: ABCD-1234\nOpen this URL to continue in your web browser: https://github.com/login/device\n";

#[test]
fn given_a_github_sign_in_when_it_is_started_then_the_code_and_address_come_back_and_waiting_ends_signed_in_with_the_account() {
    let b = bench();
    let status = Rule { stdout: "github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  - Token: gho_************\n  - Token scopes: 'repo'\n".into(), ..rule("gh", &["auth", "status"]) };
    b.clis.rules(&[login_rules("gh", &GH_LOGIN, GH_PROMPT, 600, false, 0), status], &[]);
    // When the sign-in is started
    let started = call(&b.ex, "s", AUTH_LOGIN_START, json!({"provider": "github"})).unwrap();
    // Then the one-time code and the address are the ones the CLI printed, and no token is in the answer
    assert_eq!((started["user_code"].clone(), started["verification_uri"].clone(), started["host"].clone()), (json!("ABCD-1234"), json!("https://github.com/login/device"), json!("github.com")));
    assert!(!started.to_string().contains("gho_"));
    let flow = started["flow"].as_str().unwrap().to_string();
    // And a short wait while the person has not approved is pending, a long one ends signed in as the account
    let pending = call(&b.ex, "w1", AUTH_LOGIN_WAIT, json!({"provider": "github", "flow": flow, "timeout_ms": 50})).unwrap();
    assert_eq!(pending["state"], "pending");
    let done = call(&b.ex, "w2", AUTH_LOGIN_WAIT, json!({"provider": "github", "flow": flow, "timeout_ms": 10_000})).unwrap();
    assert_eq!((done["state"].clone(), done["account"].clone()), (json!("signed_in"), json!("octocat")));
    // And the flow is over: waiting again finds none
    assert_eq!(call(&b.ex, "w3", AUTH_LOGIN_WAIT, json!({"provider": "github", "flow": flow})).unwrap_err().code, Code::NotFound);
}

#[test]
fn given_a_sign_in_that_waits_when_it_is_cancelled_then_the_cli_is_gone_and_a_failed_one_reports_its_message() {
    let b = bench();
    b.clis.rules(&[login_rules("gh", &GH_LOGIN, GH_PROMPT, 0, true, 0)], &[]);
    let started = call(&b.ex, "s", AUTH_LOGIN_START, json!({"provider": "github"})).unwrap();
    let flow = started["flow"].as_str().unwrap();
    // When it is cancelled
    let cancelled = call(&b.ex, "c", AUTH_LOGIN_CANCEL, json!({"provider": "github", "flow": flow})).unwrap();
    // Then it is reported cancelled once, and a second cancel and a wait find nothing
    assert_eq!(cancelled["cancelled"], true);
    assert_eq!(call(&b.ex, "c2", AUTH_LOGIN_CANCEL, json!({"provider": "github", "flow": flow})).unwrap()["cancelled"], false);
    assert_eq!(call(&b.ex, "w", AUTH_LOGIN_WAIT, json!({"provider": "github", "flow": flow})).unwrap_err().code, Code::NotFound);
    // And a CLI that fails after the prompt ends the wait failed with what it said
    b.clis.rules(&[login_rules("gh", &GH_LOGIN, &format!("{GH_PROMPT}X The device code expired\n"), 100, false, 1)], &[]);
    let started = call(&b.ex, "s2", AUTH_LOGIN_START, json!({"provider": "github"})).unwrap();
    let failed = call(&b.ex, "w2", AUTH_LOGIN_WAIT, json!({"provider": "github", "flow": started["flow"], "timeout_ms": 10_000})).unwrap();
    assert_eq!(failed["state"], "failed");
    assert!(failed["message"].as_str().unwrap().contains("First copy your one-time code"), "{failed}");
}

#[test]
fn given_a_cli_that_shows_no_code_when_a_sign_in_starts_then_it_is_an_error_and_a_gitlab_sign_in_needs_only_the_address() {
    let b = bench();
    b.clis.rules(&[login_rules("gh", &GH_LOGIN, "X could not reach github.com\n", 0, false, 1), login_rules("glab", &["auth", "login", "--web", "--hostname=gitlab.com"], "Opening https://gitlab.com/oauth/authorize?client_id=1 in your browser.\n", 0, true, 0)], &[]);
    let refused = call(&b.ex, "s", AUTH_LOGIN_START, json!({"provider": "github"})).unwrap_err();
    assert!(refused.message.contains("failed before it showed a code") && refused.message.contains("could not reach"), "{refused:?}");
    let started = call(&b.ex, "g", AUTH_LOGIN_START, json!({"provider": "gitlab"})).unwrap();
    assert_eq!((started["user_code"].clone(), started["verification_uri"].clone()), (Value::Null, json!("https://gitlab.com/oauth/authorize?client_id=1")));
    call(&b.ex, "gc", AUTH_LOGIN_CANCEL, json!({"provider": "gitlab", "flow": started["flow"]})).unwrap();
}

#[test]
fn given_a_gitlab_publish_when_the_repository_is_created_then_the_branch_is_pushed_to_origin_with_git_and_the_url_is_returned() {
    // Given a repository with an origin that stands for the new GitLab project, and a glab that creates it
    let repo = TempRepo::new();
    repo.write("a.txt", "hello\n");
    repo.commit_all("first");
    let remote = tempfile::tempdir().unwrap();
    let git = |dir: &Path, args: &[&str]| {
        let out = std::process::Command::new("git").args(args).current_dir(dir).envs(repo.git_env()).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(remote.path(), &["init", "--bare", "--initial-branch=main"]);
    git(&repo.path(), &["remote", "add", "origin", &remote.path().display().to_string()]);
    let clis = HostedClis::install();
    clis.rules(&[Rule { stdout: "✓ Created repository grp/proj on GitLab: https://gitlab.com/grp/proj\n".into(), ..rule("glab", &["repo", "create", "grp/proj", "--private", "--description=Hello"]) }], &[]);
    let ex = LocalExecutor::new(&repo.path()).unwrap().with_hosted_path(clis.path().to_str().unwrap());
    // When it is published
    let out = call(&ex, "p", REPO_PUBLISH, json!({"provider": "gitlab", "repo": "grp/proj", "description": "Hello"})).unwrap();
    // Then glab ran in the repository, and git pushed the branch to origin
    assert_eq!(out["url"], "https://gitlab.com/grp/proj");
    assert_eq!(Path::new(&clis.starts("glab")[0].cwd), std::fs::canonicalize(repo.path()).unwrap());
    assert_eq!(git(remote.path(), &["rev-parse", "main"]), repo.head());
}
