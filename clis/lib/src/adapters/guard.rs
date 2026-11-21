//! What may be published: only files git tracks and has not changed since HEAD, from a HEAD that `origin/dev` already holds, with no
//! credential-shaped string and no value the vault holds in them, and a manifest that names what ships (`include`). The library is
//! public, so the bucket can then hold nothing the public repository does not.
use crate::domain::libraries::Library;
use crate::domain::{manifest, secrets};
use crate::ports::{Failure, Guard};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Files `cargo package` writes itself, which are not in the tree.
const GENERATED: [&str; 3] = ["Cargo.lock", "Cargo.toml.orig", ".cargo_vcs_info.json"];

/// `vault` is every value the vault holds, read once by the caller: a file containing one is never published.
pub struct GitGuard {
    pub root: PathBuf,
    pub vault: Vec<String>,
}

fn out(root: &Path, program: &str, args: &[&str]) -> Result<(bool, String), Failure> {
    let result = Command::new(program).args(args).current_dir(root).stdin(Stdio::null()).output().map_err(|e| Failure::new(format!("{program} {}: {e}", args.join(" "))))?;
    Ok((result.status.success(), String::from_utf8_lossy(&result.stdout).into_owned()))
}

impl Guard for GitGuard {
    fn check(&self, library: &Library) -> Result<(), Failure> {
        let root = self.root.as_path();
        let (pushed, _) = out(root, "git", &["merge-base", "--is-ancestor", "HEAD", "origin/dev"])?;
        if !pushed {
            return Err(Failure::new("HEAD is not on origin/dev").next("push the commit first: a release publishes what the public repository already holds"));
        }
        let dir = root.join(&library.path);
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).map_err(|e| Failure::new(format!("{}/Cargo.toml: {e}", library.path)))?;
        if !manifest::names_what_ships(&manifest) {
            return Err(Failure::new(format!("{}/Cargo.toml has no `include` list", library.path)).next("add `include = [\"src/**\", \"Cargo.toml\"]` under [package]: a library ships only the paths it names, so fixtures, cassettes and tests stay out unless listed"));
        }
        let (listed, files) = out(&dir, "cargo", &["package", "--list", "--allow-dirty", "--quiet", "--manifest-path", "Cargo.toml"])?;
        if !listed {
            return Err(Failure::new(format!("cargo package --list failed in {}", library.path)).next("fix what cargo reports"));
        }
        for file in files.lines().filter(|f| !f.is_empty() && !GENERATED.contains(f)) {
            let path = format!("{}/{file}", library.path);
            if !out(root, "git", &["ls-files", "--error-unmatch", "--", &path])?.0 {
                return Err(Failure::new(format!("{path} is not tracked by git")).next("commit and push it, or exclude it in the manifest: only tracked files are published"));
            }
            if !out(root, "git", &["status", "--porcelain", "--", &path])?.1.trim().is_empty() {
                return Err(Failure::new(format!("{path} differs from HEAD")).next("commit and push it first: only committed files are published"));
            }
            let text = String::from_utf8_lossy(&std::fs::read(root.join(&path)).map_err(|e| Failure::new(format!("{path}: {e}")))?).into_owned();
            if let Some(what) = secrets::find(&text) {
                return Err(Failure::new(format!("{path} holds {what}: nothing is published")).next("read the value at runtime (environment or the vault) instead of writing it in the file, then release; if it was ever real, rotate it: the repository is public"));
            }
            if secrets::holds_value(&text, &self.vault) {
                return Err(Failure::new(format!("{path} holds a value the vault holds: nothing is published")).next("read that value at runtime (environment or the vault) instead of writing it in the file, then release; rotate it, the repository is public"));
            }
        }
        Ok(())
    }
}
