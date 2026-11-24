//! The flow tests of `wf t3`: the real router over a loopback socket with the clock, ids, store and log doubled (`Harness`), and
//! the real `t3-serve` binary with a SQLite state directory. A Rust client speaks the Effect RPC framing over a real WebSocket.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/access.rs"]
mod access;
#[path = "flow/agent_threads.rs"]
mod agent_threads;
#[path = "flow/binary.rs"]
mod binary;
#[path = "flow/desktop_admin.rs"]
mod desktop_admin;
#[path = "flow/desktop_profile.rs"]
mod desktop_profile;
#[path = "flow/desktop_threads.rs"]
mod desktop_threads;
#[path = "flow/desktop_vcs.rs"]
mod desktop_vcs;
#[path = "flow/crawl_decode.rs"]
mod crawl_decode;
#[path = "flow/client_activity.rs"]
mod client_activity;
#[path = "flow/guest.rs"]
mod guest;
#[path = "flow/history.rs"]
mod history;
#[path = "flow/attachments.rs"]
mod attachments;
#[path = "flow/launch.rs"]
mod launch;
#[path = "flow/mimic_t3.rs"]
mod mimic_t3;
#[path = "flow/lifecycle.rs"]
mod lifecycle;
#[path = "flow/local_host.rs"]
mod local_host;
#[path = "flow/pairing.rs"]
mod pairing;
#[path = "flow/pin_constants.rs"]
mod pin_constants;
#[path = "flow/projects.rs"]
mod projects;
#[path = "flow/pull_requests.rs"]
mod pull_requests;
#[path = "flow/preview.rs"]
mod preview;
#[path = "flow/scenarios.rs"]
mod scenarios;
#[path = "flow/schedules.rs"]
mod schedules;
#[path = "flow/rca_agent.rs"]
mod rca_agent;
#[path = "flow/rpc.rs"]
mod rpc;
#[path = "flow/orchestration.rs"]
mod orchestration;
#[path = "flow/settings.rs"]
mod settings;
#[path = "flow/terminal.rs"]
mod terminal;
#[path = "flow/vcs.rs"]
mod vcs;
#[path = "flow/voice.rs"]
mod voice;
#[path = "flow/workspace.rs"]
mod workspace;
