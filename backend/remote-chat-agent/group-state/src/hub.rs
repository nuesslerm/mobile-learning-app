//! The write path and the change log of the state groups. Each scope has a stream numbered by a count of its own that starts from the hub's
//! base (the service's start time in microseconds), so a `seq` of an earlier run is older than any tail and gets a reset; the last `keep` events
//! are kept, so a caller that lost the stream resumes by `seq`, and one older than that gets a reset.
use agent_api_proto::Event;
use agent_api_state::watch::{changed_event, SubscribeResult};
use serde_json::{Map, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::watch;

/// Events kept per scope.
pub const KEEP: usize = 256;

/// Where the values live: the adapter keeps them across restarts.
pub trait StateStore: Send + Sync {
    fn get(&self, scope: &str, key: &str) -> Option<Value>;
    /// Every value of the scope, by key.
    fn scope(&self, scope: &str) -> Map<String, Value>;
    fn set(&self, scope: &str, key: &str, value: Value) -> Result<(), String>;
}

struct Stream {
    seq: u64,
    events: VecDeque<Event>,
    changed: watch::Sender<u64>,
}

impl Stream {
    fn new(base: u64) -> Stream {
        Stream { seq: base, events: VecDeque::new(), changed: watch::channel(base).0 }
    }

    fn oldest(&self) -> u64 {
        self.events.front().map_or(self.seq + 1, |e| e.seq)
    }
}

/// Events after a `seq`, or that they are no longer all kept.
pub(crate) struct Since {
    pub events: Vec<Event>,
    pub lost: bool,
}

/// A subscription as opened: what to answer, where the stream continues from, and the wake-ups of the scope.
pub(crate) struct Opened {
    pub result: SubscribeResult,
    pub from: u64,
    pub changes: watch::Receiver<u64>,
}

pub struct StateHub {
    store: Arc<dyn StateStore>,
    keep: usize,
    base: u64,
    streams: Mutex<HashMap<String, Stream>>,
}

impl StateHub {
    pub fn new(store: Arc<dyn StateStore>) -> StateHub {
        let micros = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_micros() as u64);
        StateHub::starting_at(store, KEEP, micros)
    }

    /// A hub whose streams count on from `base`, which must exceed every `seq` an earlier run of the service gave out.
    pub fn from_base(store: Arc<dyn StateStore>, base: u64) -> StateHub {
        StateHub::starting_at(store, KEEP, base)
    }

    /// A hub that keeps `keep` events per scope.
    pub fn keeping(store: Arc<dyn StateStore>, keep: usize) -> StateHub {
        StateHub::starting_at(store, keep, 0)
    }

    pub fn starting_at(store: Arc<dyn StateStore>, keep: usize, base: u64) -> StateHub {
        StateHub { store, keep: keep.max(1), base, streams: Mutex::default() }
    }

    fn streams(&self) -> MutexGuard<'_, HashMap<String, Stream>> {
        self.streams.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub(crate) fn get(&self, scope: &str, key: &str) -> Option<Value> {
        self.store.get(scope, key)
    }

    /// Keeps the value and, only when the store kept it, numbers its change on the scope's stream.
    pub(crate) fn set(&self, scope: &str, key: &str, value: Value) -> Result<(), String> {
        let mut streams = self.streams();
        self.store.set(scope, key, value.clone())?;
        let stream = streams.entry(scope.to_string()).or_insert_with(|| Stream::new(self.base));
        stream.seq += 1;
        stream.events.push_back(changed_event(stream.seq, scope, key, &value));
        if stream.events.len() > self.keep {
            stream.events.pop_front();
        }
        stream.changed.send_replace(stream.seq);
        Ok(())
    }

    /// A caller with no `after_seq`, one older than the tail and one ahead of the stream or from an earlier run (the service restarted) each get the scope's values;
    /// a caller within the tail gets none, its missed events follow.
    pub(crate) fn open(&self, scope: &str, after_seq: Option<u64>) -> Opened {
        let mut streams = self.streams();
        let stream = streams.entry(scope.to_string()).or_insert_with(|| Stream::new(self.base));
        let reset = after_seq.is_some_and(|after| after > stream.seq || after + 1 < stream.oldest());
        let resumed = after_seq.filter(|_| !reset);
        let values = if resumed.is_some() { Map::new() } else { self.store.scope(scope) };
        let result = SubscribeResult { scope: scope.to_string(), values, seq: stream.seq, reset };
        Opened { result, from: resumed.unwrap_or(stream.seq), changes: stream.changed.subscribe() }
    }

    pub(crate) fn since(&self, scope: &str, after: u64) -> Since {
        let streams = self.streams();
        match streams.get(scope) {
            Some(stream) => Since { events: stream.events.iter().filter(|e| e.seq > after).cloned().collect(), lost: after + 1 < stream.oldest() },
            None => Since { events: Vec::new(), lost: false },
        }
    }
}
