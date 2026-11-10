//! The service's use cases: what a turn, an attachment, a one-shot run, a voice edit and an edit-latency record do. They reach the
//! machine through ports: those of `remote-chat-agent-core`, and `environment` here. `rca-service` implements them.

pub mod agent_groups;
pub mod attachments;
pub mod edit_stats;
pub mod environment;
pub mod log;
pub mod one_shot;
pub mod opencode_client;
pub mod ports;
pub mod runs;
pub mod settings;
pub mod startup;
pub mod voice_planner;
