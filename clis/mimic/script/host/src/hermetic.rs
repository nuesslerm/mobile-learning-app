//! The environment a server runs in: empty but for what it needs, so no local configuration reaches an answer. A home and a workspace of its
//! own, a fixed `PATH` whose first directory holds stub `claude`, `codex` and `opencode` that answer a version query and nothing else (the
//! agent CLIs a server looks for at start are then the same on every host), `TZ` and `LANG` pinned.
use std::path::{Path, PathBuf};

/// The stub agent CLIs, by name; each prints a fixed version line and exits.
pub const STUB_CLIS: [&str; 3] = ["claude", "codex", "opencode"];
pub const STUB_VERSION: &str = "0.0.0-mimic";

#[derive(Debug, PartialEq)]
pub struct Host {
    pub root: PathBuf,
    pub home: PathBuf,
    pub workspace: PathBuf,
    pub bin: PathBuf,
    /// This host's own copy of the scripted agent CLIs, when it was given a `cli_dir`: its scripts and invocation logs belong to it alone.
    pub cli: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

fn stub(name: &str) -> String {
    format!("#!/bin/sh\ncase \"$1\" in\n  --version|-v|version) echo \"{name} {STUB_VERSION}\" ;;\n  *) echo \"{name}: the mimic stub answers a version query only\" >&2; exit 1 ;;\nesac\n")
}

impl Host {
    /// A fresh host under `root`: its directories exist and the stubs are in place and executable. `cli_dir` is a directory of scripted agent
    /// CLIs (`exec-testkit`): its programs are linked (else copied) into `<root>/cli`, which comes before the stubs on `PATH`, so a script and an
    /// invocation log belong to one host and a case never inherits another's turn.
    pub fn prepare(root: &Path, cli_dir: Option<&Path>) -> Result<Host, String> {
        let (home, workspace, bin) = (root.join("home"), root.join("workspace"), root.join("bin"));
        for dir in [&home, &workspace, &bin] {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        for name in STUB_CLIS {
            let path = bin.join(name);
            std::fs::write(&path, stub(name)).map_err(|e| e.to_string())?;
            set_executable(&path)?;
        }
        let cli = cli_dir.map(|from| own_copy(from, &root.join("cli"))).transpose()?;
        let env = vec![
            ("PATH".to_string(), cli.as_ref().map_or_else(|| format!("{}:/usr/bin:/bin", bin.display()), |c| format!("{}:{}:/usr/bin:/bin", c.display(), bin.display()))),
            ("HOME".to_string(), home.display().to_string()),
            ("T3CODE_HOME".to_string(), home.display().to_string()),
            ("TZ".to_string(), "UTC".to_string()),
            ("LANG".to_string(), "C.UTF-8".to_string()),
        ];
        Ok(Host { root: root.to_path_buf(), home, workspace, bin, cli, env })
    }
}

/// The programs of `from` linked (else copied) into `to`; a script or a log a test left there is not carried over.
fn own_copy(from: &Path, to: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !path.is_file() || name.ends_with(".script.json") || name.ends_with(".invocations.jsonl") {
            continue;
        }
        let target = to.join(&name);
        if std::fs::hard_link(&path, &target).is_err() {
            std::fs::copy(&path, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    Ok(to.to_path_buf())
}

/// Writes what the scripted CLI `cli` does from its next start on into `dir` and forgets its earlier starts. The version line is forced, so a
/// `--version` probe of the server never consumes a turn.
pub fn write_cli_script(dir: &Path, cli: &str, turns: &serde_json::Value) -> Result<(), String> {
    let _ = std::fs::remove_file(dir.join(format!("{cli}.invocations.jsonl")));
    let script = serde_json::json!({"turns": turns, "version": format!("{cli} {STUB_VERSION}")});
    let path = dir.join(format!("{cli}.script.json"));
    std::fs::write(&path, script.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

/// The pinned T3 has no executor MCP channel and no login to write into, so a turn that calls a tool or writes a credential exists only on a candidate.
pub fn refuse_candidate_only_steps(turns: &serde_json::Value) -> Result<(), String> {
    let steps = turns.as_array().into_iter().flatten().flat_map(|t| t["steps"].as_array().into_iter().flatten());
    match steps.filter_map(|s| s["step"].as_str()).find(|s| matches!(*s, "mcp_call" | "credential")) {
        Some(step) => Err(format!("the reference cannot play a `{step}` step: it has no executor MCP channel or login to write into")),
        None => Ok(()),
    }
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn set_executable(_: &Path) -> Result<(), String> {
    Ok(())
}
