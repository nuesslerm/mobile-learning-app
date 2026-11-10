//! `wf parity coverage`: parse the line, run the core over the disk and git, write what it built, print one JSON object.
use crate::adapters::{sources_from_env, FsTree, GitHistory, HttpUpstream, MimicEvidence, RunTraces};
use parity_core::{track, track_report, App, Mode, Outcome, Request};
use serde_json::json;
use std::path::{Path, PathBuf};
use wf_kit::args::{parse_spec, Positional, Spec};

const TRACK: Spec = Spec { values: &["app", "root", "message", "file", "plan", "inputs", "out", "run", "sha"], flags: &[], positional: Positional::Any, ..Spec::NONE };

const COVERAGE: Spec = Spec { values: &["app", "root", "base", "traces"], flags: &["check"], positional: Positional::None, ..Spec::NONE };

pub fn run(args: &[String]) -> Option<i32> {
    match args.split_first()? {
        (verb, rest) if verb == "coverage" => coverage(rest),
        (verb, rest) if verb == "track" => track(rest),
        _ => None,
    }
}

fn coverage(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &COVERAGE)?;
    let Some(app) = p.val("app").and_then(App::parse) else {
        println!("{}", json!({"ok": false, "error": format!("--app must be t3 or orca, not {:?}", p.val("app").unwrap_or("")), "next": "wf parity coverage --app t3|orca [--check]"}));
        return Some(2);
    };
    let root = p.val("root").map(PathBuf::from).unwrap_or_else(wf_kit::root::repo_root);
    let req = Request { app, mode: if p.flag("check") { Mode::Check } else { Mode::Write }, base: p.val("base").map(String::from), traces: p.val("traces").map(|t| relative_to(&root, t)) };
    let outcome = parity_core::run(&req, &FsTree::new(&root), &GitHistory::new(&root), &RunTraces, &MimicEvidence);
    for (path, text) in &outcome.writes {
        let target = root.join(path);
        let written = target.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&target, text));
        if let Err(e) = written {
            println!("{}", json!({"ok": false, "app": app.name(), "error": format!("{}: {e}", target.display()), "next": "make the coverage directory writable and run again"}));
            return Some(1);
        }
    }
    println!("{}", outcome.report);
    Some(if outcome.ok { 0 } else { 1 })
}

/// A `--traces` path inside the repository as the path from its root, so the evidence file records one a later check can read; a URL or a
/// path outside the repository stays as given.
fn relative_to(root: &Path, source: &str) -> String {
    let inside = Path::new(source).canonicalize().ok().zip(root.canonicalize().ok()).and_then(|(p, r)| p.strip_prefix(r).ok().map(|rel| rel.to_string_lossy().replace('\\', "/")));
    inside.filter(|rel| !rel.is_empty()).unwrap_or_else(|| source.to_string())
}

/// `wf parity track check|bump --app <t3|orca>` and `wf parity track plan --message <text>|--file <path|->`.
fn track(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &TRACK)?;
    let root = p.val("root").map(PathBuf::from).unwrap_or_else(wf_kit::root::repo_root);
    let tree = FsTree::new(&root);
    let sources = sources_from_env();
    let upstream = HttpUpstream::new(&sources, std::env::var("GITHUB_TOKEN").ok());
    let verb = p.positional.first().map(String::as_str)?;
    if p.positional.len() != 1 {
        return None;
    }
    if verb == "report" {
        return track_report(&p);
    }
    let outcome: Outcome = match verb {
        "plan" => {
            let message = match (p.val("message"), p.val("file")) {
                (Some(m), None) => m.to_string(),
                (None, Some("-")) => std::io::read_to_string(std::io::stdin()).unwrap_or_default(),
                (None, Some(f)) => match std::fs::read_to_string(f) {
                    Ok(t) => t,
                    Err(e) => {
                        println!("{}", json!({"schema": track::SCHEMA, "ok": false, "error": format!("{f}: {e}"), "next": "name a readable --file"}));
                        return Some(1);
                    }
                },
                _ => return None,
            };
            track::plan(&message, &tree, &upstream, &sources)
        }
        "check" | "bump" => {
            let Some(app) = p.val("app").and_then(App::parse) else {
                println!("{}", json!({"schema": track::SCHEMA, "ok": false, "error": format!("--app must be t3 or orca, not {:?}", p.val("app").unwrap_or("")), "next": format!("wf parity track {verb} --app t3|orca")}));
                return Some(2);
            };
            if verb == "check" {
                track::check(app, &tree, &upstream, &sources)
            } else {
                track::bump(app, &tree, &upstream, &sources)
            }
        }
        _ => return None,
    };
    for (path, text) in &outcome.writes {
        let target = root.join(path);
        if let Err(e) = std::fs::write(&target, text) {
            println!("{}", json!({"schema": track::SCHEMA, "ok": false, "error": format!("{}: {e}", target.display()), "next": "make the pin writable and run again"}));
            return Some(1);
        }
    }
    println!("{}", outcome.report);
    Some(i32::from(!outcome.ok))
}

fn track_report(p: &wf_kit::args::Parsed) -> Option<i32> {
    let (plan_file, inputs, out) = (p.val("plan")?, p.val("inputs")?, p.val("out")?);
    let refuse = |error: String, next: &str| {
        println!("{}", json!({"schema": track::SCHEMA, "ok": false, "error": error, "next": next}));
        Some(1)
    };
    let plan = match std::fs::read_to_string(plan_file) {
        Ok(t) => t,
        Err(e) => return refuse(format!("{plan_file}: {e}"), "name the plan.json the plan step wrote"),
    };
    let files: Vec<(String, String)> = std::fs::read_dir(inputs).into_iter().flatten().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).filter_map(|e| Some((e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).ok()?))).collect();
    let now = rfc3339_now();
    let written = match track_report::build(&track_report::Input { plan: &plan, files: &files, run: p.val("run").unwrap_or(""), sha: p.val("sha").unwrap_or(""), now: &now }) {
        Ok(w) => w,
        Err(e) => return refuse(e, "pass --plan the output of wf parity track plan"),
    };
    if let Err(e) = std::fs::create_dir_all(out) {
        return refuse(format!("{out}: {e}"), "name a writable --out");
    }
    for (name, text) in &written {
        if let Err(e) = std::fs::write(Path::new(out).join(name), text) {
            return refuse(format!("{out}/{name}: {e}"), "name a writable --out");
        }
    }
    let report: serde_json::Value = written.iter().find(|(n, _)| n == "drift-report.json").and_then(|(_, t)| serde_json::from_str(t).ok()).unwrap_or_default();
    println!("{report}");
    Some(0)
}

/// `YYYY-MM-DDTHH:MM:SSZ` of the clock, by the civil-from-days rule, so no date crate is needed.
fn rfc3339_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}
