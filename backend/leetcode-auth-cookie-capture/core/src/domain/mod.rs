//! The rules of a capture attempt: what to wait for, which cookies make a session, how an attempt can end. Nothing here does I/O.

pub mod config;
pub mod cookies;
pub mod outcome;
pub mod sign_in;
