//! `wf ship <service>`: redeploy an app at a tag CI already pushed through vm-ops-api, or move `latest` with
//! `--promote-latest`. Nothing here builds an image: CI does, on a hosted runner (`wf ship build`). `--json`, a missing `--ref` and help are
//! left to the TypeScript CLI.
use wf_kit::args::parse;
use crate::build_image as bi;
use wf_kit::stop;
use wf_vmops::vm_exec::{self, Job};
use wf_vmops::vmops::{encode_form, request_json, Opts};
use serde_json::Value;
use std::io::Write;
use std::time::{Duration, Instant};

/// How a failure is reported: a WfError (`wf: msg`, its own exit code) or a plain Error (`Error: msg`, exit 1).
struct Fail {
    message: String,
    code: i32,
    plain: bool,
}

fn wf_error(message: impl Into<String>) -> Fail {
    Fail {
        message: message.into(),
        code: 1,
        plain: false,
    }
}

fn from_request(message: String) -> Fail {
    wf_error(message)
}

fn env_ms(name: &str, default: u64) -> Duration {
    Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(default),
    )
}

/// Waits out the interval in short ticks so a stop request is noticed promptly, then cancels the VM job and leaves.
fn sleep_or_stop(interval: Duration, job_id: &str) {
    let end = Instant::now() + interval;
    loop {
        if stop::requested() {
            match vm_exec::cancel(job_id) {
                Ok(()) => std::process::exit(143),
                Err(e) => {
                    wf_kit::wf_err!("wf: could not cancel the VM job: {e}");
                    std::process::exit(1);
                }
            }
        }
        let now = Instant::now();
        if now >= end {
            return;
        }
        std::thread::sleep((end - now).min(Duration::from_millis(50)));
    }
}

/// `waitForExecJob` with deploy's settings: streams only new output, rides out up to 18 failed polls.
/// The limit is on silence, not on the whole job: a job that keeps printing is alive.
const JOB_LIMIT_MS: u64 = 200_000;

fn wait_for_job(job_id: &str) -> Result<Job, Fail> {
    let interval = env_ms("WF_DEPLOY_POLL_MS", 10_000);
    let limit = env_ms("WF_VM_JOB_LIMIT_MS", JOB_LIMIT_MS);
    let mut last_output = Instant::now();
    let mut last_seen = String::new();
    stop::claim();
    let mut seen = 0;
    let mut failures = 0;
    let result = loop {
        match vm_exec::poll(job_id) {
            Err(e) => {
                failures += 1;
                if failures > 18 {
                    break Err(from_request(e));
                }
                wf_kit::wf_err!("wf: poll failed, retrying: {e}");
            }
            Ok(job) => {
                failures = 0;
                if vm_exec::utf16_len(&job.output) > seen {
                    print!("{}", vm_exec::slice_utf16(&job.output, seen));
                    let _ = std::io::stdout().flush();
                    seen = vm_exec::utf16_len(&job.output);
                }
                // The poll carries only the tail of the output, so once it is full its length stops growing while a live build keeps
                // changing it: progress is any change of the text.
                if job.output != last_seen {
                    last_seen = job.output.clone();
                    last_output = Instant::now();
                }
                if !job.running {
                    break Ok(job);
                }
                if last_output.elapsed() > limit {
                    break Err(wf_error(format!(
                        "VM job {job_id} printed nothing for {} s; it keeps running on the VM: wf vm exec --status {job_id}",
                        limit.as_secs()
                    )));
                }
            }
        }
        sleep_or_stop(interval, job_id);
    };
    stop::release();
    result
}

struct Request {
    apps: Vec<String>,
    git_ref: String,
    tag: Option<String>,
    redeploy_only: bool,
    smoke: bool,
    wipe_state: bool,
    recreate_all: bool,
    promote_latest: bool,
}

/// An app to release: named as its image, or the app that runs one.
fn release_image(app: &str) -> Result<String, Fail> {
    vm_images(&[wf_vmops::services::app_image(app).to_string()]).map(|_| app.to_string())
}

fn vm_images(names: &[String]) -> Result<Vec<String>, Fail> {
    let unknown: Vec<&String> = names.iter().filter(|n| !bi::is_buildable(n)).collect();
    if !unknown.is_empty() {
        let list: Vec<&str> = unknown.iter().map(|s| s.as_str()).collect();
        let known: Vec<&str> = pipeline::image_hash::IMAGES.iter().map(|i| i.name).collect();
        return Err(wf_error(format!(
            "unknown image(s) {} -- expected one of: {}",
            list.join(", "),
            known.join(", ")
        )));
    }
    Ok(names.to_vec())
}

/// The redeploy is idempotent (pull the tag, up -d), so riding out vm-ops-api restarting is safe: retry only a
/// request that never got an HTTP answer.
pub(crate) fn redeploy(app: &str, git_ref: &str, tag: &str, wipe_state: bool, recreate_all: bool) -> Result<Value, String> {
    let delay = env_ms("WF_DEPLOY_RETRY_MS", 15_000);
    let mut query = format!("tag={}&ref={}", encode_form(tag), encode_form(git_ref));
    if !wipe_state && !recreate_all {
        if let Some(tags) = service_tags(app, git_ref, tag) {
            query.push_str(&format!("&tags={}", encode_form(&tags)));
        }
    }
    if wipe_state {
        query.push_str("&wipeState=1");
    }
    if recreate_all {
        query.push_str("&recreate=all");
    }
    let path = format!("/services/{app}/redeploy?{query}");
    let opts = Opts {
        timeout_sec: 90,
        ok_statuses: &[200, 202],
        ..Opts::post()
    };
    let mut attempt = 1;
    loop {
        match request_json(&path, &opts) {
            Ok(v) => return Ok(v),
            Err(message) => {
                // `request_raw` words a request that got no HTTP answer as "request to <path> failed".
                let transport = message.starts_with("vm-ops-api request to ");
                if attempt >= 12 || !transport {
                    return Err(message);
                }
                wf_kit::wf_err!("wf: redeploy request failed, retrying: {message}");
                std::thread::sleep(delay);
                attempt += 1;
            }
        }
    }
}

/// The `service:tag` list of a gradual redeploy: every service of `app` that runs an image of its own whose image inputs differ between the commit
/// it runs and `git_ref` (empty when none does). None when every service moves (the plain redeploy says the same) or when the running commits
/// cannot be read: a service that cannot be shown unchanged is moved.
fn service_tags(app: &str, git_ref: &str, tag: &str) -> Option<String> {
    let services = pipeline::image_hash::service_images(app);
    if services.is_empty() {
        return None;
    }
    let containers = wf_vmops::services::ps(app).ok()?;
    let running_sha = |container: &str, image: &str| -> Option<String> {
        let marker = format!("/{image}:sha-");
        containers
            .iter()
            .filter(|c| c.get("name").and_then(Value::as_str) == Some(container) && c.get("state").and_then(Value::as_str) == Some("running"))
            .filter_map(|c| c.get("image").and_then(Value::as_str))
            .find_map(|i| i.rsplit_once(&marker[..]).map(|(_, sha)| sha.to_string()))
            .filter(|sha| !sha.is_empty() && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    };
    let unchanged = |svc: &pipeline::image_hash::ServiceImage| -> bool {
        let Some(sha) = running_sha(svc.container, svc.image) else { return false };
        match (pipeline::image_hash::hash(None, &sha, svc.image), pipeline::image_hash::hash(None, git_ref, svc.image)) {
            (Ok(then), Ok(now)) => then == now,
            _ => false,
        }
    };
    let moved: Vec<String> = services.iter().filter(|svc| !unchanged(svc)).map(|svc| format!("{}:{tag}", svc.service)).collect();
    (moved.len() < services.len()).then(|| moved.join(","))
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).expect("json"));
}

/// `--promote-latest <images> --tag <tag>`: moves each image's `latest` to a tag that has served, in a VM job that must end 0.
fn promote_latest(r: &Request) -> Result<(), Fail> {
    if r.redeploy_only {
        return Err(wf_error("--promote-latest takes no --redeploy-only"));
    }
    let tag = r.tag.as_deref().filter(|t| !t.is_empty()).ok_or_else(|| wf_error("--promote-latest needs --tag <tag>"))?;
    let script = bi::promote_script(&vm_images(&r.apps)?, tag).map_err(|m| Fail { message: m, code: 1, plain: true })?;
    let job_id = vm_exec::start(&script).map_err(from_request)?;
    eprintln!("jobId: {job_id}");
    match wait_for_job(&job_id)?.exit_code {
        Some(0) => Ok(()),
        code => Err(Fail {
            message: format!("promoting latest failed with exit code {} -- see output above", code.map_or("unknown".to_string(), |c| c.to_string())),
            code: code.map_or(1, vm_exec::exit_code_of),
            plain: false,
        }),
    }
}

fn run_request(r: &Request) -> Result<(), Fail> {
    if r.promote_latest {
        return promote_latest(r);
    }
    if r.smoke && !r.redeploy_only {
        return Err(wf_error("--smoke applies to --redeploy-only"));
    }
    if r.apps.len() != 1 {
        return Err(wf_error("ship takes exactly one <service>"));
    }
    let app = &r.apps[0];
    if r.redeploy_only {
        let tag = r
            .tag
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or("latest");
        if r.smoke {
            crate::smoke::gate(app, &r.git_ref, tag).map_err(wf_error)?;
        }
        print_json(&redeploy(app, &r.git_ref, tag, r.wipe_state, r.recreate_all).map_err(wf_error)?);
        return Ok(());
    }
    release_image(app)?;
    Err(wf_error(format!(
        "{app} is built by CI, never by wf ship: push to dev (its deploy leg builds sha-<8>), or redeploy a tag CI pushed with `wf ship {app} --ref <sha> --redeploy-only --tag sha-<8>`"
    )))
}

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse(
        args,
        &["ref", "tag"],
        &["redeploy-only", "smoke", "wipe-state", "recreate-all", "promote-latest"],
    )?;
    let flag = |name: &str| p.flags.iter().any(|f| f == name);
    // a promotion moves a tag, not a ref
    let git_ref = if flag("promote-latest") { p.opts.get("ref").cloned().unwrap_or_default() } else { p.opts.get("ref")?.clone() };
    if p.positional.is_empty() {
        return None;
    }
    let request = Request {
        apps: p.positional.clone(),
        git_ref,
        tag: p.opts.get("tag").cloned(),
        redeploy_only: flag("redeploy-only"),
        smoke: flag("smoke"),
        wipe_state: flag("wipe-state"),
        recreate_all: flag("recreate-all"),
        promote_latest: flag("promote-latest"),
    };
    match run_request(&request) {
        Ok(()) => Some(0),
        Err(f) => {
            if f.plain {
                eprintln!("Error: {}", f.message);
            } else {
                wf_kit::wf_err!("wf: {}", f.message);
            }
            Some(f.code)
        }
    }
}
