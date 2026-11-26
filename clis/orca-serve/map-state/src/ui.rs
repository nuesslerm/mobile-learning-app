//! `ui.get`, `ui.set`: the phone's UI state, kept as Orca keeps it (`ui_state`) and updated through the schema of `ui_params`.
use crate::store::{self, UI};
use crate::{ui_params, ui_state};
use orca_core::wire::{Failure, RUNTIME_ERROR};
use orca_core::{AgentApi, AgentError};
use serde_json::{json, Value};

pub(crate) fn get(agent: &dyn AgentApi) -> Result<Value, Failure> {
    Ok(json!({"ui": ui_state::project(&store::read_object(agent, UI)?)}))
}

pub(crate) fn set(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let updates = ui_params::parse(params)?;
    let _turn = orca_core::kept_state_turn();
    let mut current = store::kept(agent, UI)?.unwrap_or(Value::Null);
    for _ in 0..8 {
        let kept = match &current {
            Value::Null => Default::default(),
            Value::Object(fields) => fields.clone(),
            _ => return Err(Failure::new(RUNTIME_ERROR, "the value kept for ui is not an object")),
        };
        let next = ui_state::apply(&kept, &updates);
        if next != kept {
            let params = json!({"scope":store::SCOPE, "key":UI, "expected":current, "value":next});
            match agent.request(agent_api_state::COMPARE_AND_SET.name(), params) {
                Ok(reply) if reply["applied"] == false && reply.get("value").is_some() => {
                    current = reply["value"].clone();
                    continue;
                }
                Ok(reply) if reply["applied"] == true => {}
                Ok(_) => return Err(Failure::new(RUNTIME_ERROR, "invalid conditional state write reply")),
                Err(AgentError::Refused { code, .. }) if code == "UNKNOWN_METHOD" => {
                    store::write(agent, UI, Value::Object(next.clone()))?;
                }
                Err(error) => return Err(store::refusal(error)),
            }
        }
        return Ok(json!({"ui": ui_state::project(&next)}));
    }
    Err(Failure::new(RUNTIME_ERROR, "UI state changed while saving"))
}
