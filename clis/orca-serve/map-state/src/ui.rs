//! `ui.get` and `ui.set`: the phone's UI state is the object of the keys it has set, merged key by key.
use crate::store::{self, UI};
use orca_core::wire::Failure;
use orca_core::AgentApi;
use serde_json::{json, Value};

pub(crate) fn set(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    let updates = params.as_object().ok_or_else(|| Failure::invalid("ui.set takes an object of the values to set"))?;
    let mut ui = store::read_object(agent, UI)?;
    ui.extend(updates.iter().map(|(k, v)| (k.clone(), v.clone())));
    store::write(agent, UI, Value::Object(ui.clone()))?;
    Ok(json!({"ui": store::with_defaults(UI, ui)}))
}
