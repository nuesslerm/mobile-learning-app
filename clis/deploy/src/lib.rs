//! `wf ship` (build and push images on the VM and redeploy a service, publish the app as an EAS Update, retire a service, report the
//! deployer, plan what a push deploys), `wf check --deployed` (the verification of a deployed commit) and `wf check --health`, and
//! `wf vm`: the machine's own verbs (health, logs, recovery, drills, the agent dependency manifest, the Oracle credentials and
//! Terraform) and the tailnet's devices and auth key. A change here reruns `wf` only.
mod agent_deps;
mod build_image;
mod ci_build;
mod ci_runner;
mod runner_check;
mod cancel_run;
mod install_clis;
mod token_permissions;
mod deploy;
mod deploy_run;
mod help_table;
mod infra;
mod report_health;
mod report_run;
mod smoke;
mod ship;
pub mod ship_eas;
mod tailscale;
mod tailscale_up;
mod vm_drill;
mod vm_logs;
mod host_history;
mod vm_ops;
mod vm_recover;
mod vm_state;
pub mod verify_tier3;

use wf_kit::args::probe;

pub use help_table::HELP;
pub use ship::run;

/// What the thin binary mounts: `ship`, `check` (`--deployed` and `--health`), `vm`, and `secret`, which the health report runs again
/// through the executable.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("check", check), ("ship", run), ("vm", run_vm), ("secret", secrets::run)];

/// `check --health`, or `check --deployed <sha>`: the real run, or its plan with `--dry-run`. `wf` adds the staged gate, the budget
/// and the other gates to the same line.
pub fn check(rest: &[String]) -> Option<i32> {
    if let Some(at) = rest.iter().position(|a| a == "--health") {
        let mut args = rest.to_vec();
        args.remove(at);
        return report_health::run(&args);
    }
    let mut args = rest.to_vec();
    if let Some(at) = args.iter().position(|a| a == "--deployed") {
        args.remove(at);
    }
    let modes = probe(&args, &[&verify_tier3::DRY_RUN, &verify_tier3::SPEC])?;
    if modes.flag("dry-run") {
        verify_tier3::dry_run(&args)
    } else {
        verify_tier3::run(&args)
    }
}

fn words(parts: &[&str], rest: &[String]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).chain(rest.iter().cloned()).collect()
}

/// `wf vm`: the words after `vm`.
pub fn run_vm(rest: &[String]) -> Option<i32> {
    let (sub, tail) = rest.split_first()?;
    match sub.as_str() {
        "exec" => wf_vmops::vm_exec::run(tail),
        "logs" => vm_logs::run(tail),
        "recover" => vm_recover::run(tail),
        "ledger" => vm_ops::ledger(tail),
        "ops" => vm_ops::run(tail),
        "deps" => deps(tail),
        "creds" => match tail.split_first() {
            Some((verb, more)) if verb == "sync" => infra::run_sync_creds(more),
            _ => None,
        },
        "terraform" => infra::run_tf(tail),
        "state" => vm_state::run(tail),
        "net" => net(tail),
        _ => None,
    }
}

fn deps(rest: &[String]) -> Option<i32> {
    match agent_deps::run(rest) {
        Ok(code) => code,
        Err(message) => {
            wf_kit::wf_err!("wf: {message}");
            Some(1)
        }
    }
}

/// `vm net`: the tailnet.
fn net(rest: &[String]) -> Option<i32> {
    let (sub, tail) = rest.split_first()?;
    match (sub.as_str(), tail.split_first()) {
        ("up", _) => tailscale::run(rest),
        ("auth-key", Some((verb, more))) if verb == "status" => tailscale::run(&words(&["auth-key-status"], more)),
        // Any other action reaches the handler, which names the one it supports.
        ("auth-key", Some(_)) => tailscale::run(rest),
        ("device", _) => tailscale::run(&words(&["devices"], tail)),
        ("resolve", _) => wf_vmops::tailnet_resolve::run(tail),
        _ => None,
    }
}

