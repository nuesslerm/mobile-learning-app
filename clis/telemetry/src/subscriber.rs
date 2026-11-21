//! The `tracing` subscriber of a service: JSON lines on the console (errors on stderr, with module, file and line) and, when an
//! ingest is configured, OTLP/HTTP (JSON) spans and log records to it. A log record carries the trace of the request it was
//! written in and its call site; the resource carries the source. A failing ingest never fails a request.

use crate::redact::{key_is_sensitive, Redactor, REDACTED};
use crate::{otlp, Destination, Source};
use serde_json::{json, Value};
use std::collections::hash_map::RandomState;
use std::fmt::{Debug, Write as _};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Dispatch, Event, Level, Metadata, Subscriber};
use tracing_subscriber::filter::{EnvFilter, LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;

/// Crates whose own events must not be exported: exporting them would log about exporting.
const EXPORTER_INTERNALS: [&str; 3] = ["ureq", "ureq_proto", "rustls"];
/// Records waiting for the export thread; a full queue drops the newest, so a slow ingest never slows a request.
const QUEUE: usize = 4096;
const BATCH: usize = 256;
const FLUSH: Duration = Duration::from_secs(1);

enum Item {
    Log(Value),
    Span(Value),
    Stop,
}

/// Keeps the export thread alive; flushes and stops it on drop.
#[derive(Default)]
pub struct Telemetry {
    worker: Option<(SyncSender<Item>, JoinHandle<()>)>,
}

impl Telemetry {
    pub fn is_exporting(&self) -> bool {
        self.worker.is_some()
    }

    /// Sends what is still queued. Blocks, so call it from `spawn_blocking` inside a runtime.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if let Some((tx, handle)) = self.worker.take() {
            let _ = tx.send(Item::Stop);
            let _ = handle.join();
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.stop();
    }
}

fn post_batch(destination: &Destination, path: &str, body: &Value, failing: &mut bool) {
    match otlp::post(destination, path, body) {
        Ok(()) => *failing = false,
        Err(error) if !*failing => {
            *failing = true;
            // The export cannot report its own failure through itself.
            eprintln!("{{\"level\":\"error\",\"message\":\"telemetry.export.failed\",\"path\":{path:?},\"error\":{error:?}}}");
        }
        Err(_) => {}
    }
}

fn export_loop(rx: std::sync::mpsc::Receiver<Item>, destination: Destination, source: Source) {
    let resource = otlp::resource(&source);
    let (mut logs, mut spans, mut failing) = (Vec::new(), Vec::new(), false);
    let mut deadline = Instant::now() + FLUSH;
    loop {
        let stop = match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Item::Log(record)) => {
                logs.push(record);
                false
            }
            Ok(Item::Span(span)) => {
                spans.push(span);
                false
            }
            Ok(Item::Stop) | Err(RecvTimeoutError::Disconnected) => true,
            Err(RecvTimeoutError::Timeout) => false,
        };
        if stop || logs.len() + spans.len() >= BATCH || Instant::now() >= deadline {
            if !logs.is_empty() {
                post_batch(&destination, "/v1/logs", &otlp::logs_body(resource.clone(), &source.service, std::mem::take(&mut logs)), &mut failing);
            }
            if !spans.is_empty() {
                post_batch(&destination, "/v1/traces", &otlp::spans_body(resource.clone(), &source.service, std::mem::take(&mut spans)), &mut failing);
            }
            deadline = Instant::now() + FLUSH;
        }
        if stop {
            return;
        }
    }
}

enum Sink {
    Out(io::Stdout),
    Err(io::Stderr),
}

/// A console line with every known secret replaced.
pub struct ScrubbingWriter<W: Write> {
    inner: W,
    redactor: Arc<Redactor>,
}

impl<W: Write> Write for ScrubbingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match std::str::from_utf8(buf) {
            Ok(text) => self.inner.write_all(self.redactor.scrub(text).as_bytes())?,
            Err(_) => self.inner.write_all(buf)?,
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Sink::Out(out) => out.write(buf),
            Sink::Err(err) => err.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Sink::Out(out) => out.flush(),
            Sink::Err(err) => err.flush(),
        }
    }
}

/// Stdout for everything but errors, which go to stderr.
#[derive(Clone)]
struct Console {
    redactor: Arc<Redactor>,
}

impl<'a> MakeWriter<'a> for Console {
    type Writer = ScrubbingWriter<Sink>;

    fn make_writer(&'a self) -> Self::Writer {
        ScrubbingWriter { inner: Sink::Out(io::stdout()), redactor: self.redactor.clone() }
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        let inner = if *meta.level() == Level::ERROR { Sink::Err(io::stderr()) } else { Sink::Out(io::stdout()) };
        ScrubbingWriter { inner, redactor: self.redactor.clone() }
    }
}

/// The span and log exporter, hand-written over the one-shot OTLP/HTTP JSON POST: the standard bridges cannot redact, and the SDK
/// brings an async HTTP stack a service that logs a few lines does not need. A span becomes one OTLP span when it closes, an event
/// one log record carrying the trace of the span it was written in.
struct Export {
    tx: SyncSender<Item>,
    redactor: Arc<Redactor>,
}

struct SpanData {
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    sampled: bool,
    name: String,
    start: SystemTime,
    attributes: Vec<(String, Value)>,
    error: bool,
}

struct Fields<'a> {
    redactor: &'a Redactor,
    body: Option<String>,
    attributes: Vec<(String, Value)>,
}

impl Fields<'_> {
    fn put(&mut self, field: &Field, value: Value) {
        if field.name() == "message" {
            if let Value::String(text) = value {
                self.body = Some(text);
            }
            return;
        }
        let value = if key_is_sensitive(field.name()) { Value::from(REDACTED) } else { value };
        self.attributes.retain(|(key, _)| key != field.name());
        self.attributes.push((field.name().to_string(), value));
    }

    fn take(&mut self, key: &str) -> Option<Value> {
        let at = self.attributes.iter().position(|(k, _)| k == key)?;
        Some(self.attributes.remove(at).1)
    }
}

impl Visit for Fields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        let text = self.redactor.scrub(value).into_owned();
        self.put(field, Value::from(text));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(i64::try_from(value).unwrap_or(i64::MAX)));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, Value::from(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        let text = self.redactor.scrub(&format!("{value:?}")).into_owned();
        self.put(field, Value::from(text));
    }
}

/// Where the event was written, from the call site `tracing` records at compile time.
fn code_attributes(meta: &Metadata<'_>) -> Vec<(String, Value)> {
    let mut out = vec![("code.namespace".to_string(), Value::from(meta.module_path().unwrap_or(meta.target())))];
    out.extend(meta.file().map(|f| ("code.filepath".to_string(), Value::from(f))));
    out.extend(meta.line().map(|l| ("code.lineno".to_string(), Value::from(l))));
    out
}

fn random_hex(bytes: usize) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut out = String::with_capacity(bytes * 2 + 16);
    while out.len() < bytes * 2 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
        let _ = write!(out, "{:016x}", hasher.finish());
    }
    out.truncate(bytes * 2);
    out
}

/// `(trace id, parent span id, sampled)` of a W3C `traceparent` value, `None` when it is not a valid one.
fn parse_traceparent(value: &str) -> Option<(String, String, bool)> {
    let mut parts = value.trim().split('-');
    let (version, trace, parent, flags) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let hex = |text: &str, len: usize| text.len() == len && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) && text.bytes().any(|b| b != b'0');
    (version.len() == 2 && version != "ff" && hex(trace, 32) && hex(parent, 16) && flags.len() == 2)
        .then(|| u8::from_str_radix(flags, 16).ok())
        .flatten()
        .map(|flags| (trace.to_string(), parent.to_string(), flags & 1 == 1))
}

fn span_json(data: &SpanData, end: SystemTime) -> Value {
    let mut span = json!({
        "traceId": data.trace_id,
        "spanId": data.span_id,
        "name": data.name,
        "kind": 2,
        "startTimeUnixNano": otlp::nanos(data.start),
        "endTimeUnixNano": otlp::nanos(end),
        "attributes": otlp::attributes(data.attributes.iter().map(|(k, v)| (k.as_str(), v.clone()))),
    });
    if let Some(parent) = &data.parent_span_id {
        span["parentSpanId"] = parent.clone().into();
    }
    if data.error {
        span["status"] = json!({ "code": 2 });
    }
    span
}

fn apply_span_fields(data: &mut SpanData, mut fields: Fields<'_>) {
    if let Some(Value::String(name)) = fields.take("otel.name") {
        data.name = name;
    }
    if let Some(status) = fields.take("otel.status_code") {
        data.error = status == "ERROR";
    }
    for (key, value) in fields.attributes {
        data.attributes.retain(|(k, _)| *k != key);
        data.attributes.push((key, value));
    }
}

impl<S> Layer<S> for Export
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields { redactor: &self.redactor, body: None, attributes: Vec::new() };
        attributes.record(&mut fields);
        let remote = fields.take("traceparent").and_then(|v| v.as_str().and_then(parse_traceparent));
        let local = span.parent().and_then(|parent| parent.extensions().get::<SpanData>().map(|p| (p.trace_id.clone(), p.span_id.clone(), p.sampled)));
        let (trace_id, parent_span_id, sampled) = match remote.or(local) {
            Some((trace, parent, sampled)) => (trace, Some(parent), sampled),
            None => (random_hex(16), None, true),
        };
        let mut data = SpanData {
            trace_id,
            span_id: random_hex(8),
            parent_span_id,
            sampled,
            name: attributes.metadata().name().to_string(),
            start: SystemTime::now(),
            attributes: Vec::new(),
            error: false,
        };
        apply_span_fields(&mut data, fields);
        span.extensions_mut().insert(data);
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields { redactor: &self.redactor, body: None, attributes: Vec::new() };
        values.record(&mut fields);
        let mut extensions = span.extensions_mut();
        if let Some(data) = extensions.get_mut::<SpanData>() {
            apply_span_fields(data, fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = Fields { redactor: &self.redactor, body: None, attributes: Vec::new() };
        event.record(&mut fields);
        let meta = event.metadata();
        let mut attributes = fields.attributes;
        attributes.extend(code_attributes(meta));
        let mut record = otlp::log_record(
            SystemTime::now(),
            &meta.level().as_str().to_ascii_lowercase(),
            fields.body.as_deref().unwrap_or_default(),
            otlp::attributes(attributes),
        );
        if let Some(span) = ctx.event_span(event) {
            if let Some(data) = span.extensions().get::<SpanData>() {
                record["traceId"] = data.trace_id.clone().into();
                record["spanId"] = data.span_id.clone().into();
                record["flags"] = u8::from(data.sampled).into();
            }
        }
        let _ = self.tx.try_send(Item::Log(record));
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let data = span.extensions_mut().remove::<SpanData>();
        if let Some(data) = data.filter(|data| data.sampled) {
            let _ = self.tx.try_send(Item::Span(span_json(&data, SystemTime::now())));
        }
    }
}

fn console_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

fn export_filter() -> Targets {
    EXPORTER_INTERNALS.iter().fold(Targets::new().with_default(LevelFilter::INFO), |targets, name| targets.with_target(*name, LevelFilter::OFF))
}

/// The subscriber and the exporter for `source`, without installing anything (tests run it under `with_default`).
pub fn build(source: Source, extra_secrets: Vec<String>, destination: Option<Destination>) -> (Dispatch, Telemetry) {
    let redactor = Arc::new(Redactor::from_env(extra_secrets));
    let worker = destination.map(|destination| {
        let (tx, rx) = sync_channel(QUEUE);
        let name = source.service.clone();
        (tx, std::thread::Builder::new().name(format!("{name}-telemetry")).spawn(move || export_loop(rx, destination, source)))
    });
    let worker = worker.and_then(|(tx, spawned)| match spawned {
        Ok(handle) => Some((tx, handle)),
        Err(error) => {
            eprintln!("{{\"level\":\"error\",\"message\":\"telemetry.export.init.failed\",\"error\":{:?}}}", error.to_string());
            None
        }
    });
    let console = tracing_subscriber::fmt::layer()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_span_list(false)
        .with_file(true)
        .with_line_number(true)
        .with_writer(Console { redactor: redactor.clone() })
        .with_filter(console_filter());
    let export = worker.as_ref().map(|(tx, _)| Export { tx: tx.clone(), redactor: redactor.clone() }.with_filter(export_filter()));
    let subscriber = tracing_subscriber::registry().with(console).with(export);
    (Dispatch::new(subscriber), Telemetry { worker })
}

/// Installs the subscriber for the process, exporting to the ingest the environment names (if any). Keep the result alive until
/// exit. `init!` calls it with the source inferred.
pub fn install(source: Source, extra_secrets: Vec<String>) -> Telemetry {
    let (dispatch, telemetry) = build(source, extra_secrets, Destination::from_env());
    let _ = tracing::dispatcher::set_global_default(dispatch);
    telemetry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_known_secret_when_a_console_line_is_written_then_the_secret_is_replaced() {
        let redactor = Arc::new(Redactor::from_values(["hunter2-hunter2".to_string()]));
        let mut writer = ScrubbingWriter { inner: Vec::new(), redactor };

        writer.write_all(b"{\"message\":\"login hunter2-hunter2\"}\n").unwrap();

        assert_eq!(String::from_utf8(writer.inner).unwrap(), "{\"message\":\"login [REDACTED]\"}\n");
    }
}
