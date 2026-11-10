use wf_kit::cli::{run, Surface};
use wf_kit::help::Hooks;

fn main() {
    run(&Surface { version: env!("CARGO_PKG_VERSION"), commands: wf_login::COMMANDS, help: &[wf_login::HELP], hooks: Hooks::NONE, recorder: None })
}
