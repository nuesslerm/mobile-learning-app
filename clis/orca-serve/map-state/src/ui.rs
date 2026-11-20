//! `ui.get`, `ui.set`: the phone's UI state, kept as Orca keeps it (`ui_state`) and updated through the schema of `ui_params`.
use crate::store::{self, UI};
use crate::{ui_params, ui_state};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

pub(crate) fn get(agent: &dyn AgentApi) -> Result<Value, Failure> {
    Ok(json!({"ui": ui_state::project(&store::read_object(agent, UI)?)}))
}

pub(crate) fn set(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let updates = ui_params::parse(params)?;
    let _turn = orca_core::kept_state_turn();
    let kept = store::read_object(agent, UI)?;
    let next = ui_state::apply(&kept, &updates);
    if next != kept {
        store::write(agent, UI, Value::Object(next.clone()))?;
    }
    Ok(json!({"ui": ui_state::project(&next)}))
}
