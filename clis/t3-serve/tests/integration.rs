//! The T3 phone's wire through the whole chain, as built binaries: `t3-serve serve` -> the real rca-service -> the real rca-exec working in a temporary
//! git repository, with the scripted CLI of `exec-testkit` standing in for the model CLI the service starts. Nothing is doubled but the model.
//! Each test drives the wire the way the phone does and records what each call did on the way (a method-hit trace, `trace.json` beside the run
//! record in `$WF_TEST_OUTPUT_DIR/t3-serve-integration`): `wf mimic coverage --app t3 --traces` marks a row served only for a passing test whose trace
//! shows the call was answered and, for a row that forwards to the agent API, that method answered in the call's window.
// the support module is the flow tests' client, of which a chain uses part
#[allow(dead_code, unused_imports)]
#[path = "flow/support/mod.rs"]
mod support;

#[path = "integration/workspace_tabs.rs"]
mod workspace_tabs;
#[path = "integration/attachments.rs"]
mod attachments;
#[path = "integration/chain.rs"]
mod chain;
#[path = "integration/guest_share.rs"]
mod guest_share;
#[path = "integration/real_agent_turn.rs"]
mod real_agent_turn;
#[path = "integration/replay_features.rs"]
mod replay_features;
#[path = "integration/checkpoint_rollback.rs"]
mod checkpoint_rollback;
#[path = "integration/rig.rs"]
mod rig;
#[path = "integration/scripted_turn_replay.rs"]
mod scripted_turn_replay;
#[path = "integration/terminal_pty.rs"]
mod terminal_pty;
#[path = "integration/thread_surface.rs"]
mod thread_surface;
#[path = "integration/voice.rs"]
mod voice;
#[path = "integration/performance.rs"]
mod performance;
#[path = "integration/catalog.rs"]
mod catalog;
