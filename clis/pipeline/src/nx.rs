//! The Nx task cache under `wf test run --stage` (owner, 18 Oct): each stage hands its tests to `nx run-many`, which replays
//! a task whose inputs hash the same as an earlier pass and runs the rest.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The workspace's own `nx` (`node_modules/.bin/nx`); `WF_NX_BIN` stands in for it in tests.
pub fn bin(root: &Path) -> Result<PathBuf, String> {
    if let Some(b) = std::env::var_os("WF_NX_BIN").filter(|b| !b.is_empty()) {
        return Ok(PathBuf::from(b));
    }
    let nx = root.join("node_modules/.bin/nx");
    if nx.is_file() { Ok(nx) } else { Err(format!("no nx at {}: the workspace is not installed", nx.display())) }
}

/// `env` is what the shim gives Nx (`cache::Shim::env`), empty for a run on the local cache alone.
fn command(nx: &Path, root: &Path, target: &str, projects: &[String], parallel: usize, env: &[(&str, String)]) -> Command {
    let mut cmd = Command::new(nx);
    cmd.args(["run-many", "-t", target, "-p", &projects.join(","), &format!("--parallel={parallel}"), "--outputStyle=stream"])
        .current_dir(root)
        .env("NX_DAEMON", "false")
        .env("NX_NO_CLOUD", "true")
        .env("NX_TUI", "false")
        // In a plugin worker, what a plugin writes to stderr before it throws can be lost when Nx exits, and `run-many` prints only
        // "Failed to process project graph"; in process, the reason (a misspelled flow `uses`, say) always reaches the log.
        .env("NX_ISOLATE_PLUGINS", "false")
        .stdin(Stdio::null());
    // `WF_NX_TRACE=1`: Nx says what it does between the banner and its first task (graph, hashes, cache lookups), which is silent otherwise
    if std::env::var_os("WF_NX_TRACE").is_some_and(|v| !v.is_empty()) {
        cmd.env("NX_VERBOSE_LOGGING", "true").env("NX_PERF_LOGGING", "true");
    }
    cmd.envs(env.iter().map(|(k, v)| (*k, v.as_str())));
    cmd
}

/// Runs `target` of `projects`, its output on stderr (stdout carries this command's JSON lines); true when every task passed.
pub fn run_many(nx: &Path, root: &Path, stage: &str, target: &str, projects: &[String], parallel: usize, env: &[(&str, String)]) -> bool {
    let mut cmd = command(nx, root, target, projects, parallel, env);
    cmd.stdout(Stdio::from(std::io::stderr()));
    let since = std::time::SystemTime::now();
    let (cpu_before, written_before) = children_cost();
    let passed = traced_status(&mut cmd);
    let (cpu_after, written_after) = children_cost();
    let seconds = since.elapsed().map(|d| d.as_secs_f64()).unwrap_or(0.0);
    eprintln!("wf: nx cost {}", serde_json::json!({ "target": target, "cpuSeconds": ((cpu_after - cpu_before) * 10.0).round() / 10.0, "writtenBytes": written_after.saturating_sub(written_before), "seconds": (seconds * 10.0).round() / 10.0 }));
    crate::nx_ledger::emit(since, stage, target);
    passed
}

/// Runs the command; under `WF_NX_TRACE=1` a thread prints the process table every 30 s, so a silent stretch of a run names what it waits for.
fn traced_status(cmd: &mut Command) -> bool {
    let trace = std::env::var_os("WF_NX_TRACE").is_some_and(|v| !v.is_empty());
    let Ok(mut child) = cmd.spawn() else { return false };
    let mut waited = 0u32;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {}
            Err(_) => return false,
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        waited += 1;
        if trace && waited.is_multiple_of(60) {
            let table = Command::new("ps").args(["-eo", "pid,ppid,etime,stat,wchan:16,args", "--sort=-etime"]).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().take(14).collect::<Vec<_>>().join("\n")).unwrap_or_default();
            eprintln!("wf: nx still running after {}s\n{table}", waited / 2);
        }
    }
}

/// CPU seconds (user and system) and bytes written to block devices by every child this process has waited for so far.
fn children_cost() -> (f64, u64) {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut usage) };
    let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    (secs(usage.ru_utime) + secs(usage.ru_stime), usage.ru_oublock as u64 * 512)
}

/// Git's own environment inside a hook: a task that runs git in a throwaway repository must not see the commit's index.
const GIT_ENV: [&str; 7] = ["GIT_INDEX_FILE", "GIT_DIR", "GIT_WORK_TREE", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_COMMON_DIR", "GIT_PREFIX"];

/// What one gate call did: Nx's exit code, the tasks it ran or replayed, and how many it replayed.
pub struct Gated {
    pub code: i32,
    pub tasks: usize,
    pub replayed: usize,
}

/// `nx run-many -t <targets> -p <projects>` for the pre-commit gate, stopping at the first failed task. Its output is shown
/// only when it fails. `env` is the shim's: with none the run replays and writes the local cache alone.
pub fn run_gate(nx: &Path, root: &Path, targets: &str, projects: &[String], parallel: usize, env: &[(&str, String)]) -> Gated {
    let mut cmd = command(nx, root, targets, projects, parallel, env);
    cmd.arg("--nxBail").env("NO_COLOR", "1").env_remove("FORCE_COLOR");
    for key in GIT_ENV {
        cmd.env_remove(key);
    }
    let since = std::time::SystemTime::now();
    let Ok(out) = cmd.output() else { return Gated { code: 1, tasks: 0, replayed: 0 } };
    crate::nx_ledger::why(since, "gate");
    let code = out.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&out.stdout);
    if code != 0 {
        use std::io::Write;
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(stdout.as_bytes());
        let _ = err.write_all(&out.stderr);
    }
    let (replayed, tasks) = replay_counts(&stdout);
    Gated { code, tasks, replayed }
}

/// `(replayed, tasks)` of one Nx run. Its closing line says `Nx read the output from the cache instead of running the command for
/// 1 out of 2 tasks.` and is left out when nothing was replayed; then the tasks are the `nx run <task>` headers it printed.
fn replay_counts(summary: &str) -> (usize, usize) {
    let said = summary.lines().find_map(|l| l.split_once("instead of running the command for ").map(|(_, counts)| counts));
    if let Some([replayed, _, _, tasks]) = said.map(|c| c.split_whitespace().take(4).collect::<Vec<_>>()).as_deref() {
        if let (Ok(r), Ok(t)) = (replayed.parse(), tasks.parse()) {
            return (r, t);
        }
    }
    let headers: std::collections::BTreeSet<&str> = summary.lines().map(|l| l.trim_start_matches(|c: char| !c.is_alphanumeric())).filter(|l| l.starts_with("nx run ")).collect();
    (0, headers.len())
}

/// The e2e tests of `platform` that would run: Nx replays every cached pass and starts the others, which with
/// `WF_E2E_PROBE` set only write their name to that file and stop (a failure, so never cached).
pub fn e2e_misses(nx: &Path, root: &Path, platform: &str, tests: &[String], env: &[(&str, String)]) -> Result<Vec<String>, String> {
    let projects: Vec<String> = tests.iter().map(|t| format!("e2e-{t}")).collect();
    let file = std::env::temp_dir().join(format!("wf-e2e-probe-{}-{}", std::process::id(), wf_kit::time::now_ms()));
    let out = command(nx, root, &format!("e2e-{platform}"), &projects, 4, env).env("WF_E2E_PROBE", &file).output().map_err(|e| format!("cannot run {}: {e}", nx.display()))?;
    let misses: Vec<String> = std::fs::read_to_string(&file).unwrap_or_default().lines().map(str::to_string).collect();
    let _ = std::fs::remove_file(&file);
    // with no task to run Nx exits 0; a failure that started no task is Nx itself failing
    if misses.is_empty() && !out.status.success() {
        let said = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
        return Err(format!("nx failed before running a task: {}", said.lines().filter(|l| !l.trim().is_empty()).rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ")));
    }
    Ok(tests.iter().filter(|t| misses.contains(t)).cloned().collect())
}

/// The summary one test left: this run's (`<out>/<test>/summary.json`) when it ran, else the cached pass Nx restored.
pub fn e2e_summary(root: &Path, out: &Path, platform: &str, test: &str, ran: bool) -> Value {
    let file = if ran { out.join(test).join("summary.json") } else { root.join(".e2e-results").join(platform).join(format!("{test}.json")) };
    std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({ "ok": false, "units": [], "note": format!("no summary at {}", file.display()) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_summary_is_a_failure_that_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let s = e2e_summary(dir.path(), &dir.path().join("out"), "browser", "t", true);
        assert_eq!(s["ok"], false);
        assert!(s["note"].as_str().unwrap().ends_with("out/t/summary.json"), "{s}");
    }
}
