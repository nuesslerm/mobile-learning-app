use crate::ports::{Checkout, Failure};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(root: &Path, args: &[&str]) -> Result<String, Failure> {
    let out = Command::new("git").arg("-C").arg(root).args(args).stdin(Stdio::null()).output().map_err(|e| Failure::new(format!("git {}: {e}", args.join(" "))))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(Failure::new(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())))
}

fn named(paths: &str, file_name: &str) -> Vec<String> {
    paths.lines().filter(|p| p.rsplit('/').next() == Some(file_name)).map(str::to_string).collect()
}

/// The files git tracks in the working directory, read from disk (so an edit not yet committed counts).
pub struct WorkingTree(pub PathBuf);

impl Checkout for WorkingTree {
    fn files(&self, file_name: &str) -> Result<Vec<(String, String)>, Failure> {
        let tracked = git(&self.0, &["ls-files"])?;
        named(&tracked, file_name).into_iter().map(|p| std::fs::read_to_string(self.0.join(&p)).map(|t| (p.clone(), t)).map_err(|e| Failure::new(format!("{p}: {e}")))).collect()
    }
}

/// The files at one revision, as committed.
pub struct GitRef {
    pub root: PathBuf,
    pub rev: String,
}

impl Checkout for GitRef {
    fn files(&self, file_name: &str) -> Result<Vec<(String, String)>, Failure> {
        let tree = git(&self.root, &["ls-tree", "-r", "--name-only", &self.rev])
            .map_err(|f| Failure::new(f.error).next(format!("git fetch origin dev: {} is not a revision here", self.rev)))?;
        named(&tree, file_name).into_iter().map(|p| git(&self.root, &["show", &format!("{}:{p}", self.rev)]).map(|t| (p, t))).collect()
    }
}
