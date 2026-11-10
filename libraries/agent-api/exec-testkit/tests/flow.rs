//! The kit against the processes it is made of: the scripted CLI started as `claude`, `codex` and `opencode` (a real process that prints, logs,
//! calls a tool over HTTP, exits or hangs), the temporary repository (real git), and the recordings with the index that vouches for them.
#[path = "flow/cli.rs"]
mod cli;
#[path = "flow/hosted.rs"]
mod hosted;
#[path = "flow/recordings.rs"]
mod recordings;
#[path = "flow/repo.rs"]
mod repo;
#[path = "flow/tap.rs"]
mod tap;
#[path = "flow/trace.rs"]
mod trace;
