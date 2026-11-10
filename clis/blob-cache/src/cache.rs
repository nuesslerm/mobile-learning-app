use crate::domain::{BadName, Breaker, Budget, Degraded, Failure, Generation, Key, Kind, Meter, Seeded, Stats, Timing};
use crate::ports::{BlobStore, PutOutcome, StoreError};
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

/// Whether this run may write, as `domain::write_allowed` decided; the refusal's line says why.
#[derive(Clone, Debug)]
pub enum Writes {
    Allowed,
    ReadOnly(String),
}

/// Why a call sent nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Skip {
    Disabled,
    ReadOnly,
    TooLarge,
    Degraded(Degraded),
    /// The store refused this request but is still trusted for the next.
    Rejected,
}

impl Skip {
    pub fn as_str(self) -> &'static str {
        match self {
            Skip::Disabled => "disabled",
            Skip::ReadOnly => "read-only",
            Skip::TooLarge => "too-large",
            Skip::Degraded(why) => why.as_str(),
            Skip::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Put {
    Stored,
    Exists,
    Skipped(Skip),
}

struct State {
    meter: Meter,
    breaker: Breaker,
    stats: Stats,
    written: Vec<Key>,
}

/// The writes of one run that `verify_written` looks up; the first ones are as good a sample as the rest.
const VERIFY_SAMPLE: usize = 24;

/// A store used under a run's budget and breaker. Every failure of the store is an answer a caller can treat as cold
/// (`get` gives `None`, `put` gives `Skipped`), so a cache problem never fails the run it serves.
pub struct Cache<S> {
    store: S,
    budget: Budget,
    writes: Writes,
    generation: Generation,
    state: Mutex<State>,
}

const QUOTA_EXCEEDED: &str = "StorageQuotaExceeded";

impl<S: BlobStore> Cache<S> {
    /// Reads the generation from the store (one request).
    pub fn open(store: S, budget: Budget, writes: Writes) -> Cache<S> {
        let mut cache = Cache::with_generation(store, budget, writes, Generation::default());
        cache.generation = cache.read_generation();
        cache
    }

    /// For a caller that already knows the generation and wants no request for it.
    pub fn with_generation(store: S, budget: Budget, writes: Writes, generation: Generation) -> Cache<S> {
        let state = State { meter: Meter::new(&budget), breaker: Breaker::default(), stats: Stats::default(), written: Vec::new() };
        Cache { store, budget, writes, generation, state: Mutex::new(state) }
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    pub fn max_entry_bytes(&self, kind: Kind) -> u64 {
        self.budget.max_entry_bytes(kind)
    }

    pub fn key(&self, kind: Kind, name: &str) -> Result<Key, BadName> {
        Key::new(kind, self.generation, name)
    }

    pub fn get(&self, key: &Key) -> Option<Vec<u8>> {
        self.fetch(key).ok().flatten()
    }

    /// `get` that says why nothing came back: `Ok(None)` is a miss, `Err` a call that was not sent or that failed.
    pub fn fetch(&self, key: &Key) -> Result<Option<Vec<u8>>, Skip> {
        self.admit(1, 0)?;
        match self.timed(|s| &mut s.get, || self.store.get(key)) {
            Ok(Some(body)) => {
                let mut state = self.lock();
                state.meter.received(body.len() as u64);
                state.stats.hits += 1;
                Ok(Some(body))
            }
            Ok(None) => {
                self.lock().stats.misses += 1;
                Ok(None)
            }
            Err(error) => Err(self.failed(&error)),
        }
    }

    /// Whether the object is there (one request, a miss is not counted as one).
    pub fn exists(&self, key: &Key) -> Result<bool, Skip> {
        self.admit(1, 0)?;
        self.timed(|s| &mut s.exists, || self.store.exists(key)).map_err(|error| self.failed(&error))
    }

    /// The object streamed into `dest`: `Ok(Some(bytes))` for a hit, `Ok(None)` for a miss (no file), `Err` as `fetch`. The bytes
    /// count against the run's byte limit once they have arrived.
    pub fn get_file(&self, key: &Key, dest: &Path) -> Result<Option<u64>, Skip> {
        self.admit(1, 0)?;
        match self.timed(|s| &mut s.get, || self.store.get_file(key, dest)) {
            Ok(true) => {
                let bytes = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
                let mut state = self.lock();
                state.meter.received(bytes);
                state.stats.hits += 1;
                Ok(Some(bytes))
            }
            Ok(false) => {
                self.lock().stats.misses += 1;
                Ok(None)
            }
            Err(error) => Err(self.failed(&error)),
        }
    }

    /// `if_absent`: the first writer of a key wins (what Nx needs); false overwrites (the pnpm store, in place).
    pub fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Put {
        self.put_via(key, body.len() as u64, if_absent, |store| store.put(key, body, if_absent))
    }

    /// `put` with the body streamed from `source`, which is never held in memory.
    pub fn put_file(&self, key: &Key, source: &Path, if_absent: bool) -> Put {
        match std::fs::metadata(source) {
            Ok(meta) => self.put_via(key, meta.len(), if_absent, |store| store.put_file(key, source, if_absent)),
            Err(_) => self.skipped(Skip::Rejected),
        }
    }

    fn put_via(&self, key: &Key, bytes: u64, if_absent: bool, send: impl FnOnce(&S) -> Result<PutOutcome, StoreError>) -> Put {
        let put = self.try_put(key, bytes, if_absent, send);
        if matches!(put, Put::Skipped(Skip::Rejected)) {
            self.lock().stats.dropped_puts += 1;
        }
        put
    }

    fn try_put(&self, key: &Key, bytes: u64, if_absent: bool, send: impl FnOnce(&S) -> Result<PutOutcome, StoreError>) -> Put {
        if matches!(self.writes, Writes::ReadOnly(_)) {
            return self.skipped(Skip::ReadOnly);
        }
        if key.kind().is_some_and(|kind| bytes > self.budget.max_entry_bytes(kind)) {
            return self.skipped(Skip::TooLarge);
        }
        if let Err(skip) = self.admit(self.store.requests_per_put(if_absent), bytes) {
            return Put::Skipped(skip);
        }
        match self.timed(|s| &mut s.put, || send(&self.store)) {
            Ok(PutOutcome::Stored) => {
                let mut state = self.lock();
                state.stats.puts += 1;
                if state.written.len() < VERIFY_SAMPLE {
                    state.written.push(key.clone());
                }
                Put::Stored
            }
            Ok(PutOutcome::Exists) => {
                self.lock().stats.skipped += 1;
                Put::Exists
            }
            Err(error) => Put::Skipped(self.failed(&error)),
        }
    }

    /// Asks the store for the objects this run wrote (a sample of them), so a write it acknowledged but does not hold shows as
    /// `present < checked`. The lookups bypass the run's budget and breaker and a lookup that could not be answered is left out of
    /// the sample, so a spent budget never reads as a lost write. Nothing written, nothing asked.
    pub fn verify_written(&self) -> Option<Seeded> {
        let keys = self.lock().written.clone();
        if keys.is_empty() {
            return None;
        }
        if !self.budget.enabled {
            return None;
        }
        let answers: Vec<bool> = keys.iter().filter_map(|key| self.store.exists(key).ok()).collect();
        if answers.is_empty() {
            return None;
        }
        let seeded = Seeded { checked: answers.len() as u64, present: answers.iter().filter(|found| **found).count() as u64 };
        self.lock().stats.seeded = Some(seeded);
        Some(seeded)
    }

    pub fn writes(&self) -> &Writes {
        &self.writes
    }

    /// Moves every reader and writer to a new, empty namespace: the old entries are left to the bucket's lifecycle.
    pub fn purge(&mut self) -> Result<Generation, StoreError> {
        if let Writes::ReadOnly(why) = &self.writes {
            return Err(StoreError::Rejected(format!("read-only ({why})")));
        }
        let pointer = Key::generation_pointer();
        let stored = Generation::parse(self.store.get(&pointer)?.as_deref());
        let next = stored.max(self.generation).next();
        self.store.put(&pointer, next.to_string().as_bytes(), false)?;
        self.generation = next;
        Ok(next)
    }

    /// Why this run went cold, if it did.
    pub fn degraded(&self) -> Option<Degraded> {
        self.lock().breaker.open()
    }

    pub fn stats(&self) -> Stats {
        let state = self.lock();
        Stats { requests: state.meter.requests(), bytes: state.meter.bytes(), degraded: state.breaker.open(), ..state.stats.clone() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs a store call outside the lock and records how long it took, whatever it answered.
    fn timed<T>(&self, which: impl FnOnce(&mut Stats) -> &mut Timing, call: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let answer = call();
        let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        which(&mut self.lock().stats).record(micros);
        answer
    }

    /// Whether a call may go out. A refusal is counted as skipped; the lock is not held while the store answers.
    fn admit(&self, requests: u64, outgoing: u64) -> Result<(), Skip> {
        let mut state = self.lock();
        let refusal = if !self.budget.enabled {
            Some(Skip::Disabled)
        } else if let Some(open) = state.breaker.open() {
            Some(Skip::Degraded(open))
        } else if let Err(spent) = state.meter.admit(requests, outgoing) {
            state.breaker.trip(spent);
            Some(Skip::Degraded(spent))
        } else {
            None
        };
        match refusal {
            Some(skip) => {
                state.stats.skipped += 1;
                Err(skip)
            }
            None => Ok(()),
        }
    }

    fn skipped(&self, skip: Skip) -> Put {
        self.lock().stats.skipped += 1;
        Put::Skipped(skip)
    }

    /// A store error: the breaker opens for a store that cannot be relied on, and the call is counted as skipped.
    fn failed(&self, error: &StoreError) -> Skip {
        eprintln!("wf: build cache store said: {error}");
        let mut state = self.lock();
        state.stats.skipped += 1;
        state.stats.last_failure = Some(Failure { class: error.class(), status: error.status() });
        if error.to_string().contains(QUOTA_EXCEEDED) {
            state.stats.quota_exceeded += 1;
            if state.stats.quota_exceeded == 1 {
                eprintln!("wf: build cache storage quota exceeded: the tenancy's Object Storage is full and no write can succeed; `wf cache guard` evicts, or `wf cache gc --evict-to <bytes>`");
            }
        }
        match error {
            StoreError::Unreachable(_) => state.breaker.trip(Degraded::Unreachable),
            StoreError::Credential(_) => state.breaker.trip(Degraded::Credential),
            StoreError::Rejected(_) => return Skip::Rejected,
        }
        Skip::Degraded(state.breaker.open().unwrap_or(Degraded::Unreachable))
    }

    fn read_generation(&self) -> Generation {
        if self.admit(1, 0).is_err() {
            return Generation::default();
        }
        match self.store.get(&Key::generation_pointer()) {
            Ok(found) => {
                if let Some(bytes) = &found {
                    self.lock().meter.received(bytes.len() as u64);
                }
                Generation::parse(found.as_deref())
            }
            Err(error) => {
                self.failed(&error);
                Generation::default()
            }
        }
    }
}
