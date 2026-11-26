//! `wf mimic proxy check`: a recorded client's interactions are sent to a candidate server, and the first thing the candidate
//! answers differently is named by its route or RPC method and its place in the cassette.
use super::proxy_verb::listen_addr;
use super::{allowlist, fail, labelled, load_config, set_ref, write_suite, Done, Fail};
use crate::adapters::conn;
use crate::adapters::tcp_candidate::TcpCandidate;
use mimic_core::differ::DiffOptions;
use mimic_proxy_core::application::check::check;
use mimic_proxy_core::application::record::policy_for;
use mimic_proxy_core::domain::sanitize::Policy;
use serde_json::json;
use wf_kit::args::Parsed;

const NEXT: &str = "wf mimic proxy check <set> --addr 127.0.0.1:<port>";
const SETTLE_MS: u64 = 300;
const MAX_REPORTED: usize = 20;

/// The fixture values the candidate cannot hand out by itself, each read from the variable that holds the candidate's own value.
fn known(p: &Parsed) -> Result<Vec<(String, String)>, Fail> {
    let pairs = p.val("bind-env").unwrap_or_default().split(',').filter(|pair| !pair.is_empty());
    pairs
        .map(|pair| {
            let (fixture, name) = pair.split_once('=').ok_or_else(|| fail("--bind-env takes <fixture value>=<VARIABLE> pairs", "wf mimic proxy check --help"))?;
            match std::env::var(name) {
                Ok(value) if !value.is_empty() => Ok((fixture.to_string(), value)),
                _ => Err(fail(format!("--bind-env names {name}, which is not set"), "export the variable with the candidate's value")),
            }
        })
        .collect()
}

pub fn check_verb(p: &Parsed) -> Done {
    let set = set_ref(&p.positional[0]);
    let content = set.read()?;
    let Some(addr) = p.val("addr") else { return Err(fail("--addr <host:port> is required: the candidate to check", NEXT)) };
    let addr = listen_addr(addr)?;
    let settle = p.val("settle-ms").map(|v| v.parse::<u64>().map_err(|_| fail("--settle-ms is not a number", "wf mimic proxy check --help"))).transpose()?.unwrap_or(SETTLE_MS);
    let toml = set.dir.join("mimic.toml");
    let cfg = if toml.is_file() { Some(load_config(&toml)?) } else { None };
    let pin_hash = content.cassette.header.provenance.target.as_ref().and_then(|t| t.pin_hash.clone());
    let (allow, mut warnings) = allowlist(p.val("allowlist").map(Into::into).or_else(|| cfg.as_ref().and_then(|c| c.allowlist.clone())), pin_hash.as_deref())?;
    let opts = cfg.map_or_else(DiffOptions::default, |c| c.diff);
    let policy = policy_for(&content.rules, Policy::default());
    let started = wf_kit::time::now_ms();
    let mut candidate = TcpCandidate::new(addr);
    let checked = check(&content, &mut candidate, &policy.secret_keys, &opts, &allow, known(p)?, settle).map_err(|e| fail(e, format!("fix {}", set.dir.join("rules.json").display())))?;
    let verdict = &checked.verdict;
    warnings.extend(verdict.unused_allowlist.iter().map(|u| format!("allowlist entry {u} matched nothing: stale or over-broad")));
    let mut evidence = json!({"report": null, "trace": null});
    if let Some(suite) = p.val("suite") {
        let (report, trace) = write_suite(suite, &set.name, &content, verdict, started, &[]).map_err(|e| fail(e, "set WF_TEST_OUTPUT_DIR to a writable directory"))?;
        evidence = json!({"report": report.display().to_string(), "trace": trace.display().to_string()});
    }
    let ok = verdict.ok();
    let next = match &checked.first {
        None => "nothing to do".to_string(),
        Some(first) => format!("make the candidate answer {} as {} records it ({}), or allowlist the path with a reason", first.what, first.cassette, first.path),
    };
    let out = json!({
        "ok": ok, "set": set.name, "kind": content.cassette.header.provenance.kind, "candidate": addr.to_string(), "connections": conn::dials(),
        "severity": verdict.severity, "summary": verdict.summary, "first": checked.first,
        "results": labelled(verdict).into_iter().take(MAX_REPORTED).collect::<Vec<_>>(), "evidence": evidence, "warnings": warnings, "next": next,
    });
    Ok((out, i32::from(!ok)))
}
