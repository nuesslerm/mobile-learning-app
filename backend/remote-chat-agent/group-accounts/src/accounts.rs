//! The `accounts` group: the providers and their logins, usage and credit. No reply of it carries a token: a provider is a status, a login a
//! session with the prompt its CLI printed.
use crate::ports::{ModelApi, ModelError};
use crate::sessions::{RespondError, Sessions, StartError};
use agent_api_accounts::{self as api, code, ProviderStatus, ProviderView, UsageRow};
use agent_api_proto::{code as proto_code, GroupName, Response};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use rca_cli_driver::domain::{login_methods, Driver, LoginMethod, DRIVERS};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

pub const SERVED: [Served; 11] = [
    Served::service(api::PROVIDER_LIST, &[]),
    Served::service(api::PROVIDER_REFRESH, &[]),
    Served::service(api::PROVIDER_SELECT, &[]),
    Served::service(api::AUTH_START, &[]),
    Served::service(api::AUTH_RESPOND, &[]),
    Served::service(api::AUTH_COMPLETE, &[]),
    Served::service(api::AUTH_CANCEL, &[]),
    Served::service(api::AUTH_SUBSCRIBE, &[]),
    Served::service(api::PROVIDER_LOGOUT, &[]),
    Served::service(api::USAGE_SUMMARY, &[]),
    Served::service(api::RESET_CREDIT, &[]),
];

struct Refusal(&'static str, String);

type Answer = Result<Value, Refusal>;

fn refuse(code: &'static str, message: impl Into<String>) -> Refusal {
    Refusal(code, message.into())
}

impl From<agent_api_proto::ParamError> for Refusal {
    fn from(e: agent_api_proto::ParamError) -> Refusal {
        refuse(proto_code::INVALID_PARAMS, e.0)
    }
}

fn driver_named(name: &str) -> Result<Driver, Refusal> {
    Driver::parse(name).ok_or_else(|| refuse(code::UNKNOWN_PROVIDER, format!("{name} is not a provider; the providers are {}", DRIVERS.iter().map(|d| d.id()).collect::<Vec<_>>().join(", "))))
}

pub struct AccountsGroup {
    sessions: Arc<Sessions>,
    models: Arc<dyn ModelApi>,
}

impl AccountsGroup {
    pub fn new(sessions: Arc<Sessions>, models: Arc<dyn ModelApi>) -> AccountsGroup {
        AccountsGroup { sessions, models }
    }

    fn status_of(&self, driver: Driver) -> ProviderStatus {
        if self.sessions.pending_for(driver) {
            ProviderStatus::Authenticating
        } else if self.sessions.dirs().signed_in(driver) {
            ProviderStatus::SignedIn
        } else {
            ProviderStatus::SignedOut
        }
    }

    fn view_of(&self, driver: Driver) -> ProviderView {
        let selected = self.sessions.dirs().selected() == Some(driver);
        ProviderView { provider: driver.id().to_string(), status: self.status_of(driver), selected, auth_methods: login_methods(driver).into_iter().map(|m| m.name().to_string()).collect() }
    }

    fn list(&self) -> Value {
        api::list_result(&DRIVERS.iter().map(|d| self.view_of(*d)).collect::<Vec<_>>())
    }

    fn refresh(&self, params: &Value) -> Answer {
        if let Some(name) = api::optional_provider(params)? {
            driver_named(&name)?;
        }
        Ok(self.list())
    }

    fn select(&self, params: &Value) -> Answer {
        let driver = driver_named(&api::provider_param(params)?)?;
        if !self.sessions.dirs().signed_in(driver) {
            return Err(refuse(code::SIGNED_OUT, format!("{} holds no login; sign it in first", driver.id())));
        }
        self.sessions.dirs().select(driver).map_err(|why| refuse(code::DIRECTORY_UNAVAILABLE, why))?;
        Ok(self.list())
    }

    async fn start(&self, params: &Value) -> Answer {
        let p = api::StartParams::parse(params)?;
        let driver = driver_named(&p.provider)?;
        let offered = login_methods(driver);
        let method = match p.method.as_deref() {
            None => offered[0],
            Some(name) => LoginMethod::parse(name).filter(|m| offered.contains(m)).ok_or_else(|| refuse(code::UNSUPPORTED_METHOD, format!("{} signs in by {}; {name} is not one of them", driver.id(), offered.iter().map(|m| m.name()).collect::<Vec<_>>().join(" or "))))?,
        };
        match self.sessions.start(driver, method).await {
            Ok(view) => Ok(api::auth_result(&view)),
            Err(StartError::InProgress(auth)) => Err(refuse(code::AUTH_IN_PROGRESS, format!("{} is already being signed in by {auth}", driver.id()))),
            Err(StartError::Unavailable(why)) => Err(refuse(code::LOGIN_UNAVAILABLE, format!("{} could not start its login: {why}", driver.id()))),
            Err(StartError::Directory(why)) => Err(refuse(code::DIRECTORY_UNAVAILABLE, why)),
        }
    }

    fn respond(&self, params: &Value) -> Answer {
        let p = api::RespondParams::parse(params)?;
        match self.sessions.respond(&p.auth, &p.text) {
            Ok(view) => Ok(api::auth_result(&view)),
            Err(e) => Err(session_refusal(&p.auth, e)),
        }
    }

    async fn complete(&self, params: &Value) -> Answer {
        let p = api::CompleteParams::parse(params)?;
        match self.sessions.complete(&p.auth, Duration::from_millis(p.timeout_ms)).await {
            Some(view) => Ok(api::auth_result(&view)),
            None => Err(session_refusal(&p.auth, RespondError::Unknown)),
        }
    }

    fn cancel(&self, params: &Value) -> Answer {
        let auth = api::auth_param(params)?;
        match self.sessions.cancel(&auth) {
            Ok(view) => Ok(api::auth_result(&view)),
            Err(e) => Err(session_refusal(&auth, e)),
        }
    }

    fn subscribe(&self, params: &Value, call: &Call<'_>) -> Answer {
        let p = api::SubscribeParams::parse(params)?;
        let Some((seq, mut changes)) = self.sessions.open(&p.auth) else { return Err(session_refusal(&p.auth, RespondError::Unknown)) };
        let (sessions, sink, auth, mut after) = (self.sessions.clone(), call.connection.sink(), p.auth.clone(), p.after_seq);
        call.connection.follow(format!("provider.auth/{}", p.auth), async move {
            loop {
                let pending = sessions.events_after(&auth, after);
                for event in &pending {
                    after = event.seq;
                    if !sink.send_replay(event).await {
                        return;
                    }
                }
                if pending.is_empty() && changes.changed().await.is_err() {
                    return;
                }
            }
        });
        Ok(api::subscribe_result(&p.auth, seq))
    }

    async fn logout(&self, params: &Value) -> Answer {
        let driver = driver_named(&api::provider_param(params)?)?;
        self.sessions.cancel_for(driver);
        let dirs = self.sessions.dirs();
        dirs.clear(driver).map_err(|why| refuse(code::DIRECTORY_UNAVAILABLE, why))?;
        if dirs.selected() == Some(driver) {
            dirs.deselect().map_err(|why| refuse(code::DIRECTORY_UNAVAILABLE, why))?;
        }
        Ok(json!({ "provider": self.view_of(driver) }))
    }

    async fn usage(&self) -> Answer {
        let mut rows = Vec::new();
        for driver in DRIVERS {
            let usage = self.models.usage(driver.id()).await.ok().flatten();
            rows.push(UsageRow { provider: driver.id().to_string(), status: self.status_of(driver), usage });
        }
        Ok(api::usage_result(&rows))
    }

    async fn reset_credit(&self, params: &Value) -> Answer {
        let driver = driver_named(&api::provider_param(params)?)?;
        match self.models.reset_credit(driver.id()).await {
            Ok(credit) => Ok(api::reset_credit_result(driver.id(), &credit)),
            Err(ModelError::NotSupported(why)) => Err(refuse(code::NOT_SUPPORTED, why)),
            Err(ModelError::Unavailable(why)) => Err(refuse(code::MODEL_API_UNAVAILABLE, why)),
        }
    }
}

fn session_refusal(auth: &str, e: RespondError) -> Refusal {
    match e {
        RespondError::Unknown => refuse(code::UNKNOWN_AUTH, format!("{auth} is no login this service started")),
        RespondError::Ended => refuse(code::AUTH_ENDED, format!("{auth} has ended")),
        RespondError::NotPaste => refuse(code::UNSUPPORTED_METHOD, format!("{auth} signs in by a code the person enters at its address; nothing is pasted")),
    }
}

#[async_trait]
impl Group for AccountsGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let params = call.params;
        let answer = match call.method {
            api::PROVIDER_LIST => Ok(self.list()),
            api::PROVIDER_REFRESH => self.refresh(params),
            api::PROVIDER_SELECT => self.select(params),
            api::AUTH_START => self.start(params).await,
            api::AUTH_RESPOND => self.respond(params),
            api::AUTH_COMPLETE => self.complete(params).await,
            api::AUTH_CANCEL => self.cancel(params),
            api::AUTH_SUBSCRIBE => self.subscribe(params, &call),
            api::PROVIDER_LOGOUT => self.logout(params).await,
            api::USAGE_SUMMARY => self.usage().await,
            api::RESET_CREDIT => self.reset_credit(params).await,
            other => unreachable!("{} is not a method of accounts", other.name()),
        };
        match answer {
            Ok(result) => Response::result(call.id, result),
            Err(Refusal(code, message)) => Response::error(call.id, code, message),
        }
    }
}
