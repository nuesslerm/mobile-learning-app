//! Statistics over plan manifests joined to dispatch run records. The join is partial by nature (few units carry run ids,
//! fewer resolve), so an unresolved run is reported as unresolved, never as zero cost.
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

const PER_MTOK: f64 = 1_000_000.0;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitManifest {
    pub id: String,
    pub title: String,
    pub tier: Option<String>,
    pub status: Option<String>,
    pub seconds: Option<f64>,
    pub opencode_usd: Option<f64>,
    pub opencode_tokens: Option<f64>,
    pub run_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanManifest {
    pub slug: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub units: Vec<UnitManifest>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tokens {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub total: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DispatchRun {
    pub ts: String,
    pub run_id: String,
    pub model: Option<String>,
    pub pool: Option<String>,
    pub cost: Option<f64>,
    pub duration_ms: Option<f64>,
    pub tokens: Option<Tokens>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: Option<f64>,
}

pub type Pricing = HashMap<String, ModelPricing>;

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key)?.as_f64()
}

impl PlanManifest {
    /// A manifest needs a string slug and a units array; anything else in it is read leniently.
    pub fn from_value(value: &Value) -> Option<Self> {
        let slug = value.get("slug")?.as_str()?.to_string();
        let units = value.get("units")?.as_array()?;
        Some(Self {
            slug,
            started_at: string(value, "startedAt"),
            finished_at: string(value, "finishedAt"),
            units: units
                .iter()
                .map(|unit| UnitManifest {
                    id: string(unit, "id").unwrap_or_default(),
                    title: string(unit, "title").unwrap_or_default(),
                    tier: string(unit, "tier"),
                    status: string(unit, "status"),
                    seconds: number(unit, "seconds"),
                    opencode_usd: number(unit, "opencodeUsd"),
                    opencode_tokens: number(unit, "opencodeTokens"),
                    run_ids: unit
                        .get("runIds")
                        .and_then(Value::as_array)
                        .map(|ids| ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect())
                        .unwrap_or_default(),
                })
                .collect(),
        })
    }
}

impl DispatchRun {
    /// A record needs a string `ts` and `runId`; a torn or foreign line is `None`.
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            ts: value.get("ts")?.as_str()?.to_string(),
            run_id: value.get("runId")?.as_str()?.to_string(),
            model: string(value, "model"),
            pool: string(value, "pool"),
            cost: number(value, "cost"),
            duration_ms: number(value, "durationMs"),
            tokens: value.get("tokens").filter(|tokens| tokens.is_object()).map(|tokens| Tokens {
                input: number(tokens, "input"),
                output: number(tokens, "output"),
                cache_read: number(tokens, "cacheRead"),
                total: number(tokens, "total"),
            }),
        })
    }
}

/// The price table: only entries with numeric input and output rates are kept.
pub fn pricing_from_value(value: &Value) -> Pricing {
    let Some(models) = value.as_object() else { return Pricing::new() };
    models
        .iter()
        .filter_map(|(model, rates)| {
            Some((
                model.clone(),
                ModelPricing {
                    input_per_mtok: number(rates, "inputPerMTok")?,
                    output_per_mtok: number(rates, "outputPerMTok")?,
                    cache_read_per_mtok: number(rates, "cacheReadPerMTok"),
                },
            ))
        })
        .collect()
}

/// List-price cost of a run's token breakdown; `None` when it has no tokens or its model has no price, never 0.
pub fn notional_cost(tokens: Option<&Tokens>, pricing: Option<&ModelPricing>) -> Option<f64> {
    let (tokens, pricing) = (tokens?, pricing?);
    Some(
        (tokens.input.unwrap_or(0.0) * pricing.input_per_mtok
            + tokens.output.unwrap_or(0.0) * pricing.output_per_mtok
            + tokens.cache_read.unwrap_or(0.0) * pricing.cache_read_per_mtok.unwrap_or(0.0))
            / PER_MTOK,
    )
}

/// The UTC date of an RFC 3339 timestamp, or `unknown`.
pub fn day_of(ts: &str) -> String {
    match OffsetDateTime::parse(ts, &Rfc3339) {
        Ok(parsed) => {
            let date = parsed.to_offset(time::UtcOffset::UTC).date();
            format!("{:04}-{:02}-{:02}", date.year(), u8::from(date.month()), date.day())
        }
        Err(_) => "unknown".to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub run_id: String,
    pub ts: String,
    pub day: String,
    pub model: Option<String>,
    pub pool: Option<String>,
    pub cost: f64,
    pub tokens: f64,
    pub duration_ms: f64,
    pub priced: bool,
    pub notional_cost: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnitReport {
    pub id: String,
    pub title: String,
    pub tier: Option<String>,
    pub status: Option<String>,
    pub seconds: Option<f64>,
    pub opencode_usd: Option<f64>,
    pub opencode_tokens: Option<f64>,
    pub run_ids: Vec<String>,
    pub resolved: usize,
    pub unresolved: usize,
    pub runs: Vec<RunReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayTotal {
    pub day: String,
    pub runs: usize,
    pub tokens: f64,
    pub cost: f64,
    pub notional_cost: Option<f64>,
    pub unpriced_runs: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub slug: String,
    pub units: usize,
    pub run_ids: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub unpriced_runs: usize,
    pub days: Vec<DayTotal>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanDetail {
    pub slug: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub units: Vec<UnitReport>,
    pub run_ids: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub unpriced_runs: usize,
    pub days: Vec<DayTotal>,
}

pub fn run_report(run: &DispatchRun, pricing: &Pricing) -> RunReport {
    let model_pricing = run.model.as_ref().and_then(|model| pricing.get(model));
    let notional = notional_cost(run.tokens.as_ref(), model_pricing);
    RunReport {
        run_id: run.run_id.clone(),
        ts: run.ts.clone(),
        day: day_of(&run.ts),
        model: run.model.clone(),
        pool: run.pool.clone(),
        cost: run.cost.unwrap_or(0.0),
        tokens: run.tokens.as_ref().and_then(|tokens| tokens.total).unwrap_or(0.0),
        duration_ms: run.duration_ms.unwrap_or(0.0),
        priced: notional.is_some(),
        notional_cost: notional,
    }
}

fn unit_report(unit: &UnitManifest, runs: &[DispatchRun], pricing: &Pricing) -> UnitReport {
    let by_id: HashMap<&str, &DispatchRun> = runs.iter().map(|run| (run.run_id.as_str(), run)).collect();
    let found: Vec<RunReport> = unit.run_ids.iter().filter_map(|id| by_id.get(id.as_str())).map(|run| run_report(run, pricing)).collect();
    UnitReport {
        id: unit.id.clone(),
        title: unit.title.clone(),
        tier: unit.tier.clone(),
        status: unit.status.clone(),
        seconds: unit.seconds,
        opencode_usd: unit.opencode_usd,
        opencode_tokens: unit.opencode_tokens,
        run_ids: unit.run_ids.clone(),
        resolved: found.len(),
        unresolved: unit.run_ids.len() - found.len(),
        runs: found,
    }
}

fn days(units: &[UnitReport]) -> Vec<DayTotal> {
    let mut by_day: std::collections::BTreeMap<&str, Vec<&RunReport>> = std::collections::BTreeMap::new();
    for run in units.iter().flat_map(|unit| &unit.runs) {
        by_day.entry(run.day.as_str()).or_default().push(run);
    }
    by_day
        .into_iter()
        .map(|(day, runs)| {
            let priced: Vec<f64> = runs.iter().filter_map(|run| run.notional_cost).collect();
            DayTotal {
                day: day.to_string(),
                runs: runs.len(),
                tokens: runs.iter().map(|run| run.tokens).sum(),
                cost: runs.iter().map(|run| run.cost).sum(),
                notional_cost: (!priced.is_empty()).then(|| priced.iter().sum()),
                unpriced_runs: runs.len() - priced.len(),
            }
        })
        .collect()
}

struct Counts {
    run_ids: usize,
    resolved: usize,
    unpriced_runs: usize,
}

fn counts(units: &[UnitReport]) -> Counts {
    Counts {
        run_ids: units.iter().map(|unit| unit.run_ids.len()).sum(),
        resolved: units.iter().map(|unit| unit.resolved).sum(),
        unpriced_runs: units.iter().map(|unit| unit.runs.iter().filter(|run| !run.priced).count()).sum(),
    }
}

pub fn summarize(plan: &PlanManifest, runs: &[DispatchRun], pricing: &Pricing) -> PlanSummary {
    let units: Vec<UnitReport> = plan.units.iter().map(|unit| unit_report(unit, runs, pricing)).collect();
    let totals = counts(&units);
    PlanSummary {
        slug: plan.slug.clone(),
        units: units.len(),
        run_ids: totals.run_ids,
        resolved: totals.resolved,
        unresolved: totals.run_ids - totals.resolved,
        unpriced_runs: totals.unpriced_runs,
        days: days(&units),
    }
}

pub fn detail(plan: &PlanManifest, runs: &[DispatchRun], pricing: &Pricing) -> PlanDetail {
    let units: Vec<UnitReport> = plan.units.iter().map(|unit| unit_report(unit, runs, pricing)).collect();
    let totals = counts(&units);
    PlanDetail {
        slug: plan.slug.clone(),
        started_at: plan.started_at.clone(),
        finished_at: plan.finished_at.clone(),
        run_ids: totals.run_ids,
        resolved: totals.resolved,
        unresolved: totals.run_ids - totals.resolved,
        unpriced_runs: totals.unpriced_runs,
        days: days(&units),
        units,
    }
}
