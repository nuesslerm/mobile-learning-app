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
    pub env: Vec<(String, String)>,
}

fn stub(name: &str) -> String {
    format!("#!/bin/sh\ncase \"$1\" in\n  --version|-v|version) echo \"{name} {STUB_VERSION}\" ;;\n  *) echo \"{name}: the mimic stub answers a version query only\" >&2; exit 1 ;;\nesac\n")
}

impl Host {
    /// A fresh host under `root`: its directories exist and the stubs are in place and executable. `cli_dir` is a directory of scripted agent
    /// CLIs (`exec-testkit`) that, when given, comes before the stubs on `PATH`: a run that needs a turn answered says so.
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
        let env = vec![
            ("PATH".to_string(), cli_dir.map_or_else(|| format!("{}:/usr/bin:/bin", bin.display()), |c| format!("{}:{}:/usr/bin:/bin", c.display(), bin.display()))),
            ("HOME".to_string(), home.display().to_string()),
            ("T3CODE_HOME".to_string(), home.display().to_string()),
            ("TZ".to_string(), "UTC".to_string()),
            ("LANG".to_string(), "C.UTF-8".to_string()),
        ];
        Ok(Host { root: root.to_path_buf(), home, workspace, bin, env })
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
