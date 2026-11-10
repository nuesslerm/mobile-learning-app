//! Where events go: the JSON line on stdout that `wf vm ops logs vm-ops-api` shows, and the log sink behind `service_kit::telemetry`,
//! which gets the records `wf log emit` made for Node, in batches from a thread of its own so no event waits for the network.
use serde_json::Value;
use service_kit::redact::{Redactor, REDACTED};
use service_kit::telemetry::oneshot::{self, Record};
use service_kit::telemetry::{Destination, Source};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};
use vm_ops_api_core::application::ports::{Event, EventLog};
use vm_ops_api_core::domain::event_format;

/// Telemetry's floor: a shorter secret would turn ordinary words into noise.
const SHORTEST_SECRET: usize = 8;

/// One JSON line per event, `{level, message, ...attributes}`, which is what `wf vm ops logs vm-ops-api` shows.
pub struct StdoutEventLog<W: Write + Send> {
    out: Mutex<W>,
    secrets: Vec<String>,
}

impl StdoutEventLog<io::Stdout> {
    pub fn stdout() -> Self {
        Self::to(io::stdout())
    }
}

impl<W: Write + Send> StdoutEventLog<W> {
    pub fn to(out: W) -> Self {
        Self { out: Mutex::new(out), secrets: Vec::new() }
    }

    /// Replaces each of `secrets` wherever it stands in a string attribute; the rest of the line stays Node's bytes.
    pub fn scrubbing(mut self, secrets: impl IntoIterator<Item = String>) -> Self {
        self.secrets = secrets_to_scrub(secrets);
        self
    }

    fn out(&self) -> MutexGuard<'_, W> {
        self.out.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Longest first, so a secret that contains another goes whole.
fn secrets_to_scrub(secrets: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for secret in secrets {
        if secret.chars().count() >= SHORTEST_SECRET && !kept.contains(&secret) {
            kept.push(secret);
        }
    }
    kept.sort_by_key(|secret| std::cmp::Reverse(secret.chars().count()));
    kept
}

/// Before the JSON is written, so a secret with a quote or a backslash in it is found whatever the line makes of it.
fn without_secrets(value: &Value, secrets: &[String]) -> Value {
    match value {
        Value::String(text) => Value::String(secrets.iter().fold(text.clone(), |text, secret| text.replace(secret.as_str(), REDACTED))),
        Value::Array(items) => Value::Array(items.iter().map(|item| without_secrets(item, secrets)).collect()),
        Value::Object(fields) => Value::Object(fields.iter().map(|(key, item)| (key.clone(), without_secrets(item, secrets))).collect()),
        other => other.clone(),
    }
}

impl<W: Write + Send> EventLog for StdoutEventLog<W> {
    fn emit(&self, event: Event) {
        let line = if self.secrets.is_empty() {
            event_format::stdout_line(event.level.as_str(), event.message, &event.attributes)
        } else {
            let attributes: Vec<(&str, Value)> = event.attributes.iter().map(|(key, value)| (*key, without_secrets(value, &self.secrets))).collect();
            event_format::stdout_line(event.level.as_str(), event.message, &attributes)
        };
        // A closed stdout leaves nowhere to report to; the event is lost with it.
        let _ = self.out().write_all(line.as_bytes());
    }
}

/// Events waiting for the export. Past it an event is lost, so a sink that never answers cannot grow the process.
pub const QUEUE_CAPACITY: usize = 1024;
const BATCH: usize = 64;

enum Message {
    Event(Record),
    Flush(SyncSender<()>),
}

/// Sends each event as the record `wf log emit` made of it. It never fails or waits; what the ingest refuses or the queue cannot
/// hold is told to `problems` as `event_log.export_failed`.
pub struct OtlpEventLog {
    service: String,
    host_label: String,
    redactor: Redactor,
    queue: SyncSender<Message>,
    dropped: Arc<AtomicU64>,
}

struct Exporter {
    events: Receiver<Message>,
    destination: Destination,
    source: Source,
    dropped: Arc<AtomicU64>,
    problems: Arc<dyn EventLog>,
}

impl OtlpEventLog {
    /// `secrets` are scrubbed besides the values of the environment variables that look like secrets; `problems` must not lead
    /// back to this log.
    pub fn new(source: Source, destination: Destination, host_label: impl Into<String>, secrets: Vec<String>, problems: Arc<dyn EventLog>) -> Self {
        let (queue, events) = mpsc::sync_channel(QUEUE_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let log = Self { service: source.service.clone(), host_label: host_label.into(), redactor: Redactor::from_env(secrets), queue, dropped: dropped.clone() };
        let exporter = Exporter { events, destination, source, dropped, problems };
        // Without a thread every event is counted as lost, and the stdout line still goes out.
        let _ = std::thread::Builder::new().name("event-log-export".to_string()).spawn(move || exporter.run());
        log
    }

    /// Waits up to `within` for what was emitted before the call to be sent or refused. Blocks: at shutdown, call it from
    /// `spawn_blocking`.
    pub fn flush(&self, within: Duration) -> bool {
        let deadline = Instant::now() + within;
        let (done, answered) = mpsc::sync_channel(1);
        let mut message = Message::Flush(done);
        loop {
            match self.queue.try_send(message) {
                Ok(()) => break,
                Err(TrySendError::Full(back)) if Instant::now() < deadline => {
                    message = back;
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return false,
            }
        }
        answered.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok()
    }

    fn record(&self, event: &Event) -> Record {
        let attributes = event_format::sink_attributes(&self.service, &self.host_label, &event.attributes)
            .into_iter()
            .map(|(key, text)| {
                let text = self.redactor.field(&key, &text).into_owned();
                (key, Value::String(text))
            })
            .collect();
        Record { level: event.level.as_str(), message: self.redactor.scrub(event.message).into_owned(), attributes, site: None, at: SystemTime::now() }
    }
}

impl EventLog for OtlpEventLog {
    fn emit(&self, event: Event) {
        if self.queue.try_send(Message::Event(self.record(&event))).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl Exporter {
    fn run(self) {
        while let Ok(first) = self.events.recv() {
            let (batch, flush) = self.collect(first);
            self.deliver(&batch);
            if let Some(done) = flush {
                let _ = done.send(());
            }
        }
    }

    /// What is queued now, up to a batch; a flush ends it, so it is answered once what came before it has gone.
    fn collect(&self, first: Message) -> (Vec<Record>, Option<SyncSender<()>>) {
        let mut batch = Vec::new();
        let mut next = Some(first);
        while let Some(message) = next.take() {
            match message {
                Message::Event(record) => batch.push(record),
                Message::Flush(done) => return (batch, Some(done)),
            }
            if batch.len() < BATCH {
                next = self.events.try_recv().ok();
            }
        }
        (batch, None)
    }

    fn deliver(&self, batch: &[Record]) {
        if !batch.is_empty() {
            if let Err(reason) = oneshot::send(&self.destination, &self.source, batch) {
                self.problems.emit(Event::warn("event_log.export_failed").with("reason", reason).with("events", batch.len() as u64));
            }
        }
        let lost = self.dropped.swap(0, Ordering::Relaxed);
        if lost > 0 {
            self.problems.emit(Event::warn("event_log.export_failed").with("reason", "queue full").with("events", lost));
        }
    }
}

/// Every sink gets every event, in the order they were given.
pub struct TeeEventLog {
    sinks: Vec<Arc<dyn EventLog>>,
}

impl TeeEventLog {
    pub fn new(sinks: Vec<Arc<dyn EventLog>>) -> Self {
        Self { sinks }
    }
}

impl EventLog for TeeEventLog {
    fn emit(&self, event: Event) {
        if let Some((last, rest)) = self.sinks.split_last() {
            for sink in rest {
                sink.emit(event.clone());
            }
            last.emit(event);
        }
    }
}
