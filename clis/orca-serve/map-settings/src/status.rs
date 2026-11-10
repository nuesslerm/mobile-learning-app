use crate::{MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION, RUNTIME_PROTOCOL_VERSION};
use orca_core::wire::Failure;
use orca_core::Ctx;
use serde_json::{json, Map, Value};

pub(crate) fn get(ctx: &mut Ctx, params: &Value) -> Value {
    // A liveness probe needs only an answer: the full status is large and the phone sends one on every idle check.
    if params.get("probe") == Some(&Value::Bool(true)) {
        return json!({ "runtimeId": ctx.runtime_id });
    }
    let mut out = Map::new();
    out.insert("runtimeId".into(), ctx.runtime_id.into());
    out.insert("rendererGraphEpoch".into(), 0.into());
    out.insert("graphStatus".into(), "ready".into());
    out.insert("authoritativeWindowId".into(), Value::Null);
    out.insert("liveTabCount".into(), 0.into());
    out.insert("liveLeafCount".into(), 0.into());
    out.insert("runtimeProtocolVersion".into(), RUNTIME_PROTOCOL_VERSION.into());
    out.insert("minCompatibleRuntimeClientVersion".into(), MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION.into());
    out.insert("capabilities".into(), json!(ctx.capabilities));
    // The host answers for itself when rca does not: the phone reads a degradation, never a dead host.
    if let Err(e) = ctx.agent.status() {
        out.insert("degradations".into(), json!([{"code": "agent_unavailable", "capability": "agent", "message": format!("rca's agent API did not answer service.status: {e}")}]));
    }
    out.insert("hostPlatform".into(), ctx.host.platform.as_str().into());
    out.insert("machineName".into(), ctx.host.machine_name.as_str().into());
    out.insert("terminalWindowsShell".into(), Value::Null);
    out.insert("floatingWorkspaceEnabled".into(), true.into());
    out.insert("protocolVersion".into(), RUNTIME_PROTOCOL_VERSION.into());
    out.insert("minCompatibleMobileVersion".into(), MIN_COMPATIBLE_RUNTIME_CLIENT_VERSION.into());
    out.insert("pairedDeviceId".into(), ctx.device_id.into());
    out.insert("appVersion".into(), ctx.host.app_version.as_str().into());
    // The transport stamps the proven scope: a phone is always `mobile` here.
    out.insert("deviceScope".into(), "mobile".into());
    Value::Object(out)
}

/// `{clientCapabilities: string[]}`, nothing else: each entry 1 to 128 characters, at most 64 entries.
pub(crate) fn update_client_capabilities(ctx: &mut Ctx, params: &Value) -> Result<Value, Failure> {
    let invalid = |m: &str| Failure::invalid(m.to_string());
    let object = params.as_object().ok_or_else(|| invalid("Invalid input: expected object"))?;
    if object.keys().any(|k| k != "clientCapabilities") {
        return Err(invalid("Unrecognized key in clientCapabilities update"));
    }
    let list = object.get("clientCapabilities").and_then(Value::as_array).ok_or_else(|| invalid("Invalid input: expected array of clientCapabilities"))?;
    if list.len() > 64 {
        return Err(invalid("Too big: expected array to have at most 64 items"));
    }
    let mut capabilities = Vec::with_capacity(list.len());
    for entry in list {
        match entry.as_str() {
            Some(s) if (1..=128).contains(&s.chars().count()) => capabilities.push(s.to_string()),
            _ => return Err(invalid("Invalid capability: expected a string of 1 to 128 characters")),
        }
    }
    *ctx.client_capabilities = capabilities.clone();
    Ok(json!({ "clientCapabilities": capabilities }))
}
