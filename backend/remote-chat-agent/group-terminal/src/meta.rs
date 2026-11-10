//! The metadata stream of one workspace's terminals: upserts and removals numbered from 1 by a count of its own, the last `KEEP` kept so a
//! caller that lost the stream resumes by `seq`; one older than that is answered with the terminals as they are (a reset).
use agent_api_proto::Event;
use agent_api_terminal::{remove_event, upsert_event, TerminalInfo};
use std::sync::Mutex;
use tokio::sync::watch;

const KEEP: usize = 256;

#[derive(Default)]
struct Log {
    seq: u64,
    events: Vec<Event>,
}

pub(crate) struct MetaLog {
    workspace: String,
    log: Mutex<Log>,
    changed: watch::Sender<u64>,
}

/// Events after a `seq`, or that they are no longer all kept.
pub(crate) struct Since {
    pub events: Vec<Event>,
    pub lost: bool,
}

impl MetaLog {
    pub fn new(workspace: &str) -> MetaLog {
        MetaLog { workspace: workspace.to_string(), log: Mutex::default(), changed: watch::channel(0).0 }
    }

    fn log(&self) -> std::sync::MutexGuard<'_, Log> {
        self.log.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn push(&self, make: impl FnOnce(u64) -> Event) {
        let seq = {
            let mut log = self.log();
            log.seq += 1;
            let event = make(log.seq);
            log.events.push(event);
            if log.events.len() > KEEP {
                log.events.remove(0);
            }
            log.seq
        };
        self.changed.send_replace(seq);
    }

    pub fn upsert(&self, info: &TerminalInfo) {
        self.push(|seq| upsert_event(seq, info));
    }

    pub fn remove(&self, terminal: &str) {
        self.push(|seq| remove_event(seq, &self.workspace, terminal));
    }

    pub fn seq(&self) -> u64 {
        self.log().seq
    }

    pub fn watch(&self) -> watch::Receiver<u64> {
        self.changed.subscribe()
    }

    pub fn since(&self, after: u64) -> Since {
        let log = self.log();
        let oldest = log.events.first().map_or(log.seq + 1, |e| e.seq);
        Since { events: log.events.iter().filter(|e| e.seq > after).cloned().collect(), lost: after + 1 < oldest }
    }
}
