//! The speech-to-text adapter of the service: `Speech` implements the `SpeechToText` port over the engine the environment names.

mod speech;
#[cfg(test)]
mod tests;

pub use speech::{resolve_engine, Engine, Speech};
