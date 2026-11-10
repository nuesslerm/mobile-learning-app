use agent_api_proto::testing::{load, Fixture};
use agent_api_proto::{Event, Incoming};
use std::path::Path;

fn dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// The recorded exchanges of this crate's methods. `thread.list` is recorded against a service that holds no other thread, so each file is
/// replayed on a service of its own.
pub fn fixtures() -> Vec<Fixture> {
    load(&dir())
}

/// The events of two turns of thread `t1` as frames (`tests/fixtures/thread-events.jsonl`, one per line): every one of the thirteen kinds.
pub fn thread_events() -> Vec<Event> {
    let path = dir().join("thread-events.jsonl");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| match Incoming::parse(line) {
            Ok(Incoming::Event(event)) => event,
            other => panic!("{}: {line}: {other:?}", path.display()),
        })
        .collect()
}

/// The recorded exchanges of a sub-group (`tests/fixtures/<folder>/<method>.json`, folder `requests`, `queue` or `lifecycle`). Their service is not
/// the one of `fixtures()`: each file is replayed on a service of its own that serves the sub-group.
pub fn group_fixtures(folder: &str) -> Vec<Fixture> {
    load(&dir().join(folder))
}
