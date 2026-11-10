//! The `tracing` subscriber of a service: JSON lines on the console (errors on stderr, with module, file and line) and, when an
//! ingest is configured, OTLP/HTTP (JSON) spans and log records to it. A log record carries the trace of the request it was
//! written in and its call site; the resource carries the source. A failing ingest never fails a request.

use crate::redact::{key_is_sensitive, Redactor, REDACTED};
use crate::{Destination, Source};
use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::trace::{SpanId, TraceContextExt as _, TraceFlags, TraceId, TracerProvider as _};
use opentelemetry::{global, Key, KeyValue};
use opentelemetry_otlp::{LogExporter, Protocol, SpanExporter, WithExportConfig as _, WithHttpConfig as _};
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::collections::HashMap;
use std::fmt::Debug;
use std::io::{self, Write};
use std::sync::Arc;
use std::time::SystemTime;
use tracing::field::{Field, Visit};
use tracing::{Dispatch, Event, Level, Metadata, Subscriber};
use tracing_opentelemetry::OtelData;
use tracing_subscriber::filter::{EnvFilter, LevelFilter, Targets};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer, SubscriberExt as _};
use tracing_subscriber::registry::{LookupSpan, SpanRef};

/// Crates whose own events must not be exported: exporting them would log about exporting.
const EXPORTER_INTERNALS: [&str; 10] =
    ["hyper", "hyper_util", "h2", "reqwest", "rustls", "opentelemetry", "opentelemetry_sdk", "opentelemetry_otlp", "opentelemetry_http", "tracing_opentelemetry"];

/// Keeps the exporters alive; flushes and stops them on drop.
#[derive(Default)]
pub struct Telemetry {
    tracer: Option<SdkTracerProvider>,
    logger: Option<SdkLoggerProvider>,
}

impl Telemetry {
    pub fn is_exporting(&self) -> bool {
        self.tracer.is_some()
    }

    /// Sends what is still queued. Blocks, so call it from `spawn_blocking` inside the runtime.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        if let Some(provider) = self.tracer.take() {
            let _ = provider.shutdown();
        }
        if let Some(provider) = self.logger.take() {
            let _ = provider.shutdown();
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.stop();
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

/// The scrubbing log bridge to OTLP: the standard `opentelemetry-appender-tracing` bridge cannot redact, so this one does, and it
/// is the one hand-written piece of the stack.
struct OtelLogs {
    logger: SdkLogger,
    redactor: Arc<Redactor>,
}

struct LogFields<'a> {
    redactor: &'a Redactor,
    body: Option<String>,
    attributes: Vec<(Key, AnyValue)>,
}

impl LogFields<'_> {
    fn put(&mut self, field: &Field, value: AnyValue) {
        if field.name() == "message" {
            if let AnyValue::String(text) = &value {
                self.body = Some(text.to_string());
            }
            return;
        }
        let value = if key_is_sensitive(field.name()) { AnyValue::from(REDACTED) } else { value };
        self.attributes.push((Key::new(field.name()), value));
    }
}

impl Visit for LogFields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        let text = self.redactor.scrub(value).into_owned();
        self.put(field, AnyValue::from(text));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, AnyValue::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, AnyValue::from(i64::try_from(value).unwrap_or(i64::MAX)));
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, AnyValue::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, AnyValue::from(value));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        let text = self.redactor.scrub(&format!("{value:?}")).into_owned();
        self.put(field, AnyValue::from(text));
    }
}

/// Where the event was written, from the call site `tracing` records at compile time.
fn code_attributes(meta: &Metadata<'_>) -> Vec<(Key, AnyValue)> {
    let mut out = vec![(Key::new("code.namespace"), AnyValue::from(meta.module_path().unwrap_or(meta.target()).to_string()))];
    out.extend(meta.file().map(|f| (Key::new("code.filepath"), AnyValue::from(f.to_string()))));
    out.extend(meta.line().map(|l| (Key::new("code.lineno"), AnyValue::Int(i64::from(l)))));
    out
}

fn severity(level: &Level) -> Severity {
    match *level {
        Level::ERROR => Severity::Error,
        Level::WARN => Severity::Warn,
        Level::INFO => Severity::Info,
        Level::DEBUG => Severity::Debug,
        Level::TRACE => Severity::Trace,
    }
}

/// The trace and span the request span belongs to, read from what `tracing-opentelemetry` keeps on it: the dispatcher cannot be
/// asked for the current span from inside an event callback.
fn trace_of<S>(span: &SpanRef<'_, S>) -> Option<(TraceId, SpanId, TraceFlags)>
where
    S: for<'a> LookupSpan<'a>,
{
    let extensions = span.extensions();
    let data = extensions.get::<OtelData>()?;
    let span_id = data.builder.span_id?;
    if data.parent_cx.has_active_span() {
        let parent = data.parent_cx.span().span_context().clone();
        return Some((parent.trace_id(), span_id, parent.trace_flags()));
    }
    let trace_id = span.scope().last().and_then(|root| root.extensions().get::<OtelData>().and_then(|data| data.builder.trace_id))?;
    Some((trace_id, span_id, TraceFlags::SAMPLED))
}

impl<S> Layer<S> for OtelLogs
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut fields = LogFields { redactor: &self.redactor, body: None, attributes: Vec::new() };
        event.record(&mut fields);
        let meta = event.metadata();
        let now = SystemTime::now();
        let mut record = self.logger.create_log_record();
        record.set_timestamp(now);
        record.set_observed_timestamp(now);
        record.set_severity_number(severity(meta.level()));
        record.set_severity_text(meta.level().as_str());
        if let Some(body) = fields.body {
            record.set_body(AnyValue::from(body));
        }
        record.add_attributes(fields.attributes);
        record.add_attributes(code_attributes(meta));
        if let Some((trace_id, span_id, flags)) = ctx.event_span(event).and_then(|span| trace_of(&span)) {
            record.set_trace_context(trace_id, span_id, Some(flags));
        }
        self.logger.emit(record);
    }
}

struct Exporters {
    tracer: SdkTracerProvider,
    logger: SdkLoggerProvider,
}

fn exporters(source: &Source, destination: &Destination) -> Result<Exporters, String> {
    let failed = |e: opentelemetry_otlp::ExporterBuildError| e.to_string();
    let headers = HashMap::from([("Authorization".to_string(), format!("Bearer {}", destination.token()))]);
    let resource = Resource::builder()
        .with_service_name(source.service.clone())
        .with_attributes(source.attributes().into_iter().map(|(k, v)| KeyValue::new(k, v)))
        .build();
    let spans = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpJson)
        .with_endpoint(destination.endpoint("/v1/traces"))
        .with_headers(headers.clone())
        .build()
        .map_err(failed)?;
    let logs = LogExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpJson)
        .with_endpoint(destination.endpoint("/v1/logs"))
        .with_headers(headers)
        .build()
        .map_err(failed)?;
    Ok(Exporters {
        tracer: SdkTracerProvider::builder().with_batch_exporter(spans).with_resource(resource.clone()).build(),
        logger: SdkLoggerProvider::builder().with_batch_exporter(logs).with_resource(resource).build(),
    })
}

fn console_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

fn export_filter() -> Targets {
    EXPORTER_INTERNALS.iter().fold(Targets::new().with_default(LevelFilter::INFO), |targets, name| targets.with_target(*name, LevelFilter::OFF))
}

/// The subscriber and the exporters for `source`, without installing anything (tests run it under `with_default`).
pub fn build(source: Source, extra_secrets: Vec<String>, destination: Option<Destination>) -> (Dispatch, Telemetry) {
    global::set_text_map_propagator(TraceContextPropagator::new());
    let redactor = Arc::new(Redactor::from_env(extra_secrets));
    let exporters = destination.and_then(|destination| match exporters(&source, &destination) {
        Ok(exporters) => Some(exporters),
        Err(message) => {
            // The export cannot report its own failure through itself.
            eprintln!("{{\"level\":\"error\",\"message\":\"telemetry.export.init.failed\",\"error\":{message:?}}}");
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
    let spans = exporters
        .as_ref()
        .map(|e| tracing_opentelemetry::layer().with_tracer(e.tracer.tracer(source.service.clone())).with_filter(export_filter()));
    let logs = exporters
        .as_ref()
        .map(|e| OtelLogs { logger: e.logger.logger(source.service.clone()), redactor: redactor.clone() }.with_filter(export_filter()));
    let subscriber = tracing_subscriber::registry().with(console).with(spans).with(logs);
    let telemetry = exporters.map_or_else(Telemetry::default, |e| Telemetry { tracer: Some(e.tracer), logger: Some(e.logger) });
    (Dispatch::new(subscriber), telemetry)
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
