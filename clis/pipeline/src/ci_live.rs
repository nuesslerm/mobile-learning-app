//! `wf ci live`: updates the one comment a CI run keeps on its pull request, which is the run's report as it stands (the same text `wf ci
//! comment` leaves when the run ends): every job that starts or finishes brings it up to date, so the pull request shows where the CI is,
//! with the stats of the stages done, without opening Actions. The comment is the last on the page: the other runs' CI comments are deleted
//! and this run's is edited in place (`ci_comment::sync_live`).
use crate::ci_comment::{open_pr, redact, report, sync_live, var, Env};
use serde_json::{json, Value};
use std::sync::mpsc;
use std::time::Duration;
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { flags: &[], positional: Positional::AtMost(1), ..Spec::NONE };

fn attempt() -> Result<Value, String> {
    let env = Env::read()?;
    let Some(pr) = open_pr(&env)? else { return Ok(json!({ "ok": true, "posted": false, "reason": "no open pull request for the branch" })) };
    let (action, id) = sync_live(&env, pr, &report(&env)?.body)?;
    Ok(json!({ "ok": true, "posted": true, "action": action, "commentId": id }))
}

fn bound() -> Duration {
    Duration::from_millis(var("WF_CI_LIVE_BOUND_MS").and_then(|v| v.parse().ok()).unwrap_or(15_000))
}

/// The comment is cosmetic: a slow GitHub (each request may wait 60 s) must not hold the job, so the worker is abandoned at the bound.
fn bounded(limit: Duration) -> Result<Value, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(attempt());
    });
    rx.recv_timeout(limit).unwrap_or_else(|_| Err(format!("gave up after {}s: GitHub did not finish answering, the live comment is skipped", limit.as_secs_f32())))
}

/// `wf ci live [start]` (`start` is the same, spelled for the job that begins the run). A failure to comment is reported and never fails
/// the step: exit 0 either way.
pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    if p.positional.first().is_some_and(|w| w != "start") {
        wf_kit::wf_err!("wf ci live: `start` or nothing, not {}", p.positional[0]);
        return Some(2);
    }
    let out = bounded(bound()).unwrap_or_else(|reason| {
        let reason = redact(&reason, &var("GITHUB_TOKEN").unwrap_or_default());
        eprintln!("wf ci live: {reason}");
        json!({ "ok": false, "posted": false, "reason": reason })
    });
    println!("{out}");
    Some(0)
}
