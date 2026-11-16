//! The console-sink line pins run in a binary of their own: the sink is the process-global tracing subscriber, so a service started by
//! a sibling flow would write its startup lines into the capture.
#![allow(dead_code)]

#[path = "flow/doubles.rs"]
mod doubles;
#[path = "flow/harness.rs"]
mod harness;
#[path = "flow/log_lines.rs"]
mod log_lines;
