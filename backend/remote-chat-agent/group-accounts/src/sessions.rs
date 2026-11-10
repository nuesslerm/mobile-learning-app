//! The authentication sessions: one per login the service started. A session reads the prompt (address and code) the login command prints,
//! notes how the command ends (a login counts only when the provider's directory then holds one), and numbers each change as an event in a
//! count of its own. The sessions live in memory: a restart ends every login, whose command dies with the service.
use crate::ports::{ProviderDirs, ProviderLogin};
use agent_api_accounts::{auth_event, AuthStatus, AuthView};
use agent_api_proto::Event;
use futures_util::StreamExt;
use rca_cli_driver::domain::{read_prompt, Driver, LoginMethod};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::watch;

/// How long `start` waits for the command to print its prompt before it answers with what it has.
pub const PROMPT_WAIT: Duration = Duration::from_secs(5);

/// The longest failure text a session keeps: the tail of what the command wrote to its error output.
/// How long `start` waits for the rest of a prompt once part of it is printed.
const PROMPT_SETTLE: Duration = Duration::from_millis(200);

const ERROR_CHARS: usize = 300;

type Input = Arc<dyn Fn(String) -> bool + Send + Sync>;
type Kill = Arc<dyn Fn() + Send + Sync>;

struct Session {
    driver: Driver,
    method: LoginMethod,
    view: AuthView,
    events: Vec<Event>,
    changed: watch::Sender<u64>,
    input: Input,
    kill: Kill,
}

#[derive(Default)]
struct Inner {
    next: u64,
    sessions: HashMap<String, Session>,
}

/// Why a login did not start.
#[derive(Debug, PartialEq)]
pub enum StartError {
    /// A login of the provider is already running: its session.
    InProgress(String),
    Unavailable(String),
    Directory(String),
}

/// Why pasted text was not taken.
#[derive(Debug, PartialEq)]
pub enum RespondError {
    Unknown,
    Ended,
    /// The login waits for nothing to be pasted.
    NotPaste,
}

pub struct Sessions {
    dirs: Arc<dyn ProviderDirs>,
    login: Arc<dyn ProviderLogin>,
    prompt_wait: Duration,
    inner: Arc<Mutex<Inner>>,
}

fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|p| p.into_inner())
}

/// Changes a session and numbers the change as an event, when something changed.
fn update(inner: &Mutex<Inner>, auth: &str, change: impl FnOnce(&mut AuthView)) {
    let mut guard = lock(inner);
    let Some(session) = guard.sessions.get_mut(auth) else { return };
    let before = session.view.clone();
    change(&mut session.view);
    if session.view != before {
        let seq = session.events.len() as u64 + 1;
        session.events.push(auth_event(seq, &session.view));
        session.changed.send_replace(seq);
    }
}

fn tail(text: &str) -> String {
    let text = text.trim();
    let skip = text.chars().count().saturating_sub(ERROR_CHARS);
    text.chars().skip(skip).collect()
}

impl Sessions {
    pub fn new(dirs: Arc<dyn ProviderDirs>, login: Arc<dyn ProviderLogin>) -> Sessions {
        Sessions::waiting(dirs, login, PROMPT_WAIT)
    }

    /// Sessions whose `start` waits `prompt_wait` for the prompt.
    pub fn waiting(dirs: Arc<dyn ProviderDirs>, login: Arc<dyn ProviderLogin>, prompt_wait: Duration) -> Sessions {
        Sessions { dirs, login, prompt_wait, inner: Arc::default() }
    }

    pub fn dirs(&self) -> &Arc<dyn ProviderDirs> {
        &self.dirs
    }

    /// Whether a login of the provider is running.
    pub fn pending_for(&self, driver: Driver) -> bool {
        lock(&self.inner).sessions.values().any(|s| s.driver == driver && s.view.status == AuthStatus::Pending)
    }

    pub fn get(&self, auth: &str) -> Option<AuthView> {
        lock(&self.inner).sessions.get(auth).map(|s| s.view.clone())
    }

    /// Starts the provider's login and answers once its prompt is printed, or the command ends, or `prompt_wait` is over.
    pub async fn start(&self, driver: Driver, method: LoginMethod) -> Result<AuthView, StartError> {
        if let Some(running) = lock(&self.inner).sessions.values().find(|s| s.driver == driver && s.view.status == AuthStatus::Pending) {
            return Err(StartError::InProgress(running.view.auth.clone()));
        }
        self.dirs.ensure(driver).map_err(StartError::Directory)?;
        let run = self.login.start(driver, method).await.map_err(StartError::Unavailable)?;
        let (auth, mut changes) = {
            let mut guard = lock(&self.inner);
            guard.next += 1;
            let auth = format!("auth-{}", guard.next);
            let view = AuthView { auth: auth.clone(), provider: driver.id().to_string(), method: method.name().to_string(), status: AuthStatus::Pending, url: None, code: None, error: None };
            let changed = watch::channel(1).0;
            let changes = changed.subscribe();
            guard.sessions.insert(auth.clone(), Session { driver, method, events: vec![auth_event(1, &view)], view, changed, input: run.input.clone(), kill: run.kill.clone() });
            (auth, changes)
        };
        let (inner, dirs, id) = (self.inner.clone(), self.dirs.clone(), auth.clone());
        let (mut lines, finish) = (run.lines, run.finish);
        tokio::spawn(async move {
            while let Some(line) = lines.next().await {
                let prompt = read_prompt(&line);
                update(&inner, &id, |view| {
                    view.url = prompt.url.or(view.url.take());
                    view.code = prompt.code.or(view.code.take());
                });
            }
            let ended = finish.await;
            let kept = dirs.signed_in(driver);
            // The first login becomes the default provider before the session reads `succeeded`, so a caller that waited sees it chosen
            if ended.is_ok() && kept && dirs.selected().is_none() {
                let _ = dirs.select(driver);
            }
            update(&inner, &id, |view| {
                if view.status != AuthStatus::Pending {
                    return;
                }
                match ended {
                    Ok(()) if kept => view.status = AuthStatus::Succeeded,
                    Ok(()) => {
                        view.status = AuthStatus::Failed;
                        view.error = Some("the login ended without keeping a credential".into());
                    }
                    Err(why) => {
                        view.status = AuthStatus::Failed;
                        view.error = Some(tail(&why));
                    }
                }
            });
        });
        let wait = async {
            loop {
                let Some(view) = self.get(&auth) else { return };
                let shown = view.url.is_some() || view.code.is_some();
                // A paste login prints an address and then waits for the person; a device login prints the address and the code. A prompt with
                // one of the two is answered once nothing more has been printed for a moment.
                let whole = view.url.is_some() && (view.code.is_some() || method == LoginMethod::Paste);
                if view.status.ended() || whole {
                    return;
                }
                let quiet = if shown { tokio::time::timeout(PROMPT_SETTLE, changes.changed()).await.is_err() } else { changes.changed().await.is_err() };
                if quiet {
                    return;
                }
            }
        };
        let _ = tokio::time::timeout(self.prompt_wait, wait).await;
        self.get(&auth).ok_or_else(|| StartError::Unavailable("the session is gone".into()))
    }

    /// Sends the pasted `text` to a login that waits for one.
    pub fn respond(&self, auth: &str, text: &str) -> Result<AuthView, RespondError> {
        let (input, view) = {
            let guard = lock(&self.inner);
            let session = guard.sessions.get(auth).ok_or(RespondError::Unknown)?;
            if session.view.status.ended() {
                return Err(RespondError::Ended);
            }
            if session.method != LoginMethod::Paste {
                return Err(RespondError::NotPaste);
            }
            (session.input.clone(), session.view.clone())
        };
        if input(text.to_string()) {
            Ok(view)
        } else {
            Err(RespondError::Ended)
        }
    }

    /// Ends a running login: its command is killed and the session reads `cancelled`. None when there is no such session, or it had ended.
    pub fn cancel(&self, auth: &str) -> Result<AuthView, RespondError> {
        let kill = {
            let guard = lock(&self.inner);
            let session = guard.sessions.get(auth).ok_or(RespondError::Unknown)?;
            if session.view.status.ended() {
                return Err(RespondError::Ended);
            }
            session.kill.clone()
        };
        update(&self.inner, auth, |view| view.status = AuthStatus::Cancelled);
        kill();
        self.get(auth).ok_or(RespondError::Unknown)
    }

    /// Waits until the session ends or `timeout` is over, and answers its state then.
    pub async fn complete(&self, auth: &str, timeout: Duration) -> Option<AuthView> {
        let mut changes = lock(&self.inner).sessions.get(auth)?.changed.subscribe();
        let _ = tokio::time::timeout(timeout, async {
            loop {
                if self.get(auth).is_none_or(|v| v.status.ended()) || changes.changed().await.is_err() {
                    return;
                }
            }
        })
        .await;
        self.get(auth)
    }

    /// The numbering of the session's events: the last `seq` and the wake-ups of new ones.
    pub fn open(&self, auth: &str) -> Option<(u64, watch::Receiver<u64>)> {
        let guard = lock(&self.inner);
        let session = guard.sessions.get(auth)?;
        Some((session.events.len() as u64, session.changed.subscribe()))
    }

    pub fn events_after(&self, auth: &str, after: u64) -> Vec<Event> {
        lock(&self.inner).sessions.get(auth).map(|s| s.events.iter().filter(|e| e.seq > after).cloned().collect()).unwrap_or_default()
    }

    /// Ends the running login of a provider, if any (a logout does).
    pub fn cancel_for(&self, driver: Driver) {
        let running: Vec<String> = lock(&self.inner).sessions.values().filter(|s| s.driver == driver && s.view.status == AuthStatus::Pending).map(|s| s.view.auth.clone()).collect();
        for auth in running {
            let _ = self.cancel(&auth);
        }
    }
}
