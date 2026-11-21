//! The limits of Orca's accounts screen from rca's `usage.summary`: a window per provider as the provider's own account answered it, the host's
//! "unavailable" status for a provider nobody could ask, and the error for one that was asked and could not answer. Reading it asks the workspace
//! host's accounts, so the answer is kept for `TTL` and a forced refresh (`refreshUsage`) asks again.
use agent_api_accounts::USAGE_SUMMARY;
use orca_core::AgentApi;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct UsageCache {
    kept: Mutex<Option<(Instant, Value)>>,
}

impl UsageCache {
    /// The `usage.summary` answer, read again when the kept one is older than `TTL` or `force`; null when rca could not answer.
    pub fn summary(&self, agent: &dyn AgentApi, force: bool) -> Value {
        {
            let mut kept = self.kept.lock().unwrap_or_else(|p| p.into_inner());
            match kept.as_ref().filter(|(at, _)| !force && at.elapsed() < TTL) {
                Some((_, summary)) => summary.clone(),
                None => {
                    let fresh = agent.request(USAGE_SUMMARY.name(), json!({})).unwrap_or(Value::Null);
                    *kept = Some((Instant::now(), fresh.clone()));
                    fresh
                }
            }
        }
    }

    /// Forgets the kept answer: a credit was spent or an account changed, so the next read asks again.
    pub fn forget(&self) {
        *self.kept.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

/// What the host says when nothing could be asked of the provider's account.
pub fn unavailable(provider: &str) -> Value {
    json!({"provider": provider, "status": "unavailable", "session": null, "weekly": null, "updatedAt": 0, "error": "rca does not read usage limits"})
}

fn window(raw: &Value) -> Value {
    if !raw.is_object() {
        return Value::Null;
    }
    json!({"usedPercent": raw["used_percent"], "windowMinutes": raw["window_minutes"], "resetsAt": raw["resets_at_ms"], "resetDescription": null})
}

/// The `rateLimits` entry of `provider` (`claude` or `codex`) in a `usage.summary` answer.
pub fn limits_of(summary: &Value, provider: &str) -> Value {
    let name = if provider == "claude" { "claudeAgent" } else { provider };
    let Some(row) = summary["providers"].as_array().and_then(|rows| rows.iter().find(|r| r["provider"] == name)) else { return unavailable(provider) };
    if let Some(error) = row["limits_error"].as_str() {
        return json!({"provider": provider, "status": "error", "session": null, "weekly": null, "updatedAt": 0, "error": error});
    }
    let limits = &row["limits"];
    if !limits.is_object() {
        return unavailable(provider);
    }
    let mut out = json!({"provider": provider, "session": window(&limits["session"]), "weekly": window(&limits["weekly"]), "updatedAt": limits["updated_ms"], "error": null, "status": "ok"});
    if provider == "codex" {
        if let Some(plan) = limits["plan"].as_str() {
            out["planType"] = json!(plan);
        }
        if let Some(credits) = limits["credits"].as_object() {
            let mut shaped = json!({"availableCount": credits["available"]});
            if !credits["total_earned"].is_null() {
                shaped["totalEarnedCount"] = credits["total_earned"].clone();
            }
            shaped["nextExpiresAt"] = credits["next_expires_ms"].clone();
            out["rateLimitResetCredits"] = shaped;
        }
    }
    out
}
