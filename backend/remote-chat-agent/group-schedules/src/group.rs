//! The two groups: `schedules` answers the methods that keep and run schedules, `schedules.watch` answers `schedule.subscribe` with the schedules as
//! they are and then pushes each event of the stream until the peer is gone or falls behind the retained tail.
use crate::service::{Failure, Schedules};
use agent_api_proto::{GroupName, Response};
use agent_api_schedules::watch::{self, SubscribeParams};
use agent_api_schedules::{self as api, DeleteResult, IdParams, ListParams, ListResult, SetEnabledParams, UpsertParams};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

// `schedule.runNow` starts a turn of a CLI, which the model of that turn then calls the MCP tools of;
// the checkpoint it takes first is `git_checkpoint`, named here so this crate keeps no dependency on exec-git.
const SERVED: [Served; 6] = [Served::service(api::LIST, &[]), Served::service(api::UPSERT, &[]), Served::service(api::SET_ENABLED, &[]), Served::service(api::DELETE, &[]), Served::cli(api::RUN_NOW, &["git_checkpoint"]), Served::service(api::ROTATE_WEBHOOK_TOKEN, &[])];
const WATCH_SERVED: [Served; 1] = [Served::service(watch::SUBSCRIBE, &[])];

pub struct ScheduleGroup {
    schedules: Arc<Schedules>,
}

impl ScheduleGroup {
    pub fn new(schedules: Arc<Schedules>) -> ScheduleGroup {
        ScheduleGroup { schedules }
    }
}

fn answer<T: Serialize>(id: Value, result: Result<T, Failure>) -> Response {
    match result.map(|r| serde_json::to_value(r)) {
        Ok(Ok(value)) => Response::result(id, value),
        Ok(Err(e)) => Response::error(id, "INTERNAL", e.to_string()),
        Err(failure) => Response::error(id, &failure.code, failure.message),
    }
}

#[async_trait]
impl Group for ScheduleGroup {
    fn name(&self) -> GroupName {
        api::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
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
            api::LIST => {
                parsed!(ListParams::parse(params));
                answer(id, self.schedules.list().map(|tasks| ListResult { tasks }))
            }
            api::UPSERT => {
                let p = parsed!(UpsertParams::parse(params));
                answer(id, self.schedules.upsert(p))
            }
            api::SET_ENABLED => {
                let p = parsed!(SetEnabledParams::parse(params));
                answer(id, self.schedules.set_enabled(&p.id, p.enabled))
            }
            api::DELETE => {
                let p = parsed!(IdParams::parse(params));
                answer(id, self.schedules.delete(&p.id).map(|()| DeleteResult { id: p.id.clone() }))
            }
            api::RUN_NOW => {
                let p = parsed!(IdParams::parse(params));
                answer(id, self.schedules.run_now(&p.id).await)
            }
            api::ROTATE_WEBHOOK_TOKEN => {
                let p = parsed!(IdParams::parse(params));
                answer(id, self.schedules.rotate_webhook_token(&p.id))
            }
            other => unreachable!("the schedules group does not serve {}", other.name()),
        }
    }
}

pub struct ScheduleWatchGroup {
    schedules: Arc<Schedules>,
}

impl ScheduleWatchGroup {
    pub fn new(schedules: Arc<Schedules>) -> ScheduleWatchGroup {
        ScheduleWatchGroup { schedules }
    }
}

#[async_trait]
impl Group for ScheduleWatchGroup {
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
        let opened = match self.schedules.open(p.after_seq) {
            Ok(opened) => opened,
            Err(failure) => return Response::error(id, &failure.code, failure.message),
        };
        let (schedules, sink) = (self.schedules.clone(), connection.sink());
        let (mut changes, mut after) = (opened.changes, opened.from);
        connection.follow(watch::TOPIC, async move {
            loop {
                let slice = schedules.since(after);
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
