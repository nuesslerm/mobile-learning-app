//! `wf secret login listener` for the login-browser image: the listener of the wf-login crate, built alone with the crates it builds
//! on. The vnc-auth-session leg (clis/pipeline/src/legs.rs) and the image's builder stage list the directories of that cargo closure.
use wf_kit::{args, env_file};

fn main() {
    wf_kit::help::register(&[wf_login::HELP], wf_kit::help::Hooks::NONE);
    let argv: Vec<String> = std::env::args().skip(1).collect();
    env_file::load();
    let code = wf_login::run_listener(&argv).unwrap_or_else(|| {
        eprint!("{}", args::take_error().unwrap_or_default());
        2
    });
    std::process::exit(code);
}
