//! The watchdog without its edges: which containers to recreate and why (`domain`), and the sweep that asks Docker, decides and
//! reports (`application`). Docker, the clock and the event sink are ports; `watchdog-rs` implements them.
pub mod application;
pub mod domain;
