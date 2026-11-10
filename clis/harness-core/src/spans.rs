//! Where a run's time goes, as nested spans in `WF_RUN_TIMING` (one JSON line each): the run's phases, the script's steps,
//! each device call with its script line and outcome, Lua `sleep`, and what happens inside a call (adb, UI-server RPC).
use serde_json::json;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static NEXT: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static OPEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Milliseconds since the run started: `WF_RUN_T0_MS` (set by the harness run, inherited by its children), else since now.
fn since_start(at: std::time::SystemTime) -> u64 {
    let epoch = at.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let t0 = std::env::var("WF_RUN_T0_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(epoch);
    epoch.saturating_sub(t0)
}

/// The span a new one nests under: the innermost open one.
pub fn current() -> Option<String> {
    OPEN.with(|o| o.borrow().last().cloned())
}

pub struct Span {
    id: String,
    parent: Option<String>,
    kind: &'static str,
    name: String,
    line: Option<String>,
    started: Instant,
    at: std::time::SystemTime,
    pub outcome: &'static str,
}

/// Opens a span; it is written when dropped. Nothing is written when `WF_RUN_TIMING` is unset.
pub fn begin(kind: &'static str, name: &str, line: Option<String>) -> Span {
    let id = format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let parent = current();
    OPEN.with(|o| o.borrow_mut().push(id.clone()));
    Span { id, parent, kind, name: name.to_string(), line, started: Instant::now(), at: std::time::SystemTime::now(), outcome: "ok" }
}

impl Drop for Span {
    fn drop(&mut self) {
        OPEN.with(|o| {
            let mut o = o.borrow_mut();
            if let Some(i) = o.iter().rposition(|x| *x == self.id) {
                o.remove(i);
            }
        });
        write(&self.id, self.parent.as_deref(), self.kind, &self.name, since_start(self.at), self.started.elapsed().as_millis() as u64, self.outcome, self.line.as_deref());
    }
}

/// A span measured elsewhere (a driver call timed around a child process): written as it is, under the current span.
pub fn record(kind: &'static str, name: &str, ms: u64, outcome: &'static str) {
    let id = format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let at = std::time::SystemTime::now() - std::time::Duration::from_millis(ms);
    write(&id, current().as_deref(), kind, name, since_start(at), ms, outcome, None);
}

/// A step measured from marks elsewhere (device log lines), by its epoch milliseconds: written under the current span.
pub fn record_between(kind: &'static str, name: &str, from_ms: u64, to_ms: u64) {
    let id = format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
    let at = std::time::UNIX_EPOCH + std::time::Duration::from_millis(from_ms);
    write(&id, current().as_deref(), kind, name, since_start(at), to_ms.saturating_sub(from_ms), "ok", None);
}

#[allow(clippy::too_many_arguments)]
fn write(id: &str, parent: Option<&str>, kind: &str, name: &str, t0: u64, ms: u64, outcome: &str, line: Option<&str>) {
    let Some(path) = std::env::var_os("WF_RUN_TIMING") else { return };
    // `verb` and `ms` keep the lines readable by a report written before spans
    let mut v = json!({ "id": id, "kind": kind, "name": name, "verb": name, "t0": t0, "ms": ms, "outcome": outcome });
    if let Some(p) = parent {
        v["parent"] = json!(p);
    }
    if let Some(l) = line {
        v["line"] = json!(l);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = std::io::Write::write_all(&mut f, format!("{v}\n").as_bytes());
    }
}
