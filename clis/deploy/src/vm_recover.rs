//! `wf vm recover`: health-sweep every app through vm-ops-api and redeploy unhealthy ones with recreate=all; if
//! vm-ops-api stays unreachable, an OCI soft reboot, then a hard reset, then re-verify. The OCI calls shell out to the
//! real `oci` CLI.
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_vmops::services::{ps, RECOVER_APPS};
use wf_vmops::vmops::{encode_form, request_json, Opts};
use serde_json::Value;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Fail(String);

fn env_ms(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .unwrap_or(default)
}

/// A JS number as `String(n)` prints it.
fn js_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

struct Config {
    git_ref: String,
    max_wait: Duration,
    poll: Duration,
    grace: Duration,
    instance_id: Option<String>,
    apps: Vec<String>,
}

fn render_plan(apps: &[&str], c: &Config) -> Vec<String> {
    let id = c
        .instance_id
        .clone()
        .unwrap_or_else(|| "<--instance-id required>".into());
    let secs = |d: Duration| js_num(d.as_secs_f64());
    vec![
        format!(
            "probe vm-ops-api /health; if unreachable poll every {}s for up to {}s",
            secs(c.poll),
            secs(c.max_wait)
        ),
        format!(
            "still unreachable: OCI soft reboot {id}, wait for RUNNING, then up to {}s for /health",
            secs(c.grace)
        ),
        format!(
            "still unreachable: OCI hard reset {id}, wait for RUNNING, then up to {}s for /health",
            secs(c.grace)
        ),
        format!(
            "sweep {} apps ({}); redeploy each unhealthy one with recreate=all from {}",
            apps.len(),
            apps.join(", "),
            c.git_ref
        ),
        format!(
            "re-verify every app until healthy or {}s elapse",
            secs(c.max_wait)
        ),
    ]
}

fn probe_api() -> bool {
    request_json("/health", &Opts::get()).is_ok()
}

fn wait_for_api(window: Duration, poll: Duration) -> bool {
    let deadline = Instant::now() + window;
    loop {
        if probe_api() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(poll);
    }
}

fn container_healthy(c: &Value) -> bool {
    c.get("state").and_then(Value::as_str) == Some("running")
        && !c
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains("(unhealthy)")
}

fn app_healthy(app: &str) -> bool {
    match ps(app) {
        Ok(containers) => !containers.is_empty() && containers.iter().all(container_healthy),
        Err(_) => false,
    }
}

fn unhealthy_apps(apps: &[&str]) -> Vec<String> {
    apps.iter()
        .filter(|a| !app_healthy(a))
        .map(|a| a.to_string())
        .collect()
}

fn oci(args: &[&str]) -> Result<Value, Fail> {
    let out = Command::new("oci")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            Fail(format!(
                "oci {} {} failed: {e}",
                args[0],
                args.get(1).copied().unwrap_or("")
            ))
        })?;
    let name = format!("oci {} {}", args[0], args.get(1).copied().unwrap_or(""));
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!("Command failed: {}", args.join(" "))
        } else {
            stderr
        };
        return Err(Fail(format!("{name} failed: {message}")));
    }
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    serde_json::from_str(&stdout).map_err(|_| {
        Fail(format!(
            "{name} returned unparseable JSON: {}",
            stdout.chars().take(200).collect::<String>()
        ))
    })
}

/// Reboots an instance and waits for RUNNING: 30 attempts, 5 s apart (`WF_OCI_POLL_MS` shortens that for tests).
fn reboot(instance_id: &str, hard: bool) -> Result<(bool, String), Fail> {
    let interval = Duration::from_millis(env_ms("WF_OCI_POLL_MS", 5000.0) as u64);
    oci(&[
        "compute",
        "instance",
        "action",
        "--instance-id",
        instance_id,
        "--action",
        if hard { "RESET" } else { "SOFTRESET" },
    ])?;
    let state = |v: Value| {
        v["data"]["lifecycle-state"]
            .as_str()
            .map_or("null".to_string(), str::to_string)
    };
    for _ in 0..30 {
        let s = state(oci(&[
            "compute",
            "instance",
            "get",
            "--instance-id",
            instance_id,
        ])?);
        if s == "RUNNING" {
            return Ok((true, s));
        }
        std::thread::sleep(interval);
    }
    Ok((
        false,
        state(oci(&[
            "compute",
            "instance",
            "get",
            "--instance-id",
            instance_id,
        ])?),
    ))
}

/// Recreates every container of `app` (the redeploy with `recreate=all`), so it reads the state a restore wrote.
pub(crate) fn recreate_app(app: &str, git_ref: &str) -> Result<(), String> {
    redeploy_recreate(app, git_ref).map_err(|f| f.0)
}

fn redeploy_recreate(app: &str, git_ref: &str) -> Result<(), Fail> {
    let path = format!(
        "/services/{app}/redeploy?tag=latest&ref={}&recreate=all",
        encode_form(git_ref)
    );
    request_json(
        &path,
        &Opts {
            timeout_sec: 180,
            ok_statuses: &[200, 202],
            ..Opts::post()
        },
    )
    .map(|_| ())
    .map_err(Fail)
}

fn recover(c: &Config, dry_run: bool) -> Result<bool, Fail> {
    let apps: Vec<&str> = c.apps.iter().map(String::as_str).collect();
    let apps = apps.as_slice();
    if dry_run {
        for (i, step) in render_plan(apps, c).iter().enumerate() {
            println!("plan {}. {step}", i + 1);
        }
        return Ok(true);
    }
    let mut reboot_kind = "none";
    let mut api = wait_for_api(c.max_wait, c.poll);
    for hard in [false, true] {
        if api {
            break;
        }
        let Some(id) = &c.instance_id else {
            return Err(Fail(
                "vm-ops-api unreachable and no --instance-id given -- cannot reboot the VM".into(),
            ));
        };
        reboot_kind = if hard { "hard" } else { "soft" };
        println!("vm-ops-api unreachable -- OCI {reboot_kind} reboot of {id}");
        let (ok, state) = reboot(id, hard)?;
        println!("reboot {reboot_kind}: finalState={state}");
        api = ok && wait_for_api(c.grace, c.poll);
    }
    if !api {
        println!("vm-ops-api still unreachable after hard reset");
        return Ok(false);
    }
    let redeployed = unhealthy_apps(apps);
    for app in &redeployed {
        println!("redeploy {app} recreate=all ref={}", c.git_ref);
        redeploy_recreate(app, &c.git_ref)?;
    }
    let mut unhealthy = if !redeployed.is_empty() || reboot_kind != "none" {
        unhealthy_apps(apps)
    } else {
        vec![]
    };
    let deadline = Instant::now() + c.max_wait;
    while !unhealthy.is_empty() && Instant::now() < deadline {
        std::thread::sleep(c.poll);
        unhealthy = unhealthy_apps(apps);
    }
    if unhealthy.is_empty() {
        println!("all apps healthy");
    } else {
        println!("still unhealthy: {}", unhealthy.join(", "));
    }
    Ok(unhealthy.is_empty())
}

pub fn run(args: &[String]) -> Option<i32> {
    // `--only <apps...>` takes every following word up to the next option.
    let spec = Spec { values: &["instance-id", "max-wait", "ref"], variadic: &["only"], flags: &["dry-run"], positional: Positional::None, ..Spec::NONE };
    let p = parse_spec(args, &spec)?;
    let only: Option<Vec<String>> = p.flag("only").then(|| p.values("only").into_iter().map(str::to_string).collect());
    let max_wait_arg = p.opts.get("max-wait").map_or("600", String::as_str);
    let Some(max_wait_sec) = max_wait_arg
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n > 0.0)
    else {
        wf_kit::wf_err!("wf: invalid --max-wait {max_wait_arg}");
        return Some(1);
    };
    let dry_run = p.flag("dry-run");
    let c = Config {
        git_ref: p.opts.get("ref").cloned().unwrap_or_else(|| "dev".into()),
        max_wait: Duration::from_secs_f64(max_wait_sec),
        poll: Duration::from_secs_f64(env_ms("WF_RECOVER_POLL_MS", 30_000.0) / 1000.0),
        grace: Duration::from_secs_f64(env_ms("WF_RECOVER_GRACE_MS", 180_000.0) / 1000.0),
        apps: only.unwrap_or_else(|| RECOVER_APPS.iter().map(|s| s.to_string()).collect()),
        instance_id: p
            .opts
            .get("instance-id")
            .cloned()
            .or_else(|| std::env::var("OCI_INSTANCE_ID").ok())
            .filter(|v| !v.is_empty()),
    };
    match recover(&c, dry_run) {
        Ok(healthy) => Some(if dry_run || healthy { 0 } else { 1 }),
        Err(Fail(message)) => {
            wf_kit::wf_err!("wf: {message}");
            Some(1)
        }
    }
}
