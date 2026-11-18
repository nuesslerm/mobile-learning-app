//! The environment a server runs in: empty but for what it needs, so no local configuration reaches an answer. A home and a workspace of its
//! own, a fixed `PATH` whose first directory holds stub `claude`, `codex` and `opencode` that answer a version query and nothing else (the
//! agent CLIs a server looks for at start are then the same on every host), `TZ` and `LANG` pinned. A directory of real agent CLIs
//! (`real_clis`) replaces the stub and the scripted program of each CLI it holds, and is on the `PATH` too.
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
    /// The directory of real agent CLIs on this host's `PATH`, when it was given one.
    pub real: Option<PathBuf>,
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
    ///
    /// `real_clis` is a directory of real agent CLIs: each program in it is the CLI of its name, so that name gets no stub and no scripted copy
    /// (the others stay scripted or stubbed), and the directory comes after the stubs on `PATH`.
    pub fn prepare(root: &Path, cli_dir: Option<&Path>, real_clis: Option<&Path>) -> Result<Host, String> {
        let real_names = real_clis.map(programs_of).transpose()?.unwrap_or_default();
        let (home, workspace, bin) = (root.join("home"), root.join("workspace"), root.join("bin"));
        for dir in [&home, &workspace, &bin] {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        for name in STUB_CLIS.into_iter().filter(|n| !real_names.iter().any(|r| r == n)) {
            let path = bin.join(name);
            std::fs::write(&path, stub(name)).map_err(|e| e.to_string())?;
            set_executable(&path)?;
        }
        let cli = cli_dir.map(|from| own_copy(from, &root.join("cli"), &real_names)).transpose()?;
        let real = real_clis.map(Path::to_path_buf);
        let lead: Vec<String> = cli.iter().chain(std::iter::once(&bin)).chain(real.iter()).map(|d| d.display().to_string()).collect();
        let env = vec![
            ("PATH".to_string(), format!("{}:/usr/bin:/bin", lead.join(":"))),
            ("HOME".to_string(), home.display().to_string()),
            ("T3CODE_HOME".to_string(), home.display().to_string()),
            ("TZ".to_string(), "UTC".to_string()),
            ("LANG".to_string(), "C.UTF-8".to_string()),
        ];
        Ok(Host { root: root.to_path_buf(), home, workspace, bin, real, cli, env })
    }
}

/// A credential for one real agent CLI travels in the server's environment as `MIMIC_CRED_<CLI>__<VAR>` and reaches only that CLI, as `VAR`.
pub const CREDENTIAL_PREFIX: &str = "MIMIC_CRED_";

/// The credential variable that carries Codex's base64 login file (`MIMIC_CRED_CODEX__CODEX_OAUTH_JSON_B64`).
pub const CODEX_LOGIN: &str = "CODEX_OAUTH_JSON_B64";

/// `("opencode", "OPENCODE_API_KEY")` for `MIMIC_CRED_OPENCODE__OPENCODE_API_KEY`; none for any other name.
pub fn credential_of(name: &str) -> Option<(String, String)> {
    let (cli, var) = name.strip_prefix(CREDENTIAL_PREFIX)?.split_once("__")?;
    (!cli.is_empty() && !var.is_empty()).then(|| (cli.to_ascii_lowercase(), var.to_string()))
}

impl Host {
    /// Puts a wrapper in front of each real CLI that holds a credential in this host's environment: the wrapper hands the CLI its own `VAR` and
    /// drops every `MIMIC_CRED_*` before it execs the real program, so a CLI never sees another CLI's credential and no value is in an argument or a file.
    pub fn wrap_credentials(&mut self) -> Result<(), String> {
        let Some(real) = self.real.clone() else { return Ok(()) };
        let mut per_cli: std::collections::BTreeMap<String, Vec<(String, String)>> = std::collections::BTreeMap::new();
        for (name, _) in &self.env {
            if let Some((cli, var)) = credential_of(name) {
                per_cli.entry(cli).or_default().push((name.clone(), var));
            }
        }
        if per_cli.is_empty() {
            return Ok(());
        }
        let held = programs_of(&real)?;
        let wrap = self.root.join("real-wrap");
        std::fs::create_dir_all(&wrap).map_err(|e| format!("{}: {e}", wrap.display()))?;
        for (cli, vars) in per_cli {
            if !held.contains(&cli) {
                return Err(format!("a credential is declared for {cli:?}, which is not a real agent CLI of this host (a stub or a scripted program never holds one)"));
            }
            let program = real.join(&cli).display().to_string();
            let unset_all = "for n in $(env | sed -n 's/^\\(MIMIC_CRED_[A-Za-z0-9_]*\\)=.*/\\1/p'); do unset \"$n\"; done\n";
            let mut script = String::from("#!/bin/sh\n");
            if let Some((carrier, _)) = vars.iter().find(|(_, var)| var == CODEX_LOGIN) {
                // Codex reads a login file, not a key: it gets a CODEX_HOME of its own (a private tmpfs directory when there is one) holding the decoded login for the life of
                // the process only; the directory is removed when it exits or is stopped. A login Codex rotates is not written back anywhere.
                script.push_str(&format!(
                    "umask 077\nbase=/dev/shm; [ -d \"$base\" ] && [ -w \"$base\" ] || base=\"${{TMPDIR:-/tmp}}\"\nCODEX_HOME=$(mktemp -d \"$base/mimic-codex.XXXXXX\") || exit 1\nexport CODEX_HOME\nprintf %s \"${carrier}\" | base64 -d > \"$CODEX_HOME/auth.json\" || {{ rm -rf \"$CODEX_HOME\"; exit 1; }}\n"
                ));
                for (carrier, var) in vars.iter().filter(|(_, var)| var != CODEX_LOGIN) {
                    script.push_str(&format!("{var}=\"${carrier}\"\nexport {var}\n"));
                }
                script.push_str(unset_all);
                script.push_str(&format!("'{program}' \"$@\" &\nchild=$!\ntrap 'kill $child 2>/dev/null; wait $child 2>/dev/null; rm -rf \"$CODEX_HOME\"; exit 143' TERM INT HUP\nwait $child\nstatus=$?\nrm -rf \"$CODEX_HOME\"\nexit $status\n"));
            } else {
                for (carrier, var) in &vars {
                    script.push_str(&format!("{var}=\"${carrier}\"\nexport {var}\n"));
                }
                script.push_str(unset_all);
                script.push_str(&format!("exec '{program}' \"$@\"\n"));
            }
            let path = wrap.join(&cli);
            std::fs::write(&path, script).map_err(|e| format!("{}: {e}", path.display()))?;
            set_executable(&path)?;
        }
        let dir = wrap.display().to_string();
        let real_dir = real.display().to_string();
        for (k, v) in &mut self.env {
            if k == "PATH" {
                *v = v.replacen(&real_dir, &format!("{dir}:{real_dir}"), 1);
            }
        }
        Ok(())
    }
}

/// The names of the programs in `dir`.
fn programs_of(dir: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(entries.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
}

/// The programs of `from` linked (else copied) into `to`, but the ones a real CLI stands in for; a script or a log a test left there is not carried over.
fn own_copy(from: &Path, to: &Path, skip: &[String]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !path.is_file() || name.ends_with(".script.json") || name.ends_with(".invocations.jsonl") || skip.contains(&name) {
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
