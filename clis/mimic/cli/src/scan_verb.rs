//! `wf mimic scan`: the exact-value scan of a run's output before `wf test publish`, for the chain that ran real agent CLIs.
use super::{fail, Done};
use crate::real_clis::{parse_mode, scan, Resolved, MODE_ENV};
use serde_json::json;
use std::path::Path;
use wf_kit::args::Parsed;

pub fn scan_verb(p: &Parsed) -> Done {
    if p.positional.is_empty() {
        return Err(fail("name the directories to scan", "wf mimic scan <output-dir>..."));
    }
    let mode = p.val("clis").map(String::from).or_else(|| std::env::var(MODE_ENV).ok());
    if mode.as_deref().is_none_or(|m| m.trim().is_empty()) {
        return Err(fail(format!("no mode: a scan that guesses scripted would let a real run through unscanned; pass --clis or set {MODE_ENV} to what the chain ran"), "wf mimic scan --clis scripted|real|real:<names> <output-dir>..."));
    }
    let clis = parse_mode(mode.as_deref()).map_err(|e| fail(e, "wf mimic scan --help"))?;
    let dirs: Vec<&Path> = p.positional.iter().map(Path::new).collect();
    if let Some(missing) = dirs.iter().find(|d| !d.is_dir()) {
        return Err(fail(format!("{} is not a directory", missing.display()), "wf mimic scan <output-dir>..."));
    }
    if clis.is_empty() {
        return Ok((json!({"ok": true, "skipped": "no real agent CLI ran: nothing to scan for", "real_clis": [], "findings": [], "warnings": [], "next": "wf test publish"}), 0));
    }
    let home = std::env::temp_dir().join(format!("mimic-scan-{}", std::process::id()));
    let resolved = Resolved::load(&clis, &home);
    let _ = std::fs::remove_dir_all(&home);
    let resolved = resolved.map_err(|e| fail(e, "make the key available to the scan (the vault, or the environment without one)"))?;
    let findings = scan(&dirs, resolved.secrets());
    let ok = findings.is_empty();
    let next = if ok { "wf test publish".to_string() } else { "hold the output back: delete the files named, find what wrote the value, then run it again".to_string() };
    Ok((json!({"ok": ok, "real_clis": resolved.names(), "scanned": dirs.len(), "findings": findings, "warnings": [], "next": next}), if ok { 0 } else { 1 }))
}
