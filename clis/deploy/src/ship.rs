//! `wf ship`: redeploy a service at a tag CI pushed, `ship build` builds in CI; `ship eas` publishes the app to Expo; `ship retire <service>`
//! takes a removed service down; `--changed` prints what a push must deploy; `--status` reports the deployer.
use wf_kit::args::parse;
use crate::deploy;
use std::time::{Duration, Instant};
use pipeline::legs;

const VALUES: &[&str] = &["ref", "tag", "since", "also"];
const FLAGS: &[&str] = &["redeploy-only", "smoke", "wipe-state", "recreate-all", "changed", "status", "force", "github-output", "wait-serving", "promote-latest"];

/// The commit the latest EAS Update was published from, and the backend hosts it dials that have moved.
fn latest_update() -> Result<(String, Vec<String>), String> {
    let pointer = pipeline::eas::latest_pointer("dev")?;
    let commit = pointer["commit"].as_str().ok_or_else(|| "it names no commit".to_string())?;
    Ok((commit.to_string(), pipeline::eas::moved_hosts(&pointer)))
}

fn fail(message: &str) -> Option<i32> {
    wf_kit::wf_err!("wf: {message}");
    Some(1)
}

fn code(result: Result<Option<i32>, String>) -> Option<i32> {
    match result {
        Ok(code) => code,
        Err(message) => fail(&message),
    }
}

pub fn run(args: &[String]) -> Option<i32> {
    let (first, rest) = args.split_first()?;
    match first.as_str() {
        "eas" => return crate::ship_eas::run(rest),
        "worker" => {
            println!("{}", serde_json::json!({ "error": "wf ship worker is gone: there is no VM test worker; device runs are the Device tests workflow (wf test device)" }));
            return Some(2);
        }
        "build" => return code(crate::ci_build::run(rest)),
        "promote" => return code(crate::ci_build::promote(rest)),
        "retire" => {
            let p = parse(rest, &[], &[])?;
            return (p.positional.len() == 1).then(|| code(deploy::retire(&p.positional[0]))).flatten();
        }
        _ => {}
    }
    if args.iter().any(|a| a == "--build-only") {
        return fail("unknown option --build-only: wf ship builds nothing; CI builds and pushes on every push to dev (`wf ship build <app> --tag <tag>`), and `wf ship <app> --ref <sha> --redeploy-only --tag <tag>` redeploys what it pushed");
    }
    let p = parse(args, VALUES, FLAGS)?;
    let flag = |name: &str| p.flags.iter().any(|f| f == name);
    if flag("changed") {
        return if p.positional.is_empty() { code(legs::plan(flag("force"), &p.val("also").map(|a| a.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect::<Vec<_>>()).unwrap_or_default(), p.val("since"), flag("github-output"), latest_update)) } else { None };
    }
    if flag("status") {
        return code(deploy::status(&p.positional));
    }
    if p.positional.iter().any(|a| a == legs::APP_PACKAGE) {
        return fail(&format!("{} is no service: CI's eas-update job publishes its EAS Update; by hand, `wf ship eas --initial-route <route>`", legs::APP_PACKAGE));
    }
    // a promotion names images, vm-ops-api-rs among them, and ships nothing
    if flag("promote-latest") {
        return crate::deploy_run::run(args);
    }
    // vm-ops-api cannot recreate itself through the per-service route: it proves the image CI pushed and swaps itself in
    if p.positional.iter().any(|a| a == "vm-ops-api") {
        if p.positional.len() != 1 {
            return fail("vm-ops-api ships alone");
        }
        if !flag("redeploy-only") {
            return fail("vm-ops-api is built by CI, never by wf ship: push to dev, or `wf ship vm-ops-api --ref <sha> --redeploy-only --tag sha-<8>`");
        }
        return match p.val("tag").ok_or("vm-ops-api --redeploy-only needs --tag <tag> that CI pushed") {
            Ok(tag) => code(ship_self(p.val("ref"), flag("wait-serving"), tag)),
            Err(message) => fail(message),
        };
    }
    let rest: Vec<String> = args.iter().filter(|a| *a != "--wait-serving").cloned().collect();
    let app = p.positional.first().map(String::as_str).unwrap_or_default();
    let wait = flag("wait-serving") && p.positional.len() == 1;
    then_serving(crate::deploy_run::run(&rest), wait, app, p.val("ref"), |_| None)
}

/// The most `--wait-serving` waits for /health once a deploy has returned. A deploy that built and proved its image either serves within
/// seconds or has failed: 30 s was three times the slowest of three swaps on an idle VM, but a swap beside the other CI flavour took 36 s
/// (run 42724986943), so the limit is 120 s; a failure recorded for the commit still ends the wait at once.
const SERVING_LIMIT: Duration = Duration::from_secs(120);

fn phase(name: &str, since: Instant) {
    eprintln!("phase {name}: {} s", since.elapsed().as_secs());
}

/// A full 40-character commit of this checkout: the image is labelled with it and the smoke test checks for it.
pub(crate) fn full_commit(reference: &str) -> Result<String, String> {
    let root = wf_kit::root::repo_root();
    let sha = wf_kit::proc::git_out(&root, &["rev-parse", "--verify", &format!("{reference}^{{commit}}")]).map(|o| o.trim().to_string()).ok_or_else(|| format!("{reference} is not a commit of this checkout"))?;
    if sha.len() == 40 { Ok(sha) } else { Err(format!("{reference} resolves to {sha}, not a full commit sha")) }
}

/// vm-ops-api's deploy of the tag CI pushed: have it prove the pulled image as a throwaway container beside the live one (a failure
/// carries that container's log and swaps nothing), swap, then read `/health` and the recorded outcome. Each phase prints its seconds.
fn ship_self(git_ref: Option<&str>, wait: bool, tag: &str) -> Result<Option<i32>, String> {
    let sha = full_commit(git_ref.ok_or("vm-ops-api ships at --ref <commit>")?)?;
    crate::smoke::gate("vm-ops-api", &sha, tag)?;
    let swapping = Instant::now();
    let reply = wf_vmops::services::redeploy_self(&sha, tag)?;
    phase("swap", swapping);
    println!("{}", serde_json::to_string_pretty(&reply).map_err(|e| e.to_string())?);
    if !wait {
        return Ok(Some(0));
    }
    Ok(then_serving(Some(0), true, "vm-ops-api", Some(&sha), wf_vmops::services::recorded_self_failure))
}

/// `--wait-serving`: a deploy is done when the service serves the pushed commit (its /health), not when its container restarted. The wait is
/// bounded by `SERVING_LIMIT` and ends at once on a failure `recorded` finds for the commit.
fn then_serving(shipped: Option<i32>, wait: bool, app: &str, sha: Option<&str>, recorded: impl Fn(&str) -> Option<String>) -> Option<i32> {
    if shipped != Some(0) || !wait {
        return shipped;
    }
    let Some(sha) = sha.filter(|s| s.len() >= 7) else { return fail("--wait-serving needs --ref <commit sha>") };
    let started = Instant::now();
    match crate::verify_tier3::wait_serving(app, sha, SERVING_LIMIT, || recorded(sha)) {
        Ok(state) => {
            phase("serving", started);
            println!("{}", serde_json::json!({ "app": app, "sha": sha, "serving": state }));
            Some(0)
        }
        Err(e) => fail(&e),
    }
}
