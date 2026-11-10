//! Run statistics and the `wf offload ask --stats` renderings (`plugins/ledger/stats.ts`).
use crate::run_log::{RunLogEntry, RunTokens};
use crate::time::parse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub runs: f64,
    pub ok_rate: f64,
}

pub type ShapeStats = HashMap<String, Stat>;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub input_per_m_tok: f64,
    pub output_per_m_tok: f64,
    pub cache_read_per_m_tok: Option<f64>,
    pub cache_write_per_m_tok: Option<f64>,
}

pub type PricingTable = HashMap<String, ModelPricing>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunGroupStats {
    pub key: String,
    pub runs: usize,
    pub ok: usize,
    pub wrote_nothing: usize,
    pub p50_duration_ms: f64,
    pub total_tokens: f64,
    pub total_cost: f64,
    pub total_notional_cost: Option<f64>,
    pub unpriced_runs: usize,
    pub output_chars: f64,
    pub summary_chars: f64,
    pub rotations: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStats {
    pub total: RunGroupStats,
    pub by_caller: Vec<RunGroupStats>,
    pub by_model: Vec<RunGroupStats>,
    pub by_session: Vec<RunGroupStats>,
    pub by_shape_model: Vec<RunGroupStats>,
}

pub fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let i = ((p / 100.0 * sorted.len() as f64).floor() as usize).min(sorted.len() - 1);
    sorted[i]
}

/// `None` when the run has no tokens or its model has no price: zero would read as a free model.
pub fn notional_cost(tokens: Option<&RunTokens>, pricing: Option<&ModelPricing>) -> Option<f64> {
    let (t, p) = (tokens?, pricing?);
    Some(
        (t.input * p.input_per_m_tok
            + t.output * p.output_per_m_tok
            + t.cache_read * p.cache_read_per_m_tok.unwrap_or(0.0)
            + t.cache_write * p.cache_write_per_m_tok.unwrap_or(0.0))
            / 1_000_000.0,
    )
}

fn summarize_group(key: &str, rows: &[&RunLogEntry], pricing: Option<&PricingTable>) -> RunGroupStats {
    let mut notional_sum = 0.0;
    let mut priced = 0;
    for r in rows {
        let price = pricing.and_then(|p| p.get(r.model.as_deref().unwrap_or("")));
        if let Some(n) = notional_cost(r.tokens.as_ref(), price) {
            notional_sum += n;
            priced += 1;
        }
    }
    let sum = |pick: &dyn Fn(&RunLogEntry) -> f64| rows.iter().map(|r| pick(r)).sum::<f64>();
    RunGroupStats {
        key: key.to_string(),
        runs: rows.len(),
        ok: rows.iter().filter(|r| r.ok).count(),
        wrote_nothing: rows.iter().filter(|r| r.wrote_nothing).count(),
        p50_duration_ms: percentile(&rows.iter().map(|r| r.duration_ms).collect::<Vec<_>>(), 50.0),
        total_tokens: sum(&|r| r.tokens.as_ref().map_or(0.0, |t| t.total)),
        total_cost: sum(&|r| r.cost.unwrap_or(0.0)),
        total_notional_cost: (priced > 0).then_some(notional_sum),
        unpriced_runs: rows.len() - priced,
        output_chars: sum(&|r| r.output_chars),
        summary_chars: sum(&|r| r.summary_chars),
        rotations: rows.iter().map(|r| r.attempts.len().saturating_sub(1)).sum(),
    }
}

fn group_by(rows: &[RunLogEntry], key_of: &dyn Fn(&RunLogEntry) -> String, pricing: Option<&PricingTable>) -> Vec<RunGroupStats> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&RunLogEntry>> = HashMap::new();
    for r in rows {
        let k = key_of(r);
        if !groups.contains_key(&k) {
            order.push(k.clone());
        }
        groups.entry(k).or_default().push(r);
    }
    let mut out: Vec<RunGroupStats> = order.iter().map(|k| summarize_group(k, &groups[k], pricing)).collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.runs)); // stable, like the TypeScript's sort
    out
}

/// `pricing` maps ladder model id to per-MTok rates; without it every run is unpriced.
pub fn stats(records: &[RunLogEntry], pricing: Option<&PricingTable>) -> RunStats {
    let all: Vec<&RunLogEntry> = records.iter().collect();
    RunStats {
        total: summarize_group("all", &all, pricing),
        by_caller: group_by(records, &|r| r.caller.clone(), pricing),
        by_model: group_by(records, &|r| r.model.clone().unwrap_or_else(|| "(failed)".into()), pricing),
        by_session: group_by(
            records,
            &|r| r.claude_session_id.clone().unwrap_or_else(|| "(no session id)".into()),
            pricing,
        ),
        by_shape_model: group_by(
            records,
            &|r| {
                [
                    r.shape.as_deref().unwrap_or("unknown"),
                    r.tier.as_deref().unwrap_or("unknown"),
                    r.model.as_deref().unwrap_or("(failed)"),
                    r.pool.as_deref().unwrap_or("unknown"),
                ]
                .join("|")
            },
            pricing,
        ),
    }
}

#[derive(Clone, Debug, Default)]
pub struct StatsFilter {
    pub since_ms: Option<i64>,
    pub claude_session_id: Option<String>,
    pub caller: Option<String>,
}

pub fn filter_runs(rows: &[RunLogEntry], filter: &StatsFilter) -> Vec<RunLogEntry> {
    rows.iter()
        .filter(|r| {
            if let Some(since) = filter.since_ms {
                // A timestamp that does not parse compares as NaN in JavaScript: never older than `since`.
                if parse(&r.ts).is_some_and(|t| t < since) {
                    return false;
                }
            }
            if filter.claude_session_id.as_ref().is_some_and(|s| r.claude_session_id.as_ref() != Some(s)) {
                return false;
            }
            !filter.caller.as_ref().is_some_and(|c| &r.caller != c)
        })
        .cloned()
        .collect()
}

/// `7d`, `24h`, `90m`, or an ISO date or timestamp; `None` for anything else.
pub fn parse_since(spec: Option<&str>, now_ms: i64) -> Option<i64> {
    let spec = spec.filter(|s| !s.is_empty())?;
    let trimmed = spec.trim();
    let digits = trimmed.len().saturating_sub(1);
    if let (Some(unit), true) = (trimmed.chars().last(), trimmed.is_char_boundary(digits)) {
        let n = &trimmed[..digits];
        if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && matches!(unit, 'd' | 'h' | 'm') {
            let unit_ms = match unit {
                'd' => 86_400_000,
                'h' => 3_600_000,
                _ => 60_000,
            };
            return Some(now_ms - n.parse::<i64>().ok()? * unit_ms);
        }
    }
    // A bare date is midnight UTC, as in JavaScript.
    parse(&if spec.contains('T') { spec.to_string() } else { format!("{spec}T00:00:00Z") })
}

pub(crate) fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// `Number.prototype.toFixed(digits)`: rounds on the exact decimal expansion, ties up, keeps the sign of a negative that
/// rounds to zero.
pub fn to_fixed(x: f64, digits: usize) -> String {
    let exact = format!("{:.80}", x.abs());
    let (int, frac) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut n: u128 = format!("{int}{}", &frac[..digits]).parse().unwrap_or(0);
    if frac.as_bytes().get(digits).is_some_and(|d| *d >= b'5') {
        n += 1;
    }
    let sign = if x < 0.0 { "-" } else { "" };
    if digits == 0 {
        return format!("{sign}{n}");
    }
    let scale = 10u128.pow(digits as u32);
    format!("{sign}{}.{:0width$}", n / scale, n % scale, width = digits)
}

pub(crate) fn to_fixed3(x: f64) -> String {
    to_fixed(x, 3)
}

pub(crate) fn js_num(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e21 {
        format!("{}", x as i128)
    } else {
        format!("{x}")
    }
}

pub(crate) fn pad_start(s: &str, w: usize) -> String {
    format!("{}{s}", " ".repeat(w.saturating_sub(s.encode_utf16().count())))
}

pub(crate) fn pad_end(s: &str, w: usize) -> String {
    format!("{s}{}", " ".repeat(w.saturating_sub(s.encode_utf16().count())))
}

fn fmt_group(g: &RunGroupStats) -> String {
    let rate = if g.runs > 0 { js_round(100.0 * g.ok as f64 / g.runs as f64) } else { 0.0 };
    let saved = if g.output_chars > 0.0 {
        format!("{}%", js_num(js_round(100.0 * (g.output_chars - g.summary_chars) / g.output_chars)))
    } else {
        "n/a".to_string()
    };
    [
        pad_end(&g.key, 40),
        pad_start(&g.runs.to_string(), 5),
        pad_start(&format!("{}%", js_num(rate)), 6),
        pad_start(&g.wrote_nothing.to_string(), 6),
        pad_start(&format!("{}s", js_num(js_round(g.p50_duration_ms / 1000.0))), 7),
        pad_start(&js_num(g.total_tokens), 9),
        pad_start(&format!("${}", to_fixed3(g.total_cost)), 9),
        pad_start(&g.rotations.to_string(), 5),
        pad_start(&saved, 8),
    ]
    .join(" ")
}

/// The `wf offload ask --stats` table.
pub fn format_stats(s: &RunStats) -> String {
    let header = [
        pad_end("group", 40),
        pad_start("runs", 5),
        pad_start("ok", 6),
        pad_start("wrote0", 6),
        pad_start("p50", 7),
        pad_start("tokens", 9),
        pad_start("cost", 9),
        pad_start("rot", 5),
        pad_start("ctx-cut", 8),
    ]
    .join(" ");
    let mut out = vec![header, fmt_group(&s.total), String::new()];
    for (title, groups) in [
        ("by caller", &s.by_caller),
        ("by model", &s.by_model),
        ("by Claude session", &s.by_session),
        ("by shape|tier|model|pool", &s.by_shape_model),
    ] {
        if groups.is_empty() {
            continue;
        }
        out.push(format!("# {title}"));
        out.extend(groups.iter().map(fmt_group));
        out.push(String::new());
    }
    out.push("ctx-cut = share of dispatched output a --summary caller never had to read (outputChars vs summaryChars).".into());
    out.join("\n")
}

/// Per-model outcome for one shape, from final-outcome rows only: the ladder's start-rung input.
pub fn shape_stats(records: &[RunLogEntry], shape: &str) -> ShapeStats {
    let mut acc: HashMap<&str, (f64, f64)> = HashMap::new();
    for r in records {
        let Some(model) = r.model.as_deref().filter(|m| !m.is_empty()) else { continue };
        if r.shape.as_deref() != Some(shape) {
            continue;
        }
        let e = acc.entry(model).or_default();
        e.0 += 1.0;
        if r.ok {
            e.1 += 1.0;
        }
    }
    acc.into_iter()
        .map(|(m, (runs, ok))| (m.to_string(), Stat { runs, ok_rate: if runs > 0.0 { ok / runs } else { 0.0 } }))
        .collect()
}

/// `JSON.stringify(value, null, 2)`: two-space indent, `[]` and `{}` for empty containers, numbers as JavaScript
/// prints them (`1`, not `1.0`).
pub fn js_json_pretty(v: &serde_json::Value) -> String {
    fn go(v: &serde_json::Value, depth: usize, out: &mut String) {
        use serde_json::Value::*;
        let pad = |n: usize| "  ".repeat(n);
        match v {
            Null => out.push_str("null"),
            Bool(b) => out.push_str(&b.to_string()),
            Number(n) => out.push_str(&n.as_f64().map_or_else(|| n.to_string(), js_num)),
            String(s) => out.push_str(&serde_json::to_string(s).expect("string")),
            Array(a) if a.is_empty() => out.push_str("[]"),
            Array(a) => {
                out.push_str("[\n");
                for (i, x) in a.iter().enumerate() {
                    out.push_str(&pad(depth + 1));
                    go(x, depth + 1, out);
                    out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(depth));
                out.push(']');
            }
            Object(o) if o.is_empty() => out.push_str("{}"),
            Object(o) => {
                out.push_str("{\n");
                for (i, (k, x)) in o.iter().enumerate() {
                    out.push_str(&pad(depth + 1));
                    out.push_str(&serde_json::to_string(k).expect("string"));
                    out.push_str(": ");
                    go(x, depth + 1, out);
                    out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
                }
                out.push_str(&pad(depth));
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    go(v, 0, &mut out);
    out
}
