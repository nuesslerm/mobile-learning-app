//! The application layer of leetcode-backend: what the service does (relay to leetcode.com under a stored session, run and
//! format code locally, keep the login capture exclusive) and the ports it needs from outside. It names no HTTP framework, no
//! HTTP client and no process API; the service crate implements the ports and wires them.

pub mod application;

#[cfg(feature = "testing")]
pub mod testing;
