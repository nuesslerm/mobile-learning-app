//! The Orca phone's wire through the whole chain, as built binaries: `orca-serve` -> the real rca-service -> the real rca-exec working in a
//! temporary git repository, with the scripted CLI of `exec-testkit` standing in for the model CLI the service starts. Nothing is doubled but
//! the model. Each test drives the wire the way the phone does (the handshake and sealed frames of `orca-crypto`, whose vectors the pinned
//! reference recorded) and records what each call did on the way (a method-hit trace, `trace.json` beside the run record in
//! `$WF_TEST_OUTPUT_DIR/orca-serve-integration`): `wf parity coverage --app orca --traces` marks a row served only for a passing test whose trace
//! shows the call was answered and not merely refused.
#[path = "integration/agent_m7.rs"]
mod agent_m7;
#[path = "integration/agent_catalog_m7b.rs"]
mod agent_catalog_m7b;
#[path = "integration/git_m9.rs"]
mod git_m9;
#[path = "integration/hosted_m11.rs"]
mod hosted_m11;
#[path = "integration/mimic_x2.rs"]
mod mimic_x2;
#[path = "integration/mimic_x6.rs"]
mod mimic_x6;
#[path = "integration/recorded_r1.rs"]
mod recorded_r1;
#[path = "integration/rig.rs"]
mod rig;
#[path = "integration/schedules_m12.rs"]
mod schedules_m12;
#[path = "integration/settings_m10.rs"]
mod settings_m10;
#[path = "integration/terminal_m8.rs"]
mod terminal_m8;
#[path = "integration/transport_m5.rs"]
mod transport_m5;
#[path = "integration/web_m5.rs"]
mod web_m5;
#[path = "integration/workspace_m6.rs"]
mod workspace_m6;
