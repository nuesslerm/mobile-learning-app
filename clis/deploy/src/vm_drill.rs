//! `wf vm ops drill stop|freeze`: the second-door drill of plan unit D2b. The service takes its own container down (`POST /drill`,
//! answered 202 before a detached sibling acts), wf asserts `/health` on 8080 stops answering, waits for it to come back and reads
//! who brought it back from the watchdog's own `watchdog.recreated` line. There is no standby: a service that stays down is fixed or
//! the VM is recreated.
use wf_kit::args::{json_seen, parse};
use wf_kit::time::now_ms;
use wf_vmops::vmops::{send, PORT};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const PROBE: Duration = Duration::from_secs(5);
const HUNG: Duration = Duration::from_secs(3);
const MUTATE: Duration = Duration::from_secs(30);
const LOG_TRIES: usize = 3;

fn env_ms(name: &str, default: u64) -> Duration {
    Duration::from_millis(std::env::var(name).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default))
}

/// The gap between two `/health` polls.
fn poll() -> Duration {
    env_ms("WF_VM_POLL_MS", 5_000)
}

fn down_timeout() -> Duration {
    env_ms("WF_VM_DOWN_TIMEOUT_MS", 60_000)
}

fn recover_timeout() -> Duration {
    env_ms("WF_VM_RECOVER_TIMEOUT_MS", 600_000)
}

fn seconds(since: Instant) -> f64 {
    (since.elapsed().as_secs_f64() * 10.0).round() / 10.0
}

fn head(text: &str) -> String {
    text.chars().take(300).collect()
}

/// One JSON document on stdout: a single line under `--json`, indented otherwise.
fn print(value: &Value) {
    if json_seen() {
        println!("{value}");
    } else {
        println!("{}", serde_json::to_string_pretty(value).expect("json"));
    }
}

const NEXT: &str = "wf vm ops logs vm-ops-api shows the watchdog's lines; a service that stays down is fixed, or the VM is recreated with Terraform";

/// The failure shape: what the command gathered, `ok:false`, the one line of what went wrong and what to run next.
fn failed(mut evidence: Value, error: &str, next: &str) -> Option<i32> {
    evidence["ok"] = json!(false);
    evidence["error"] = json!(error);
    evidence["next"] = json!(next);
    print(&evidence);
    wf_kit::wf_err!("wf: {error}");
    Some(1)
}

/// `GET /health` on the primary's port, as the body it answered with.
fn health(timeout: Duration) -> Result<Value, String> {
    let reply = send(PORT, "GET", "/health", None, timeout).map_err(|e| format!("{PORT}: {e}"))?;
    let text = reply.text();
    if reply.status != 200 {
        return Err(format!("{PORT}: /health returned HTTP {}: {}", reply.status, head(&text)));
    }
    serde_json::from_str(&text).map_err(|_| format!("{PORT}: /health returned non-JSON: {}", head(&text)))
}

/// The `watchdog.recreated` line of `container` that is newer than `since` (Unix ms) and succeeded, in the watchdog's own log text.
fn recreate_event(log: &str, container: &str, since: i64) -> Option<Value> {
    log.lines().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok()).find(|line| {
        line["message"] == "watchdog.recreated"
            && line["container"] == container
            && line["ok"] == true
            && line["timestamp"].as_str().and_then(rca_core::time::parse).is_some_and(|at| at >= since)
    }).map(|line| json!({ "timestamp": line["timestamp"], "container": line["container"], "reason": line["reason"], "ok": true }))
}

/// Who brought the service back: `watchdog` with its event, `other` when it came back without one, `unknown` when its log could not be read.
fn who(container: Option<&str>, since: i64) -> (&'static str, Value, Option<String>) {
    let Some(container) = container else { return ("unknown", Value::Null, Some("the drill named no container to look for".into())) };
    let mut error = None;
    for attempt in 0..LOG_TRIES {
        if attempt > 0 {
            std::thread::sleep(poll());
        }
        match send(PORT, "GET", "/services/vm-ops-api/logs?tail=400", None, Duration::from_secs(60)) {
            Ok(r) if r.status == 200 => {
                error = None;
                let body: Value = serde_json::from_str(&r.text()).unwrap_or(Value::Null);
                if let Some(event) = recreate_event(body["logs"]["watchdog"].as_str().unwrap_or(""), container, since) {
                    return ("watchdog", event, None);
                }
            }
            Ok(r) => error = Some(format!("logs returned HTTP {}: {}", r.status, head(&r.text()))),
            Err(e) => error = Some(e),
        }
    }
    (if error.is_some() { "unknown" } else { "other" }, Value::Null, error)
}

fn drill(action: &str) -> Option<i32> {
    let base = json!({ "action": action });
    let before = match health(PROBE) {
        Ok(h) => h,
        Err(e) => return failed(base, &format!("the service does not answer, so there is nothing to drill: {e}"), NEXT),
    };
    // The VM's own clock, so a skew between it and this machine cannot hide the watchdog's line
    let since = before["time"].as_str().and_then(rca_core::time::parse).unwrap_or_else(now_ms);
    let evidence = |more: Value| {
        let mut e = base.clone();
        for (k, v) in more.as_object().expect("an object") {
            e[k] = v.clone();
        }
        e
    };

    let started = Instant::now();
    let reply = match send(PORT, "POST", &format!("/drill?action={action}"), None, MUTATE) {
        Ok(r) if r.status == 202 => r,
        Ok(r) => return failed(evidence(json!({ "status": r.status })), &format!("the service refused the drill: HTTP {}: {}", r.status, head(&r.text())), NEXT),
        Err(e) => return failed(evidence(json!({})), &e, NEXT),
    };
    let accepted: Value = serde_json::from_str(&reply.text()).unwrap_or(Value::Null);
    let container = accepted["container"].as_str().filter(|c| !c.is_empty()).map(str::to_string);

    let mut down_s = None;
    while down_s.is_none() && started.elapsed() < down_timeout() {
        if health(HUNG).is_err() {
            down_s = Some(seconds(started));
        } else {
            std::thread::sleep(poll());
        }
    }
    let Some(down_s) = down_s else {
        let error = format!("/health on {PORT} never stopped answering within {} s of the 202", down_timeout().as_secs());
        return failed(evidence(json!({ "container": container, "down": false })), &error, NEXT);
    };

    let mut back = None;
    while back.is_none() {
        if let Ok(h) = health(PROBE) {
            back = Some(h);
        } else if started.elapsed() >= recover_timeout() {
            break;
        } else {
            std::thread::sleep(poll());
        }
    }
    let Some(back) = back else {
        let error = format!("the service did not recover within {} s", recover_timeout().as_secs());
        return failed(evidence(json!({ "container": container, "down": true, "downS": down_s, "recovered": false, "by": null })), &error, NEXT);
    };

    let (by, event, logs_error) = who(container.as_deref(), since);
    print(&evidence(json!({
        "ok": true, "container": container, "down": true, "downS": down_s, "recovered": true, "recoveredS": seconds(started), "by": by,
        "event": event, "logsError": logs_error, "commit": back["commit"],
    })));
    Some(0)
}

/// `args` follow `drill`.
pub fn run(args: &[String]) -> Option<i32> {
    let p = parse(args, &[], &[])?;
    match p.positional.as_slice() {
        [action] if action == "stop" || action == "freeze" => drill(action),
        _ => {
            wf_kit::wf_err!("wf: drill takes stop or freeze; got {:?}", p.positional.join(" "));
            Some(1)
        }
    }
}
