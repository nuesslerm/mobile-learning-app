//! The listener's structured log lines: one scrubbed JSON line to stdout for `docker logs`, and the same record to the
//! BetterStack OTLP ingest when one is configured, sent off the calling thread. Each line carries its caller's file and line.
//! Event names are static; variables ride in the fields. Never pass a link, a token, a cookie value or a credential.
use ::telemetry::oneshot::{self, Record};
use ::telemetry::redact::Redactor;

#[track_caller]
fn emit(level: &'static str, event: &str, fields: &[(&str, String)]) {
    let record = fields.iter().fold(Record::new(level, event), |r, (k, v)| r.attr(*k, v.as_str()));
    println!("{}", record.line(&Redactor::from_env(Vec::new())));
    std::thread::spawn(move || oneshot::emit(&::telemetry::source!("vnc-auth-capture"), &record));
}

#[track_caller]
pub fn info(event: &str, fields: &[(&str, String)]) {
    emit("info", event, fields);
}

#[track_caller]
pub fn error(event: &str, fields: &[(&str, String)]) {
    emit("error", event, fields);
}
