use wf_kit::cli::{run, Surface};
use wf_kit::help::Hooks;

fn main() {
    run(&Surface { version: env!("CARGO_PKG_VERSION"), commands: parity::COMMANDS, help: &[parity::HELP], hooks: Hooks::NONE, recorder: None })
}
