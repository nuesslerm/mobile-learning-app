//! Which model a turn runs (`routes/chat.ts`'s `resolveCompatibleModel`, `use-cases/agentCliSelection.ts`): a requested model that
//! belongs to another CLI is replaced by the configured default, and by nothing when that does not fit either.

/// The default model (`OPENCODE_DEFAULT_MODEL`) as `providerID/modelID`; unset, empty or without a provider means no default.
pub fn default_model(raw: Option<&str>) -> Option<(String, String)> {
    let raw = raw.filter(|v| !v.is_empty())?;
    let slash = raw.find('/').filter(|&i| i >= 1)?;
    Some((raw[..slash].to_string(), raw[slash + 1..].to_string()))
}

/// claude's models carry the provider `anthropic`; no other CLI's do.
pub fn compatible(cli: &str, provider_id: &str) -> bool {
    if cli == "claude" {
        provider_id == "anthropic"
    } else {
        provider_id != "anthropic"
    }
}

pub fn resolve_compatible(cli: &str, requested: Option<(String, String)>, default: Option<&str>) -> Option<(String, String)> {
    if let Some(model) = requested.filter(|(provider, _)| compatible(cli, provider)) {
        return Some(model);
    }
    default_model(default).filter(|(provider, _)| compatible(cli, provider))
}
