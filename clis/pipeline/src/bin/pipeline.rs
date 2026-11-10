use wf_kit::cli::{run, Surface};
use wf_kit::help::Hooks;

fn main() {
    run(&Surface { version: env!("CARGO_PKG_VERSION"), commands: pipeline::COMMANDS, help: &[pipeline::HELP], hooks: Hooks::NONE, recorder: None })
}
