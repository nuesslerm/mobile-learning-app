//! Doubles of the ports: schedules in memory (that can be made to fail), a clock that moves when told, counting ids, and a trigger that records what
//! it was asked to start and ends each turn when told.
use crate::ports::{Clock, Ids, Refusal, RunRequest, ScheduleStore, Started, Stored, TurnTrigger};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Default)]
pub struct MemoryStore {
    kept: Mutex<Vec<Stored>>,
    failing: Mutex<Option<String>>,
}

impl MemoryStore {
    /// From now on every read and write fails with `why`; none when `why` is none.
    pub fn fail_with(&self, why: Option<&str>) {
        *self.failing.lock().unwrap() = why.map(str::to_string);
    }

    pub fn kept(&self) -> Vec<Stored> {
        self.kept.lock().unwrap().clone()
    }

    pub fn put(&self, all: Vec<Stored>) {
        *self.kept.lock().unwrap() = all;
    }
}

impl ScheduleStore for MemoryStore {
    fn load(&self) -> Result<Vec<Stored>, String> {
        self.failing.lock().unwrap().clone().map_or_else(|| Ok(self.kept()), Err)
    }

    fn save(&self, all: &[Stored]) -> Result<(), String> {
        if let Some(why) = self.failing.lock().unwrap().clone() {
            return Err(why);
        }
        self.put(all.to_vec());
        Ok(())
    }
}

pub struct FakeClock(AtomicU64);

impl FakeClock {
    pub fn at(ms: u64) -> FakeClock {
        FakeClock(AtomicU64::new(ms))
    }

    pub fn advance(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn set(&self, ms: u64) {
        self.0.store(ms, Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// `prefix-1`, `prefix-2`, ... for each prefix; tokens are `tok-1`, `tok-2`, ...
#[derive(Default)]
pub struct CountingIds(Mutex<HashMap<String, u64>>);

impl CountingIds {
    fn next(&self, prefix: &str) -> u64 {
        let mut counts = self.0.lock().unwrap();
        let n = counts.entry(prefix.to_string()).or_insert(0);
        *n += 1;
        *n
    }
}

impl Ids for CountingIds {
    fn id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.next(prefix))
    }

    fn token(&self) -> String {
        format!("tok-{}", self.next("tok"))
    }
}

/// Starts nothing: records each request, answers with thread `th-<n>` (or the thread asked for) and turn `tu-<n>`, or the refusal set, and lets a
/// turn end only when `end` says so.
#[derive(Default)]
pub struct ScriptedTrigger {
    asked: Mutex<Vec<RunRequest>>,
    refusal: Mutex<Option<Refusal>>,
    ended: Mutex<HashMap<String, Result<(), String>>>,
}

impl ScriptedTrigger {
    pub fn refuse_with(&self, refusal: Option<Refusal>) {
        *self.refusal.lock().unwrap() = refusal;
    }

    pub fn asked(&self) -> Vec<RunRequest> {
        self.asked.lock().unwrap().clone()
    }

    /// The turn `turn` ends: well, or with the reason.
    pub fn end(&self, turn: &str, how: Result<(), String>) {
        self.ended.lock().unwrap().insert(turn.to_string(), how);
    }
}

#[async_trait]
impl TurnTrigger for ScriptedTrigger {
    async fn start(&self, run: &RunRequest) -> Result<Started, Refusal> {
        let n = {
            let mut asked = self.asked.lock().unwrap();
            asked.push(run.clone());
            asked.len()
        };
        if let Some(refusal) = self.refusal.lock().unwrap().clone() {
            return Err(refusal);
        }
        Ok(Started { thread: run.thread.clone().unwrap_or_else(|| format!("th-{n}")), turn: format!("tu-{n}") })
    }

    async fn finished(&self, started: &Started) -> Result<(), String> {
        loop {
            if let Some(how) = self.ended.lock().unwrap().get(&started.turn) {
                return how.clone();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
