//! The browser harness: `wf browser` (the device verbs and the daemon's own), the browser daemon that drives a Chromium framed as a
//! device, the tunnel and tailnet forward, and the `Device` and `Platform` of the browser. It builds on `harness-core`.
pub mod browser;
pub mod browser_daemon;
mod browser_launch;
mod browser_tunnel;
pub mod driver;
mod help_table;
mod slot;
mod tailnet_forward;
mod tailnet_url;
#[cfg(feature = "testing")]
pub mod testing;

pub use help_table::HELP;

/// The unit tests that read help text register this crate's table, once per test process.
#[cfg(test)]
pub(crate) fn register_help() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| wf_kit::help::register(&[HELP], harness_core::device::cli::help_hooks()));
}
