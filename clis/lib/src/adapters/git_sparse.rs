//! Our registry kept in the repository: the same sparse layout as the bucket (`registry/crates/`, `registry/index/`), written to the
//! working tree, committed and pushed to `dev`. The public repository then serves it at `raw.githubusercontent.com`, so no write link
//! and no bucket are involved.
use super::bucket_sparse::BucketSparse;
use crate::adapters::index_source::PublicIndex;
use crate::ports::{Failure, Published, Publisher, Release};
use blob_cache::adapters::DirStore;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const DIR: &str = "registry";

pub struct GitSparse {
    root: PathBuf,
    inner: BucketSparse,
}

fn git(root: &Path, args: &[&str]) -> Result<String, Failure> {
    // The vault credentials the guard needed are not for git's hooks: they run hermetic tests that must not see them.
    let out = Command::new("git").arg("-C").arg(root).args(args).env_remove("BWS_ACCESS_TOKEN").env_remove("BWS_ADMIN_ACCESS_TOKEN").stdin(Stdio::null()).output().map_err(|e| Failure::new(format!("git {}: {e}", args.join(" "))))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    Err(Failure::new(format!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim())))
}

impl GitSparse {
    pub fn new(root: PathBuf, index_url: &str) -> Result<GitSparse, Failure> {
        let registry = root.join(DIR);
        let inner = BucketSparse::new(Box::new(DirStore::new(&registry)), Box::new(PublicIndex::new(Box::new(DirStore::new(registry.join("index"))))), index_url)?;
        Ok(GitSparse { root, inner })
    }

    /// The registry as `origin/dev` holds it must be the registry here: an entry appended to a stale index would not merge.
    fn up_to_date(&self) -> Result<(), Failure> {
        git(&self.root, &["fetch", "-q", "origin", "dev"])?;
        if git(&self.root, &["diff", "--name-only", "HEAD", "origin/dev", "--", DIR])?.trim().is_empty() {
            return Ok(());
        }
        Err(Failure::new("origin/dev holds registry entries this checkout does not").next("git pull --rebase origin dev, then release again"))
    }

    fn undo(&self) {
        let _ = git(&self.root, &["reset", "-q", "--soft", "HEAD~1"]);
        self.discard();
    }

    fn discard(&self) {
        let _ = git(&self.root, &["reset", "-q", "--", DIR]);
        let _ = git(&self.root, &["checkout", "-q", "--", DIR]);
        let _ = git(&self.root, &["clean", "-fdq", "--", DIR]);
    }
}

impl Publisher for GitSparse {
    fn id(&self) -> &'static str {
        "git-sparse"
    }

    fn already_released(&self, name: &str, version: &str) -> Result<bool, Failure> {
        self.up_to_date()?;
        self.inner.already_released(name, version)
    }

    fn publish(&self, release: &Release) -> Result<Published, Failure> {
        let published = self.inner.publish(release)?;
        let message = format!("registry: {} {}", release.library.name, release.version);
        let done = git(&self.root, &["add", "--", DIR])
            .and_then(|_| git(&self.root, &["commit", "-q", "-m", &message, "--", DIR]))
            .inspect_err(|_| self.discard())
            .and_then(|_| git(&self.root, &["push", "-q", "origin", "HEAD:dev"]).inspect_err(|_| self.undo()));
        done.map_err(|f| Failure::new(f.error).next("nothing was published and the manifest is as it was; fix what git reports and release again"))?;
        Ok(published)
    }
}
