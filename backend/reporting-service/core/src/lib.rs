//! The reporting service without its edges: what a sample, a series, a headroom figure and a plan report are, and the use
//! cases that take samples and answer questions about them. Storage, the Docker socket, `/proc` and the repository files
//! are ports; `reporting-service-rs` implements them.
pub mod application;
pub mod domain;
