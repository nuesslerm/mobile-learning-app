//! The VM as every other `wf` crate reaches it: signed vm-ops-api requests over the tailnet, `wf vm exec`, the tailnet name resolver, the
//! device-run sampler, and what runs there (the apps, the commit each was built from, the device image). A change reruns every crate that talks to it.
pub mod betterstack;
mod help_table;
pub mod host_summary;
pub mod services;
pub mod tailnet_resolve;
#[cfg(feature = "testing")]
pub mod testing;
pub mod vm_exec;
pub mod vm_sampler;
pub mod vmops;

pub use help_table::HELP;

/// What the thin binary mounts: the two commands of `wf vm` that need nothing but vm-ops-api and the tailnet.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("vm", run_vm)];

/// `wf vm exec <file>` and `wf vm net resolve <name>...`: the words after `vm`.
fn run_vm(rest: &[String]) -> Option<i32> {
    match rest.split_first()? {
        (verb, tail) if verb == "exec" => vm_exec::run(tail),
        (noun, tail) if noun == "net" => match tail.split_first()? {
            (verb, names) if verb == "resolve" => tailnet_resolve::run(names),
            _ => None,
        },
        _ => None,
    }
}
