//! The `TurnTrigger` of the schedules groups: the agent group's own methods, `thread.create` (when the schedule names no thread) and `turn.start`,
//! called as a mapper would, so a schedule's turn is a turn like any other; and the thread's log read for how the turn ended.
use agent_api_agent::{events::FinishReason, ThreadView, THREAD_CREATE, TURN_START};
use agent_api_proto::{Method, Response};
use async_trait::async_trait;
use rca_agent_core::{Build, Call, Connection, Group};
use rca_group_agent::port::ThreadEngine;
use rca_group_agent::AgentThreadsGroup;
use rca_group_schedules::{Refusal, RunRequest, Started, TurnTrigger};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(100);
const PAGE: usize = 200;

pub struct ThreadsTrigger {
    threads: Arc<AgentThreadsGroup>,
    engine: Arc<dyn ThreadEngine>,
}

impl ThreadsTrigger {
    pub fn new(threads: Arc<AgentThreadsGroup>, engine: Arc<dyn ThreadEngine>) -> ThreadsTrigger {
        ThreadsTrigger { threads, engine }
    }

    async fn ask(&self, method: Method, params: Value) -> Result<Value, Refusal> {
        let call = Call { id: json!("schedule"), method, params: &params, build: Build { version: "", commit: "" }, listed: &[], connection: &Connection::answers_only() };
        match self.threads.handle(call).await {
            Response::Result { result, .. } => Ok(result),
            Response::Error { error, .. } => Err(Refusal { code: error.code, message: error.message }),
        }
    }
}

fn object(pairs: impl IntoIterator<Item = (&'static str, Option<Value>)>) -> Value {
    Value::Object(pairs.into_iter().filter_map(|(k, v)| v.map(|v| (k.to_string(), v))).collect::<Map<_, _>>())
}

#[async_trait]
impl TurnTrigger for ThreadsTrigger {
    async fn start(&self, run: &RunRequest) -> Result<Started, Refusal> {
        let thread = match &run.thread {
            Some(thread) => thread.clone(),
            None => {
                let made = self.ask(THREAD_CREATE, object([("title", Some(json!(run.title))), ("workspace", Some(json!(run.workspace))), ("driver", Some(json!(run.driver))), ("model", run.model.as_ref().map(|m| json!(m)))])).await?;
                let view = ThreadView::from_result(&made).map_err(|e| Refusal { code: "INTERNAL".into(), message: e.0 })?;
                view.id
            }
        };
        let params = object([
            ("thread", Some(json!(thread))),
            ("workspace", Some(json!(run.workspace))),
            ("driver", Some(json!(run.driver))),
            ("model", run.model.as_ref().map(|m| json!(m))),
            ("prompt", Some(json!(run.prompt))),
            ("runtime_mode", run.runtime_mode.as_ref().map(|m| json!(m))),
            ("interaction_mode", run.interaction_mode.as_ref().map(|m| json!(m))),
        ]);
        let started = self.ask(TURN_START, params).await?;
        match started["turn"].as_str() {
            Some(turn) => Ok(Started { thread, turn: turn.to_string() }),
            None => Err(Refusal { code: "INTERNAL".into(), message: "turn.start answered no turn".into() }),
        }
    }

    async fn finished(&self, started: &Started) -> Result<(), String> {
        let (mut after, mut why) = (0, None);
        loop {
            let page = match self.engine.history(&started.thread, after, PAGE) {
                Ok(Some(page)) => page,
                Ok(None) => return Err(format!("thread {} no longer exists", started.thread)),
                Err(e) => return Err(e.0),
            };
            for event in page.events.iter().inspect(|e| after = e.seq).filter(|e| e.turn.as_deref() == Some(started.turn.as_str())) {
                match event.kind.as_str() {
                    "error" => why = event.data.get("message").and_then(Value::as_str).map(str::to_string).or(why),
                    "turn.finished" => {
                        return match event.data.get("reason").and_then(Value::as_str).and_then(|r| serde_json::from_value::<FinishReason>(json!(r)).ok()) {
                            Some(FinishReason::Completed) => Ok(()),
                            Some(FinishReason::Interrupted) => Err("the turn was interrupted".into()),
                            _ => Err(why.unwrap_or_else(|| "the turn failed".into())),
                        };
                    }
                    _ => {}
                }
            }
            if !page.more {
                tokio::time::sleep(POLL).await;
            }
        }
    }
}
