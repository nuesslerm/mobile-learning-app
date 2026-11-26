//! The three groups: `preview` asks the workspace's executor, `preview.sessions` answers from the tabs, and `preview.watch` answers
//! `preview.subscribe` and then pushes each event of the stream until the peer is gone or falls behind the retained tail.
use rca_agent_core::BrowserPages;
use crate::sessions::{Failure, Previews};
use agent_api_preview::sessions::{self as tabs, AdjustParams, CloseParams, ListParams, OpenParams, ReportParams, ResizeParams, SessionResult};
use agent_api_preview::watch::{self, SubscribeParams};
use agent_api_preview::{self as api};
use agent_api_proto::{code, GroupName, Response};
use async_trait::async_trait;
use exec_proto::Code;
use rca_agent_core::{Call, Group, Served};
use rca_exec_channel::remote::RemoteError;
use rca_exec_channel::Executors;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

pub const SERVED: [Served; 3] = [Served::service(api::START, &[exec_preview::PREVIEW_START]), Served::service(api::STOP, &[exec_preview::PREVIEW_STOP]), Served::service(api::SERVERS, &[exec_preview::PREVIEW_LIST])];
const SESSIONS_SERVED: [Served; 6] = [Served::service(tabs::OPEN, &[]), Served::service(tabs::REPORT, &[]), Served::service(tabs::RESIZE, &[]), Served::service(tabs::ADJUST, &[]), Served::service(tabs::CLOSE, &[]), Served::service(tabs::LIST, &[])];
const WATCH_SERVED: [Served; 1] = [Served::service(watch::SUBSCRIBE, &[])];

pub struct PreviewGroup {
    executors: Arc<dyn Executors>,
}

impl PreviewGroup {
    pub fn new(executors: Arc<dyn Executors>) -> PreviewGroup {
        PreviewGroup { executors }
    }

    async fn relay(&self, spec: &api::Spec, params: &Value) -> Result<Value, (String, String)> {
        let workspace = api::workspace_param(params).map_err(|e| (code::INVALID_PARAMS.to_string(), e.0))?;
        let channel = self.executors.get(&workspace).ok_or_else(|| (Code::NoExecutor.as_str().to_string(), format!("no executor is attached for workspace {workspace}")))?;
        channel.call(spec.action, api::executor_args(spec, params)).await.map(|out| out.result).map_err(|e: RemoteError| (e.code().to_string(), e.message().to_string()))
    }
}

#[async_trait]
impl Group for PreviewGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let spec = api::spec_of(call.method).unwrap_or_else(|| unreachable!("{} is not a method of preview", call.method.name()));
        match self.relay(spec, call.params).await {
            Ok(result) => Response::result(call.id, result),
            Err((code, message)) => Response::error(call.id, &code, message),
        }
    }
}

fn answer<T: Serialize>(id: Value, result: Result<T, Failure>) -> Response {
    match result.map(|r| serde_json::to_value(r)) {
        Ok(Ok(value)) => Response::result(id, value),
        Ok(Err(e)) => Response::error(id, "INTERNAL", e.to_string()),
        Err(failure) => Response::error(id, &failure.code, failure.message),
    }
}

pub struct PreviewSessionGroup {
    browser: Option<Arc<dyn BrowserPages>>,
    previews: Arc<Previews>,
}

impl PreviewSessionGroup {
    pub fn new(previews: Arc<Previews>) -> PreviewSessionGroup {
        PreviewSessionGroup { previews, browser: None }
    }
    pub fn with_browser(mut self, browser: Arc<dyn BrowserPages>) -> Self {
        self.browser = Some(browser);
        self
    }

    async fn open(&self, params: OpenParams) -> Result<SessionResult, Failure> {
        if params.runtime != Some(tabs::Runtime::Server) {
            return self.previews.open(params).map(|session| SessionResult { session });
        }
        let browser = self.browser.as_ref().ok_or_else(|| Failure { code: "BROWSER_DISABLED".into(), message: "This host cannot stream a browser".into() })?;
        let workspace = params.workspace.clone().filter(|w| !w.is_empty()).ok_or_else(|| Failure { code: code::INVALID_PARAMS.into(), message: "A server preview needs a workspace".into() })?;
        let key = self.previews.next_tab_id();
        browser.open_page(&workspace, &key, params.url.as_deref()).await.map_err(|(code,message)| Failure {code,message})?;
        match self.previews.open_with_id(params, key.clone()) {
            Ok(session) => Ok(SessionResult { session }),
            Err(error) => {
                browser.close_page(&workspace, &key).await.map_err(|(code,message)| Failure {code,message})?;
                Err(error)
            }
        }
    }
    async fn close(&self, params: CloseParams) -> Result<tabs::CloseResult, Failure> {
        let sessions = self.previews.list(ListParams { thread: params.thread.clone() }).sessions;
        let mut closed = Vec::new();
        for tab in sessions.into_iter().filter(|s| params.tab.as_ref().is_none_or(|id| *id == s.tab)) {
            if let (Some(browser), Some(workspace), Some(tabs::Runtime::Server)) = (&self.browser, &tab.workspace, tab.runtime) {
                browser.close_page(workspace, &tab.tab).await.map_err(|(code,message)| Failure {code,message})?;
            }
            closed.extend(self.previews.close(CloseParams { thread:params.thread.clone(),tab:Some(tab.tab) }).closed);
        }
        Ok(tabs::CloseResult {closed})
    }

}

#[async_trait]
impl Group for PreviewSessionGroup {
    fn name(&self) -> GroupName {
        tabs::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SESSIONS_SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, .. } = call;
        macro_rules! parsed {
            ($parse:expr) => {
                match $parse {
                    Ok(p) => p,
                    Err(e) => return e.response(id),
                }
            };
        }
        match method {
            tabs::OPEN => {
                let p = parsed!(OpenParams::parse(params));
                answer(id, self.open(p).await)
            }
            tabs::REPORT => {
                let p = parsed!(ReportParams::parse(params));
                answer(id, self.previews.report(p).map(|session| SessionResult { session }))
            }
            tabs::RESIZE => {
                let p = parsed!(ResizeParams::parse(params));
                answer(id, self.previews.resize(p).map(|session| SessionResult { session }))
            }
            tabs::ADJUST => {
                let p = parsed!(AdjustParams::parse(params));
                answer(id, self.previews.adjust(p).map(|session| SessionResult { session }))
            }
            tabs::CLOSE => {
                let p = parsed!(CloseParams::parse(params));
                answer(id, self.close(p).await)
            }
            tabs::LIST => {
                let p = parsed!(ListParams::parse(params));
                answer(id, Ok(self.previews.list(p)))
            }
            other => unreachable!("preview.sessions does not serve {}", other.name()),
        }
    }
}

pub struct PreviewWatchGroup {
    previews: Arc<Previews>,
}

impl PreviewWatchGroup {
    pub fn new(previews: Arc<Previews>) -> PreviewWatchGroup {
        PreviewWatchGroup { previews }
    }
}

#[async_trait]
impl Group for PreviewWatchGroup {
    fn name(&self) -> GroupName {
        watch::GROUP
    }

    fn methods(&self) -> &[Served] {
        &WATCH_SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, connection, .. } = call;
        debug_assert_eq!(method, watch::SUBSCRIBE);
        let p = match SubscribeParams::parse(params) {
            Ok(p) => p,
            Err(e) => return e.response(id),
        };
        let opened = self.previews.open_stream(p.epoch.as_deref(), p.after_seq);
        let (previews, sink) = (self.previews.clone(), connection.sink());
        let (mut changes, mut after) = (opened.changes, opened.from);
        connection.follow(watch::TOPIC, async move {
            loop {
                let slice = previews.since(after);
                if slice.lost {
                    return;
                }
                for event in &slice.events {
                    after = event.seq;
                    if !sink.send_replay(event).await {
                        return;
                    }
                }
                if slice.events.is_empty() && changes.changed().await.is_err() {
                    return;
                }
            }
        });
        answer(id, Ok::<_, Failure>(opened.result))
    }
}
