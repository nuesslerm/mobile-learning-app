//! The doubles of the crate's ports: the providers' directories in memory, a login that plays a script, and a model that answers from a fixture.
use crate::ports::{AccountError, Completed, Completion, LoginRun, ModelApi, ModelError, ProviderAccounts, ProviderDirs, ProviderLogin};
use agent_api_accounts::{Limits, ResetOutcome, Updated};
use async_trait::async_trait;
use rca_cli_driver::domain::{Driver, LoginMethod};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::{mpsc, oneshot, Notify};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// The providers' directories as sets in memory: which provider holds a login, and which is selected.
#[derive(Default)]
pub struct MemoryDirs {
    signed: Mutex<HashSet<&'static str>>,
    selected: Mutex<Option<Driver>>,
    ensured: Mutex<Vec<&'static str>>,
}

impl MemoryDirs {
    pub fn new() -> Arc<MemoryDirs> {
        Arc::default()
    }

    /// Puts a login in the provider's directory, as its CLI does.
    pub fn sign_in(&self, driver: Driver) {
        lock(&self.signed).insert(driver.id());
    }

    /// The providers whose directories were made, in order.
    pub fn ensured(&self) -> Vec<&'static str> {
        lock(&self.ensured).clone()
    }
}

impl ProviderDirs for MemoryDirs {
    fn ensure(&self, driver: Driver) -> Result<(), String> {
        lock(&self.ensured).push(driver.id());
        Ok(())
    }

    fn signed_in(&self, driver: Driver) -> bool {
        lock(&self.signed).contains(driver.id())
    }

    fn clear(&self, driver: Driver) -> Result<(), String> {
        lock(&self.signed).remove(driver.id());
        Ok(())
    }

    fn selected(&self) -> Option<Driver> {
        *lock(&self.selected)
    }

    fn select(&self, driver: Driver) -> Result<(), String> {
        *lock(&self.selected) = Some(driver);
        Ok(())
    }

    fn deselect(&self) -> Result<(), String> {
        *lock(&self.selected) = None;
        Ok(())
    }
}

/// How a scripted login ends.
#[derive(Clone, Debug, PartialEq)]
pub enum Ending {
    /// The command keeps a login in the provider's directory and exits cleanly.
    SignsIn,
    /// The command exits cleanly and keeps nothing.
    KeepsNothing,
    /// The command exits with a failure and this error output.
    Fails(String),
    /// The command keeps a login once a line is sent to its input.
    SignsInOnInput,
    /// The command stays up until it is killed.
    Waits,
}

#[derive(Clone, Debug)]
pub struct LoginScript {
    pub lines: Vec<String>,
    pub ending: Ending,
}

impl LoginScript {
    pub fn new(lines: &[&str], ending: Ending) -> LoginScript {
        LoginScript { lines: lines.iter().map(|l| l.to_string()).collect(), ending }
    }
}

/// The logins as scripted commands, keyed by provider: each prints its lines and ends as scripted, keeping a login in `dirs` when it signs in.
pub struct ScriptedLogin {
    dirs: Arc<MemoryDirs>,
    scripts: Mutex<HashMap<&'static str, LoginScript>>,
    started: Mutex<Vec<(Driver, LoginMethod)>>,
    inputs: Arc<Mutex<Vec<String>>>,
    kills: Arc<AtomicUsize>,
    unavailable: Mutex<Option<String>>,
}

impl ScriptedLogin {
    pub fn new(dirs: Arc<MemoryDirs>) -> Arc<ScriptedLogin> {
        Arc::new(ScriptedLogin { dirs, scripts: Mutex::default(), started: Mutex::default(), inputs: Arc::default(), kills: Arc::default(), unavailable: Mutex::default() })
    }

    pub fn script(&self, driver: Driver, script: LoginScript) {
        lock(&self.scripts).insert(driver.id(), script);
    }

    /// Makes every later start fail with `why`, as a CLI that is not installed does.
    pub fn cannot_start(&self, why: &str) {
        *lock(&self.unavailable) = Some(why.to_string());
    }

    pub fn started(&self) -> Vec<(Driver, LoginMethod)> {
        lock(&self.started).clone()
    }

    /// The lines sent to the logins' input, in order.
    pub fn inputs(&self) -> Vec<String> {
        lock(&self.inputs).clone()
    }

    pub fn kills(&self) -> usize {
        self.kills.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl ProviderLogin for ScriptedLogin {
    async fn start(&self, driver: Driver, method: LoginMethod) -> Result<LoginRun, String> {
        if let Some(why) = lock(&self.unavailable).clone() {
            return Err(why);
        }
        lock(&self.started).push((driver, method));
        let script = lock(&self.scripts).get(driver.id()).cloned().unwrap_or_else(|| LoginScript::new(&[], Ending::SignsIn));
        let (lines_tx, mut lines_rx) = mpsc::channel::<String>(64);
        let (done_tx, done_rx) = oneshot::channel::<Result<(), String>>();
        let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();
        let stop = Arc::new(Notify::new());
        let (waiting, dirs) = (stop.clone(), self.dirs.clone());
        tokio::spawn(async move {
            for line in script.lines {
                if lines_tx.send(line).await.is_err() {
                    break;
                }
            }
            let ended = match script.ending {
                Ending::SignsIn => {
                    dirs.sign_in(driver);
                    Ok(())
                }
                Ending::KeepsNothing => Ok(()),
                Ending::Fails(stderr) => Err(stderr),
                Ending::SignsInOnInput => {
                    tokio::select! {
                        got = input_rx.recv() => { if got.is_some() { dirs.sign_in(driver); } Ok(()) }
                        _ = waiting.notified() => Ok(()),
                    }
                }
                Ending::Waits => {
                    waiting.notified().await;
                    Ok(())
                }
            };
            drop(lines_tx);
            let _ = done_tx.send(ended);
        });
        let (inputs, kills) = (self.inputs.clone(), self.kills.clone());
        let lines = futures_util::stream::poll_fn(move |cx| lines_rx.poll_recv(cx));
        Ok(LoginRun {
            lines: Box::pin(lines),
            finish: Box::pin(async move { done_rx.await.unwrap_or(Ok(())) }),
            input: Arc::new(move |line| {
                lock(&inputs).push(line.clone());
                input_tx.send(line).is_ok()
            }),
            kill: Arc::new(move || {
                kills.fetch_add(1, Ordering::SeqCst);
                stop.notify_one();
            }),
        })
    }
}

/// The model as a double: it answers each completion with a text and notes the request.
pub struct ScriptedModelApi {
    reply: Mutex<Result<Completed, ModelError>>,
    pull_request: Mutex<Option<Completed>>,
    requests: Mutex<Vec<Completion>>,
}

impl ScriptedModelApi {
    pub fn replying(text: &str, model: &str) -> Arc<ScriptedModelApi> {
        Arc::new(ScriptedModelApi { reply: Mutex::new(Ok(Completed { text: text.into(), model: model.into() })), pull_request: Mutex::new(None), requests: Mutex::default() })
    }

    /// The model that answers with the message the `git.ai` fixture records (`agent-api-git`) and gives the credit the accounts fixture records,
    /// so a replay of the fixtures is a replay of what the contracts say.
    pub fn from_fixture() -> Arc<ScriptedModelApi> {
        let fixtures = agent_api_git::testing::ai_fixtures();
        let success = |method: &str| fixtures.iter().filter(|f| f.method == method).flat_map(|f| &f.exchanges).find(|e| e.succeeds()).unwrap_or_else(|| panic!("the git.ai fixture of {method} records a success"));
        let recorded = success("git.generateCommitMessage");
        let result = &recorded.response["result"];
        let api = ScriptedModelApi::replying(result["message"].as_str().expect("a recorded message"), result["model"].as_str().expect("a recorded model"));
        // And the pull request fields the git.ai fixture records, as the JSON the model is asked for
        let fields = &success("git.generatePullRequestFields").response["result"];
        let text = serde_json::json!({"title": fields["title"], "body": fields["body"], "draft": fields["draft"]}).to_string();
        *lock(&api.pull_request) = Some(Completed { text, model: fields["model"].as_str().expect("a recorded model").into() });
        api
    }

    /// Makes the model answer a pull request request (the one whose system text is `PULL_REQUEST_SYSTEM`) with `text`.
    pub fn replying_to_pull_requests(&self, text: &str, model: &str) {
        *lock(&self.pull_request) = Some(Completed { text: text.into(), model: model.into() });
    }

    pub fn fails(&self, error: ModelError) {
        *lock(&self.reply) = Err(error);
    }

    pub fn requests(&self) -> Vec<Completion> {
        lock(&self.requests).clone()
    }
}

#[async_trait]
impl ModelApi for ScriptedModelApi {
    async fn complete(&self, request: Completion) -> Result<Completed, ModelError> {
        let for_pull_request = request.system == crate::PULL_REQUEST_SYSTEM;
        lock(&self.requests).push(request);
        match (for_pull_request, lock(&self.reply).clone(), lock(&self.pull_request).clone()) {
            (true, Ok(_), Some(answer)) => Ok(answer),
            (_, reply, _) => reply,
        }
    }
}

/// The providers' own accounts as a double: the limits, the spend outcome and the update each provider was given, and the keys the spends used.
#[derive(Default)]
pub struct ScriptedAccounts {
    limits: Mutex<HashMap<&'static str, Result<Option<Limits>, AccountError>>>,
    spends: Mutex<HashMap<&'static str, Result<ResetOutcome, AccountError>>>,
    updates: Mutex<HashMap<&'static str, Result<Updated, AccountError>>>,
    keys: Mutex<Vec<String>>,
}

impl ScriptedAccounts {
    pub fn new() -> Arc<ScriptedAccounts> {
        Arc::default()
    }

    pub fn with_limits(&self, driver: Driver, answer: Result<Option<Limits>, AccountError>) {
        lock(&self.limits).insert(driver.id(), answer);
    }

    pub fn with_spend(&self, driver: Driver, answer: Result<ResetOutcome, AccountError>) {
        lock(&self.spends).insert(driver.id(), answer);
    }

    pub fn with_update(&self, driver: Driver, answer: Result<Updated, AccountError>) {
        lock(&self.updates).insert(driver.id(), answer);
    }

    /// The idempotency keys the spends were made with, in order.
    pub fn keys(&self) -> Vec<String> {
        lock(&self.keys).clone()
    }
}

#[async_trait]
impl ProviderAccounts for ScriptedAccounts {
    async fn limits(&self, driver: Driver) -> Result<Option<Limits>, AccountError> {
        lock(&self.limits).get(driver.id()).cloned().unwrap_or(Ok(None))
    }

    async fn reset_credit(&self, driver: Driver, key: &str) -> Result<ResetOutcome, AccountError> {
        lock(&self.keys).push(key.to_string());
        lock(&self.spends).get(driver.id()).cloned().unwrap_or_else(|| Err(AccountError::NotSupported(format!("{} has no reset credit to spend", driver.id()))))
    }

    async fn update(&self, driver: Driver) -> Result<Updated, AccountError> {
        lock(&self.updates).get(driver.id()).cloned().unwrap_or_else(|| Err(AccountError::NotSupported(format!("{} has no update command of its own", driver.id()))))
    }
}
