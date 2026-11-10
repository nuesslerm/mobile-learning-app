//! The two groups: `state` answers `state.get/set`, `state.watch` answers `state.subscribe` with the scope's values and then pushes each write
//! to the scope as a `state.changed` event until the peer is gone or falls behind the retained tail.
use crate::hub::StateHub;
use agent_api_proto::{GroupName, Response};
use agent_api_state::{self as state, watch};
use async_trait::async_trait;
use rca_agent_core::{Call, Group, Served};
use std::sync::Arc;

const SERVED: [Served; 2] = [Served::service(state::GET, &[]), Served::service(state::SET, &[])];
const WATCH_SERVED: [Served; 1] = [Served::service(watch::SUBSCRIBE, &[])];

pub struct StateGroup {
    hub: Arc<StateHub>,
}

impl StateGroup {
    pub fn new(hub: Arc<StateHub>) -> StateGroup {
        StateGroup { hub }
    }
}

#[async_trait]
impl Group for StateGroup {
    fn name(&self) -> GroupName {
        state::GROUP
    }

    fn methods(&self) -> &[Served] {
        &SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, .. } = call;
        match method {
            state::GET => match state::Key::parse(params) {
                Ok(key) => Response::result(id, state::get_result(self.hub.get(&key.scope, &key.key))),
                Err(e) => e.response(id),
            },
            state::SET => match state::SetArgs::parse(params) {
                Ok(args) => match self.hub.set(&args.key.scope, &args.key.key, args.value) {
                    Ok(()) => Response::result(id, state::set_result()),
                    Err(why) => Response::error(id, state::STATE_UNAVAILABLE, why),
                },
                Err(e) => e.response(id),
            },
            other => unreachable!("the state group does not serve {}", other.name()),
        }
    }
}

pub struct StateWatchGroup {
    hub: Arc<StateHub>,
}

impl StateWatchGroup {
    pub fn new(hub: Arc<StateHub>) -> StateWatchGroup {
        StateWatchGroup { hub }
    }
}

#[async_trait]
impl Group for StateWatchGroup {
    fn name(&self) -> GroupName {
        watch::GROUP
    }

    fn methods(&self) -> &[Served] {
        &WATCH_SERVED
    }

    async fn handle(&self, call: Call<'_>) -> Response {
        let Call { id, method, params, connection, .. } = call;
        debug_assert_eq!(method, watch::SUBSCRIBE);
        let p = match watch::SubscribeParams::parse(params) {
            Ok(p) => p,
            Err(e) => return e.response(id),
        };
        let opened = self.hub.open(&p.scope, p.after_seq);
        let (hub, sink, scope) = (self.hub.clone(), connection.sink(), p.scope);
        let (mut changes, mut after) = (opened.changes, opened.from);
        connection.follow(format!("state/{scope}"), async move {
            loop {
                let slice = hub.since(&scope, after);
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
        match serde_json::to_value(&opened.result) {
            Ok(result) => Response::result(id, result),
            Err(e) => Response::error(id, "INTERNAL", e.to_string()),
        }
    }
}
