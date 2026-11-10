//! Orca methods that say what this host is and let the phone say what it is: `status.get`, `host.platform`, `runtime.clientCapabilities.update`
//! and the `runtime.clientEvents` stream. The values mirror `references/orca/src/main/runtime/rpc/methods` at the pinned commit.
mod client_events;
mod pairing;
mod status;
mod streams;
mod trackers;

pub use pairing::PairingArea;

use orca_core::wire::{self, Failure};
use orca_core::{Area, Ctx, Kind, Method, Subscription};
use serde_json::{json, Value};

pub const STATUS_GET: &str = "status.get";
pub const HOST_PLATFORM: &str = "host.platform";
pub const CLIENT_CAPABILITIES_UPDATE: &str = "runtime.clientCapabilities.update";
pub const CLIENT_EVENTS_SUBSCRIBE: &str = "runtime.clientEvents.subscribe";
pub const CLIENT_EVENTS_UNSUBSCRIBE: &str = "runtime.clientEvents.unsubscribe";

/// The `RUNTIME_PROTOCOL_VERSION` and `MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION` of the pin: a flow test reads them back from the reference.
pub const RUNTIME_PROTOCOL_VERSION: u64 = 3;
pub const MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION: u64 = 2;

const METHODS: &[Method] = &[
    Method { name: STATUS_GET, kind: Kind::Unary },
    Method { name: HOST_PLATFORM, kind: Kind::Unary },
    Method { name: CLIENT_CAPABILITIES_UPDATE, kind: Kind::Unary },
    Method { name: CLIENT_EVENTS_SUBSCRIBE, kind: Kind::Streaming },
    Method { name: CLIENT_EVENTS_UNSUBSCRIBE, kind: Kind::Unary },
    Method { name: streams::ACCOUNTS_UNSUBSCRIBE, kind: Kind::Unary },
    Method { name: streams::NOTIFICATIONS_UNSUBSCRIBE, kind: Kind::Unary },
    Method { name: streams::TERMINAL_UNSUBSCRIBE, kind: Kind::Unary },
    Method { name: "linear.status", kind: Kind::Unary },
    Method { name: "linear.selectWorkspace", kind: Kind::Unary },
    Method { name: "linear.listTeams", kind: Kind::Unary },
    Method { name: "linear.teamStates", kind: Kind::Unary },
    Method { name: "linear.searchIssues", kind: Kind::Unary },
    Method { name: "linear.issueComments", kind: Kind::Unary },
    Method { name: "linear.getIssue", kind: Kind::Unary },
    Method { name: "linear.listIssues", kind: Kind::Unary },
    Method { name: "linear.createIssue", kind: Kind::Unary },
    Method { name: "linear.updateIssue", kind: Kind::Unary },
    Method { name: "linear.addIssueComment", kind: Kind::Unary },
    Method { name: "jira.status", kind: Kind::Unary },
    Method { name: "jira.getIssue", kind: Kind::Unary },
    Method { name: "jira.issueComments", kind: Kind::Unary },
    Method { name: "jira.searchIssues", kind: Kind::Unary },
    Method { name: "jira.listIssues", kind: Kind::Unary },
];

pub struct SettingsArea;

impl Area for SettingsArea {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        match method {
            STATUS_GET => Ok(status::get(ctx, params)),
            HOST_PLATFORM => Ok(json!({ "platform": ctx.host.platform })),
            CLIENT_CAPABILITIES_UPDATE => status::update_client_capabilities(ctx, params),
            CLIENT_EVENTS_UNSUBSCRIBE => client_events::unsubscribe(ctx, params),
            other => streams::call(other, params).or_else(|| trackers::call(other, params)).unwrap_or_else(|| Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {other}")))),
        }
    }

    fn open(&self, ctx: &mut Ctx, method: &str, _params: &Value) -> Result<Box<dyn Subscription>, Failure> {
        match method {
            CLIENT_EVENTS_SUBSCRIBE => Ok(client_events::open(ctx)),
            other => Err(Failure::new(wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}
