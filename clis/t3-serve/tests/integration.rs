//! The T3 phone's wire through the whole chain, as built binaries: `t3-serve serve` -> the real rca-service -> the real rca-exec working in a temporary
//! git repository, with the scripted CLI of `exec-testkit` standing in for the model CLI the service starts. Nothing is doubled but the model.
//! Each test drives the wire the way the phone does and records what each call did on the way (a method-hit trace, `trace.json` beside the run
//! record in `$WF_TEST_OUTPUT_DIR/t3-serve-integration`): `wf parity coverage --app t3 --traces` marks a row served only for a passing test whose trace
//! shows the call was answered and, for a row that forwards to the agent API, that method answered in the call's window.
// the support module is the flow tests' client, of which a chain uses part
#[allow(dead_code, unused_imports)]
#[path = "flow/support/mod.rs"]
mod support;

#[path = "integration/attachments_m12.rs"]
mod attachments_m12;
#[path = "integration/chain.rs"]
mod chain;
#[path = "integration/hosted_m11.rs"]
mod hosted_m11;
#[path = "integration/local_m5.rs"]
mod local_m5;
#[path = "integration/preview_m12.rs"]
mod preview_m12;
#[path = "integration/recorded_r1.rs"]
mod recorded_r1;
#[path = "integration/rollback_m12.rs"]
mod rollback_m12;
#[path = "integration/projects_m12.rs"]
mod projects_m12;
#[path = "integration/prs_m12.rs"]
mod prs_m12;
#[path = "integration/rig.rs"]
mod rig;
#[path = "integration/schedules_m12.rs"]
mod schedules_m12;
#[path = "integration/settings_m10.rs"]
mod settings_m10;
#[path = "integration/terminal_m8.rs"]
mod terminal_m8;
#[path = "integration/thread_surface.rs"]
mod thread_surface;
#[path = "integration/vcs_m9.rs"]
mod vcs_m9;
#[path = "integration/workspace_m6.rs"]
mod workspace_m6;
