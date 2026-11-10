//! The e2e tests of `wf browser` and `wf bench`'s browser journey: a real Chromium, a real ffmpeg, the real Rust daemon. The flow
//! tests (`flow.rs`) double all of them.
#[allow(dead_code)]
#[path = "flow/support/mod.rs"]
mod support;

#[path = "e2e/browser.rs"]
mod browser;
