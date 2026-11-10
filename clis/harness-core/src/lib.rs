//! What every device harness shares: the command table and Lua script API over a `Device` (`device`), the run engine and its
//! `Platform` trait (`run`, `platform`), goldens, run reports, traces, spans, recordings, and the local servers a run stands up.
//! The Android and browser harnesses (`harness-android`, `harness-browser`) implement `Platform` and `Device` on top of it, and the
//! flow harness (`harness-flow`) builds its runner and goldens on the same core.
pub mod android_logs;
pub mod browser_debug;
pub mod device;
pub mod golden;
pub mod pixelmatch;
pub mod platform;
pub mod run;
pub mod run_report;
pub mod run_stop;
pub mod spans;
pub mod staticd;
pub mod test_goldens;
pub mod trace;
pub mod video_check;
