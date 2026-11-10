//! `wf vm exec`: run a local script inside agent-shell through vm-ops-api's start/poll/cancel job model.
use wf_kit::args::parse;
use crate::vmops::{encode_form, encode_uri_component, request_json, Opts};
use base64::Engine;
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

const MAX_SCRIPT_BYTES: usize = 64 * 1024;
const POLL: Duration = Duration::from_millis(2000);
const TIMEOUT: Duration = Duration::from_millis(1_500_000);
const MAX_CONSECUTIVE_POLL_FAILURES: u32 = 6;

fn fail(message: &str) -> Option<i32> {
    wf_kit::wf_err!("wf: {message}");
    Some(1)
}

pub struct Job {
    pub running: bool,
    pub exit_code: Option<i64>,
    pub output: String,
}

pub fn start(script: &str) -> Result<String, String> {
    let opts = Opts {
        body: Some(script.to_string()),
        ok_statuses: &[202],
        ..Opts::post()
    };
    let v = request_json("/services/agent-shell/exec-script", &opts)?;
    Ok(v.get("jobId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

fn poll_json(job_id: &str) -> Result<Value, String> {
    request_json(
        &format!(
            "/services/agent-shell/exec-script/{}",
            encode_uri_component(job_id)
        ),
        &Opts::get(),
    )
}

pub fn poll(job_id: &str) -> Result<Job, String> {
    let v = poll_json(job_id)?;
    Ok(Job {
        running: v.get("running").and_then(Value::as_bool).unwrap_or(false),
        exit_code: v.get("exitCode").and_then(Value::as_i64),
        output: v
            .get("output")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// Runs a script on the VM and returns its exit code and output once the job ends.
pub fn capture_code(script: &str) -> Result<(i64, String), String> {
    let id = start(script)?;
    let mut failed_polls = 0;
    loop {
        // vm-ops-api answers a poll with a timeout now and then while a long job runs; the job itself is unaffected.
        let job = match poll(&id) {
            Ok(j) => {
                failed_polls = 0;
                j
            }
            Err(_) if failed_polls < 30 => {
                failed_polls += 1;
                std::thread::sleep(Duration::from_secs(5));
                continue;
            }
            Err(e) => return Err(e),
        };
        if !job.running {
            return Ok((job.exit_code.unwrap_or(0), job.output));
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// `capture_code` that gives up after `limit`: the job is cancelled (best effort) and the error says so. For a caller that must
/// not wait on the VM, where `capture_code` retries a failing poll for minutes.
pub fn capture_within(script: &str, limit: Duration) -> Result<(i64, String), String> {
    let deadline = Instant::now() + limit;
    let id = start(script)?;
    let mut last_error = String::new();
    while Instant::now() < deadline {
        match poll(&id) {
            Ok(job) if !job.running => return Ok((job.exit_code.unwrap_or(0), job.output)),
            Ok(_) => {}
            Err(error) => last_error = error,
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let _ = cancel(&id);
    Err(format!("job {id} did not finish within {}s and was cancelled{}", limit.as_secs(), if last_error.is_empty() { String::new() } else { format!(" (last poll: {last_error})") }))
}

pub fn cancel(job_id: &str) -> Result<(), String> {
    request_json(
        &format!(
            "/services/agent-shell/exec-script/{}/cancel",
            encode_uri_component(job_id)
        ),
        &Opts::post(),
    )
    .map(|_| ())
}

fn fetch_artifact(rel: &str) -> Result<Vec<u8>, String> {
    // A `..` segment escapes the job's artifact root server-side, so reject before signing anything.
    if rel.split('/').any(|s| s == "..") {
        return Err(format!("artifact path may not contain '..': {rel}"));
    }
    let v = request_json(
        &format!("/services/agent-shell/artifact?path={}", encode_form(rel)),
        &Opts::get(),
    )?;
    let b64 = v.get("base64").and_then(Value::as_str).unwrap_or_default();
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("artifact {rel} is not valid base64: {e}"))
}

/// The part of `text` after the first `from` UTF-16 code units, as JS `slice(from)` sees it.
pub fn slice_utf16(text: &str, from: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&units[from.min(units.len())..])
}

pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Streams new output to stdout until the job ends, as `waitForExecJob` does (a failed poll is fatal).
pub fn wait_streaming(job_id: &str) -> Result<Job, String> {
    let mut seen = 0;
    let mut failed_polls = 0;
    loop {
        // A poll can fail now and then (the proxy, vm-ops-api under load) while the job itself keeps running.
        let job = match poll(job_id) {
            Ok(j) => {
                failed_polls = 0;
                j
            }
            Err(_) if failed_polls < 30 => {
                failed_polls += 1;
                std::thread::sleep(POLL * 2);
                continue;
            }
            Err(e) => return Err(e),
        };
        if utf16_len(&job.output) > seen {
            print!("{}", slice_utf16(&job.output, seen));
            let _ = std::io::stdout().flush();
            seen = utf16_len(&job.output);
        }
        if !job.running {
            return Ok(job);
        }
        std::thread::sleep(POLL);
    }
}

struct Ran {
    exit_code: i64,
    output: String,
    artifacts: Vec<(String, Vec<u8>)>,
}

/// Size check, start, then poll with a deadline until the job ends: its exit code and output.
fn run_job(script: &str) -> Result<(i64, String), String> {
    if script.len() > MAX_SCRIPT_BYTES {
        return Err(format!(
            "Script exceeds {MAX_SCRIPT_BYTES}-byte cap (was {} bytes)",
            script.len()
        ));
    }
    let job_id = start(script)?;
    let deadline = Instant::now() + TIMEOUT;
    let mut failures = 0;
    let job = loop {
        match poll(&job_id) {
            Err(e) => {
                failures += 1;
                if failures >= MAX_CONSECUTIVE_POLL_FAILURES {
                    return Err(e);
                }
                if Instant::now() >= deadline {
                    cancel(&job_id)?;
                    return Err(format!("Script execution timed out for job {job_id}"));
                }
            }
            Ok(job) => {
                failures = 0;
                if !job.running {
                    break job;
                }
                if Instant::now() >= deadline {
                    cancel(&job_id)?;
                    return Err(format!("Script execution timed out for job {job_id}"));
                }
            }
        }
        std::thread::sleep(POLL);
    };
    Ok((job.exit_code.unwrap_or(0), job.output))
}

/// `runScriptOnVm`: run the job, then fetch every artifact whatever the exit code.
fn run_collecting(script: &str, artifacts: &[String]) -> Result<Ran, String> {
    let (exit_code, output) = run_job(script)?;
    let mut fetched = Vec::new();
    for rel in artifacts {
        fetched.push((rel.clone(), fetch_artifact(rel)?));
    }
    Ok(Ran {
        exit_code,
        output,
        artifacts: fetched,
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn exit_code_of(code: i64) -> i32 {
    code.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

const ARTIFACT_ROOT: &str = "/home/agent/e2e-artifacts/";

/// `--log <jobId>`: the job's whole output.log from its job dir in agent-shell; `--status` carries only the last 4 KB.
fn job_log(id: &str) -> Result<Vec<u8>, String> {
    if !all_of(id, |c| c.is_ascii_hexdigit() || c == '-') {
        return Err(format!("not a job id: {id}"));
    }
    let rel = format!("job-logs/{id}.log");
    let script = format!("mkdir -p {ARTIFACT_ROOT}job-logs && cp /tmp/exec-jobs/{id}/output.log {ARTIFACT_ROOT}{rel}\n");
    let ran = run_collecting(&script, std::slice::from_ref(&rel))?;
    if ran.exit_code != 0 {
        return Err(format!("no log kept for job {id}: {}", ran.output.trim()));
    }
    Ok(ran.artifacts.into_iter().next().map(|(_, bytes)| bytes).unwrap_or_default())
}

fn all_of(s: &str, ok: impl Fn(char) -> bool) -> bool {
    !s.is_empty() && s.chars().all(ok)
}

fn run_inner(args: &[String]) -> Result<Option<i32>, String> {
    let Some(p) = parse(args, &["artifact", "out", "status", "log", "cancel"], &["no-wait"]) else {
        return Ok(None);
    };
    if p.positional.len() > 1 {
        return Ok(None);
    }
    let wait = !p.flags.iter().any(|f| f == "no-wait");
    let artifacts: Vec<String> = p
        .all
        .iter()
        .filter(|(k, _)| k == "artifact")
        .map(|(_, v)| v.clone())
        .collect();

    if let Some(id) = p.opts.get("status").filter(|v| !v.is_empty()) {
        println!(
            "{}",
            serde_json::to_string_pretty(&poll_json(id)?).expect("json")
        );
        return Ok(Some(0));
    }
    if let Some(id) = p.opts.get("log").filter(|v| !v.is_empty()) {
        use std::io::Write;
        std::io::stdout().write_all(&job_log(id)?).map_err(|e| format!("cannot print the log: {e}"))?;
        return Ok(Some(0));
    }
    if let Some(id) = p.opts.get("cancel").filter(|v| !v.is_empty()) {
        cancel(id)?;
        println!("ok");
        return Ok(Some(0));
    }
    let Some(file) = p.positional.first() else {
        return Err("wf vm exec needs a <file>, --status <jobId> or --cancel <jobId>".into());
    };
    if !artifacts.is_empty() && !wait {
        return Err("--artifact cannot be used with --no-wait (artifacts are fetched after the script exits)".into());
    }
    let script = String::from_utf8_lossy(
        &std::fs::read(file)
            .map_err(|e| format!("ENOENT: no such file or directory, open '{file}' ({e})"))?,
    )
    .into_owned();
    if !wait {
        println!("{}", start(&script)?);
        return Ok(Some(0));
    }
    if let Some(out) = p.opts.get("out").filter(|v| !v.is_empty()) {
        let ran = run_collecting(&script, &artifacts)?;
        let dir = Path::new(out);
        write_file(&dir.join("output.log"), ran.output.as_bytes())?;
        write_file(
            &dir.join("exit-code"),
            format!("{}\n", ran.exit_code).as_bytes(),
        )?;
        for (rel, bytes) in &ran.artifacts {
            write_file(&dir.join(rel), bytes)?;
        }
        return Ok(Some(exit_code_of(ran.exit_code)));
    }
    let job_id = start(&script)?;
    eprintln!("jobId: {job_id}");
    let job = wait_streaming(&job_id)?;
    Ok(Some(match job.exit_code {
        Some(0) => 0,
        Some(code) => exit_code_of(code),
        None => 1,
    }))
}

pub fn run(args: &[String]) -> Option<i32> {
    match run_inner(args) {
        Ok(code) => code,
        Err(message) => fail(&message),
    }
}

fn wf_on_vm_script(sub: &[&str], rest: &[String]) -> String {
    let quote = |a: &str| format!("'{}'", a.replace('\'', "'\\''"));
    let args: Vec<String> = sub.iter().map(|a| quote(a)).chain(rest.iter().map(|a| quote(a))).chain(std::iter::once("--local".to_string())).collect();
    format!("W=/workspace/home/wf-bin/wf; [ -x $W ] || W=wf; WF_SITE=vm $W {}\n", args.join(" "))
}

/// Runs `wf <sub...> <rest...> --local` on the VM (the wf the worker keeps there, `/workspace/home/wf-bin/wf`, else the one
/// on PATH), collects its output and returns the last JSON line it printed (its envelope) with the exit code.
pub fn json_wf_on_vm(sub: &[&str], rest: &[String]) -> Result<(serde_json::Value, i32), String> {
    let (code, out) = capture_code(&wf_on_vm_script(sub, rest))?;
    let envelope = out.lines().rev().find_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok().filter(|v| v.is_object()));
    Ok((envelope.unwrap_or_else(|| serde_json::json!({ "ok": code == 0 })), code as i32))
}
