//! `wf offload ask` (one prompt down the cheap-model ladder, `--batch`, `--runbook`, `--stats`, `--savings`), the ladder's admin verbs
//! (`wf offload ladder`), the rca service's device bearers (`wf offload bearers`), the pool-cooldown mirror on vm-ops-api, and the git-state worker (`wf offload plan`, `wf offload worker`). A change here
//! reruns the crates above it, which is `wf` only.
mod bearers;
mod rca_client;
mod run_format;
mod dispatch;
mod dispatch_batch;
pub mod dispatch_run;
mod help_table;
mod pricing_sync;
mod runbook;
mod worker_plan;
mod worker_run;

pub use help_table::HELP;
pub use worker_plan::run_worker;

/// What the thin binary mounts: `offload`, whose `ask` a worker runs again through the executable for each unit.
pub const COMMANDS: &[(&str, wf_kit::cli::Handler)] = &[("offload", offload)];

/// `ask` runs one prompt; the ladder's admin modes are `offload ladder`.
pub fn run_ask(rest: &[String]) -> Option<i32> {
    if rest.iter().any(|a| a == "--clear-cooldown" || a == "--pricing-sync" || a.starts_with("--clear-cooldown=") || a.starts_with("--pricing-sync=")) {
        return None;
    }
    dispatch::run(rest)
}

/// `plan <slug>` runs a units file; `--status` shows each unit.
pub fn run_plan(rest: &[String]) -> Option<i32> {
    match rest.iter().position(|a| a == "--status") {
        Some(at) => {
            let mut args = rest.to_vec();
            args.remove(at);
            worker_plan::run_status(&args)
        }
        None => worker_plan::run_work(rest),
    }
}

/// `wf offload ask|stats|plan|worker|ladder ...`: the words after `offload`.
pub fn offload(rest: &[String]) -> Option<i32> {
    match rest.split_first()? {
        (verb, tail) if verb == "ask" => run_ask(tail),
        (verb, tail) if verb == "stats" => run_ask(&std::iter::once("--stats".to_string()).chain(tail.iter().cloned()).collect::<Vec<_>>()),
        (verb, tail) if verb == "plan" => run_plan(tail),
        (verb, tail) if verb == "worker" => run_worker(tail),
        (verb, tail) if verb == "ladder" => ladder(tail),
        (verb, tail) if verb == "sessions" => sessions(tail),
        (verb, tail) if verb == "bearers" => bearers::run(tail),
        _ => None,
    }
}

/// `wf offload ladder clear-cooldown|sync-pricing`: the words after `ladder`.
pub fn ladder(rest: &[String]) -> Option<i32> {
    let (verb, more) = rest.split_first()?;
    let flag = match verb.as_str() {
        "clear-cooldown" => "--clear-cooldown",
        "sync-pricing" => "--pricing-sync",
        _ => return None,
    };
    dispatch::run(&std::iter::once(flag.to_string()).chain(more.iter().cloned()).collect::<Vec<_>>())
}

/// `wf offload sessions rm --caller <name>`: drops every session that caller made on the rca service, from opencode and from the
/// service's kept directories, and prints how many (`{"dropped": n}`). A test that is done with its sessions says so.
pub fn sessions(rest: &[String]) -> Option<i32> {
    let (verb, more) = rest.split_first()?;
    if verb != "rm" {
        return None;
    }
    let spec = wf_kit::args::Spec { values: &["caller"], flags: &[], positional: wf_kit::args::Positional::None, ..wf_kit::args::Spec::NONE };
    let parsed = wf_kit::args::parse_spec(more, &spec)?;
    let Some(caller) = parsed.val("caller").filter(|c| !c.is_empty()) else {
        wf_kit::wf_err!("wf offload sessions rm: --caller <name> is required");
        return Some(2);
    };
    let path = format!("/runs/sessions?caller={}", cli_http::encode_uri_component(caller));
    match rca_client::send("DELETE", &path, None, std::time::Duration::from_secs(60)) {
        Ok(answered) if answered.status == 200 => {
            println!("{}", answered.body);
            Some(0)
        }
        Ok(answered) => {
            wf_kit::wf_err!("wf offload sessions rm: the rca service answered {}: {}", answered.status, answered.body["error"].as_str().unwrap_or("no reason given"));
            Some(1)
        }
        Err(why) => {
            wf_kit::wf_err!("wf offload sessions rm: {why}");
            Some(1)
        }
    }
}
