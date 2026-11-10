//! `hosted.pr`, `hosted.clone` and `hosted.auth` end to end over the agent socket, against the real `rca-exec` binary whose `PATH` holds the
//! replay double of `gh` and `glab` (exec-testkit): the service holds no token and starts no process, so every answer is what the executor
//! made of the CLIs' output. Every recorded exchange of the executor contract is replayed through the method that reaches its action, and
//! the flows show the sign-in a phone drives, the repository access, a clone that runs and one that is cancelled, and what the executor refuses.

use crate::harness::{service, Env, Options, Service, SECRET};
use crate::mapper::{Agent, WAIT};
use agent_api_hosted::all_specs;
use exec_hosted::testing::fixtures;
use exec_testkit::{HostedClis, Rule, TempRepo};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

struct Rig {
    _svc: Service,
    _exec: exec_testkit::RcaExec,
    port: u16,
    token: String,
    repo: TempRepo,
    clis: HostedClis,
}

async fn rig(env: &Env) -> Rig {
    let repo = TempRepo::new();
    let clis = HostedClis::install();
    let svc = service(env, Options::default());
    let port = svc.listen().await;
    let url = format!("http://127.0.0.1:{port}");
    let path = clis.path();
    let exec = tokio::task::block_in_place(|| exec_testkit::RcaExec::serve_on(&url, "w1", &repo, SECRET, &[], &path));
    let token = svc.mint("hosted flow").await;
    Rig { _svc: svc, _exec: exec, port, token, repo, clis }
}

impl Rig {
    async fn agent(&self) -> Agent {
        Agent::open(self.port, &self.token).await
    }

    fn root(&self) -> std::path::PathBuf {
        std::fs::canonicalize(self.repo.path()).unwrap()
    }
}

fn rule(program: &str, args: &[&str]) -> Rule {
    Rule { program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), ..Rule::default() }
}

fn w(extra: Value) -> Value {
    let mut params = json!({"workspace": "w1"});
    params.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    params
}

async fn ok(agent: &mut Agent, method: &str, params: Value) -> Value {
    let answer = agent.ask(method, params).await;
    assert!(answer.get("result").is_some(), "{method} failed: {answer}");
    answer["result"].clone()
}

async fn wait_for(agent: &mut Agent, method: &str, params: Value, state: &str) -> Value {
    let deadline = Instant::now() + WAIT;
    loop {
        let answer = ok(agent, method, params.clone()).await;
        if answer["state"] == state || Instant::now() > deadline {
            return answer;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The executor's refusal as the service words it: a missing CLI and a missing sign-in have the service's codes.
fn expected_error(error: &Value) -> (String, String) {
    let message = error["message"].as_str().unwrap().to_string();
    let code = agent_api_hosted::code_of(&message).map_or_else(|| error["code"].as_str().unwrap().to_string(), str::to_string);
    (code, message)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_every_recorded_exchange_of_the_executor_contract_when_asked_through_its_method_then_the_service_answers_as_the_executor_did() {
    // Given the service, the executor and the double of gh and glab
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let mut agent = rig.agent().await;
    let mut replayed = 0;
    for fixture in fixtures().iter().filter(|f| ![exec_hosted::AUTH_STATUS, exec_hosted::REPO_CLONE, exec_hosted::REPO_PUBLISH, exec_hosted::AUTH_LOGIN_START, exec_hosted::AUTH_LOGIN_WAIT, exec_hosted::AUTH_LOGIN_CANCEL].contains(&f.action.as_str())) {
        let spec = all_specs().into_iter().find(|s| s.actions.first().copied() == Some(fixture.action.as_str())).unwrap_or_else(|| panic!("no method reaches {}", fixture.action));
        for exchange in &fixture.exchanges {
            // When the exchange's request is made through the method of the action, with the double printing what the CLIs printed
            let root = rig.root().display().to_string();
            let rules: Vec<Value> = exchange.cli.iter().map(|r| serde_json::from_str(&r.to_string().replace("{root}", &root)).unwrap()).collect();
            rig.clis.replay(&rules, &[]);
            for (path, content) in &exchange.files {
                rig.repo.write(path, content);
            }
            let answer = agent.ask(spec.method.name(), w(exchange.request["args"].clone())).await;
            // Then the answer is the executor's result, or its refusal with the service's code
            match exchange.response["ok"].as_bool() {
                Some(true) => assert_eq!(answer["result"], exchange.response["result"], "{}: {}", fixture.action, exchange.name),
                _ => {
                    let (code, message) = expected_error(&exchange.response["error"]);
                    assert_eq!((answer["error"]["code"].clone(), answer["error"]["message"].clone()), (json!(code), json!(message)), "{}: {}", fixture.action, exchange.name);
                }
            }
            replayed += 1;
        }
    }
    assert!(replayed > 40, "{replayed} exchanges replayed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_machine_signed_out_when_a_phone_signs_in_then_it_gets_the_code_and_address_and_waits_until_the_account_shows_and_no_frame_holds_a_token() {
    // Given gh signed out until its web login is approved: the login prints the code and the address, then ends; the status shows octocat
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let login = Rule { stderr: "! First copy your one-time code: ABCD-1234\nOpen this URL to continue in your web browser: https://github.com/login/device\n".into(), sleep_ms: 600, ..rule("gh", &["auth", "login", "--web", "--hostname=github.com", "--git-protocol=https"]) };
    let status = Rule { stdout: "github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  - Token: gho_SECRETTOKEN0000\n  - Token scopes: 'repo'\n".into(), ..rule("gh", &["auth", "status"]) };
    rig.clis.rules(&[login, status], &[]);
    let mut agent = rig.agent().await;
    // When the phone starts the sign-in, waits briefly, then waits for it to finish
    let started = ok(&mut agent, "sourceControl.signInStart", w(json!({"provider": "github"}))).await;
    let flow = started["flow"].as_str().unwrap().to_string();
    let pending = ok(&mut agent, "sourceControl.signInWait", w(json!({"provider": "github", "flow": flow, "timeout_ms": 50}))).await;
    let done = ok(&mut agent, "sourceControl.signInWait", w(json!({"provider": "github", "flow": flow, "timeout_ms": 10_000}))).await;
    let discovered = ok(&mut agent, "sourceControl.discover", w(json!({"provider": "github"}))).await;
    // Then the code and the address are the CLI's, the wait ended signed in as the account, and the token appears in no answer
    assert_eq!((started["user_code"].clone(), started["verification_uri"].clone()), (json!("ABCD-1234"), json!("https://github.com/login/device")));
    assert_eq!(pending["state"], "pending");
    assert_eq!((done["state"].clone(), done["account"].clone()), (json!("signed_in"), json!("octocat")));
    assert_eq!((discovered["providers"][0]["account"].clone(), discovered["providers"][0]["scopes"].clone()), (json!("octocat"), json!(["repo"])));
    for frame in [&started, &pending, &done, &discovered] {
        assert!(!frame.to_string().contains("SECRETTOKEN"), "{frame}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_sign_in_in_progress_when_the_phone_cancels_it_then_gh_is_stopped_and_the_flow_is_over() {
    // Given a login that stays open
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let login = Rule { stderr: "! First copy your one-time code: WXYZ-9999\nOpen this URL to continue in your web browser: https://github.com/login/device\n".into(), hang: true, ..rule("gh", &["auth", "login", "--web", "--hostname=github.com", "--git-protocol=https"]) };
    rig.clis.rules(&[login], &[]);
    let mut agent = rig.agent().await;
    let started = ok(&mut agent, "sourceControl.signInStart", w(json!({"provider": "github"}))).await;
    let flow = started["flow"].as_str().unwrap().to_string();
    // When it is cancelled and cancelled again
    let first = ok(&mut agent, "sourceControl.signInCancel", w(json!({"provider": "github", "flow": flow}))).await;
    let second = ok(&mut agent, "sourceControl.signInCancel", w(json!({"provider": "github", "flow": flow}))).await;
    // Then the first stopped it and the second found none
    assert_eq!((first["cancelled"].clone(), second["cancelled"].clone()), (json!(true), json!(false)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_clone_when_a_phone_starts_it_then_the_executor_clones_into_the_workspace_and_the_events_say_running_then_done_and_a_hung_one_is_cancelled() {
    // Given a phone subscribed to the workspace's clones and a gh that clones octo/hello at once and octo/big never
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    let root = rig.root();
    let hello = root.join("clones/hello").display().to_string();
    let big = root.join("big").display().to_string();
    rig.clis.rules(&[rule("gh", &["repo", "clone", "octo/hello", &hello]), Rule { hang: true, ..rule("gh", &["repo", "clone", "octo/big", &big]) }], &[]);
    let mut agent = rig.agent().await;
    ok(&mut agent, "clone.subscribe", w(json!({}))).await;
    // When a clone of octo/hello is started
    let started = ok(&mut agent, "clone.start", w(json!({"repo": "octo/hello", "dest": "clones/hello"}))).await;
    let id = started["clone"].as_str().unwrap().to_string();
    let done = wait_for(&mut agent, "clone.status", w(json!({"clone": id})), "done").await;
    // Then it is done at the path under the root, relative to it, and the events were running, then done
    assert_eq!((done["state"].clone(), done["detail"]["path"].clone()), (json!("done"), json!("clones/hello")));
    let first = tokio::time::timeout(WAIT, agent.event()).await.expect("running");
    let second = tokio::time::timeout(WAIT, agent.event()).await.expect("done");
    assert_eq!((first.seq, first.data["state"].clone(), second.seq, second.data["state"].clone()), (1, json!("running"), 2, json!("done")));
    // And a clone that hangs is cancelled: gh was started, and the clone is cancelled and stays so
    let hung = ok(&mut agent, "clone.start", w(json!({"repo": "octo/big", "dest": "big"}))).await["clone"].as_str().unwrap().to_string();
    let deadline = Instant::now() + WAIT;
    while rig.clis.starts("gh").len() < 2 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let cancelled = ok(&mut agent, "clone.cancel", w(json!({"clone": hung}))).await;
    assert_eq!((cancelled["state"].clone(), cancelled["cancelled"].clone()), (json!("cancelled"), json!(true)));
    let status = ok(&mut agent, "clone.status", w(json!({"clone": hung}))).await;
    assert_eq!(status["state"], "cancelled");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_arguments_that_would_reach_a_cli_as_options_or_leave_the_root_when_asked_then_the_executor_refuses_and_no_cli_starts() {
    // Given the double with no rule, so any start would be logged as unmatched
    let env = Env::new(&[]);
    let rig = rig(&env).await;
    rig.clis.rules(&[], &[]);
    let mut agent = rig.agent().await;
    // When a repository named like an option, a branch like one, a label like one, and a clone outside the root are asked for
    let asks = [
        ("pr.list", json!({"repo": "--web"})),
        ("pr.view", json!({"repo": "octo/hello", "number": "1; rm -rf /"})),
        ("pr.create", json!({"repo": "octo/hello", "title": "t", "head": "--head=x", "base": "main"})),
        ("pr.setLabels", json!({"repo": "octo/hello", "number": 1, "add": ["--remove-label=x"]})),
        ("clone.start", json!({"repo": "octo/hello", "dest": "../elsewhere"})),
    ];
    for (method, params) in asks {
        let answer = agent.ask(method, w(params)).await;
        if method == "clone.start" {
            let id = answer["result"]["clone"].as_str().unwrap().to_string();
            let failed = wait_for(&mut agent, "clone.status", w(json!({"clone": id})), "failed").await;
            assert_eq!(failed["state"], "failed", "{failed}");
        } else {
            // The executor's own refusal, so the request reached it and not the service's check of params
            assert_eq!(answer["error"]["code"], "INVALID_ARGS", "{method} was not refused by the executor: {answer}");
        }
    }
    // Then no program was started
    assert_eq!(rig.clis.argvs(), Vec::<Vec<String>>::new());
}
