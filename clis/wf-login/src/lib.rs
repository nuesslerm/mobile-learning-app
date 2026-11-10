//! `wf secret login`: logging a tool in here, the capture verbs that drive the shared desktop's listener, and the listener itself
//! (`wf secret login listener`), which the login-browser image builds alone as `vnc-auth-listener`. A change reruns the crates above it,
//! which is `wf` only, and ships the desktop's deploy leg.
mod auth;
mod help_table;
mod vnc;

pub use help_table::HELP;
pub use vnc::run_listener;

/// What the thin binary mounts: `wf secret login`, the only part of `secret` this crate serves.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("secret", secret)];

fn secret(rest: &[String]) -> Option<i32> {
    match rest.split_first() {
        Some((sub, tail)) if sub == "login" => run(tail),
        _ => None,
    }
}

/// `secret login <tool> [--copy]`, and the session verbs of the capture flow: the words after `login`.
pub fn run(rest: &[String]) -> Option<i32> {
    if matches!(rest.first().map(String::as_str), Some("list" | "capture" | "done" | "launch-url" | "listener")) {
        return auth::run(rest);
    }
    if rest.iter().any(|a| a == "--remote") {
        eprintln!("wf: --remote was removed: `wf secret login <tool>` always logs in through the VM's browser (add --copy to also copy the credential to this machine)");
        return Some(2);
    }
    let args: Vec<String> = std::iter::once("login".to_string()).chain(rest.iter().cloned()).collect();
    auth::run(&args)
}
