//! The per-pool cooldown ledger (`plugins/cooldowns/poolLedger.ts` and `nextLedger`). Entries serialize with the
//! TypeScript field names and omit absent fields, so the file is interchangeable. Pools are kept sorted, where the
//! TypeScript keeps insertion order: a cosmetic difference in the written file only.
use rca_core::time::{iso, next_utc_midnight, parse};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const INITIAL_RESET_MS: i64 = 5 * 60 * 60 * 1000;
pub const TIMEOUT_COLD_MS: i64 = 30 * 60 * 1000;
const TIMEOUT_WINDOW_MS: i64 = 30 * 60 * 1000;
pub const INSTANT_CRASH_MS: f64 = 5_000.0;
const CLAUDE_POOL: &str = "claude-max";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub cold_until: String,
    pub learned_reset_ms: i64,
    pub last_cap_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_timeout_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cold_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consecutive_timeouts: Option<i64>,
}

pub type Ledger = BTreeMap<String, Entry>;

fn is_cold_at(entry: &Entry, at: i64) -> bool {
    parse(&entry.cold_until).is_some_and(|until| at < until)
}

/// First cap: 5 h; cap while cold: keep the window; cap after recovery: grow it 25 %.
pub fn mark_cold(ledger: &Ledger, pool: &str, at: i64) -> Ledger {
    let learned = match ledger.get(pool) {
        None => INITIAL_RESET_MS,
        Some(e) if is_cold_at(e, at) => e.learned_reset_ms,
        // Math.round: halves go up.
        Some(e) => ((e.learned_reset_ms as f64 * 1.25) + 0.5).floor() as i64,
    };
    let mut next = ledger.clone();
    next.insert(
        pool.to_string(),
        Entry {
            cold_until: iso(at + learned),
            learned_reset_ms: learned,
            last_cap_at: iso(at),
            last_timeout_at: None,
            cold_source: Some("cap".into()),
            consecutive_timeouts: None,
        },
    );
    next
}

pub fn observe_ok(ledger: &Ledger, pool: &str, at: i64) -> Ledger {
    let Some(existing) = ledger.get(pool) else {
        return ledger.clone();
    };
    let cold = is_cold_at(existing, at);
    let mut next = ledger.clone();
    if cold && existing.cold_source.as_deref() == Some("timeout") {
        // Never a cap, so there is no outage length to learn from.
        next.insert(
            pool.to_string(),
            Entry {
                cold_until: iso(at),
                cold_source: None,
                last_timeout_at: None,
                consecutive_timeouts: None,
                ..existing.clone()
            },
        );
    } else if cold {
        let last_cap = parse(&existing.last_cap_at).unwrap_or(at);
        next.insert(
            pool.to_string(),
            Entry {
                cold_until: iso(at),
                learned_reset_ms: (at - last_cap).max(0),
                last_cap_at: existing.last_cap_at.clone(),
                last_timeout_at: None,
                cold_source: None,
                consecutive_timeouts: None,
            },
        );
    } else if existing.last_timeout_at.is_some() || existing.consecutive_timeouts.is_some() {
        next.insert(
            pool.to_string(),
            Entry {
                last_timeout_at: None,
                consecutive_timeouts: None,
                ..existing.clone()
            },
        );
    }
    next
}

pub fn cold_window_for_reset_hint(reset_hint: Option<&str>, now: i64) -> i64 {
    if reset_hint != Some("daily") {
        return INITIAL_RESET_MS;
    }
    (next_utc_midnight(now) - now).max(0)
}

/// Two timeouts within 30 min cool the pool for min(30 min, its resetHint window).
pub fn record_timeout(ledger: &Ledger, pool: &str, at: i64, reset_hint: Option<&str>) -> Ledger {
    let existing = ledger.get(pool);
    let mut count = 1;
    if let Some(last) = existing.and_then(|e| e.last_timeout_at.as_deref()).and_then(parse) {
        if at - last <= TIMEOUT_WINDOW_MS {
            count = existing.and_then(|e| e.consecutive_timeouts).unwrap_or(0) + 1;
        }
    }
    let last_timeout_at = Some(iso(at));
    let mut next = ledger.clone();
    let entry = if count < 2 {
        let base = existing.cloned().unwrap_or(Entry {
            cold_until: iso(0),
            learned_reset_ms: INITIAL_RESET_MS,
            last_cap_at: iso(0),
            last_timeout_at: None,
            cold_source: None,
            consecutive_timeouts: None,
        });
        Entry {
            last_timeout_at,
            consecutive_timeouts: Some(count),
            ..base
        }
    } else {
        let cold_ms = TIMEOUT_COLD_MS.min(cold_window_for_reset_hint(reset_hint, at));
        Entry {
            cold_until: iso(at + cold_ms),
            learned_reset_ms: existing.map_or(INITIAL_RESET_MS, |e| e.learned_reset_ms),
            last_cap_at: existing.map_or_else(|| iso(at), |e| e.last_cap_at.clone()),
            last_timeout_at,
            cold_source: Some("timeout".into()),
            consecutive_timeouts: Some(0),
        }
    };
    next.insert(pool.to_string(), entry);
    next
}

pub fn clear_cooldown(ledger: &Ledger, pool: &str, at: i64) -> Ledger {
    let Some(existing) = ledger.get(pool) else {
        return ledger.clone();
    };
    let mut next = ledger.clone();
    next.insert(
        pool.to_string(),
        Entry {
            cold_until: iso(at),
            cold_source: None,
            last_timeout_at: None,
            consecutive_timeouts: None,
            ..existing.clone()
        },
    );
    next
}

pub fn cold_pools(ledger: &Ledger, now: i64) -> BTreeSet<String> {
    ledger
        .iter()
        .filter(|(_, e)| is_cold_at(e, now))
        .map(|(p, _)| p.clone())
        .collect()
}

/// What one finished attempt teaches the ledger (`nextLedger`). `failure_class` is `None` for an ok verdict.
pub fn next_ledger(
    ledger: &Ledger,
    pool: &str,
    failure_class: Option<&str>,
    at: i64,
    attempt_ms: f64,
    reset_hint: Option<&str>,
) -> Ledger {
    let Some(class) = failure_class else {
        return observe_ok(ledger, pool, at);
    };
    match class {
        "cap" => mark_cold(ledger, pool, at),
        "timeout" => record_timeout(ledger, pool, at, reset_hint),
        // A failure this fast never reached a model (dead binary, missing credential); the claude-max rung
        // counts any non-fatal one.
        _ if attempt_ms < INSTANT_CRASH_MS && (class == "crash" || (pool == CLAUDE_POOL && class != "fatal")) => {
            record_timeout(ledger, pool, at, reset_hint)
        }
        _ => ledger.clone(),
    }
}

/// A missing or malformed file reads as empty; an entry that does not fit is dropped.
pub fn parse_ledger(text: &str) -> Ledger {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str(text) else {
        return Ledger::new();
    };
    map.into_iter()
        .filter_map(|(pool, v)| serde_json::from_value(v).ok().map(|e| (pool, e)))
        .collect()
}

pub fn serialize_ledger(ledger: &Ledger) -> String {
    format!("{}\n", serde_json::to_string_pretty(ledger).expect("ledger serializes"))
}
