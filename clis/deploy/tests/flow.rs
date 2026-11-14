//! The flow tests of `wf ship`, `wf check --deployed|--health` and `wf vm`: they enter through the thin `deploy` binary's argv, with
//! git, pnpm, oci, gh, bws, terraform, tailscale, apt, npm and mise replaced by stubs, and vm-ops-api, the bucket, the Expo servers,
//! ClickHouse and ORDS by fakes behind a fake tailnet.
#[path = "flow/support/mod.rs"]
mod support;

#[path = "flow/agents.rs"]
mod agents;
#[path = "flow/check.rs"]
mod check;
#[path = "flow/ci_build.rs"]
mod ci_build;
#[path = "flow/ci_port.rs"]
mod ci_port;
#[path = "flow/deploy_legs.rs"]
mod deploy_legs;
#[path = "flow/report.rs"]
mod report;
#[path = "flow/ship.rs"]
mod ship;
#[path = "flow/tailnet.rs"]
mod tailnet;
#[path = "flow/cancel_run.rs"]
mod cancel_run;
#[path = "flow/token_permissions.rs"]
mod token_permissions;
#[path = "flow/vm.rs"]
mod vm;
#[path = "flow/vm_drill.rs"]
mod vm_drill;
#[path = "flow/vm_state.rs"]
mod vm_state;
#[path = "flow/vm_state_boot.rs"]
mod vm_state_boot;
#[path = "flow/vm_stock.rs"]
mod vm_stock;
