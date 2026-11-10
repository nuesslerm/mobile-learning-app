//! Failure classification and rung selection (`plugins/ladder/ladder.ts`).
use rca_core::stats::ShapeStats;
use rca_core::text::has_word;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const MODEL_LADDERS_JSON: &str =
    include_str!("../data/model-ladders.json");

const STATS_MIN_RUNS: f64 = 5.0;
const STATS_MIN_OK_RATE: f64 = 0.8;

#[derive(Clone, Debug, Default, Deserialize)]
pub struct PoolConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default, rename = "resetHint")]
    pub reset_hint: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ModelConfig {
    pub pool: String,
    pub strength: f64,
    pub automatic: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LadderFile {
    pub pools: HashMap<String, PoolConfig>,
    pub models: HashMap<String, ModelConfig>,
    pub ladders: HashMap<String, Vec<String>>,
}

/// The ladders the repository ships, from the single source file the TypeScript also imports.
pub fn model_ladders() -> LadderFile {
    serde_json::from_str(MODEL_LADDERS_JSON).expect("model-ladders.json is valid")
}

pub fn looks_model_missing(output: &str) -> bool {
    output.contains("ProviderModelNotFoundError") || output.to_lowercase().contains("model not found:")
}

fn looks_fatal(output: &str) -> bool {
    let lower = output.to_lowercase();
    has_word(output, "401")
        || lower.contains("unauthorized")
        || lower.contains("invalid api key")
        || lower.contains("cannot be used with root/sudo privileges")
        || (has_word(output, "ENOENT") && has_word(&lower, "config"))
}

/// `reason` is one of ok, capped, crashed, timedOut, other.
pub fn classify_failure(reason: &str, output: &str, wrote_nothing: bool) -> &'static str {
    match reason {
        "capped" => return "cap",
        "crashed" => return "crash",
        "timedOut" => return "timeout",
        _ => {}
    }
    if looks_model_missing(output) {
        "model-missing"
    } else if looks_fatal(output) {
        "fatal"
    } else if wrote_nothing {
        "wrote-nothing"
    } else {
        "bad-output"
    }
}

pub fn should_escalate(previous: Option<&str>, current: &str) -> bool {
    current != "model-missing" && previous == Some(current)
}

pub fn pool_of<'a>(ladders: &'a LadderFile, model: &str) -> &'a str {
    ladders.models.get(model).map_or("", |m| m.pool.as_str())
}

pub fn resolve_ladder(
    shape: &str,
    ladders: &LadderFile,
    cold: &HashSet<String>,
    stats: Option<&ShapeStats>,
    pins: Option<&[String]>,
) -> Vec<String> {
    if let Some(pins) = pins.filter(|p| !p.is_empty()) {
        return pins
            .iter()
            .filter(|m| !cold.contains(pool_of(ladders, m)))
            .cloned()
            .collect();
    }
    let Some(ladder) = ladders.ladders.get(shape) else {
        return Vec::new();
    };
    let available: Vec<String> = ladder
        .iter()
        .filter(|m| {
            let info = ladders.models.get(m.as_str());
            let pool = info.map_or("", |i| i.pool.as_str());
            if info.is_some_and(|i| !i.automatic) {
                return false;
            }
            if ladders.pools.get(pool).and_then(|p| p.enabled) == Some(false) {
                return false;
            }
            !cold.contains(pool)
        })
        .cloned()
        .collect();
    if let Some(stats) = stats {
        let first_proven = available.iter().position(|m| {
            stats
                .get(m)
                .is_some_and(|s| s.runs >= STATS_MIN_RUNS && s.ok_rate >= STATS_MIN_OK_RATE)
        });
        if let Some(i) = first_proven {
            return available[i..].to_vec();
        }
    }
    available
}

fn strength_at(ladders: &LadderFile, ladder: &[String], i: usize) -> f64 {
    ladder
        .get(i)
        .and_then(|m| ladders.models.get(m))
        .map_or(0.0, |m| m.strength)
}

/// Two identical failure classes in a row skip every rung not strictly stronger than the failed one.
pub fn next_rung(
    ladders: &LadderFile,
    ladder: &[String],
    from: usize,
    current: &str,
    previous: Option<&str>,
) -> (usize, bool) {
    if !should_escalate(previous, current) {
        return (from + 1, false);
    }
    let strength = strength_at(ladders, ladder, from);
    let mut j = from + 1;
    while j < ladder.len() && strength_at(ladders, ladder, j) <= strength {
        j += 1;
    }
    (j, true)
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    pub ok: bool,
    #[serde(default)]
    pub failure_class: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Attempt {
    #[serde(default)]
    pub skipped: bool,
    #[serde(default)]
    pub verdict: Option<Verdict>,
}

/// Replays the attempts so far over the resolved ladder; `None` when the run is over.
pub fn next_rung_index(ladders: &LadderFile, ladder: &[String], tried: &[Attempt]) -> Option<usize> {
    let mut i = 0;
    let mut previous: Option<String> = None;
    for attempt in tried {
        let Some(verdict) = attempt.verdict.as_ref().filter(|_| !attempt.skipped) else {
            i += 1;
            continue;
        };
        if verdict.ok {
            return None;
        }
        let current = verdict.failure_class.clone().unwrap_or_default();
        if current == "fatal" {
            return None;
        }
        i = next_rung(ladders, ladder, i, &current, previous.as_deref()).0;
        previous = Some(current);
    }
    (i < ladder.len()).then_some(i)
}
