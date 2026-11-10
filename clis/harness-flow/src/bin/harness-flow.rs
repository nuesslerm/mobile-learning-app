use wf_kit::cli::{run, Surface};

fn main() {
    harness_core::run::set_publisher(|_, _, _| Err("this binary publishes no run: use wf".to_string()));
    run(&Surface { version: env!("CARGO_PKG_VERSION"), commands: harness_flow::COMMANDS, help: &[harness_flow::HELP], hooks: wf_kit::help::Hooks::NONE, recorder: None })
}
