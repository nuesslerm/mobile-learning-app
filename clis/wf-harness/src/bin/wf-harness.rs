use wf_kit::cli::{run, Surface};

fn main() {
    run(&Surface { version: env!("CARGO_PKG_VERSION"), commands: wf_harness::COMMANDS, help: &wf_harness::HELP, hooks: wf_harness::device::cli::help_hooks(), recorder: None })
}
