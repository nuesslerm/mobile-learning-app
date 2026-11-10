//! The commands `wf` does not link, so a change to one rebuilds only its own binary, installed beside `wf`. `wf <command> ...`
//! runs `<binary> <command> ...` in place of this process, which hands it wf's pid, signals and exit code.
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

/// Each command word served by a separate binary, and that binary.
const EXTERNAL: &[(&str, &str)] = &[("t3", "t3-serve"), ("exec", "rca-exec"), ("mimic", "wf-mimic")];

pub fn t3(rest: &[String]) -> Option<i32> {
    run("t3", "t3", rest)
}

pub fn exec(rest: &[String]) -> Option<i32> {
    run("exec", "exec", rest)
}

pub fn mimic(rest: &[String]) -> Option<i32> {
    run("mimic", "mimic", rest)
}

/// `wf help <command>` for a command the help table has no row for: the binary's own `help <command>`, else commander's wording.
pub fn help(rest: &[String]) -> Option<i32> {
    let [command] = rest else { return None };
    run(command, "help", rest).or_else(|| {
        eprintln!("error: unknown command '{command}'");
        Some(1)
    })
}

/// `<binary> <word> <rest>...` for the binary serving `command`; `None` when none does. Returns only when it cannot run.
fn run(command: &str, word: &str, rest: &[String]) -> Option<i32> {
    let binary = EXTERNAL.iter().find(|(c, _)| *c == command)?.1;
    let next = format!("build it beside wf: cd clis && cargo build --release -p {binary} (CI installs it with wf), or put {binary} on PATH");
    let Some(path) = locate(binary) else {
        let wf = std::env::current_exe().map_or_else(|_| "wf".to_string(), |p| p.display().to_string());
        return Some(refuse(&format!("wf {command} runs the separate binary {binary}, which is neither beside {wf} nor on PATH"), binary, &next));
    };
    let error = std::process::Command::new(&path).arg(word).args(rest).exec();
    Some(refuse(&format!("cannot run {}: {error}", path.display()), binary, &next))
}

fn refuse(error: &str, binary: &str, next: &str) -> i32 {
    println!("{}", json!({ "ok": false, "error": error, "binary": binary, "next": next }));
    1
}

/// Beside the running `wf` first, so an installed tree runs its own build; then the first on PATH.
fn locate(binary: &str) -> Option<PathBuf> {
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(binary)));
    let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).map(|dir| dir.join(binary)).collect::<Vec<_>>()).unwrap_or_default();
    beside.into_iter().chain(path).find(|candidate| executable(candidate))
}

fn executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
