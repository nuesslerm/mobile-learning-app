//! Events for the service's own modules, written through `tracing` (the console line and the OTLP export are `service-kit`'s). The
//! event name is the message and the metadata rides in one `meta` object. Never pass a token, a cookie value or a credential.
//! A call site names typed fields, `log::info!("server.listening", port = port)`; the slice functions serve the sites whose fields are built at run time.

use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn meta_of(meta: &[(&str, Value)]) -> Value {
    Value::Object(meta.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect::<Map<_, _>>())
}

/// A field's value as the `meta` object carries it.
pub fn field<T: serde::Serialize + ?Sized>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[macro_export]
macro_rules! log_event {
    ($level:ident, $event:expr $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::application::log::$level($event, &[$((stringify!($key), $crate::application::log::field(&$value))),*])
    };
}

#[macro_export]
macro_rules! log_info {
    ($($args:tt)*) => { $crate::log_event!(info, $($args)*) };
}
#[macro_export]
macro_rules! log_warn {
    ($($args:tt)*) => { $crate::log_event!(warn, $($args)*) };
}
#[macro_export]
macro_rules! log_error {
    ($($args:tt)*) => { $crate::log_event!(error, $($args)*) };
}
pub use crate::{log_error as error, log_info as info, log_warn as warn};

pub fn info(event: &str, meta: &[(&str, Value)]) {
    tracing::info!(meta = %meta_of(meta), "{event}");
}

pub fn warn(event: &str, meta: &[(&str, Value)]) {
    tracing::warn!(meta = %meta_of(meta), "{event}");
}

pub fn error(event: &str, meta: &[(&str, Value)]) {
    tracing::error!(meta = %meta_of(meta), "{event}");
}
