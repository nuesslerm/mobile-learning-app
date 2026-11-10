//! `wf vm state backup|restore|list|verify`: the sensitive state of the VM (the `stateVolumes` of `infra/oracle-cloud/vm-apps.json`),
//! backed up so a recreate loses none of it, as restic snapshots in the versioned bucket (incremental, encrypted, deduplicated, retention
//! built in). Everything that touches a volume or the bucket runs on agent-shell, which holds the OCI key and mounts the volumes: the
//! caller asks it through vm-ops-api's exec route, the way `wf cache guard` does, and prints the one JSON line it answers. `--local` is
//! that agent-shell side. The restore of a fresh VM is not here: cloud-init does it before any app starts (`infra/oracle-cloud/cloud-init.sh.tftpl`).
mod local;
mod plan;
mod sqlite;
mod tools;

use plan::AppPlan;
use serde_json::{json, Value};
use std::time::Duration;
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_kit::root::repo_root;

const SPEC: Spec = Spec { values: &["app", "plan"], flags: &["local"], positional: Positional::None, ..Spec::NONE };
const VERBS: [&str; 4] = ["backup", "restore", "list", "verify"];
/// What the runner waits for agent-shell, by verb: a backup reads the volumes, a verify restores the newest snapshots to check them.
const LIMIT_MS: [(&str, u64); 4] = [("backup", 40 * 60_000), ("restore", 40 * 60_000), ("verify", 20 * 60_000), ("list", 5 * 60_000)];
pub(crate) const WORKSPACE_APP: &str = "agent-shell";
/// Where agent-shell mounts its RAM-backed scratch (`tmpfs` in `infra/oracle-cloud/agent-shell/docker-compose.prod.yml`): the staged SQLite copies and
/// the restores of a verify, plaintext of credentials and databases, live only here, so a killed run leaves nothing on a disk.
const SCRATCH: &str = "/scratch";

fn plans_of_repo(only: Option<&str>) -> Result<Vec<AppPlan>, String> {
    let text = std::fs::read_to_string(repo_root().join("infra/oracle-cloud/vm-apps.json")).map_err(|e| format!("infra/oracle-cloud/vm-apps.json: {e} (run from the repository)"))?;
    let all = plan::from_apps(&text)?;
    let committed = blob_cache::domain::Budget::committed();
    let (repository, bucket) = (plan::repository_budget_bytes(&all), plan::bucket_budget_bytes(&all));
    if repository != committed.vm_state_repository_bytes || bucket != committed.vm_state_bytes {
        return Err(format!("the budgets are {} bytes for the repository and {} for the bucket (vmStateRepositoryBytes and vmStateBytes of clis/blob-cache/data/budget.json, vm_state_repository_bytes and vm_state_budget_bytes of vm-state.tf) but twice and four times the sum of the maxBytes of vm-apps.json are {repository} and {bucket}: change them together", committed.vm_state_repository_bytes, committed.vm_state_bytes));
    }
    match only {
        None => Ok(all),
        Some(app) => {
            let kept: Vec<AppPlan> = all.into_iter().filter(|p| p.app == app).collect();
            if kept.is_empty() { Err(format!("{app} declares no stateVolumes in vm-apps.json")) } else { Ok(kept) }
        }
    }
}

fn refusal(verb: &str, error: String, next: &str) -> Value {
    json!({ "event": "vm-state", "verb": verb, "ok": false, "bucket": tools::BUCKET, "apps": [], "error": error, "next": next })
}

/// The script agent-shell runs: its scratch directory (the staged SQLite copies, the restore of a verify) lives on a tmpfs, so a kill of any
/// kind leaves nothing on a disk, and is removed on every way out the shell can still see; the CLI also sweeps what a KILL left behind
/// (a tmpfs that survives the container, which it does not, would otherwise hold it). Without the tmpfs the run refuses to start.
fn script(verb: &str, plans: &[AppPlan]) -> String {
    let refusal = refusal(verb, format!("agent-shell has no tmpfs at {SCRATCH} for the plaintext scratch of a backup and a verify"), "redeploy agent-shell: its compose file mounts the tmpfs (infra/oracle-cloud/agent-shell/docker-compose.prod.yml)");
    format!(
        "W=/workspace/home/wf-bin/wf; [ -x $W ] || W=wf\nS=${{WF_VM_STATE_SCRATCH:-{SCRATCH}}}\nif [ -z \"$WF_VM_STATE_SCRATCH\" ] && ! grep -q ' {SCRATCH} tmpfs ' /proc/mounts; then echo '{refusal}'; exit 1; fi\nD=$S/wf-vm-state-run.$$\ntrap 'rm -rf \"$D\"' EXIT HUP INT TERM\numask 077; mkdir -p \"$D\"; echo $$ > \"$D/.wf-pid\"\nTMPDIR=\"$D\" SQLITE_TMPDIR=\"$D\" WF_VM_STATE_SWEEP_ROOT=\"$S\" WF_SITE=vm $W vm state {verb} --local --plan {}\n",
        plan::encode(plans)
    )
}

fn relay_once(verb: &str, plans: &[AppPlan], limit: u64) -> Value {
    match wf_vmops::vm_exec::capture_within(&script(verb, plans), Duration::from_millis(limit)) {
        Err(why) => refusal(verb, format!("agent-shell did not answer: {why}"), "wf vm ops health; wf vm recover if vm-ops-api or agent-shell is down"),
        Ok((code, output)) => output
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok().filter(|v| v["event"] == "vm-state"))
            .unwrap_or_else(|| {
                let tail: String = output.chars().rev().take(300).collect::<String>().chars().rev().collect();
                refusal(verb, format!("agent-shell printed no result (exit {code}): {}", tail.replace(['\n', '\r'], " ")), "agent-shell runs an older wf without `vm state`, or has no restic/rclone/oci: ship agent-shell and the clis build, then rerun")
            }),
    }
}

const RELAY_ATTEMPTS: u32 = 4;

/// The answer of `verb` run on agent-shell, or a refusal that says why there is none. No answer is retried with a growing pause: an
/// agent-shell redeploy in flight (by hand, so no workflow concurrency group covers it) restarts the container under the run.
pub(crate) fn relay(verb: &str, plans: &[AppPlan]) -> Value {
    let limit = std::env::var("WF_VM_STATE_RELAY_LIMIT_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or_else(|| LIMIT_MS.iter().find(|(v, _)| *v == verb).map_or(600_000, |(_, ms)| *ms));
    let pause = std::env::var("WF_VM_STATE_RELAY_BACKOFF_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(60_000);
    let mut line = relay_once(verb, plans, limit);
    for attempt in 1..RELAY_ATTEMPTS {
        let no_answer = line["error"].as_str().is_some_and(|e| e.starts_with("agent-shell printed no result") || e.starts_with("agent-shell did not answer"));
        if !no_answer {
            break;
        }
        wf_kit::wf_err!("wf: agent-shell gave no answer to vm state {verb} (attempt {attempt}/{RELAY_ATTEMPTS}); retrying");
        std::thread::sleep(Duration::from_millis(pause * u64::from(attempt)));
        line = relay_once(verb, plans, limit);
    }
    line
}

/// Apps whose restore is followed by a recreate of their containers.
fn restart_apps(line: &Value) -> Vec<String> {
    line["apps"].as_array().into_iter().flatten().filter(|a| a["ok"] == true).filter_map(|a| a["app"].as_str()).map(String::from).collect()
}

/// The steps a restore needs when it cannot go ahead: the volume must be empty and its app stopped, and no route stops an app.
const RESTORE_STEPS: &str = "a restore only fills an empty volume of an app that is not running, and vm-ops-api has no route that stops an app: restore belongs to a fresh VM, where cloud-init does it before any app starts (wf vm terraform apply --recreate); for one damaged volume stop the app (wf vm exec: docker compose -p <app> stop), empty the volume, rerun this verb, then redeploy the app (docs/runbooks/vm-recreate.md, \"Restoring state by hand\")";

/// Whether `app` has a running container: a restore never extracts under a running app, and an app that cannot be asked counts as running.
fn running(app: &str) -> Result<bool, String> {
    let containers = wf_vmops::services::ps(app).map_err(|e| format!("could not ask vm-ops-api whether {app} is running: {e}"))?;
    Ok(containers.iter().any(|c| c.get("state").and_then(Value::as_str) == Some("running")))
}

pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    if !VERBS.contains(&verb.as_str()) {
        return None;
    }
    if rest.iter().any(|a| a == "--force") {
        wf_kit::wf_err!("wf vm state {verb}: there is no --force; a restore only fills an empty volume of an app that is not running");
        return Some(2);
    }
    let p = parse_spec(rest, &SPEC)?;
    let local = p.flag("local");
    if local {
        return Some(finish(&local_run(verb, &p)));
    }
    if verb == "restore" && p.val("app") == Some(WORKSPACE_APP) {
        return Some(finish(&refusal(verb, local::WORKSPACE_RESTORE.into(), "restore the workspace only by cloud-init of a fresh VM (docs/runbooks/vm-recreate.md)")));
    }
    if verb == "restore" && p.val("app").is_none() {
        return Some(finish(&refusal(verb, "wf vm state restore needs --app <app>: it fills that app's volumes".into(), RESTORE_STEPS)));
    }
    let plans = match plans_of_repo(p.val("app")) {
        Ok(plans) => plans,
        Err(error) => return Some(finish(&refusal(verb, error, "pass --app <an app with stateVolumes>"))),
    };
    if verb == "restore" {
        let app = p.val("app").unwrap_or_default();
        match running(app) {
            Ok(false) => {}
            Ok(true) => return Some(finish(&refusal(verb, format!("{app} is running: its volumes are in use and nothing was written"), RESTORE_STEPS))),
            Err(error) => return Some(finish(&refusal(verb, error, RESTORE_STEPS))),
        }
    }
    let mut line = relay(verb, &plans);
    if verb == "restore" {
        let mut restarted = Vec::new();
        let mut failed = Vec::new();
        for app in restart_apps(&line) {
            match crate::vm_recover::recreate_app(&app, "dev") {
                Ok(()) => restarted.push(app),
                Err(error) => failed.push(json!({ "app": app, "error": error })),
            }
        }
        line["restarted"] = json!(restarted);
        if !failed.is_empty() {
            line["ok"] = json!(false);
            line["restartFailed"] = json!(failed);
            line["next"] = json!("the data is restored: wf vm recover, or wf ship --redeploy-only <app> --ref dev --tag latest");
        }
    }
    report(verb, &line);
    Some(finish(&line))
}

/// The gauges and one event per app on the CI cache-health path (`docs/runbooks/cache-health-alerts.md`, rule 7): best effort, never a failure.
fn report(verb: &str, line: &Value) {
    use wf_bucket::ci_metrics::{event, Gauges, ALL};
    if !matches!(verb, "backup" | "verify") {
        return;
    }
    let mut gauges = Gauges::new();
    for row in line["apps"].as_array().into_iter().flatten() {
        let app = row["app"].as_str().unwrap_or("unknown");
        let ok = row["ok"] == true;
        gauges.set("ci.vmstate.ok", app, verb, if ok { 1.0 } else { 0.0 });
        if let Some(age) = row["ageSeconds"].as_f64() {
            gauges.set("ci.vmstate.age_seconds", app, ALL, age);
        } else if verb == "backup" && ok {
            gauges.set("ci.vmstate.age_seconds", app, ALL, 0.0);
        }
        let error = row["error"].as_str().unwrap_or_default().chars().take(200).collect::<String>();
        event(if ok { "info" } else { "warn" }, &format!("vm.state.{verb}"), &[("app", app.to_string()), ("ok", ok.to_string()), ("stamp", row["stamp"].as_str().unwrap_or_default().to_string()), ("error", error)]);
    }
    gauges.send();
}

fn finish(line: &Value) -> i32 {
    println!("{line}");
    i32::from(line["ok"] != true)
}

const SCRATCH_PID: &str = ".wf-pid";

/// A run killed outright (a timeout's KILL, an OOM, a restart) cannot clean up after itself: the next run removes every scratch directory
/// of this verb whose owner process is gone. A directory without the owner file is never touched.
fn sweep_stale_scratch() {
    let root = std::env::var("WF_VM_STATE_SWEEP_ROOT").ok().filter(|r| !r.is_empty()).map_or_else(std::env::temp_dir, std::path::PathBuf::from);
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let (path, name) = (entry.path(), entry.file_name().to_string_lossy().into_owned());
        if !name.starts_with("wf-vm-state-") {
            continue;
        }
        let owner = std::fs::read_to_string(path.join(SCRATCH_PID)).ok().and_then(|t| t.trim().parse::<u32>().ok());
        if owner.is_some_and(|pid| !std::path::Path::new(&format!("/proc/{pid}")).exists()) {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// The agent-shell side: the plan arrives as `--plan` (agent-shell has no checkout), the keys come from the environment or the vault.
fn local_run(verb: &str, p: &wf_kit::args::Parsed) -> Value {
    let plans = match p.val("plan") {
        Some(word) => plan::decode(word),
        None => plans_of_repo(p.val("app")),
    };
    let mut plans = match plans {
        Ok(plans) => plans,
        Err(error) => return refusal(verb, error, "run wf vm state without --local: it passes the plan"),
    };
    if let Some(app) = p.val("app") {
        plans.retain(|x| x.app == app);
    }
    let keys = secrets::load_values(&["VM_STATE_RESTIC_PASSWORD"]);
    let Some(password) = keys.get("VM_STATE_RESTIC_PASSWORD").cloned().flatten().filter(|v| !v.trim().is_empty()) else {
        return refusal(verb, "VM_STATE_RESTIC_PASSWORD does not resolve (the environment, then the vault)".into(), "mint it once with `wf secret` (docs/runbooks/vm-recreate.md, \"State backup\")");
    };
    sweep_stale_scratch();
    let dir = match tempfile::Builder::new().prefix("wf-vm-state-").tempdir() {
        Ok(dir) => dir,
        Err(e) => return refusal(verb, format!("no temp directory: {e}"), "free disk space on agent-shell"),
    };
    let _ = std::fs::write(dir.path().join(SCRATCH_PID), std::process::id().to_string());
    // best effort: without it `oci` says ConfigFileNotFound, which the repository error carries
    let _ = wf_bucket::cache::ensure_oci_config();
    let restic = match tools::Restic::new(password.trim().to_string()) {
        Ok(restic) => restic,
        Err(error) => return refusal(verb, error, "check the OCI key on agent-shell (wf cache guard --local reports it)"),
    };
    let committed = blob_cache::domain::Budget::committed();
    let budget = |var: &str, default: u64| std::env::var(var).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default);
    let ctx = local::Ctx {
        restic,
        now_ms: std::env::var("WF_VM_STATE_NOW_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or_else(wf_kit::time::now_ms),
        tmp: dir.path(),
        repository_budget_bytes: budget("WF_VM_STATE_BUDGET_BYTES", committed.vm_state_repository_bytes),
        bucket_budget_bytes: budget("WF_VM_STATE_BUCKET_BUDGET_BYTES", committed.vm_state_bytes),
    };
    match verb {
        "backup" => local::backup(&ctx, &plans),
        "restore" => local::restore(&ctx, &plans),
        "list" => local::list(&ctx, &plans),
        _ => local::verify(&ctx, &plans),
    }
}

/// The backup check of the recreate preflight: every app with state volumes has a backup younger than a day that verifies.
pub(crate) fn backups_current(repo: &std::path::Path) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(repo.join("infra/oracle-cloud/vm-apps.json")).map_err(|e| e.to_string())?;
    let plans = plan::from_apps(&text)?;
    if plans.is_empty() {
        return Ok(Vec::new());
    }
    let line = relay("verify", &plans);
    let mut problems = Vec::new();
    if let Some(error) = line["error"].as_str() {
        problems.push(error.to_string());
    }
    if line["repository"]["ok"] == false {
        problems.push(format!("repository: {}", line["repository"]["error"].as_str().unwrap_or("does not check")));
    }
    for p in &plans {
        let row = line["apps"].as_array().into_iter().flatten().find(|a| a["app"] == p.app.as_str());
        match row {
            None => problems.push(format!("{}: no answer", p.app)),
            Some(row) if row["ok"] != true => problems.push(format!("{}: {}", p.app, row["error"].as_str().unwrap_or("does not verify"))),
            Some(row) => match row["ageSeconds"].as_u64() {
                Some(age) if age < 86_400 => {}
                Some(age) => problems.push(format!("{}: the latest backup is {} h old (over 24)", p.app, age / 3600)),
                None => problems.push(format!("{}: the latest backup has no age", p.app)),
            },
        }
    }
    Ok(problems)
}
