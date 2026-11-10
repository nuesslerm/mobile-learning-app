//! The event log on disk: `<root>/threads/<id>/events.ndjson` (append only, one event per line), a snapshot per thread and
//! `<root>/index.json` with the summaries (`data/fileThreadStore.ts`). A half-written last line, what a crash mid-append leaves, is
//! dropped on read; a corrupt earlier line is an error.

use remote_chat_agent_core::application::store::Store;
use remote_chat_agent_core::domain::types::{Event, Summary, Thread};
use std::io::Write;
use std::path::PathBuf;

pub struct FileStore {
    root: PathBuf,
}

impl FileStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// A thread id names a directory, so one that could name another place (a separator, a dot segment, nothing) is refused.
    fn dir(&self, thread_id: &str) -> Result<PathBuf, String> {
        if thread_id.is_empty() || thread_id == "." || thread_id == ".." || thread_id.contains(['/', '\\', '\0']) {
            return Err(format!("invalid thread id: {thread_id:?}"));
        }
        Ok(self.root.join("threads").join(thread_id))
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    fn read_index(&self) -> Result<Vec<Summary>, String> {
        match std::fs::read_to_string(self.index_path()) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// The lines of a log with the trailing blank and a truncated last line removed.
fn complete_lines(contents: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = contents.split('\n').collect();
    while let Some(last) = lines.last() {
        if last.is_empty() {
            lines.pop();
            continue;
        }
        if serde_json::from_str::<serde_json::Value>(last).is_err() {
            lines.pop();
        }
        break;
    }
    lines
}

fn parse_events(contents: &str, after: Option<i64>) -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    for line in complete_lines(contents) {
        let event: Event = serde_json::from_str(line).map_err(|e| format!("bad event line: {e}"))?;
        if after.is_none_or(|a| event.sequence > a) {
            events.push(event);
        }
    }
    Ok(events)
}

/// A log that does not end in a newline was cut mid-append. The cut fragment is ignored on read already, so it is cut off for good
/// before the next event lands on it (which would leave it mid-file as a corrupt line); a complete last event just gets its newline.
fn heal_tail(path: &std::path::Path) -> Result<(), String> {
    let contents = match std::fs::read(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if contents.is_empty() || contents.ends_with(b"\n") {
        return Ok(());
    }
    let keep = contents.iter().rposition(|b| *b == b'\n').map_or(0, |at| at + 1);
    if serde_json::from_slice::<serde_json::Value>(&contents[keep..]).is_ok() {
        return std::fs::OpenOptions::new().append(true).open(path).and_then(|mut f| f.write_all(b"\n")).map_err(|e| e.to_string());
    }
    std::fs::OpenOptions::new().write(true).open(path).and_then(|f| f.set_len(keep as u64)).map_err(|e| e.to_string())
}

impl Store for FileStore {
    fn append(&self, thread_id: &str, events: &[Event]) -> Result<(), String> {
        let dir = self.dir(thread_id)?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut payload = String::new();
        for event in events {
            payload.push_str(&serde_json::to_string(event).map_err(|e| e.to_string())?);
            payload.push('\n');
        }
        let path = dir.join("events.ndjson");
        heal_tail(&path)?;
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
        file.write_all(payload.as_bytes()).map_err(|e| e.to_string())
    }

    fn read_events(&self, thread_id: &str, after: Option<i64>) -> Result<Vec<Event>, String> {
        match std::fs::read_to_string(self.dir(thread_id)?.join("events.ndjson")) {
            Ok(contents) => parse_events(&contents, after),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn list_summaries(&self) -> Result<Vec<Summary>, String> {
        self.read_index()
    }

    fn list_thread_ids(&self) -> Result<Vec<String>, String> {
        match std::fs::read_dir(self.root.join("threads")) {
            Ok(entries) => {
                let mut ids: Vec<String> = entries.filter_map(Result::ok).filter(|e| e.path().join("events.ndjson").exists()).filter_map(|e| e.file_name().into_string().ok()).collect();
                ids.sort();
                Ok(ids)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e.to_string()),
        }
    }

    fn read_snapshot(&self, thread_id: &str) -> Result<Option<Thread>, String> {
        match std::fs::read_to_string(self.dir(thread_id)?.join("snapshot.json")) {
            Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}
