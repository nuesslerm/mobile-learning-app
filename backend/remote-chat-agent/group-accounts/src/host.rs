//! The accounts over an executor: the provider CLIs log in at the workspace host (decision 9), so what the group reads and asks is the host's.
//! `HostDirs` learns which providers hold a login from `cli_detect` and ends one with the executor's `logout`; `HostLogin` runs a login as the
//! executor's `login_start`, `login_status`, `login_send` and `login_cancel`, polled into the lines and the end a `LoginRun` has. The choice of the
//! default provider stays with the service (`selection`).
use crate::limits::{claude_limits, codex_limits};
use crate::ports::{AccountError, LoginRun, ProviderAccounts, ProviderDirs, ProviderLogin};
use agent_api_accounts::{Limits, ResetOutcome, Updated};
use async_trait::async_trait;
use futures_util::stream::StreamExt;
use rca_cli_driver::domain::{Driver, LoginMethod, DRIVERS};
use rca_exec_channel::remote::RemoteError;
use rca_exec_channel::{Executors, RouteError};
use serde_json::{json, Value};

const CLI_DETECT: &str = "cli_detect";
const LOGIN_START: &str = "login_start";
const LOGIN_STATUS: &str = "login_status";
const LOGIN_SEND: &str = "login_send";
const LOGIN_CANCEL: &str = "login_cancel";
const LOGOUT: &str = "logout";

/// What `login_status` answers (the fields of `exec_proc::LoginState` this group reads).
#[derive(serde::Deserialize)]
struct LoginState {
    output: String,
    next: u64,
    ended: bool,
    ok: Option<bool>,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
struct LoginStarted {
    flow: String,
}

#[derive(serde::Deserialize)]
struct Detected {
    program: String,
    signed_in: Option<bool>,
}

#[derive(serde::Deserialize)]
struct DetectResult {
    clis: Vec<Detected>,
}
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// How often a running login is asked what it printed.
const POLL: Duration = Duration::from_millis(250);

fn route(executors: &dyn Executors) -> Result<rca_exec_channel::Route, String> {
    executors.route_new(None).map_err(|e| match e {
        RouteError::AmbiguousExecutor(all) => format!("several executors are attached ({}): name one", all.join(", ")),
        RouteError::UnknownExecutor(name) => format!("no executor {name} is attached"),
        RouteError::NoExecutor | RouteError::UnknownWorkspace(_) => "no executor is attached".to_string(),
    })
}

fn said(e: RemoteError) -> String {
    e.message()
}

fn read<T: serde::de::DeserializeOwned>(result: Value) -> Result<T, String> {
    serde_json::from_value(result).map_err(|e| format!("the executor's answer is not the contract's: {e}"))
}

/// Which providers hold a login at the executor, and the service's own choice of the default one.
pub struct HostDirs {
    executors: Arc<dyn Executors>,
    selection: Arc<dyn ProviderDirs>,
    signed: Mutex<HashMap<&'static str, bool>>,
}

impl HostDirs {
    pub fn new(executors: Arc<dyn Executors>, selection: Arc<dyn ProviderDirs>) -> HostDirs {
        HostDirs { executors, selection, signed: Mutex::default() }
    }

    fn remember(&self, driver: Driver, signed_in: bool) {
        self.signed.lock().unwrap_or_else(|p| p.into_inner()).insert(driver.id(), signed_in);
    }
}

#[async_trait]
impl ProviderDirs for HostDirs {
    async fn refresh(&self) {
        let Ok(route) = route(self.executors.as_ref()) else { return };
        let programs: Vec<&str> = DRIVERS.iter().map(|d| d.program()).collect();
        let Ok(reply) = route.channel.call(CLI_DETECT, json!({"programs": programs})).await else { return };
        let Ok(found) = read::<DetectResult>(reply.result) else { return };
        for driver in DRIVERS {
            let signed_in = found.clis.iter().find(|c| c.program == driver.program()).is_some_and(|c| c.signed_in == Some(true));
            self.remember(driver, signed_in);
        }
    }

    fn ensure(&self, _driver: Driver) -> Result<(), String> {
        Ok(())
    }

    fn signed_in(&self, driver: Driver) -> bool {
        self.signed.lock().unwrap_or_else(|p| p.into_inner()).get(driver.id()).copied().unwrap_or(false)
    }

    fn clear(&self, driver: Driver) -> Result<(), String> {
        self.remember(driver, false);
        Ok(())
    }

    async fn logout(&self, driver: Driver) -> Result<(), String> {
        let route = route(self.executors.as_ref())?;
        route.channel.call(LOGOUT, json!({"program": driver.program()})).await.map_err(said)?;
        self.refresh().await;
        Ok(())
    }

    fn selected(&self) -> Option<Driver> {
        self.selection.selected()
    }

    fn select(&self, driver: Driver) -> Result<(), String> {
        self.selection.select(driver)
    }

    fn deselect(&self) -> Result<(), String> {
        self.selection.deselect()
    }
}

/// A login run by the executor.
pub struct HostLogin {
    executors: Arc<dyn Executors>,
}

impl HostLogin {
    pub fn new(executors: Arc<dyn Executors>) -> HostLogin {
        HostLogin { executors }
    }
}

#[async_trait]
impl ProviderLogin for HostLogin {
    /// The executor runs each CLI's own login, so `method` says only how the person answers it (a pasted code or not).
    async fn start(&self, driver: Driver, _method: LoginMethod) -> Result<LoginRun, String> {
        let channel = route(self.executors.as_ref())?.channel;
        let started: LoginStarted = read(channel.call(LOGIN_START, json!({"program": driver.program()})).await.map_err(said)?.result)?;
        let flow = started.flow;
        let (lines_tx, lines_rx) = mpsc::unbounded_channel::<String>();
        let (done_tx, done_rx) = oneshot::channel::<Result<(), String>>();
        let polled = channel.clone();
        let polling = flow.clone();
        tokio::spawn(async move {
            let (mut after, mut partial) = (0u64, String::new());
            let ended = loop {
                tokio::time::sleep(POLL).await;
                let state: LoginState = match polled.call(LOGIN_STATUS, json!({"flow": polling, "after": after})).await {
                    Ok(reply) => match read(reply.result) {
                        Ok(state) => state,
                        Err(why) => break Err(why),
                    },
                    Err(e) => break Err(said(e)),
                };
                after = state.next;
                partial.push_str(&state.output);
                while let Some(end) = partial.find('\n') {
                    let line: String = partial.drain(..=end).collect();
                    let _ = lines_tx.send(line.trim_end().to_string());
                }
                if state.ended {
                    if !partial.trim().is_empty() {
                        let _ = lines_tx.send(partial.trim().to_string());
                    }
                    break if state.ok == Some(true) { Ok(()) } else { Err(state.error.unwrap_or_else(|| "the login failed".to_string())) };
                }
            };
            drop(lines_tx);
            let _ = done_tx.send(ended);
        });
        let (sender, killer, flow_sent, flow_killed) = (channel.clone(), channel, flow.clone(), flow);
        Ok(LoginRun {
            lines: tokio_stream_of(lines_rx),
            finish: Box::pin(async move { done_rx.await.unwrap_or_else(|_| Err("the login was dropped".to_string())) }),
            input: Arc::new(move |text| {
                let (channel, flow) = (sender.clone(), flow_sent.clone());
                tokio::spawn(async move {
                    let _ = channel.call(LOGIN_SEND, json!({"flow": flow, "text": text})).await;
                });
                true
            }),
            kill: Arc::new(move || {
                let (channel, flow) = (killer.clone(), flow_killed.clone());
                tokio::spawn(async move {
                    let _ = channel.call(LOGIN_CANCEL, json!({"flow": flow})).await;
                });
            }),
        })
    }
}

fn tokio_stream_of(mut rx: mpsc::UnboundedReceiver<String>) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = String> + Send>> {
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx)).boxed()
}

const CLI_UPDATE: &str = "cli_update";
const CODEX_APP_SERVER: &str = "codex_app_server_request";
const CLAUDE_ACCOUNT: &str = "claude_account_request";

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The providers' own accounts asked at the executor: Codex's limits and credit through its app-server, Claude's limits through its usage
/// service, a CLI's update through its own command. The executor holds the logins; what comes back is windows, counts and an outcome.
pub struct HostAccounts {
    executors: Arc<dyn Executors>,
}

impl HostAccounts {
    pub fn new(executors: Arc<dyn Executors>) -> HostAccounts {
        HostAccounts { executors }
    }

    async fn ask(&self, action: &str, args: Value) -> Result<Value, AccountError> {
        let route = route(self.executors.as_ref()).map_err(AccountError::Unavailable)?;
        route.channel.call(action, args).await.map(|reply| reply.result).map_err(|e| AccountError::Unavailable(said(e)))
    }
}

#[async_trait]
impl ProviderAccounts for HostAccounts {
    async fn limits(&self, driver: Driver) -> Result<Option<Limits>, AccountError> {
        match driver {
            Driver::Codex => {
                let answer = self.ask(CODEX_APP_SERVER, json!({"method": "account/rateLimits/read"})).await?;
                Ok(Some(codex_limits(&answer["result"], now_ms())))
            }
            Driver::ClaudeAgent => {
                let answer = self.ask(CLAUDE_ACCOUNT, json!({"request": "usage"})).await?;
                match answer["status"].as_u64() {
                    Some(200) => Ok(Some(claude_limits(&answer["body"], now_ms()))),
                    Some(status) => Err(AccountError::Unavailable(format!("the Claude usage service answered {status}"))),
                    None => Err(AccountError::Unavailable("the executor's answer is not the contract's".into())),
                }
            }
            _ => Ok(None),
        }
    }

    async fn reset_credit(&self, driver: Driver, key: &str) -> Result<ResetOutcome, AccountError> {
        if driver != Driver::Codex {
            return Err(AccountError::NotSupported(format!("{} has no reset credit to spend", driver.id())));
        }
        let answer = self.ask(CODEX_APP_SERVER, json!({"method": "account/rateLimitResetCredit/consume", "params": {"idempotencyKey": key}})).await?;
        let name = answer["result"]["outcome"].as_str().or_else(|| answer["result"]["code"].as_str()).unwrap_or_default();
        ResetOutcome::parse(name).ok_or_else(|| AccountError::Unavailable(format!("the Codex account answered a spend with {name:?}, which is no outcome")))
    }

    async fn update(&self, driver: Driver) -> Result<Updated, AccountError> {
        if driver == Driver::Codex {
            return Err(AccountError::NotSupported("codex has no update command of its own; update it with the package manager that installed it".into()));
        }
        let answer = self.ask(CLI_UPDATE, json!({"program": driver.program()})).await?;
        serde_json::from_value(json!({"ok": answer["ok"], "exit_code": answer["exit_code"], "output": answer["output"], "version": answer.get("version").cloned().unwrap_or(Value::Null)})).map_err(|e| AccountError::Unavailable(format!("the executor's answer is not the contract's: {e}")))
    }
}
