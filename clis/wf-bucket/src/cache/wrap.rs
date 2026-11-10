//! `wf cache nx -- <command>` and `wf cache sccache -- <command>`: run a command with one cache layer served from the bucket, the way
//! `wf test run --stage` runs Nx. The loopback server starts before the command and stops after it, its stats go to stderr, and the
//! command's exit code is the verb's, so a measurement step can time a build through it (`pipeline.yml`, `[bench-cache]`).
use super::{Layers, Scope, Shim};
use std::process::{Command, Stdio};

pub fn run(verb: &str, rest: &[String], layers: Layers) -> Option<i32> {
    let why = verb == "nx" && rest.first().is_some_and(|a| a == "--why");
    let argv: Vec<&String> = rest.iter().skip(usize::from(why)).skip_while(|a| a.as_str() == "--").collect();
    let Some((program, args)) = argv.split_first() else {
        wf_kit::wf_err!("wf cache {verb}: a command to run, e.g. `wf cache {verb} -- cargo test`");
        return Some(2);
    };
    let root = wf_kit::root::repo_root();
    let shim = Shim::start_with(&root, Scope::Tree, layers);
    let since = std::time::SystemTime::now();
    let status = Command::new(program.as_str()).args(args.iter().map(|a| a.as_str())).envs(super::env_of(&shim)).stdin(Stdio::inherit()).status();
    drop(shim);
    if why {
        explain_misses(&root, since);
    }
    match status {
        Ok(status) => Some(status.code().unwrap_or(1)),
        Err(error) => {
            wf_kit::wf_err!("wf cache {verb}: {program} did not start: {error}");
            Some(1)
        }
    }
}

/// `--why`: each task Nx ran in the command is explained against the bucket's history and recorded, on one stderr line `wf: nx why {...}`.
fn explain_misses(root: &std::path::Path, since: std::time::SystemTime) {
    let Some(home) = std::env::var_os("HOME") else { return };
    let tasks = crate::nx_why::run_tasks(std::path::Path::new(&home), since);
    if tasks.is_empty() {
        wf_kit::wf_err!("wf cache nx --why: the command left no Nx run summary, so there is nothing to explain");
        return;
    }
    eprintln!("wf: nx why {}", crate::nx_why::report(root, &tasks, true));
}
