//! The three groups over an executor registry and doubles of the executors' channels: what each method asks of the executor and how its
//! answers and refusals come back, the sign-in through the credentials port with no token in any frame, and a clone's life from start to
//! cancel, as numbered events.
use super::*;
use crate::testing::ScriptedCredentials;
use agent_api_hosted::{self as api, auth, clone, pr};
use agent_api_proto::{Event, Method};
use async_trait::async_trait;
use rca_agent_core::{AgentApi, Build, Connection, EventSink, Group};
use rca_exec_channel::remote::{ActionOutput, ExecutorChannel, RemoteError};
use rca_exec_channel::testing::ScriptedChannel;
use rca_exec_channel::{Executors, Registry};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

const BUILD: Build<'static> = Build { version: "1", commit: "c" };

fn ok(result: Value) -> Result<ActionOutput, RemoteError> {
    Ok(ActionOutput { result, output: String::new() })
}

fn refused(code: &str, message: &str) -> Result<ActionOutput, RemoteError> {
    Err(RemoteError::Action { code: code.into(), message: message.into() })
}

struct Rig {
    api: AgentApi,
    credentials: Arc<ScriptedCredentials>,
}

fn rig(channel: Arc<dyn ExecutorChannel>, account: Option<&str>) -> Rig {
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "root", channel).unwrap();
    let executors: Arc<dyn Executors> = executors;
    let credentials = Arc::new(ScriptedCredentials::new(account, "ABCD-1234"));
    let groups: Vec<Arc<dyn Group>> = vec![
        Arc::new(HostedPrGroup::new(executors.clone())),
        Arc::new(HostedCloneGroup::new(executors.clone())),
        Arc::new(HostedAuthGroup::new(executors, credentials.clone())),
    ];
    Rig { api: AgentApi::new(groups).unwrap(), credentials }
}

impl Rig {
    async fn ask(&self, method: Method, params: Value) -> Value {
        self.api.dispatch_text(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string()).await
    }
}

#[derive(Default)]
struct Pushed(Mutex<Vec<Event>>);

#[async_trait]
impl EventSink for Pushed {
    fn send(&self, event: &Event) -> bool {
        self.0.lock().unwrap().push(event.clone());
        true
    }

    async fn send_replay(&self, event: &Event) -> bool {
        self.send(event)
    }
}

impl Pushed {
    fn states(&self) -> Vec<(u64, String, String)> {
        self.0.lock().unwrap().iter().map(|e| (e.seq, e.data["clone"].as_str().unwrap().to_string(), e.data["state"].as_str().unwrap().to_string())).collect()
    }
}

/// An executor whose clone stays open until it is let go, and that notes when the clone request was dropped (the executor's cancel).
struct Gated {
    release: Notify,
    dropped: Arc<AtomicBool>,
    path: &'static str,
}

struct NoteDrop(Arc<AtomicBool>);

impl Drop for NoteDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait]
impl ExecutorChannel for Gated {
    async fn call(&self, tool: &str, _args: Value) -> Result<ActionOutput, RemoteError> {
        assert_eq!(tool, exec_hosted::REPO_CLONE);
        let _noted = NoteDrop(self.dropped.clone());
        self.release.notified().await;
        std::mem::forget(_noted);
        ok(json!({"repo": "octo/hello", "path": self.path}))
    }

    fn close(&self, _code: u16) {}

    fn is_closed(&self) -> bool {
        false
    }

    async fn closed(&self) {
        std::future::pending::<()>().await
    }
}

fn gated(path: &'static str) -> Arc<Gated> {
    Arc::new(Gated { release: Notify::new(), dropped: Arc::new(AtomicBool::new(false)), path })
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(50)).await;
}

#[test]
fn given_the_groups_when_compared_with_the_contract_then_each_serves_its_methods_in_order_naming_the_actions_of_their_specs() {
    // Given the three groups
    let r = rig(ScriptedChannel::new(|_, _| ok(json!({}))), None);
    let rows: Vec<(Method, Vec<&str>)> = r.api.served().map(|s| (s.method, s.actions.to_vec())).collect();
    // Then the rows are the contract's methods in order, with the actions of their specs
    assert_eq!(rows.iter().map(|r| r.0).collect::<Vec<_>>(), api::all_methods());
    for spec in api::all_specs() {
        assert_eq!(rows.iter().find(|r| r.0 == spec.method).unwrap().1, spec.actions, "{}", spec.method.name());
    }
    // And every group is complete, so service.status lists all three
    assert_eq!(r.api.listed().iter().map(|g| g.as_str()).collect::<Vec<_>>(), ["hosted.pr", "hosted.clone", "hosted.auth"]);
}

#[tokio::test]
async fn given_every_pr_and_mr_method_when_called_then_the_executor_is_asked_its_action_with_the_forwarded_params_and_the_providers_default_and_the_result_comes_back_as_it_came() {
    // Given an executor that answers each action with a marker naming it
    let channel = ScriptedChannel::new(|tool, _| ok(json!({"action": tool})));
    let r = rig(channel.clone(), None);
    for spec in pr::SPECS {
        // When the method is called with every property of its action and one the action does not list
        let mut params = json!({"workspace": "root", "token": "ghp_x", "argv": ["--web"]});
        for name in api::properties_of(spec.actions[0]) {
            if name != "provider" {
                params[name] = json!("v");
            }
        }
        let answer = r.ask(spec.method, params).await;
        // Then the result is the executor's, and it was given the properties and the default provider, and nothing else
        assert_eq!(answer, json!({"id": 1, "result": {"action": spec.actions[0]}}), "{}", spec.method.name());
        let (tool, args) = channel.calls().pop().unwrap();
        assert_eq!(tool, spec.actions[0]);
        assert_eq!(args["provider"], spec.default_provider.unwrap(), "{}", spec.method.name());
        assert!(args.get("token").is_none() && args.get("argv").is_none() && args.get("workspace").is_none(), "{args}");
    }
}

#[tokio::test]
async fn given_executor_refusals_when_a_method_is_called_then_a_missing_cli_and_a_missing_sign_in_have_their_codes_and_the_rest_keep_the_executors() {
    // Given an executor that refuses by the repository asked for
    let channel = ScriptedChannel::new(|_, args| match args["repo"].as_str() {
        Some("octo/nocli") => refused("NOT_FOUND", "gh is not installed on the executor machine (not found on PATH)"),
        Some("octo/signedout") => refused("IO_ERROR", "not signed in to github.com: gh says run gh auth login"),
        _ => refused("NOT_FOUND", "no such pull request"),
    });
    let r = rig(channel, None);
    let code = |repo: &'static str| {
        let r = &r;
        async move { r.ask(pr::VIEW, json!({"workspace": "root", "repo": repo, "number": 1})).await["error"]["code"].clone() }
    };
    // When pr.view is asked three ways, Then the codes are the service's two, and the executor's own
    assert_eq!(code("octo/nocli").await, json!("CLI_NOT_INSTALLED"));
    assert_eq!(code("octo/signedout").await, json!("PROVIDER_NOT_SIGNED_IN"));
    assert_eq!(code("octo/other").await, json!("NOT_FOUND"));
    // And a missing workspace is INVALID_PARAMS and a detached one NO_EXECUTOR
    assert_eq!(r.ask(pr::LIST, json!({})).await["error"]["code"], json!("INVALID_PARAMS"));
    assert_eq!(r.ask(pr::LIST, json!({"workspace": "w2"})).await["error"]["code"], json!("NO_EXECUTOR"));
}

#[tokio::test]
async fn given_a_sign_in_when_started_waited_and_cancelled_then_the_phone_gets_the_code_and_address_and_no_frame_holds_the_token() {
    // Given an executor whose answers also hold a token under several names
    let channel = ScriptedChannel::new(|tool, _| match tool {
        exec_hosted::AUTH_LOGIN_START => ok(json!({"flow": "f9", "provider": "github", "host": "github.com", "user_code": "WXYZ-0001", "verification_uri": "https://github.com/login/device", "token": "ghp_SECRET"})),
        exec_hosted::AUTH_LOGIN_WAIT => ok(json!({"flow": "f9", "state": "signed_in", "account": "octocat", "token": "ghp_SECRET", "oauth_token": "ghp_SECRET"})),
        exec_hosted::AUTH_STATUS => ok(json!({"provider": "github", "installed": true, "signed_in": true, "host": "github.com", "account": "octocat", "scopes": ["repo"], "token": "ghp_SECRET"})),
        _ => ok(json!({"flow": "f9", "cancelled": true})),
    });
    let executors: Arc<Registry> = Arc::default();
    executors.open_executor("e1", "root", channel.clone()).unwrap();
    let group: Arc<dyn Group> = Arc::new(HostedAuthGroup::new(executors.clone(), Arc::new(ExecutorCredentials::new(executors))));
    let api = AgentApi::new(vec![group]).unwrap();
    let ask = |method: Method, params: Value| {
        let api = &api;
        async move { api.dispatch_text(BUILD, &json!({"id": 1, "method": method.name(), "params": params}).to_string()).await }
    };
    // When the phone starts, waits for, cancels a sign-in and asks the status
    let start = ask(auth::SIGN_IN_START, json!({"workspace": "root", "provider": "github"})).await;
    let wait = ask(auth::SIGN_IN_WAIT, json!({"workspace": "root", "provider": "github", "flow": "f9", "timeout_ms": 5000})).await;
    let cancel = ask(auth::SIGN_IN_CANCEL, json!({"workspace": "root", "provider": "github", "flow": "f9"})).await;
    let discover = ask(auth::DISCOVER, json!({"workspace": "root", "provider": "github"})).await;
    // Then the code and address came, the wait says who signed in, and the executor was asked the closed actions with the flow and timeout
    assert_eq!(start["result"], json!({"flow": "f9", "provider": "github", "host": "github.com", "user_code": "WXYZ-0001", "verification_uri": "https://github.com/login/device"}));
    assert_eq!(wait["result"], json!({"flow": "f9", "state": "signed_in", "account": "octocat"}));
    assert_eq!(cancel["result"], json!({"flow": "f9", "cancelled": true}));
    assert_eq!(discover["result"]["providers"][0]["account"], "octocat");
    let calls = channel.calls();
    assert_eq!(calls.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), [exec_hosted::AUTH_LOGIN_START, exec_hosted::AUTH_LOGIN_WAIT, exec_hosted::AUTH_LOGIN_CANCEL, exec_hosted::AUTH_STATUS]);
    assert_eq!(calls[1].1, json!({"provider": "github", "flow": "f9", "timeout_ms": 5000}));
    // And no frame the phone saw holds the token
    for frame in [&start, &wait, &cancel, &discover] {
        assert!(!frame.to_string().contains("ghp_SECRET"), "{frame}");
    }
    // And a flow, a provider the CLIs do not serve, and a detached workspace are refused before the executor is asked
    assert_eq!(ask(auth::SIGN_IN_WAIT, json!({"workspace": "root", "provider": "github"})).await["error"]["message"], "flow is required");
    assert_eq!(ask(auth::SIGN_IN_START, json!({"workspace": "root", "provider": "bitbucket"})).await["error"]["code"], "INVALID_PARAMS");
    assert_eq!(ask(auth::SIGN_IN_START, json!({"workspace": "w2", "provider": "github"})).await["error"]["code"], "NO_EXECUTOR");
    assert_eq!(channel.calls().len(), 4);
}

#[tokio::test]
async fn given_no_provider_when_discover_is_asked_then_both_providers_status_is_asked_through_the_port() {
    // Given a machine signed in as octocat
    let r = rig(ScriptedChannel::new(|_, _| ok(json!({}))), Some("octocat"));
    // When discover is asked without a provider
    let answer = r.ask(auth::DISCOVER, json!({"workspace": "root"})).await;
    // Then both providers answer, in order, and the port was asked for each
    let providers = answer["result"]["providers"].as_array().unwrap();
    assert_eq!(providers.iter().map(|p| p["provider"].as_str().unwrap()).collect::<Vec<_>>(), ["github", "gitlab"]);
    assert_eq!(r.credentials.asked(), ["status:github", "status:gitlab"]);
}

#[tokio::test]
async fn given_a_machine_signed_out_or_a_repository_it_cannot_see_when_app_access_is_asked_then_the_permission_is_none_and_signed_in_it_is_the_repositorys() {
    // Given an executor that shows octo/hello with write permission and no other repository
    let channel = ScriptedChannel::new(|_, args| match args["repo"].as_str() {
        Some("octo/hello") => ok(json!({"repo": {"slug": "octo/hello", "permission": "write"}})),
        _ => refused("NOT_FOUND", "could not resolve"),
    });
    let ask = |account: Option<&'static str>, repo: &'static str| {
        let (r, channel) = (rig(channel.clone(), account), channel.clone());
        async move {
            let before = channel.calls().len();
            let answer = r.ask(auth::APP_ACCESS, json!({"workspace": "root", "provider": "github", "repo": repo})).await;
            (answer, channel.calls().len() - before)
        }
    };
    // When it is asked signed out, signed in for a visible repository and for one that is not
    let (out, out_calls) = ask(None, "octo/hello").await;
    let (seen, _) = ask(Some("octocat"), "octo/hello").await;
    let (hidden, _) = ask(Some("octocat"), "octo/secret").await;
    // Then signed out asked the executor nothing, a visible one has its permission, and a hidden one none
    assert_eq!((out["result"]["permission"].clone(), out["result"]["status"]["signed_in"].clone(), out_calls), (Value::Null, json!(false), 0));
    assert_eq!((seen["result"]["permission"].clone(), seen["result"]["repo"]["slug"].clone()), (json!("write"), json!("octo/hello")));
    assert_eq!((hidden["result"]["permission"].clone(), hidden["result"]["status"]["account"].clone()), (Value::Null, json!("octocat")));
}

#[tokio::test]
async fn given_lookup_and_publish_when_called_then_the_executor_does_them_with_the_params_of_their_actions() {
    // Given an executor that answers each action with its args
    let channel = ScriptedChannel::new(|_, args| ok(args.clone()));
    let r = rig(channel.clone(), None);
    // When a repository is looked up and one published
    let lookup = r.ask(auth::LOOKUP_REPOSITORY, json!({"workspace": "root", "provider": "gitlab", "repo": "grp/proj", "host": "git.example.com"})).await;
    let publish = r.ask(auth::PUBLISH_REPOSITORY, json!({"workspace": "root", "provider": "github", "repo": "octo/new", "dir": "app", "visibility": "public", "description": "d", "token": "x"})).await;
    // Then each is its action, with the properties it lists
    assert_eq!(lookup["result"], json!({"provider": "gitlab", "repo": "grp/proj", "host": "git.example.com"}));
    assert_eq!(publish["result"], json!({"provider": "github", "repo": "octo/new", "dir": "app", "visibility": "public", "description": "d"}));
    assert_eq!(channel.calls().iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), [exec_hosted::REPO_VIEW, exec_hosted::REPO_PUBLISH]);
}

#[tokio::test]
async fn given_a_clone_when_started_then_it_runs_until_the_executor_ends_it_and_the_status_and_the_events_say_so() {
    // Given an executor whose clone stays open and a peer that subscribed before it started
    let channel = gated("clones/hello");
    let r = rig(channel.clone(), None);
    let sink = Arc::new(Pushed::default());
    let connection = Connection::new(sink.clone());
    let subscribed = r.api.dispatch_on(BUILD, &json!({"id": 1, "method": "clone.subscribe", "params": {"workspace": "root"}}).to_string(), &connection).await;
    for job in connection.take_following() {
        tokio::spawn(job.job);
    }
    // When a clone is started, then let go
    let started = r.ask(clone::START, json!({"workspace": "root", "repo": "octo/hello", "dest": "clones/hello"})).await;
    let id = started["result"]["clone"].as_str().unwrap().to_string();
    let running = r.ask(clone::STATUS, json!({"workspace": "root", "clone": id})).await;
    channel.release.notify_one();
    settle().await;
    let done = r.ask(clone::STATUS, json!({"workspace": "root", "clone": id})).await;
    // Then the start answered at once with the id, the status was running and then done with the path, and the peer saw both as numbered events
    assert_eq!(subscribed["result"], json!({"workspace": "root", "seq": 0}));
    assert_eq!((started["result"]["state"].clone(), running["result"]["state"].clone()), (json!("running"), json!("running")));
    assert_eq!((done["result"]["state"].clone(), done["result"]["detail"]["path"].clone()), (json!("done"), json!("clones/hello")));
    assert_eq!(sink.states(), [(1, id.clone(), "running".to_string()), (2, id.clone(), "done".to_string())]);
    // And a later subscriber after seq 1 replays only the end
    let late = Arc::new(Pushed::default());
    let connection = Connection::new(late.clone());
    r.api.dispatch_on(BUILD, &json!({"id": 2, "method": "clone.subscribe", "params": {"workspace": "root", "after_seq": 1}}).to_string(), &connection).await;
    for job in connection.take_following() {
        tokio::spawn(job.job);
    }
    settle().await;
    assert_eq!(late.states(), [(2, id, "done".to_string())]);
}

#[tokio::test]
async fn given_a_running_clone_when_cancelled_then_the_executors_request_is_dropped_and_the_clone_stays_cancelled() {
    // Given a clone that is running
    let channel = gated("x");
    let r = rig(channel.clone(), None);
    let id = r.ask(clone::START, json!({"workspace": "root", "repo": "octo/hello", "dest": "d", "provider": "github"})).await["result"]["clone"].as_str().unwrap().to_string();
    settle().await;
    // When it is cancelled, and again, and an unknown id is asked about
    let cancelled = r.ask(clone::CANCEL, json!({"workspace": "root", "clone": id})).await;
    settle().await;
    let again = r.ask(clone::CANCEL, json!({"workspace": "root", "clone": id})).await;
    let unknown = r.ask(clone::STATUS, json!({"workspace": "root", "clone": "nope"})).await;
    // Then the request to the executor was dropped (the executor's cancel), the state is cancelled, a second cancel says nothing was running, and the unknown id has its code
    assert!(channel.dropped.load(Ordering::SeqCst));
    assert_eq!((cancelled["result"]["state"].clone(), cancelled["result"]["cancelled"].clone()), (json!("cancelled"), json!(true)));
    assert_eq!((again["result"]["state"].clone(), again["result"]["cancelled"].clone()), (json!("cancelled"), json!(false)));
    assert_eq!(unknown["error"]["code"], json!("CLONE_NOT_FOUND"));
}

#[tokio::test]
async fn given_an_executor_that_fails_the_clone_or_a_start_without_its_params_when_asked_then_the_clone_is_failed_with_the_message_or_refused_at_once() {
    // Given an executor that refuses the clone
    let channel = ScriptedChannel::new(|_, _| refused("ALREADY_EXISTS", "taken exists and is not an empty directory"));
    let r = rig(channel.clone(), None);
    // When a clone starts, and starts lacking repo and dest
    let id = r.ask(clone::START, json!({"workspace": "root", "repo": "octo/hello", "dest": "taken"})).await["result"]["clone"].as_str().unwrap().to_string();
    settle().await;
    let status = r.ask(clone::STATUS, json!({"workspace": "root", "clone": id})).await;
    let no_repo = r.ask(clone::START, json!({"workspace": "root", "dest": "d"})).await;
    let no_dest = r.ask(clone::START, json!({"workspace": "root", "repo": "octo/hello"})).await;
    // Then the failure carries the executor's message, the GitHub default was sent, and the two bad starts asked nothing
    assert_eq!((status["result"]["state"].clone(), status["result"]["detail"]["message"].clone()), (json!("failed"), json!("taken exists and is not an empty directory")));
    assert_eq!(channel.calls()[0].1["provider"], "github");
    assert_eq!((no_repo["error"]["message"].clone(), no_dest["error"]["message"].clone()), (json!("repo is required"), json!("dest is required")));
    assert_eq!(channel.calls().len(), 1);
}
