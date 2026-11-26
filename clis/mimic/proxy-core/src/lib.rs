//! The pure core of `wf mimic proxy record|replay|check`: traffic a proxy saw becomes a sanitised cassette, a cassette answers
//! an unmodified client, and a candidate is checked against it. No I/O here: sockets, TLS, files and devices are adapters of
//! the `wf-mimic` crate, and time comes in as an argument.
pub mod application;
pub mod domain;

#[cfg(feature = "testing")]
pub mod testing;
