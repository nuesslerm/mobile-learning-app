//! `wf offload ask --savings` (`use-cases/dispatch/savings.ts`): what delegating to the cheap models avoided.
use crate::run_log::RunLogEntry;
use crate::stats::{js_num, js_round, pad_end, pad_start, to_fixed3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeRatio {
    pub claude_usd: f64,
    pub claude_seconds: f64,
    pub n: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptCharsBuckets {
    pub small: f64,
    pub medium: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sizes {
    pub small: SizeRatio,
    pub medium: SizeRatio,
    pub large: SizeRatio,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavingsRatios {
    pub source: String,
    pub measured_at: String,
    pub prompt_chars_buckets: PromptCharsBuckets,
    pub sizes: Sizes,
}

/// The 2026-09-02 delegation benchmark (B1, arm A Sonnet), the table shipped as the default.
pub fn default_ratios() -> SavingsRatios {
    SavingsRatios {
        source: "docs/audits/2026-09-02-cheap-model-delegation-report.md, benchmark B1 (arm A Sonnet, gate-passing runs, 7 tasks x n=3)".into(),
        measured_at: "2026-09-03".into(),
        prompt_chars_buckets: PromptCharsBuckets { small: 300.0, medium: 1200.0 },
        sizes: Sizes {
            small: SizeRatio { claude_usd: 0.1595, claude_seconds: 42.3, n: 9.0 },
            medium: SizeRatio { claude_usd: 0.2495, claude_seconds: 71.7, n: 9.0 },
            large: SizeRatio { claude_usd: 0.6167, claude_seconds: 193.3, n: 3.0 },
        },
    }
}

/// Callers whose runs stand in for work Claude would otherwise have done.
const DELEGATION_CALLERS: [&str; 4] = ["dispatch", "mcp", "cheap", "batch"];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationReportRow {
    pub ts: String,
    pub session_id: Option<String>,
    pub delegations: f64,
    pub direct_work: f64,
}

/// Rows are cumulative per session; the last one per session id wins, in first-seen order. Torn lines and rows
/// without a string `ts` are skipped.
pub fn parse_delegation_report(text: &str) -> Vec<DelegationReportRow> {
    let mut rows: Vec<(String, DelegationReportRow)> = Vec::new();
    let mut anonymous = 0;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Ok(serde_json::Value::Object(o)) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        let Some(ts) = o.get("ts").and_then(|v| v.as_str()) else { continue };
        let session_id = o.get("sessionId").and_then(|v| v.as_str()).map(String::from);
        let key = session_id.clone().unwrap_or_else(|| {
            anonymous += 1;
            format!("anon-{}", anonymous - 1)
        });
        let num = |k: &str| o.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0);
        let row = DelegationReportRow { ts: ts.into(), session_id, delegations: num("delegations"), direct_work: num("directWork") };
        match rows.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = row,
            None => rows.push((key, row)),
        }
    }
    rows.into_iter().map(|(_, r)| r).collect()
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeSavings {
    pub size: &'static str,
    pub runs: usize,
    pub opencode_usd: f64,
    pub opencode_seconds: f64,
    pub est_claude_usd: f64,
    pub est_claude_seconds: f64,
    pub borrowed_ratio: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RatiosInfo {
    pub source: String,
    #[serde(rename = "measuredAt")]
    pub measured_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionsInfo {
    pub count: usize,
    pub delegations: f64,
    pub direct_work: f64,
    pub delegation_share: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Savings {
    pub ratios: RatiosInfo,
    pub runs: usize,
    pub failed_runs: usize,
    pub unpriced_runs: usize,
    pub opencode_usd: f64,
    pub opencode_seconds: f64,
    pub est_claude_usd: f64,
    pub est_claude_seconds: f64,
    pub saved_usd: f64,
    pub saved_pct: f64,
    pub extra_seconds: f64,
    pub by_size: Vec<SizeSavings>,
    pub sessions: Option<SessionsInfo>,
}

fn size_of_prompt(chars: f64, r: &SavingsRatios) -> usize {
    if chars < r.prompt_chars_buckets.small {
        0
    } else if chars < r.prompt_chars_buckets.medium {
        1
    } else {
        2
    }
}

pub fn compute_savings(rows: &[RunLogEntry], ratios: &SavingsRatios, report: &[DelegationReportRow]) -> Savings {
    let delegated: Vec<&RunLogEntry> = rows.iter().filter(|r| DELEGATION_CALLERS.contains(&r.caller.as_str())).collect();
    let succeeded: Vec<&RunLogEntry> = delegated.iter().copied().filter(|r| r.ok).collect();
    let priced: Vec<&RunLogEntry> = succeeded
        .iter()
        .copied()
        .filter(|r| r.tokens.as_ref().map_or(0.0, |t| t.total) > 0.0 || r.cost.unwrap_or(0.0) > 0.0)
        .collect();
    let table = [("small", ratios.sizes.small), ("medium", ratios.sizes.medium), ("large", ratios.sizes.large)];
    let mut by_size: Vec<SizeSavings> = table
        .iter()
        .map(|(name, r)| SizeSavings {
            size: name,
            runs: 0,
            opencode_usd: 0.0,
            opencode_seconds: 0.0,
            est_claude_usd: 0.0,
            est_claude_seconds: 0.0,
            borrowed_ratio: r.n == 0.0,
        })
        .collect();
    for r in &priced {
        let i = size_of_prompt(r.prompt_chars, ratios);
        let cell = &mut by_size[i];
        cell.runs += 1;
        cell.opencode_usd += r.cost.unwrap_or(0.0);
        cell.opencode_seconds += r.duration_ms / 1000.0;
        cell.est_claude_usd += table[i].1.claude_usd;
        cell.est_claude_seconds += table[i].1.claude_seconds;
    }
    let sum = |f: &dyn Fn(&SizeSavings) -> f64| by_size.iter().fold(0.0, |s, c| s + f(c));
    let (opencode_usd, est_claude_usd) = (sum(&|c| c.opencode_usd), sum(&|c| c.est_claude_usd));
    let (opencode_seconds, est_claude_seconds) = (sum(&|c| c.opencode_seconds), sum(&|c| c.est_claude_seconds));
    let saved_usd = est_claude_usd - opencode_usd;
    let delegations = report.iter().fold(0.0, |s, r| s + r.delegations);
    let direct_work = report.iter().fold(0.0, |s, r| s + r.direct_work);
    Savings {
        ratios: RatiosInfo { source: ratios.source.clone(), measured_at: ratios.measured_at.clone() },
        runs: priced.len(),
        failed_runs: delegated.len() - succeeded.len(),
        unpriced_runs: succeeded.len() - priced.len(),
        opencode_usd,
        opencode_seconds,
        est_claude_usd,
        est_claude_seconds,
        saved_usd,
        saved_pct: if est_claude_usd > 0.0 { saved_usd / est_claude_usd } else { 0.0 },
        extra_seconds: opencode_seconds - est_claude_seconds,
        by_size,
        sessions: (!report.is_empty()).then(|| SessionsInfo {
            count: report.len(),
            delegations,
            direct_work,
            delegation_share: if delegations + direct_work > 0.0 { delegations / (delegations + direct_work) } else { 0.0 },
        }),
    }
}

fn usd(n: f64) -> String {
    format!("${}", to_fixed3(n))
}

fn secs(n: f64) -> String {
    format!("{}s", js_num(js_round(n)))
}

pub fn format_savings(s: &Savings) -> String {
    let mut out = vec![
        format!(
            "delegated runs: {} ok, {} failed, {} unpriced (no tokens: mock/test rows) | opencode {} in {} | Claude would have been ~{} in ~{}",
            s.runs, s.failed_runs, s.unpriced_runs, usd(s.opencode_usd), secs(s.opencode_seconds), usd(s.est_claude_usd), secs(s.est_claude_seconds)
        ),
        format!(
            "saved ~{} ({}%), wall-clock {}{}",
            usd(s.saved_usd),
            js_num(js_round(s.saved_pct * 100.0)),
            if s.extra_seconds >= 0.0 { '+' } else { '-' },
            secs(s.extra_seconds.abs())
        ),
        String::new(),
        "size     runs  opencode$  claude$(est)  opencode-s  claude-s(est)".to_string(),
    ];
    for c in &s.by_size {
        out.push(format!(
            "{} {}  {}  {}  {}  {}{}",
            pad_end(c.size, 8),
            pad_start(&c.runs.to_string(), 4),
            pad_start(&usd(c.opencode_usd), 9),
            pad_start(&usd(c.est_claude_usd), 12),
            pad_start(&secs(c.opencode_seconds), 10),
            pad_start(&secs(c.est_claude_seconds), 13),
            if c.borrowed_ratio { "  (ratio borrowed, size unmeasured)" } else { "" }
        ));
    }
    if let Some(x) = &s.sessions {
        out.push(String::new());
        out.push(format!(
            "sessions: {} | delegations {} vs direct read/edit/write/grep {} | delegation share {}%",
            x.count, js_num(x.delegations), js_num(x.direct_work), js_num(js_round(x.delegation_share * 100.0))
        ));
    }
    out.push(String::new());
    out.push(format!(
        "ratios: {}, measured {}. Estimate: size from prompt length x arm-A mean per size.",
        s.ratios.source, s.ratios.measured_at
    ));
    out.join("\n")
}
