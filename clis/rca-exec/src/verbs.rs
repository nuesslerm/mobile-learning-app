//! `wf exec serve` (`rca-exec serve`): parse the line, build the local executor, attach it to rca and serve until the channel ends. Every
//! line on stdout is one JSON object (`{ok: true, event: "attached"}` once the channel is open, then `closed`); a failure prints
//! `{ok: false, error, next}` and exits 1, a malformed line exits 2. No verb prompts; the bearer is read from the environment variable
//! `--secret-env` names, never from the command line.
use crate::adapters::exec_channel::ExecChannel;
use crate::adapters::local_executor::LocalExecutor;
use crate::domain::ToolExecutor;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};

const SERVE: Spec = Spec { values: &["rca-url", "workspace", "executor", "root", "worktree-base", "browse-allow", "secret-env"], flags: &["reconnect", "git-hooks"], positional: Positional::None, ..Spec::NONE };
const DEFAULT_BEARER_ENV: &str = "CHAT_AGENT_API_SECRET";

pub fn run(args: &[String]) -> Option<i32> {
    let (verb, rest) = args.split_first()?;
    match verb.as_str() {
        "serve" => serve_verb(&parse_spec(rest, &SERVE)?),
        _ => None,
    }
}

/// `rca-exec serve ...`: the binary's own spelling of `exec serve`.
pub fn serve_alone(args: &[String]) -> Option<i32> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", wf_kit::help::text("exec serve")?.replace("wf exec serve", "rca-exec serve"));
        return Some(0);
    }
    serve_verb(&parse_spec(args, &SERVE)?)
}

/// Opens the channel again after it dropped, for the same executor: its ptys and processes go on. Gives up after about a minute.
fn reattach(url: &str, id: Option<&str>, workspace: &str, bearer: &str, executor: &Arc<dyn ToolExecutor>) -> Option<ExecChannel> {
    for _ in 0..300 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Ok(channel) = ExecChannel::open_as(url, id, workspace, bearer, executor.clone()) {
            return Some(channel);
        }
    }
    None
}

fn failure(error: impl Into<String>, next: &str, extra: Value) -> i32 {
    let error = error.into();
    wf_kit::logfmt::mcp("error", "exec", "wf exec serve failed", &[("error", error.clone())]);
    let mut out = json!({"ok": false, "error": error, "next": next});
    if let (Some(out), Some(extra)) = (out.as_object_mut(), extra.as_object()) {
        out.extend(extra.clone());
    }
    println!("{out}");
    1
}

fn missing(flag: &str) -> i32 {
    println!("{}", json!({"ok": false, "error": format!("--{flag} is required"), "next": "see wf exec serve --help"}));
    2
}

fn serve_verb(p: &Parsed) -> Option<i32> {
    let (Some(url), Some(workspace), Some(root)) = (p.val("rca-url"), p.val("workspace"), p.val("root")) else {
        return Some(missing(["rca-url", "workspace", "root"].iter().find(|f| p.val(f).is_none()).copied().unwrap_or("rca-url")));
    };
    let bearer_env = p.val("secret-env").unwrap_or(DEFAULT_BEARER_ENV);
    let Some(bearer) = std::env::var(bearer_env).ok().filter(|v| !v.is_empty()) else {
        return Some(failure(format!("the environment variable {bearer_env} is empty or unset"), "export the rca service's bearer in that variable (wf secret env) or name another with --secret-env", json!({})));
    };
    let allow: Vec<PathBuf> = p.val("browse-allow").map(|list| list.split(',').filter(|d| !d.is_empty()).map(PathBuf::from).collect()).unwrap_or_default();
    let built = LocalExecutor::new(&PathBuf::from(root)).and_then(|e| match p.val("worktree-base") {
        Some(base) => e.with_worktree_base(&PathBuf::from(base)),
        None => Ok(e),
    });
    let executor: Arc<dyn ToolExecutor> = match built.and_then(|e| e.with_browse_allow(&allow)) {
        Ok(executor) => Arc::new(executor.with_git_hooks(p.flag("git-hooks")).with_skill_home(std::env::var_os("HOME").map(PathBuf::from).as_deref())),
        Err(error) => return Some(failure(error, "pass an existing directory as --root; --worktree-base must be a directory outside it and --browse-allow existing directories", json!({}))),
    };
    let channel = match ExecChannel::open_as(url, p.val("executor"), workspace, &bearer, executor.clone()) {
        Ok(channel) => channel,
        Err(error) => return Some(failure(error, "check --rca-url (http://host:port) and that the bearer is the rca service's", json!({}))),
    };
    let mut attached = json!({"ok": true, "event": "attached", "workspace": workspace, "executor": p.val("executor").unwrap_or(workspace), "root": root, "rcaUrl": url});
    if let Some(base) = p.val("worktree-base") {
        attached["worktreeBase"] = json!(base);
    }
    println!("{attached}");
    let reconnect = p.flag("reconnect");
    let mut outcome = channel.wait();
    while reconnect && !matches!(outcome, Ok(Some(exec_proto::REPLACED))) {
        match reattach(url, p.val("executor"), workspace, &bearer, &executor) {
            Some(again) => {
                println!("{}", json!({"ok": true, "event": "reattached", "workspace": workspace}));
                outcome = again.wait();
            }
            None => break,
        }
    }
    Some(match outcome {
        Ok(Some(exec_proto::REPLACED)) => failure("replaced by a newer executor for this workspace", "stop the other executor of this workspace, or give each one its own --workspace", json!({"closeCode": exec_proto::REPLACED})),
        Ok(code) => {
            println!("{}", json!({"ok": true, "event": "closed", "closeCode": code}));
            0
        }
        Err(error) => failure(error, "restart rca-exec serve; the channel to rca dropped", json!({})),
    })
}
