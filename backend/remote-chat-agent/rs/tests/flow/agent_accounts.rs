//! `accounts` and `git.ai` end to end over the agent socket, with the service's real adapters: the providers' directories under the flow's home,
//! the login commands and the turns started as real processes (the scripted CLI of `exec-testkit`, found on the `PATH`, which writes its login to
//! the directory its environment names as a real CLI does), and the model a double that answers from the `git.ai` fixture. The flows show a
//! login through a fake device flow (and a paste flow) and then a turn run in that provider's directory, that a second provider cannot see the
//! first one's login in its environment or its directories, that no reply carries a token, and every recorded exchange of `agent-api-accounts`
//! and `git.ai` replayed as recorded.

use crate::agent_threads::{chain, create, said, Chain};
use crate::harness::Env;
use crate::mapper::Agent;
use agent_api_proto::testing::Fixture;
use exec_testkit::script::{Script, Turn};
use rca_service::infrastructure::model_api::HttpModelApi;
use rca_group_accounts::{Completion, ModelApi, ModelError};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::sync::Arc;

const URL_LINE: &str = "Open https://login.example.test/device to sign in.";
const CODE_LINE: &str = "Enter the code ABCD-1234 there.";
const PASTE_LINE: &str = "Open https://login.example.test/device and paste the code.";
/// The variables a scripted CLI keeps the value of: the provider variables of every provider, and the service's own.
const WATCHED: [&str; 9] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "KIMI_API_KEY", "CHAT_AGENT_API_SECRET", "GH_TOKEN"];

/// What the service's environment holds that no provider but its own may see, as a deployed service has it.
fn service_env(env: &Env) {
    for (name, value) in [("CLAUDE_CONFIG_DIR", "/root/.claude"), ("CODEX_HOME", "/root/.codex"), ("ANTHROPIC_API_KEY", "anthropic-key-value"), ("OPENAI_API_KEY", "openai-key-value"), ("KIMI_API_KEY", "kimi-key-value"), ("CHAT_AGENT_API_SECRET", "service-secret-value"), ("GH_TOKEN", "github-token-value")] {
        env.set(name, value);
    }
}

/// Codex's login: prints the address and the code, waits a moment as a person would, then keeps its token in `CODEX_HOME`.
fn codex_login(token: &str) -> Turn {
    Turn::default().then_line(URL_LINE).then_line(CODE_LINE).then_sleeping(500).then_writing_credential("CODEX_HOME", "auth.json", &format!("{{\"access_token\":\"{token}\"}}")).logging_env(&WATCHED).listing_env(&["CODEX_HOME"])
}

/// Claude's paste login: prints the address, reads the pasted code and keeps it as its token in `CLAUDE_CONFIG_DIR`.
fn claude_login() -> Turn {
    Turn::default().then_line(PASTE_LINE).then_reading_a_line().then_writing_credential("CLAUDE_CONFIG_DIR", ".credentials.json", "{\"access_token\":\"{paste}\"}").logging_env(&WATCHED).listing_env(&["CLAUDE_CONFIG_DIR"])
}

/// opencode's paste login: keeps the pasted code as its token below its XDG data home, where opencode keeps `auth.json`.
fn opencode_login() -> Turn {
    Turn::default().then_line(PASTE_LINE).then_reading_a_line().then_writing_credential("XDG_DATA_HOME", "opencode/auth.json", "{\"access_token\":\"{paste}\"}").logging_env(&WATCHED).listing_env(&["XDG_DATA_HOME", "XDG_CONFIG_HOME"])
}

/// A turn that says one thing and keeps what it could see: the provider variables and the files of the directories it was told of.
fn watching(turn: Turn, dirs: &[&str]) -> Turn {
    turn.logging_env(&WATCHED).listing_env(dirs)
}

/// Every frame the mapper saw, as text, so a test can look for a token in all of it.
#[derive(Default)]
struct Seen(Vec<String>);

impl Seen {
    async fn ask(&mut self, agent: &mut Agent, method: &str, params: Value) -> Value {
        let answered = agent.ask(method, params).await;
        self.0.push(answered.to_string());
        answered
    }

    fn mentions(&self, text: &str) -> bool {
        self.0.iter().any(|frame| frame.contains(text))
    }
}

fn auth(answered: &Value) -> Value {
    assert!(answered.get("result").is_some(), "{answered}");
    answered["result"]["auth"].clone()
}

async fn sign_in_codex(agent: &mut Agent, seen: &mut Seen) {
    let started = auth(&seen.ask(agent, "provider.auth.start", json!({"provider": "codex"})).await);
    let done = auth(&seen.ask(agent, "provider.auth.complete", json!({"auth": started["auth"]})).await);
    assert_eq!(done["status"], json!("succeeded"), "{done}");
}

async fn sign_in_claude(agent: &mut Agent, seen: &mut Seen, code: &str) {
    let started = auth(&seen.ask(agent, "provider.auth.start", json!({"provider": "claudeAgent", "method": "paste"})).await);
    seen.ask(agent, "provider.auth.respond", json!({"auth": started["auth"], "text": code})).await;
    let done = auth(&seen.ask(agent, "provider.auth.complete", json!({"auth": started["auth"]})).await);
    assert_eq!(done["status"], json!("succeeded"), "{done}");
}

fn credential_text(root: &std::path::Path, provider_dir: &str, file: &str) -> String {
    std::fs::read_to_string(root.join(provider_dir).join(file)).unwrap_or_else(|e| panic!("{provider_dir}/{file}: {e}"))
}

fn start(chain: &Chain, program: &str, index: usize) -> exec_testkit::Invocation {
    chain.cli.invocations(program).into_iter().nth(index).unwrap_or_else(|| panic!("{program} was started fewer than {} times", index + 1))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_fake_device_flow_when_a_provider_signs_in_and_a_turn_runs_then_the_cli_wrote_its_login_to_its_own_directory_and_the_turn_ran_in_it_with_no_reply_carrying_a_token() {
    // Given a service with every provider's credentials in its environment, and Claude's CLI scripted to log in by paste and then to say one thing
    let env = Env::process(&[]);
    service_env(&env);
    let chain = chain(&env).await;
    let root = env.providers_root();
    chain.cli.script("claude", &Script::of(vec![claude_login(), watching(Turn::replaying(&[said("hello")]), &["CLAUDE_CONFIG_DIR"])]));
    let (mut agent, mut seen) = (chain.agent().await, Seen::default());
    // When Claude signs in by pasting a code (the CLI keeps it as its token), and a turn then runs on it
    sign_in_claude(&mut agent, &mut seen, "pasted-secret-code-1").await;
    create(&mut agent, "t1").await;
    agent.ask("thread.subscribe", json!({"thread": "t1"})).await;
    seen.ask(&mut agent, "turn.start", json!({"thread": "t1", "workspace": "w1", "driver": "claudeAgent", "prompt": "hi"})).await;
    let events = agent.events_through("turn.finished").await;
    // Then the CLI itself wrote the login below the directory the service named for it, and the pasted code is in that file only
    assert_eq!(credential_text(&root, "claude", ".credentials.json"), "{\"access_token\":\"pasted-secret-code-1\"}");
    let (login, turn) = (start(&chain, "claude", 0), start(&chain, "claude", 1));
    assert_eq!(login.argv, ["auth", "login"]);
    assert_eq!(login.env["CLAUDE_CONFIG_DIR"], Some(root.join("claude").to_string_lossy().into_owned()));
    // And the turn ran with the same directory as its config, and that directory held the login when it started
    assert_eq!(turn.env["CLAUDE_CONFIG_DIR"], login.env["CLAUDE_CONFIG_DIR"]);
    assert_eq!(turn.listing["CLAUDE_CONFIG_DIR"], [".credentials.json"]);
    assert_eq!(turn.cwd, env.home.to_string_lossy());
    // And the provider reads signed in and selected, the turn finished, and no frame the mapper saw holds the pasted code or any key
    let listed = seen.ask(&mut agent, "provider.list", json!({})).await;
    assert_eq!(listed["result"]["selected"], json!("claudeAgent"));
    assert_eq!(events.last().unwrap().data["reason"], json!("completed"));
    for secret in ["pasted-secret-code-1", "anthropic-key-value", "openai-key-value", "kimi-key-value", "service-secret-value", "github-token-value"] {
        assert!(!seen.mentions(secret), "a reply holds {secret}");
        assert!(events.iter().all(|e| !e.to_value().to_string().contains(secret)), "an event holds {secret}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_two_providers_signed_in_when_each_runs_a_turn_then_neither_sees_the_others_login_variables_or_files_nor_the_services_secrets() {
    // Given every provider's credentials and the service's secrets in the service's environment, Claude signed in by a pasted code and opencode
    // by another (Codex runs no remote turn yet, so the second provider that can is opencode)
    let env = Env::process(&[]);
    service_env(&env);
    let chain = chain(&env).await;
    let root = env.providers_root();
    chain.cli.script("opencode", &Script::of(vec![opencode_login(), watching(Turn::replaying(&exec_testkit::recordings::named("opencode-text").lines), &["XDG_DATA_HOME", "XDG_CONFIG_HOME"])]));
    chain.cli.script("claude", &Script::of(vec![claude_login(), watching(Turn::replaying(&[said("hello")]), &["CLAUDE_CONFIG_DIR"])]));
    let (mut agent, mut seen) = (chain.agent().await, Seen::default());
    for (provider, code) in [("opencode", "opencode-paste-token-5"), ("claudeAgent", "claude-paste-token-7")] {
        let started = auth(&seen.ask(&mut agent, "provider.auth.start", json!({"provider": provider, "method": "paste"})).await);
        seen.ask(&mut agent, "provider.auth.respond", json!({"auth": started["auth"], "text": code})).await;
        let done = auth(&seen.ask(&mut agent, "provider.auth.complete", json!({"auth": started["auth"]})).await);
        assert_eq!(done["status"], json!("succeeded"), "{provider}: {done}");
    }
    // When a turn runs on each provider
    for (thread, driver) in [("to", "opencode"), ("tl", "claudeAgent")] {
        let made = agent.ask("thread.create", json!({"thread": thread, "title": "T", "workspace": "w1", "driver": driver})).await;
        assert!(made.get("result").is_some(), "{made}");
        agent.ask("thread.subscribe", json!({"thread": thread})).await;
        let started = agent.ask("turn.start", json!({"thread": thread, "workspace": "w1", "driver": driver, "prompt": "hi"})).await;
        assert!(started.get("result").is_some(), "{driver}: {started}");
        let events = agent.events_through("turn.finished").await;
        assert_eq!(events.last().unwrap().data["reason"], json!("completed"), "{driver}");
    }
    let (opencode, claude) = (start(&chain, "opencode", 1), start(&chain, "claude", 1));
    let (opencode_dir, claude_dir) = (root.join("opencode").to_string_lossy().into_owned(), root.join("claude").to_string_lossy().into_owned());
    // Then opencode's process was told only its own data and config directories, and listed only its own login file
    assert_eq!((opencode.env["XDG_DATA_HOME"].clone(), opencode.env["XDG_CONFIG_HOME"].clone()), (Some(format!("{opencode_dir}/data")), Some(format!("{opencode_dir}/config"))));
    assert_eq!((opencode.env["CLAUDE_CONFIG_DIR"].clone(), opencode.env["CODEX_HOME"].clone()), (None, None));
    assert_eq!((opencode.listing["XDG_DATA_HOME"].clone(), opencode.listing["XDG_CONFIG_HOME"].clone()), (vec!["opencode/auth.json".to_string()], Vec::new()));
    // And Claude's process only its own directory, with its own login file in it
    assert_eq!((claude.env["CLAUDE_CONFIG_DIR"].clone(), claude.env["CODEX_HOME"].clone()), (Some(claude_dir.clone()), None));
    assert_eq!((claude.env["XDG_DATA_HOME"].clone(), claude.env["XDG_CONFIG_HOME"].clone()), (None, None));
    assert_eq!(claude.listing["CLAUDE_CONFIG_DIR"], [".credentials.json"]);
    // And neither environment names the other's directory or holds the other provider's key
    let values = |run: &exec_testkit::Invocation| run.env.values().flatten().cloned().collect::<Vec<String>>();
    assert!(values(&opencode).iter().all(|v| !v.contains(&claude_dir) && v != "anthropic-key-value" && v != "openai-key-value"), "{:?}", opencode.env);
    assert!(values(&claude).iter().all(|v| !v.contains(&opencode_dir) && v != "openai-key-value" && v != "kimi-key-value"), "{:?}", claude.env);
    // And neither holds the service's secret or a GitHub token, each keeps its own provider's key (opencode the model API key it reads) and no other
    for run in [&opencode, &claude] {
        for name in ["CHAT_AGENT_API_SECRET", "GH_TOKEN"] {
            assert_eq!(run.env[name], None, "{} holds {name}", run.program);
        }
    }
    assert_eq!((opencode.env["KIMI_API_KEY"].clone(), opencode.env["ANTHROPIC_API_KEY"].clone()), (Some("kimi-key-value".into()), None));
    assert_eq!((claude.env["ANTHROPIC_API_KEY"].clone(), claude.env["KIMI_API_KEY"].clone()), (Some("anthropic-key-value".into()), None));
    // And the two logins are in files of their own, and neither pasted code is in any reply
    assert!(credential_text(&root, "opencode", "data/opencode/auth.json").contains("opencode-paste-token-5") && !credential_text(&root, "claude", ".credentials.json").contains("opencode-paste-token-5"));
    assert!(!seen.mentions("opencode-paste-token-5") && !seen.mentions("claude-paste-token-7"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_provider_signed_in_when_it_logs_out_then_its_login_file_is_gone_and_the_other_providers_stay() {
    // Given Codex and Claude signed in
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let root = env.providers_root();
    chain.cli.script("codex", &Script::of(vec![codex_login("codex-token-3")]));
    chain.cli.script("claude", &Script::of(vec![claude_login()]));
    let (mut agent, mut seen) = (chain.agent().await, Seen::default());
    sign_in_codex(&mut agent, &mut seen).await;
    sign_in_claude(&mut agent, &mut seen, "claude-token-4").await;
    // When Codex logs out
    let out = seen.ask(&mut agent, "provider.logout", json!({"provider": "codex"})).await;
    // Then its login file is gone from the volume, Claude's stays, and the list says so
    assert_eq!(out["result"]["provider"]["status"], json!("signed_out"));
    assert!(!root.join("codex/auth.json").exists() && root.join("claude/.credentials.json").exists());
    let listed = seen.ask(&mut agent, "provider.list", json!({})).await;
    let status = |name: &str| listed["result"]["providers"].as_array().unwrap().iter().find(|p| p["provider"] == name).unwrap()["status"].clone();
    assert_eq!((status("codex"), status("claudeAgent")), (json!("signed_out"), json!("signed_in")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_login_command_that_fails_when_it_ends_then_the_session_says_why_and_the_provider_stays_signed_out() {
    // Given Codex's login scripted to print its address and then to fail with an error, as a login whose code expired does
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.cli.script("codex", &Script::of(vec![Turn::default().then_line(URL_LINE).complaining("error: the device code expired").exiting(1)]));
    let (mut agent, mut seen) = (chain.agent().await, Seen::default());
    // When Codex's login starts and its command fails
    let started = auth(&seen.ask(&mut agent, "provider.auth.start", json!({"provider": "codex"})).await);
    let done = auth(&seen.ask(&mut agent, "provider.auth.complete", json!({"auth": started["auth"]})).await);
    // Then the session failed with the error output of the command, and Codex holds no login
    assert_eq!(done["status"], json!("failed"));
    assert!(done["error"].as_str().unwrap().contains("the device code expired"), "{done}");
    assert!(!env.providers_root().join("codex/auth.json").exists());
}

fn contract_methods() -> BTreeSet<&'static str> {
    agent_api_accounts::METHODS.iter().chain(agent_api_git::ai::METHODS).map(|m| m.name()).collect()
}

fn recorded() -> Vec<Fixture> {
    [agent_api_accounts::testing::fixtures(), agent_api_git::testing::ai_fixtures()].concat()
}

/// A service of its own for each fixture, with the scripted logins the recorded exchanges were made against: Codex by device code (the
/// token is kept after half a second), Claude by paste.
async fn replay_rig(env: &Env) -> Chain {
    let chain = chain(env).await;
    chain.cli.script("codex", &Script::of(vec![codex_login("recorded-codex-token")]));
    chain.cli.script("claude", &Script::of(vec![claude_login()]));
    chain
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn every_method_of_accounts_and_git_ai_has_recorded_exchanges_that_are_answered_as_recorded_and_call_the_declared_actions() {
    // Given the methods of the two contracts, the exchanges they record and the actions the registered groups declare
    let fixtures = recorded();
    let mut named: Vec<&str> = fixtures.iter().map(|f| f.method.as_str()).collect();
    named.sort_unstable();
    assert_eq!(named, contract_methods().into_iter().collect::<Vec<_>>());
    let declared = |method: &str| crate::harness::agent_api(&Env::detached()).groups.served().find(|s| s.method.name() == method).map(|s| s.actions.to_vec()).unwrap_or_default();
    for fixture in &fixtures {
        assert!(fixture.exchanges.iter().all(|e| e.request["method"] == fixture.method.as_str()), "{}", fixture.method);
        assert!(fixture.exchanges.iter().any(|e| e.succeeds()), "{} has no exchange that succeeds", fixture.method);
        for exchange in fixture.exchanges.iter().filter(|e| e.succeeds()) {
            assert_eq!(exchange.actions, declared(&fixture.method), "{}: {}", fixture.method, exchange.name);
        }
    }
    // When each fixture is replayed on a service of its own, a staged change in the repository for the git.ai exchanges
    for fixture in &fixtures {
        let env = Env::process(&[]);
        let chain = replay_rig(&env).await;
        chain.repo.write("README.md", "temp repo\nedited\n");
        chain.repo.git(&["add", "README.md"]);
        let mut agent = chain.agent().await;
        for exchange in &fixture.exchanges {
            for before in &exchange.before {
                let setup = agent.ask(before["method"].as_str().unwrap(), before["params"].clone()).await;
                assert!(setup.get("result").is_some(), "{}: the setup request failed: {setup}", exchange.name);
            }
            // Then the answer is the recorded one
            let answered = agent.ask(exchange.request["method"].as_str().unwrap(), exchange.request["params"].clone()).await;
            let mut response = exchange.response.clone();
            response["id"] = answered["id"].clone();
            assert_eq!(exchange.masked(answered), exchange.masked(response), "{}: {}", fixture.method, exchange.name);
        }
        // And a git.ai exchange committed nothing: the change is still only staged
        if fixture.method == "git.generateCommitMessage" {
            assert_eq!(chain.repo.git(&["status", "--porcelain"]), "M  README.md");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_a_staged_change_in_the_repository_when_a_message_is_generated_then_the_model_is_given_the_real_diff_and_nothing_is_committed() {
    // Given a repository with one staged edit, and the service's model double
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    chain.repo.write("README.md", "temp repo\nedited by the flow\n");
    chain.repo.git(&["add", "README.md"]);
    let head = chain.repo.head();
    let mut agent = chain.agent().await;
    // When a message is generated for the workspace, and again in plain style for the whole working tree after an unstaged edit
    let answered = agent.ask("git.generateCommitMessage", json!({"workspace": "w1"})).await;
    chain.repo.write("other.txt", "tracked later\n");
    chain.repo.git(&["add", "-N", "other.txt"]);
    let plain = agent.ask("git.generateCommitMessage", json!({"workspace": "w1", "staged": false, "style": "plain"})).await;
    // Then the answer is the model's message, the model was given the executor's diff of the staged edit, and the head did not move
    assert_eq!(answered["result"]["message"], json!("docs: describe the temp repo"));
    let asked: Vec<Completion> = chain.models().requests();
    assert_eq!(asked.len(), 2);
    assert!(asked[0].prompt.contains("+edited by the flow") && asked[0].prompt.contains("diff --git a/README.md"), "{}", asked[0].prompt);
    assert!(!asked[0].prompt.contains("other.txt"));
    // And the working-tree request carried both changes, in the plain style
    assert!(plain["result"].get("message").is_some() && asked[1].prompt.contains("other.txt") && !asked[1].prompt.contains("Conventional Commits"), "{plain}");
    assert_eq!(chain.repo.head(), head);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn given_nothing_staged_when_a_message_is_generated_then_it_is_refused_without_asking_the_model() {
    // Given a clean repository
    let env = Env::process(&[]);
    let chain = chain(&env).await;
    let mut agent = chain.agent().await;
    // When a message is asked for the staged change
    let answered = agent.ask("git.generateCommitMessage", json!({"workspace": "w1"})).await;
    // Then it is refused as nothing to commit, and the model was not asked
    assert_eq!(answered["error"]["code"], json!("NOTHING_TO_COMMIT"));
    assert!(chain.models().requests().is_empty());
}

fn http_model(base: &str, key: Option<&str>) -> (HttpModelApi, Arc<crate::doubles::FakeOutbound>) {
    let outbound = crate::doubles::FakeOutbound::new();
    (HttpModelApi::new(Arc::new(crate::doubles::OutboundPort(outbound.clone())), base, "kimi-test", key.map(String::from)), outbound)
}

#[tokio::test]
async fn given_a_model_key_when_a_completion_is_asked_then_the_key_goes_in_the_authorization_header_of_the_one_address_and_the_reply_text_comes_back() {
    // Given the model API over a fake HTTP service that answers a chat completion
    let (api, outbound) = http_model("https://models.example.test/v1/", Some("model-key-value"));
    outbound.answer(|_| Ok((200, json!({"model": "kimi-served", "choices": [{"message": {"role": "assistant", "content": "fix: the thing"}}]}))));
    // When a completion is asked
    let done = api.complete(Completion { system: "sys".into(), prompt: "the diff".into(), max_tokens: 50 }).await.unwrap();
    // Then the text and the serving model come back, and one request went to the chat address with the key as a bearer and not in the body
    assert_eq!((done.text.as_str(), done.model.as_str()), ("fix: the thing", "kimi-served"));
    let posts = outbound.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].url, "https://models.example.test/v1/chat/completions");
    assert_eq!(posts[0].headers, [("Authorization".to_string(), "Bearer model-key-value".to_string())]);
    let body = posts[0].json.clone().unwrap();
    assert!(!body.to_string().contains("model-key-value"));
    assert_eq!((body["model"].clone(), body["max_tokens"].clone(), body["messages"][1]["content"].clone()), (json!("kimi-test"), json!(50), json!("the diff")));
}

async fn ask_for_text(api: &HttpModelApi) -> Result<rca_group_accounts::Completed, ModelError> {
    api.complete(Completion { system: String::new(), prompt: "p".into(), max_tokens: 1 }).await
}

#[tokio::test]
async fn given_no_key_or_a_failing_endpoint_when_a_completion_is_asked_then_it_is_unavailable_with_why_and_usage_and_credit_are_not_offered() {
    // Given an API with no key, and one whose service answers 500, 200 with no message, and 200 with no JSON
    let (keyless, silent) = http_model("https://models.example.test/v1", None);
    let (api, outbound) = http_model("https://models.example.test/v1", Some("k"));
    // When a completion is asked of each
    let no_key = ask_for_text(&keyless).await.unwrap_err();
    outbound.answer(|_| Ok((500, json!({}))));
    let server = ask_for_text(&api).await.unwrap_err();
    outbound.answer(|_| Ok((200, json!({"choices": []}))));
    let empty = ask_for_text(&api).await.unwrap_err();
    outbound.answer(|_| Err("connection refused".into()));
    let down = ask_for_text(&api).await.unwrap_err();
    // Then each is unavailable with its reason, and the keyless API never made a request
    let reason = |e: ModelError| match e {
        ModelError::Unavailable(why) => why,
        other => panic!("{other:?}"),
    };
    assert!(reason(no_key).contains("KIMI_API_KEY") && silent.posts().is_empty());
    assert_eq!(reason(server), "the model API answered 500");
    assert_eq!(reason(empty), "the model API answered with no message");
    assert!(reason(down).contains("connection refused"));
    // And usage is not reported and a credit cannot be reset
    assert_eq!(api.usage("codex").await, Ok(None));
    assert!(matches!(api.reset_credit("codex").await, Err(ModelError::NotSupported(_))));
}
