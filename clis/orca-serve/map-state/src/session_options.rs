//! `settings.mutateNativeChatSessionOptions`: the model a person picked for an agent becomes that agent's default for new chats, kept under
//! `nativeChatSessionOptions` of the settings. A new chat on that agent runs on it unless the launch or send names a model (`map-agent`).
use crate::store::{self, SETTINGS};
use orca_core::wire::{received, Failure};
use orca_core::AgentApi;
use serde_json::{json, Value};

fn known_agent(agent: &str) -> bool {
    const DEFAULTS: &str = include_str!("defaults.json");
    serde_json::from_str::<Value>(DEFAULTS).is_ok_and(|d| d["settings"]["agentDefaultArgs"].get(agent).is_some())
}

fn text<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, Failure> {
    v[key].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= max).ok_or_else(|| Failure::invalid(format!("Invalid input: {key} must be a string of 1 to {max} characters")))
}

pub(crate) fn mutate(agent: &dyn AgentApi, params: &Value) -> Result<Value, Failure> {
    if !params.is_object() {
        return Err(Failure::invalid(if params.is_null() { "Invalid discriminator value. Expected 'apply-picks' | 'clear-model-if-missing'".to_string() } else { format!("Invalid input: expected object, received {}", received(params)) }));
    }
    if !matches!(params["type"].as_str(), Some("apply-picks" | "clear-model-if-missing")) {
        return Err(Failure::invalid("Invalid discriminator value. Expected 'apply-picks' | 'clear-model-if-missing'"));
    }
    let chat_agent = text(params, "agent", 512).map_err(|_| Failure::invalid("Invalid input: expected string, received undefined"))?;
    if !known_agent(chat_agent) {
        return Err(Failure::invalid("Unknown agent"));
    }
    match params["type"].as_str() {
        Some("apply-picks") => {
            let picks = params["picks"].as_array().filter(|p| (1..=8).contains(&p.len())).ok_or_else(|| Failure::invalid("Invalid input: picks must hold 1 to 8 picks"))?;
            let mut settings = store::read_object(agent, SETTINGS)?;
            let mut all = settings.get("nativeChatSessionOptions").and_then(Value::as_object).cloned().unwrap_or_default();
            let mut mine = all.get(chat_agent).and_then(Value::as_object).cloned().unwrap_or_default();
            for pick in picks {
                let (model, option) = (text(pick, "modelId", 512)?, text(pick, "optionId", 512)?);
                let value = pick.get("value").filter(|v| v.is_string() || v.is_boolean() || v.is_number()).ok_or_else(|| Failure::invalid("Invalid input: value must be a string, boolean or number"))?;
                let mut by_model = mine.get("valuesByModel").and_then(Value::as_object).cloned().unwrap_or_default();
                if option != "model" {
                    let mut values = by_model.get(model).and_then(Value::as_object).cloned().unwrap_or_default();
                    values.insert(option.into(), value.clone());
                    by_model.insert(model.into(), Value::Object(values));
                }
                mine.insert("valuesByModel".into(), Value::Object(by_model));
                if pick["adoptModelAsLaunchDefault"] != false {
                    let adopted = if option == "model" { value.as_str().map_or_else(|| value.to_string(), str::to_string) } else { model.to_string() };
                    mine.insert("model".into(), json!(adopted));
                }
            }
            all.insert(chat_agent.into(), Value::Object(mine));
            settings.insert("nativeChatSessionOptions".into(), Value::Object(all));
            store::write(agent, SETTINGS, Value::Object(settings))?;
        }
        // The host ignores it too: an observation of the catalog cannot change a saved pick.
        Some("clear-model-if-missing") => {
            params["availableModelIds"].as_array().filter(|a| (1..=256).contains(&a.len())).ok_or_else(|| Failure::invalid("Invalid input: availableModelIds must hold 1 to 256 ids"))?;
        }
        _ => return Err(Failure::invalid("Invalid discriminator value. Expected 'apply-picks' | 'clear-model-if-missing'")),
    }
    Ok(json!({"ok": true}))
}
