//! Orca's kept state over rca's state group (`state.get`, `state.set`): the host-side settings a phone reads and updates (`settings.get`,
//! `settings.update`), the terminal quick commands and the UI state (`ui.get`, `ui.set`). Each is one object (or list) under a key of the scope
//! `orca`, so a restart of the server or of rca reads it back. Orca derives defaults for every setting the object lacks and the phone reads an
//! absent key as that default, so `settings.get` answers what was set and nothing else. A settings key the pinned schema does not know is
//! refused, never dropped. What needs more than kept values (the agent launch settings, which are a projection of settings rca keeps none of;
//! the native chat session picks, which nothing in rca reads) is not mapped and stays pending.
use agent_api_proto::Method as AgentMethod;
use agent_api_state::SET;
use orca_core::wire::{Failure, METHOD_NOT_FOUND};
use orca_core::{Area, Ctx, Kind, Method};
use serde_json::{json, Value};

mod quick;
mod settings;
mod store;
mod ui;

pub const SETTINGS_GET: &str = "settings.get";
pub const SETTINGS_UPDATE: &str = "settings.update";
pub const SETTINGS_GET_QUICK_COMMANDS: &str = "settings.getTerminalQuickCommands";
pub const SETTINGS_UPDATE_QUICK_COMMANDS: &str = "settings.updateTerminalQuickCommands";
pub const UI_GET: &str = "ui.get";
pub const UI_SET: &str = "ui.set";

/// The agent methods this crate sends (it reads with the port's `state_get`): a flow test holds the list to the methods really sent.
pub const USES: &[AgentMethod] = &[SET];

const METHODS: &[Method] = &[
    Method { name: SETTINGS_GET, kind: Kind::Unary },
    Method { name: SETTINGS_UPDATE, kind: Kind::Unary },
    Method { name: SETTINGS_GET_QUICK_COMMANDS, kind: Kind::Unary },
    Method { name: SETTINGS_UPDATE_QUICK_COMMANDS, kind: Kind::Unary },
    Method { name: UI_GET, kind: Kind::Unary },
    Method { name: UI_SET, kind: Kind::Unary },
];

pub struct StateArea;

impl Area for StateArea {
    fn name(&self) -> &'static str {
        "state"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn call(&self, ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        let agent = ctx.agent;
        match method {
            SETTINGS_GET => Ok(json!({"settings": store::with_defaults(store::SETTINGS, store::read_object(agent, store::SETTINGS)?)})),
            SETTINGS_UPDATE => settings::update(agent, params),
            SETTINGS_GET_QUICK_COMMANDS => Ok(json!({"terminalQuickCommands": quick::list(agent)?})),
            SETTINGS_UPDATE_QUICK_COMMANDS => quick::update(agent, params),
            UI_GET => Ok(json!({"ui": store::with_defaults(store::UI, store::read_object(agent, store::UI)?)})),
            UI_SET => ui::set(agent, params),
            other => Err(Failure::new(METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}
