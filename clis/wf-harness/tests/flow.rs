//! The flow tests of `wf android` and `wf browser`: they enter through the thin `wf-harness` binary's argv, with adb and the VM
//! replaced by stubs and the fake vm-ops-api of `wf_vmops::testing`, and the browser daemon by the mock of `wf_harness::testing`
//! (the launched-browser journeys use a real Chromium and say so when there is none).
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/android.rs"]
mod android;
#[path = "flow/browser.rs"]
mod browser;
#[path = "flow/goldens.rs"]
mod goldens;
#[path = "flow/harness.rs"]
mod harness;
#[path = "flow/maestro.rs"]
mod maestro;
#[path = "flow/surface.rs"]
mod surface;
