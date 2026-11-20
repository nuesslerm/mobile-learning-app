//! The checks of a pull or merge request in Orca's detail rows, and the one status they add up to.
use orca_core::wire::{Failure, RUNTIME_ERROR};
use serde_json::{json, Value};

/// `pr.checks`' answer as Orca's check details: a state outside the five rca reports is refused, not shown as pending.
pub fn details(answer: &Value) -> Result<Vec<Value>, Failure> {
    let list = answer["checks"].as_array().ok_or_else(|| Failure::new(RUNTIME_ERROR, "rca's pr.checks answered no checks list"))?;
    list.iter()
        .map(|c| {
            let name = c["name"].as_str().ok_or_else(|| Failure::new(RUNTIME_ERROR, "rca's pr.checks answered a check with no name"))?;
            let (status, conclusion) = match c["state"].as_str() {
                Some("pass") => ("completed", json!("success")),
                Some("fail") => ("completed", json!("failure")),
                Some("cancel") => ("completed", json!("cancelled")),
                Some("skipped") => ("completed", json!("skipped")),
                Some("pending") => ("in_progress", Value::Null),
                other => return Err(Failure::new(RUNTIME_ERROR, format!("rca's pr.checks answered the check {name} in state {other:?}"))),
            };
            Ok(json!({"name": name, "status": status, "conclusion": conclusion, "url": c["url"]}))
        })
        .collect()
}

/// The host's rollup of its own check details (`provider-check-summary.ts` of the pinned Orca): a cancelled check fails like a failed one and a skipped
/// one counts as passed; `failure` when any failed, `pending` while one runs, `success` when any passed, `neutral` with none.
pub fn rollup(details: &[Value]) -> &'static str {
    let any = |conclusions: &[Option<&str>]| details.iter().any(|d| conclusions.contains(&d["conclusion"].as_str()));
    if any(&[Some("failure"), Some("cancelled")]) {
        "failure"
    } else if any(&[None]) {
        "pending"
    } else if any(&[Some("success"), Some("skipped")]) {
        "success"
    } else {
        "neutral"
    }
}

/// The host's `summarizeProviderChecks` of its own check details: each check is passed, failed (a cancelled one too, counted again as cancelled), pending
/// while it has no conclusion, or neutral; the state is `none` with no check.
pub fn summary(details: &[Value]) -> Value {
    let count = |keep: &dyn Fn(Option<&str>) -> bool| details.iter().filter(|d| keep(d["conclusion"].as_str())).count();
    let passed = count(&|c| matches!(c, Some("success" | "skipped")));
    let failed = count(&|c| matches!(c, Some("failure" | "cancelled")));
    let pending = count(&|c| c.is_none());
    let state = match (details.len(), failed, pending, passed) {
        (0, ..) => "none",
        (_, f, ..) if f > 0 => "failure",
        (_, _, p, _) if p > 0 => "pending",
        (_, _, _, s) if s > 0 => "success",
        _ => "neutral",
    };
    json!({"state": state, "total": details.len(), "passed": passed, "failed": failed, "cancelled": count(&|c| c == Some("cancelled")), "pending": pending, "neutral": details.len() - passed - failed - pending})
}
