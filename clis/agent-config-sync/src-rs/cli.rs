//! The command line as a function, so the standalone binary and the Rust `wf` (`wf agent-config sync ...`)
//! share one implementation and no second process is spawned.
use crate::fsx::is_not_ours_to_write;
use crate::{guard, sync};
use std::io::Read;
use clap::{Arg, ArgAction};
use std::process::Command;

pub struct Outcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

fn project_root() -> String {
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| ".".into())
}

#[derive(Debug, PartialEq)]
enum Mode {
    Sync { check: bool },
    McpGuard,
}

/// `[sync] [--check]` or `mcp-guard`; anything else is clap's error.
fn parse(args: &[String]) -> Result<Mode, String> {
    let matches = clap::Command::new("wf agent-config sync")
        .no_binary_name(true)
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .arg(Arg::new("check").long("check").action(ArgAction::SetTrue))
        // `wf`'s own options, read and dropped wherever they stand: there is one app and nothing here prints JSON.
        .arg(Arg::new("json").long("json").hide(true).global(true).action(ArgAction::SetTrue))
        .arg(Arg::new("app").long("app").hide(true).global(true).num_args(1).allow_hyphen_values(true).action(ArgAction::Append))
        .subcommand(clap::Command::new("sync").arg(Arg::new("check").long("check").action(ArgAction::SetTrue)))
        .subcommand(clap::Command::new("mcp-guard"))
        .try_get_matches_from(args)
        .map_err(|e| e.render().to_string())?;
    Ok(match matches.subcommand() {
        Some(("mcp-guard", _)) => Mode::McpGuard,
        Some(("sync", sub)) => Mode::Sync { check: sub.get_flag("check") || matches.get_flag("check") },
        _ => Mode::Sync { check: matches.get_flag("check") },
    })
}

fn run_inner(
    args: &[String],
    stdin: &mut dyn Read,
    project_root: &dyn Fn() -> String,
    out: &mut String,
    err: &mut String,
) -> Result<i32, String> {
    let mode = match parse(args) {
        Ok(mode) => mode,
        Err(text) => {
            err.push_str(&text);
            return Ok(2);
        }
    };
    let root = project_root();
    if mode == Mode::McpGuard {
        let mut input = String::new();
        stdin
            .read_to_string(&mut input)
            .map_err(|e| e.to_string())?;
        if let Some(line) = guard::run_guard(&root, &input)? {
            out.push_str(&line);
            out.push('\n');
        }
        return Ok(0);
    }
    if mode == (Mode::Sync { check: true }) {
        let drifted: Vec<_> = sync::check_sync(&root)?
            .into_iter()
            .filter(|r| !r.ok)
            .collect();
        for r in &drifted {
            err.push_str(&format!(
                "drift: {} (run `pnpm run sync:agent-config`)\n",
                r.path
            ));
        }
        return Ok(if drifted.is_empty() { 0 } else { 1 });
    }
    for r in sync::sync(&root)? {
        out.push_str(&format!("synced: {}\n", r.path));
    }
    Ok(0)
}

pub fn run(args: &[String], stdin: &mut dyn Read) -> Outcome {
    run_in(args, stdin, &project_root)
}

/// `run` with the checkout's root given by `project_root` instead of read from git.
pub fn run_in(args: &[String], stdin: &mut dyn Read, project_root: &dyn Fn() -> String) -> Outcome {
    let (mut stdout, mut stderr) = (String::new(), String::new());
    let code = match run_inner(args, stdin, project_root, &mut stdout, &mut stderr) {
        Ok(code) => code,
        // A checkout owned by another uid is not a policy failure; a non-zero exit would hang opencode.
        Err(message) if is_not_ours_to_write(&message) => {
            stderr.push_str(&format!(
                "agent-config-sync: skipping sync, this checkout is not writable by uid {} -- {message}\n",
                nix::unistd::getuid().as_raw()
            ));
            0
        }
        // Exit 1, loud: mcp-guard is a PreToolUse hook and a swallowed failure would read as allowed.
        Err(message) => {
            stderr.push_str(&format!(
                "agent-config-sync NOT ENFORCING — failed: {message}\n"
            ));
            1
        }
    };
    Outcome {
        code,
        stdout,
        stderr,
    }
}
