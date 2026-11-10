//! In-memory doubles of the ports: a clock that is told the time, an event log that keeps what it is given and a fixed reading
//! of the process's memory.
use crate::application::ports::{Clock, Event, EventLog, ProcessMemory};
use async_trait::async_trait;
use std::future::poll_fn;
use std::sync::{Mutex, MutexGuard};
use std::task::{Poll, Waker};
use std::time::Duration;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

mod containers;
mod host;
mod siblings;

pub use containers::{ContainersCall, FakeContainers, FollowEnd};
pub use host::{FakeHostFiles, FakeIds};
pub use siblings::{FakeSiblings, SiblingCall};

#[derive(Default)]
struct ClockState {
    now_ms: i64,
    sleeps: Vec<Duration>,
    allowed: Option<usize>,
    parked: bool,
    waker: Option<Waker>,
    advancing: bool,
}

/// Tells the time it is told. A sleep gives the other tasks one turn, returns and is recorded, and moves the time on by its length
/// when the clock is [`FakeClock::advancing`]; given a number of sleeps to allow, the next one after them never returns and wakes
/// whoever waits on [`FakeClock::exhausted`].
#[derive(Default)]
pub struct FakeClock {
    state: Mutex<ClockState>,
}

impl FakeClock {
    pub fn at(now_ms: i64) -> Self {
        Self { state: Mutex::new(ClockState { now_ms, ..ClockState::default() }) }
    }

    pub fn with_sleeps_allowed(self, sleeps: usize) -> Self {
        lock(&self.state).allowed = Some(sleeps);
        self
    }

    /// Each sleep moves the time on by its length, so a loop that waits on the clock reaches a deadline without waiting for it.
    pub fn advancing(self) -> Self {
        lock(&self.state).advancing = true;
        self
    }

    pub fn set(&self, now_ms: i64) {
        lock(&self.state).now_ms = now_ms;
    }

    pub fn sleeps(&self) -> Vec<Duration> {
        lock(&self.state).sleeps.clone()
    }

    /// Resolves once a sleep past the allowed number has begun.
    pub async fn exhausted(&self) {
        poll_fn(|cx| {
            let mut state = lock(&self.state);
            if state.parked {
                return Poll::Ready(());
            }
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await;
    }
}

#[async_trait]
impl Clock for FakeClock {
    fn now_ms(&self) -> i64 {
        lock(&self.state).now_ms
    }

    async fn sleep(&self, duration: Duration) {
        let park = {
            let mut state = lock(&self.state);
            state.sleeps.push(duration);
            if state.advancing {
                state.now_ms = state.now_ms.saturating_add(i64::try_from(duration.as_millis()).unwrap_or(i64::MAX));
            }
            let over = state.allowed.is_some_and(|allowed| state.sleeps.len() > allowed);
            if over {
                state.parked = true;
                if let Some(waker) = state.waker.take() {
                    waker.wake();
                }
            }
            over
        };
        if park {
            std::future::pending::<()>().await;
        }
        let mut yielded = false;
        poll_fn(|cx| {
            if yielded {
                return Poll::Ready(());
            }
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        })
        .await;
    }
}

#[derive(Default)]
pub struct FakeEventLog {
    events: Mutex<Vec<Event>>,
}

impl FakeEventLog {
    pub fn events(&self) -> Vec<Event> {
        lock(&self.events).clone()
    }
}

impl EventLog for FakeEventLog {
    fn emit(&self, event: Event) {
        lock(&self.events).push(event);
    }
}

pub struct FakeMemory(pub u64);

impl ProcessMemory for FakeMemory {
    fn rss_mb(&self) -> u64 {
        self.0
    }
}

use crate::application::ports::{CooldownLedger, LedgerState, LedgerWrite, Object, RunLog, RunMeta, StateError};
use crate::domain::run_log;
use std::collections::BTreeMap;

/// The cooldown ledger in memory, with the file store's compare-and-set.
#[derive(Default)]
pub struct FakeCooldownLedger {
    state: Mutex<LedgerState>,
    write_fails: bool,
}

impl FakeCooldownLedger {
    pub fn holding(ledger: Object) -> Self {
        Self { state: Mutex::new(LedgerState { ledger, version: 0 }), write_fails: false }
    }

    pub fn whose_writes_fail(mut self) -> Self {
        self.write_fails = true;
        self
    }
}

#[async_trait]
impl CooldownLedger for FakeCooldownLedger {
    async fn read(&self) -> LedgerState {
        lock(&self.state).clone()
    }

    async fn write(&self, ledger: Object, expected: i64) -> LedgerWrite {
        let mut state = lock(&self.state);
        if expected != state.version || self.write_fails {
            return LedgerWrite { ok: false, version: state.version };
        }
        state.ledger = ledger;
        state.version += 1;
        LedgerWrite { ok: true, version: state.version }
    }
}

#[derive(Default)]
struct FakeRun {
    meta: Option<RunMeta>,
    lines: String,
}

/// The run log in memory: lines are kept as the file keeps them, so a line with a newline in it reads back as two.
#[derive(Default)]
pub struct FakeRunLog {
    runs: Mutex<BTreeMap<String, FakeRun>>,
    write_fails: bool,
}

impl FakeRunLog {
    pub fn whose_writes_fail(mut self) -> Self {
        self.write_fails = true;
        self
    }

    fn refusal(&self, run_id: &str, file: &str) -> Result<(), StateError> {
        if self.write_fails {
            return Err(StateError(format!("EACCES: permission denied, open '/state/opencode-log/{run_id}/{file}'")));
        }
        Ok(())
    }
}

#[async_trait]
impl RunLog for FakeRunLog {
    async fn read_meta(&self, run_id: &str) -> Option<RunMeta> {
        lock(&self.runs).get(run_id).and_then(|run| run.meta.clone())
    }

    async fn write_meta(&self, run_id: &str, meta: RunMeta) -> Result<(), StateError> {
        self.refusal(run_id, "meta.json")?;
        lock(&self.runs).entry(run_id.to_string()).or_default().meta = Some(meta);
        Ok(())
    }

    async fn append_lines(&self, run_id: &str, lines: &[String]) -> Result<(), StateError> {
        if lines.is_empty() {
            return Ok(());
        }
        self.refusal(run_id, "lines.jsonl")?;
        let mut runs = lock(&self.runs);
        let run = runs.entry(run_id.to_string()).or_default();
        for line in lines {
            run.lines.push_str(line);
            run.lines.push('\n');
        }
        Ok(())
    }

    async fn read_lines(&self, run_id: &str) -> Vec<String> {
        lock(&self.runs).get(run_id).map(|run| run_log::last_lines(&run.lines)).unwrap_or_default()
    }

    async fn list_runs(&self) -> Vec<RunMeta> {
        let mut metas: Vec<RunMeta> = lock(&self.runs).values().filter_map(|run| run.meta.clone()).collect();
        run_log::newest_first(&mut metas);
        metas
    }
}

#[cfg(test)]
mod state_fakes_tests {
    use super::*;
    use serde_json::{json, Value};

    fn object(value: Value) -> Object {
        value.as_object().cloned().expect("an object")
    }

    #[tokio::test]
    async fn given_the_fake_ledger_when_written_then_only_the_current_version_stores_and_moves_it_on() {
        let ledger = FakeCooldownLedger::holding(object(json!({ "p": {} })));

        let stale = ledger.write(object(json!({ "q": {} })), 3).await;
        let stored = ledger.write(object(json!({ "r": {} })), 0).await;
        let again = ledger.write(object(json!({ "s": {} })), 0).await;

        assert_eq!((stale, stored, again), (LedgerWrite { ok: false, version: 0 }, LedgerWrite { ok: true, version: 1 }, LedgerWrite { ok: false, version: 1 }));
        assert_eq!(ledger.read().await, LedgerState { ledger: object(json!({ "r": {} })), version: 1 });
        let failing = FakeCooldownLedger::default().whose_writes_fail();
        assert_eq!(failing.write(Object::new(), 0).await, LedgerWrite { ok: false, version: 0 });
    }

    #[tokio::test]
    async fn given_the_fake_run_log_when_written_then_it_reads_back_as_the_file_log_does() {
        let log = FakeRunLog::default();
        let meta = |id: &str, at: &str| object(json!({ "runId": id, "startedAt": at }));

        log.write_meta("old", meta("old", "2026-10-20T10:00:00.000Z")).await.expect("stored");
        log.write_meta("new", meta("new", "2026-10-21T10:00:00.000Z")).await.expect("stored");
        log.append_lines("new", &["a".to_string(), String::new(), "b\nc".to_string()]).await.expect("stored");
        log.append_lines("none", &[]).await.expect("nothing to store");

        assert_eq!(log.read_lines("new").await, ["a", "b", "c"]);
        assert_eq!(log.read_lines("none").await, Vec::<String>::new());
        assert_eq!(log.read_meta("none").await, None);
        let listed: Vec<String> = log.list_runs().await.iter().map(|run| run["runId"].as_str().unwrap_or_default().to_string()).collect();
        assert_eq!(listed, ["new", "old"]);
        let failing = FakeRunLog::default().whose_writes_fail();
        assert_eq!(failing.write_meta("r", Object::new()).await, Err(StateError("EACCES: permission denied, open '/state/opencode-log/r/meta.json'".to_string())));
    }
}
