//! `wf secret`: the registry of every secret, the vault and keychain backends behind it, the sealed local copy and the writers.
//! Every `wf` crate that needs a value reads it through here, so a change reruns the tests of each of them.
mod help_table;
mod secrets;

pub use help_table::HELP;
pub use secrets::{ambient, load_all_into_process_env, load_values, run, scope_violations, secret, secret_from, update_secret_value, vault_is_authoritative, verify_for, write_urls_refused};

/// What the thin binary mounts. The full `wf` routes `secret login` to its own handler first.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("secret", run)];
