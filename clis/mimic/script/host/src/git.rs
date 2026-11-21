//! The repositories a case's populated state is made of: one commit, the same bytes and so the same commit id every time.
use std::path::Path;
use std::process::{Command, Stdio};

const GIT_ENV: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "mimic"),
    ("GIT_AUTHOR_EMAIL", "mimic@example.invalid"),
    ("GIT_COMMITTER_NAME", "mimic"),
    ("GIT_COMMITTER_EMAIL", "mimic@example.invalid"),
    ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
    ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
];

/// A repository at `path` on branch `main` with a `README.md` and one commit, made with a fixed author and date.
pub fn make_repo(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(path.join("README.md"), "# mimic\n").map_err(|e| format!("{}: {e}", path.display()))?;
    let git = |args: &[&str]| -> Result<(), String> {
        let status = Command::new("git")
            .args(["-c", "commit.gpgsign=false", "-C"])
            .arg(path)
            .args(args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", path)
            .envs(GIT_ENV)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("git: {e}"))?;
        status.success().then_some(()).ok_or_else(|| format!("git {} failed", args.join(" ")))
    };
    git(&["init", "-q", "-b", "main"])?;
    git(&["add", "."])?;
    git(&["commit", "-q", "-m", "mimic"])
}

#[cfg(test)]
mod tests {
    use super::make_repo;
    use std::process::Command;

    fn head(path: &std::path::Path) -> String {
        String::from_utf8(Command::new("git").arg("-C").arg(path).args(["rev-parse", "HEAD"]).output().unwrap().stdout).unwrap().trim().to_string()
    }

    #[test]
    fn given_two_folders_when_a_repository_is_made_in_each_then_the_commit_id_is_the_same() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());

        make_repo(&a.path().join("main")).unwrap();
        make_repo(&b.path().join("other")).unwrap();

        assert_eq!(head(&a.path().join("main")), head(&b.path().join("other")));
        assert_eq!(head(&a.path().join("main")).len(), 40);
    }
}
