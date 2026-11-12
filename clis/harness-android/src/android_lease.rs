//! `wf android lease [acquire|heartbeat|release|status]`: one holder at a time on a device, a FIFO line behind it, and a
//! TTL so a crashed holder frees the device. The state is a JSON file under a flock on the machine that owns the device
//! (the VM); a sandbox reaches it through `wf vm exec`, the queue worker on the VM calls it with `--local`.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_TTL_MS: u64 = 600_000;
/// A waiter that has not asked again for this long has gone away.
const WAITER_TTL_MS: u64 = 30_000;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lease {
    pub id: String,
    pub who: String,
    pub acquired_ms: u64,
    pub heartbeat_ms: u64,
    pub ttl_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Waiter {
    pub id: String,
    pub who: String,
    pub since_ms: u64,
    pub seen_ms: u64,
    pub ttl_ms: u64,
}

#[derive(Default, Clone, Debug, PartialEq)]
pub(crate) struct State {
    pub holder: Option<Lease>,
    pub queue: Vec<Waiter>,
}

fn u(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

impl State {
    fn to_json(&self) -> Value {
        let lease = |l: &Lease| json!({ "id": l.id, "who": l.who, "acquired_ms": l.acquired_ms, "heartbeat_ms": l.heartbeat_ms, "ttl_ms": l.ttl_ms });
        json!({
            "holder": self.holder.as_ref().map(lease),
            "queue": self.queue.iter().map(|w| json!({ "id": w.id, "who": w.who, "since_ms": w.since_ms, "seen_ms": w.seen_ms, "ttl_ms": w.ttl_ms })).collect::<Vec<_>>(),
        })
    }

    fn from_json(v: &Value) -> State {
        State {
            holder: v["holder"].is_object().then(|| Lease { id: s(&v["holder"], "id"), who: s(&v["holder"], "who"), acquired_ms: u(&v["holder"], "acquired_ms"), heartbeat_ms: u(&v["holder"], "heartbeat_ms"), ttl_ms: u(&v["holder"], "ttl_ms") }),
            queue: v["queue"].as_array().into_iter().flatten().map(|w| Waiter { id: s(w, "id"), who: s(w, "who"), since_ms: u(w, "since_ms"), seen_ms: u(w, "seen_ms"), ttl_ms: u(w, "ttl_ms") }).collect(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    Granted(Lease),
    Queued { position: usize, holder: Option<Lease> },
}

/// Drops an expired holder and waiters that stopped asking.
fn expire(state: &mut State, now: u64) {
    if state.holder.as_ref().is_some_and(|h| now > h.heartbeat_ms + h.ttl_ms) {
        state.holder = None;
    }
    state.queue.retain(|w| now <= w.seen_ms + WAITER_TTL_MS);
}

/// Grants the device when it is free and `ticket` is first in line; otherwise queues (idempotent per ticket).
pub(crate) fn acquire(state: &mut State, ticket: &str, who: &str, ttl_ms: u64, now: u64) -> Outcome {
    expire(state, now);
    if let Some(h) = &state.holder {
        if h.id == ticket {
            return Outcome::Granted(h.clone());
        }
    }
    if state.holder.is_none() && state.queue.first().is_none_or(|w| w.id == ticket) {
        state.queue.retain(|w| w.id != ticket);
        let lease = Lease { id: ticket.to_string(), who: who.to_string(), acquired_ms: now, heartbeat_ms: now, ttl_ms };
        state.holder = Some(lease.clone());
        return Outcome::Granted(lease);
    }
    match state.queue.iter_mut().find(|w| w.id == ticket) {
        Some(w) => w.seen_ms = now,
        None => state.queue.push(Waiter { id: ticket.to_string(), who: who.to_string(), since_ms: now, seen_ms: now, ttl_ms }),
    }
    let position = state.queue.iter().position(|w| w.id == ticket).unwrap_or(0) + 1;
    Outcome::Queued { position, holder: state.holder.clone() }
}

pub(crate) fn heartbeat(state: &mut State, id: &str, now: u64) -> Result<(), String> {
    expire(state, now);
    match state.holder.as_mut() {
        Some(h) if h.id == id => {
            h.heartbeat_ms = now;
            Ok(())
        }
        Some(h) => Err(format!("lease {id} is not the holder; {} holds the device", h.who)),
        None => Err(format!("lease {id} has expired or was never granted")),
    }
}

/// Releases the holder, or leaves the line when `id` is only waiting.
pub(crate) fn release(state: &mut State, id: &str, now: u64) -> Result<(), String> {
    expire(state, now);
    if state.holder.as_ref().is_some_and(|h| h.id == id) {
        state.holder = None;
        return Ok(());
    }
    let before = state.queue.len();
    state.queue.retain(|w| w.id != id);
    if state.queue.len() < before {
        Ok(())
    } else {
        Err(format!("lease {id} is neither the holder nor waiting"))
    }
}

/// Whether `my_id` may drive the device now: free, or held by it. Anyone else is told who holds it.
fn may_drive(state: &State, my_id: Option<&str>, now: u64) -> Result<(), String> {
    let mut live = state.clone();
    expire(&mut live, now);
    match &live.holder {
        None => Ok(()),
        Some(h) if my_id == Some(h.id.as_str()) => Ok(()),
        Some(h) => Err(format!("the device is leased by {} (held {}s); take a turn with `wf android lease --wait 600` and set WF_ANDROID_LEASE_ID to the lease id it prints, or WF_ANDROID_LEASE=none to skip the check", h.who, now.saturating_sub(h.acquired_ms) / 1000)),
    }
}

fn local_device() -> String {
    crate::android::container(None)
}

/// The device's lease state where it is local (on the VM, in the queue worker); None in a sandbox, or with the check off.
fn local_state(device: &str) -> Option<State> {
    if std::env::var("WF_ANDROID_LEASE").as_deref() == Ok("none") {
        return None;
    }
    let text = std::fs::read_to_string(dir().join(format!("{device}.json"))).ok()?;
    serde_json::from_str::<Value>(&text).ok().map(|v| State::from_json(&v))
}

/// The check before every command that changes the device, in process: free, or held by `WF_ANDROID_LEASE_ID` (whose
/// lease it renews). A sandbox has no local state, so it is not blocked; it uses `wf android lease` to take its turn.
pub(crate) fn check() -> Result<(), String> {
    let device = local_device();
    let Some(state) = local_state(&device) else { return Ok(()) };
    let id = std::env::var("WF_ANDROID_LEASE_ID").ok();
    may_drive(&state, id.as_deref(), now_ms())?;
    if let Some(id) = id {
        let _ = with_state(&device, |s| heartbeat(s, &id, now_ms()));
    }
    Ok(())
}

/// `check` for today's verbs: the reason on stderr and exit 3.
pub(crate) fn guard() -> Option<i32> {
    check().err().map(|e| {
        wf_kit::wf_err!("wf: {e}");
        3
    })
}

/// Keeps a run's lease (`WF_ANDROID_LEASE_ID`) alive while the run lasts, read-only waits included: a heartbeat every 60 s,
/// or every third of the lease's TTL when that is shorter. It stops when dropped.
pub(crate) struct Renewal {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Renewal {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub(crate) fn renew_while_running() -> Option<Renewal> {
    let id = std::env::var("WF_ANDROID_LEASE_ID").ok().filter(|v| !v.is_empty())?;
    let device = local_device();
    let ttl = local_state(&device)?.holder.filter(|h| h.id == id).map_or(DEFAULT_TTL_MS, |h| h.ttl_ms);
    let every = Duration::from_millis((ttl / 3).clamp(100, 60_000));
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let thread = std::thread::spawn(move || {
        let mut warned = false;
        while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
            match with_state(&device, |s| heartbeat(s, &id, now_ms())).and_then(|r| r) {
                Ok(()) => warned = false,
                Err(e) if !warned => {
                    wf_kit::wf_err!("wf: the run's lease was not renewed: {e}");
                    warned = true;
                }
                Err(_) => {}
            }
        }
    });
    Some(Renewal { stop: Some(stop), thread: Some(thread) })
}

/// The lease of `device` where its state is local (on the VM): who holds it and who waits. None elsewhere.
pub(crate) fn status_of(device: &str) -> Option<Value> {
    let mut state = local_state(device)?;
    expire(&mut state, now_ms());
    Some(status_json(&state, now_ms()))
}

fn status_json(state: &State, now: u64) -> Value {
    let holder = state.holder.as_ref().map(|h| json!({ "id": h.id, "who": h.who, "heldMs": now.saturating_sub(h.acquired_ms), "expiresInMs": (h.heartbeat_ms + h.ttl_ms).saturating_sub(now) }));
    let queue: Vec<Value> = state.queue.iter().enumerate().map(|(i, w)| json!({ "position": i + 1, "id": w.id, "who": w.who, "waitingMs": now.saturating_sub(w.since_ms) })).collect();
    json!({ "holder": holder, "queue": queue })
}

fn now_ms() -> u64 {
    // the flow tests age a lease without waiting out its TTL
    let offset: i64 = std::env::var("WF_LEASE_CLOCK_OFFSET_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    (wf_kit::time::now_ms() + offset) as u64
}

fn dir() -> PathBuf {
    match std::env::var_os("WF_ANDROID_LEASE_DIR") {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => std::env::temp_dir().join("wf-android-lease"),
    }
}

/// Runs `f` on the device's state while holding an exclusive flock, and writes the state back.
fn with_state<T>(device: &str, f: impl FnOnce(&mut State) -> T) -> Result<T, String> {
    use std::os::fd::AsRawFd;
    let dir = dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!("{device}.lock"))).map_err(|e| e.to_string())?;
    // SAFETY: the descriptor belongs to `lock`, which lives until the end of this function; the lock is released on close.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(format!("flock: {}", std::io::Error::last_os_error()));
    }
    let path = dir.join(format!("{device}.json"));
    let mut state = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| State::from_json(&v)).unwrap_or_default();
    let out = f(&mut state);
    write_atomic(&path, &serde_json::to_string_pretty(&state.to_json()).map_err(|e| e.to_string())?)?;
    Ok(out)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn ticket() -> String {
    format!("{:x}{:x}", now_ms(), std::process::id())
}

fn who_default() -> String {
    std::env::var("WF_LEASE_WHO").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| format!("{}:{}", wf_kit::proc::hostname(), std::process::id()))
}

/// Why a lease command did not answer: its exit code (1, or 3 when the device is busy), the error and the next step.
pub(crate) type Refusal = (i32, String, Option<String>);

/// Takes the device for `id`, waiting in line up to `wait_ms`; the error names the holder when the wait runs out.
pub fn acquire_blocking(device: &str, id: &str, who: &str, ttl_ms: u64, wait_ms: u64) -> Result<(), String> {
    let start = now_ms();
    loop {
        match with_state(device, |s| acquire(s, id, who, ttl_ms, now_ms()))? {
            Outcome::Granted(_) => return Ok(()),
            Outcome::Queued { holder, .. } if now_ms() - start >= wait_ms => {
                let _ = with_state(device, |s| release(s, id, now_ms()));
                return Err(format!("device busy: held by {} for longer than the {}s wait", holder.map_or("nobody".to_string(), |h| h.who), wait_ms / 1000));
            }
            Outcome::Queued { .. } => std::thread::sleep(Duration::from_secs(2)),
        }
    }
}

pub fn release_and_hand_over(device: &str, id: &str, clear: Option<&str>) {
    let _ = with_state(device, |s| release(s, id, now_ms()));
    hand_over(device, clear);
}

/// What the next holder inherits: every user-installed app stopped, the named one cleared, logcat empty. Where docker is
/// not available (anywhere but the VM) there is nothing to clean.
fn hand_over(device: &str, clear: Option<&str>) {
    let docker = |args: &[&str]| crate::android_lab::on_device_daemon(std::process::Command::new("docker").args(args)).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).to_string());
    let Some(listed) = docker(&["exec", device, "pm", "list", "packages", "-3"]) else { return };
    for pkg in listed.lines().filter_map(|l| l.trim().strip_prefix("package:")) {
        let _ = docker(&["exec", device, "am", "force-stop", pkg]);
    }
    if let Some(pkg) = clear {
        let _ = docker(&["exec", device, "pm", "clear", pkg]);
    }
    let _ = docker(&["exec", device, "logcat", "-c"]);
}

fn local(action: &str, device: &str, p: &wf_kit::args::Parsed) -> Result<Value, Refusal> {
    let failed = |e: String| (1, e, None);
    let ttl_ms = p.val("ttl").and_then(|v| v.parse::<u64>().ok()).map_or(DEFAULT_TTL_MS, |s| s * 1000);
    let who = p.val("who").map(str::to_string).unwrap_or_else(who_default);
    match action {
        "acquire" => {
            let wait_ms = if p.flag("no-wait") { 0 } else { p.val("wait").and_then(|v| v.parse::<u64>().ok()).map_or(600, |s| s) * 1000 };
            let id = p.val("id").map(str::to_string).unwrap_or_else(ticket);
            let start = now_ms();
            loop {
                match with_state(device, |s| acquire(s, &id, &who, ttl_ms, now_ms())).map_err(failed)? {
                    Outcome::Granted(l) => return Ok(json!({ "granted": true, "leaseId": l.id, "who": l.who, "ttlSeconds": l.ttl_ms / 1000 })),
                    Outcome::Queued { position, holder } if now_ms() - start >= wait_ms => {
                        let _ = with_state(device, |s| release(s, &id, now_ms()));
                        let holder_who = holder.map_or("nobody".to_string(), |h| h.who);
                        return Err((3, format!("device busy: held by {holder_who} (position {position})"), Some(format!("retry with --wait <seconds>, or ask {holder_who} to run `wf android lease release --id <its lease id>`"))));
                    }
                    Outcome::Queued { .. } => std::thread::sleep(Duration::from_secs(2)),
                }
            }
        }
        "heartbeat" | "release" => {
            let id = p.val("id").ok_or_else(|| failed(format!("{action} needs --id <leaseId>")))?;
            let r = with_state(device, |s| if action == "heartbeat" { heartbeat(s, id, now_ms()) } else { release(s, id, now_ms()) });
            r.and_then(|inner| inner).map_err(failed)?;
            if action == "release" {
                hand_over(device, p.val("clean-package"));
            }
            Ok(json!({ "action": action, "leaseId": id }))
        }
        _ => with_state(device, |s| {
            expire(s, now_ms());
            status_json(s, now_ms())
        })
        .map_err(failed),
    }
}

/// `wf android lease [acquire|heartbeat|release|status]` for the container `name`: here with `--local` (on the VM), else
/// relayed to the VM, where the state lives.
pub(crate) fn run(name: &str, p: &wf_kit::args::Parsed, rest: &[String]) -> Result<Value, Refusal> {
    let action = p.positional.first().map_or("acquire", String::as_str);
    if p.flag("local") {
        return local(action, name, p);
    }
    let mut args = rest.to_vec();
    args.push("--json".into());
    let (v, code) = wf_vmops::vm_exec::json_wf_on_vm(&["android", "lease"], &args).map_err(|e| (1, e, None))?;
    if code == 0 {
        return Ok(v.get("result").cloned().unwrap_or_else(|| {
            let mut v = v.clone();
            if let Some(o) = v.as_object_mut() {
                o.remove("ok");
            }
            v
        }));
    }
    Err((code, v["error"].as_str().unwrap_or("the lease command failed on the VM").to_string(), v["next"].as_str().map(str::to_string)))
}
