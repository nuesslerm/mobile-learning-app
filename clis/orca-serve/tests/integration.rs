//! The Orca phone's wire through the whole chain, as built binaries: `orca-serve` -> the real rca-service -> the real rca-exec working in a
//! temporary git repository, with the scripted CLI of `exec-testkit` standing in for the model CLI the service starts. Nothing is doubled but
//! the model. Each test drives the wire the way the phone does (the handshake and sealed frames of `orca-crypto`, whose vectors the pinned
//! reference recorded) and records what each call did on the way (a method-hit trace, `trace.json` beside the run record in
//! `$WF_TEST_OUTPUT_DIR/orca-serve-integration`): `wf mimic coverage --app orca --traces` marks a row served only for a passing test whose trace
//! shows the call was answered and not merely refused.
#[path = "integration/agent_sessions.rs"]
mod agent_sessions;
#[path = "integration/agent_catalog.rs"]
mod agent_catalog;
#[path = "integration/mimic_x2.rs"]
mod mimic_x2;
#[path = "integration/mimic_x6.rs"]
mod mimic_x6;
#[path = "integration/replay_features.rs"]
mod replay_features;
#[path = "integration/rig.rs"]
mod rig;
#[path = "integration/speech.rs"]
mod speech;
#[path = "integration/terminal_pty.rs"]
mod terminal_pty;
