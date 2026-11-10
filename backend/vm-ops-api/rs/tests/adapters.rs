//! The adapters against their outside world: a captured stdout, a file in a temp dir, a loopback socket, the real clock and the
//! kernel. One module per adapter. Given/When/Then.
#[path = "adapters/binary_sha.rs"]
mod binary_sha;
#[path = "adapters/clock.rs"]
mod clock;
#[path = "adapters/docker_engine.rs"]
mod docker_engine;
#[path = "adapters/event_log.rs"]
mod event_log;
#[path = "adapters/healthcheck.rs"]
mod healthcheck;
#[path = "adapters/host_files.rs"]
mod host_files;
#[path = "adapters/http_server.rs"]
mod http_server;
#[path = "adapters/ids.rs"]
mod ids;
#[path = "adapters/opencode_log.rs"]
mod opencode_log;
#[path = "adapters/process_memory.rs"]
mod process_memory;
#[path = "adapters/siblings.rs"]
mod siblings;
#[path = "adapters/state_files.rs"]
mod state_files;
