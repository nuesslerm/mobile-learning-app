//! The leetcode-backend service: `infrastructure` implements the ports of `leetcode-backend-core`, `api` serves them over HTTP,
//! and `main.rs` wires the two.

pub mod api;
pub mod infrastructure;
