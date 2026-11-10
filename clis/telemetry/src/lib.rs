//! The one logging stack of the Rust code: the backend services (through `service-kit`), `wf` and the desktop listener.
//! A caller states its service name once; everything else a record carries is inferred. The version comes from the calling
//! crate at compile time, the commit (`GIT_COMMIT`), host and OS at start, and the file and line from each call site.
//! Every sink is scrubbed (`redact`), and without an ingest (`Destination::from_env`) nothing leaves the process.

pub mod redact;
mod destination;
mod metrics;
mod source;
#[cfg(feature = "metrics")]
mod otlp_metrics;
#[cfg(feature = "testing")]
pub mod testing;
#[cfg(feature = "oneshot")]
pub mod oneshot;
#[cfg(feature = "cli")]
pub mod cli;
#[cfg(feature = "callsite")]
pub mod callsite;
#[cfg(feature = "service")]
mod subscriber;

pub use destination::Destination;
pub use metrics::{MetricsSink, Reading};
#[cfg(feature = "metrics")]
pub use otlp_metrics::OtlpMetricsSink;
pub use source::{function_of, Site, Source};
#[cfg(feature = "service")]
pub use subscriber::{build, install, Telemetry};

/// This process's identity for `service`: the calling crate's version, and the commit and host it runs on.
#[macro_export]
macro_rules! source {
    ($service:expr) => {
        $crate::Source::new($service, env!("CARGO_PKG_VERSION"))
    };
}

/// The name of the function this is written in (the innermost named one: a closure or async block reports the function around
/// it), found at compile time; nothing is passed by hand.
#[macro_export]
macro_rules! function_name {
    () => {{
        fn __function_name_marker() {}
        $crate::function_of(::std::any::type_name_of_val(&__function_name_marker))
    }};
}

/// The call site of this line: file, line and function, all inferred.
#[macro_export]
macro_rules! site {
    () => {
        $crate::Site { file: file!(), line: line!(), function: ::std::option::Option::Some($crate::function_name!()) }
    };
}

/// A record made at this line, with its function: `record!("info", "started")`.
#[cfg(feature = "oneshot")]
#[macro_export]
macro_rules! record {
    ($level:expr, $message:expr) => {
        $crate::oneshot::Record::at($crate::site!(), $level, $message)
    };
}

/// Installs the service subscriber as the process default; keep the result alive until exit. `extra_secrets` are values the
/// environment does not name, scrubbed from every sink too.
#[cfg(feature = "service")]
#[macro_export]
macro_rules! init {
    ($service:expr) => {
        $crate::install($crate::source!($service), ::std::vec::Vec::new())
    };
    ($service:expr, $extra_secrets:expr) => {
        $crate::install($crate::source!($service), $extra_secrets)
    };
}
