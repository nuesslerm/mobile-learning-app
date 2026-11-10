use wf_kit::cli::{run, Surface};
use wf_kit::help::Hooks;

fn main() {
    run(&Surface { version: env!("CARGO_PKG_VERSION"), commands: orca_serve::COMMANDS, help: &[orca_serve::HELP], hooks: Hooks::NONE, recorder: None })
}
