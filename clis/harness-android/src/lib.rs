//! The Android harness: `wf android` (adb, the on-device UI server, the device lease, the lab on the VM) and the `Device` and
//! `Platform` of an Android device. It builds on `harness-core`.
pub mod android;
mod android_features;
mod android_input;
pub mod android_lab;
pub mod android_lease;
mod android_u2;
pub mod driver;
mod help_table;
mod stub_server;

pub use help_table::HELP;

/// The unit tests that read help text register this crate's table, once per test process.
#[cfg(test)]
pub(crate) fn register_help() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| wf_kit::help::register(&[HELP], harness_core::device::cli::help_hooks()));
}
