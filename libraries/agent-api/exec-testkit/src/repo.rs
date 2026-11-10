//! A temporary git repository for the tests of what acts in a workspace: a `main` branch with one commit, and git that reads nothing of the
//! machine it runs on (no global or system configuration, no user, no signing hook), so the same test passes on a laptop and on the VM.
use std::path::Path;
use std::process::Command;

/// The environment that isolates git from the machine: its own home, no global or system configuration, a fixed author and committer. A
/// process under test that runs git (`rca-exec`) gets these too.
pub fn git_env(home: &Path) -> Vec<(String, String)> {
    let home = home.display().to_string();
    [
        ("HOME", home.as_str()),
        ("XDG_CONFIG_HOME", home.as_str()),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_SYSTEM", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_AUTHOR_NAME", "Test Author"),
        ("GIT_AUTHOR_EMAIL", "author@example.test"),
        ("GIT_COMMITTER_NAME", "Test Committer"),
        ("GIT_COMMITTER_EMAIL", "committer@example.test"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub struct TempRepo {
    dir: tempfile::TempDir,
}

/// The repository's directory and the home git is given, both inside the temporary directory.
const REPO: &str = "repo";
const HOME: &str = "home";

impl TempRepo {
    /// A repository on `main` whose first commit holds `README.md`.
    pub fn new() -> TempRepo {
        let repo = TempRepo { dir: tempfile::Builder::new().prefix("temp-repo-").tempdir().expect("a directory for the repository") };
        for sub in [REPO, HOME] {
            std::fs::create_dir(repo.dir.path().join(sub)).expect("a directory inside the temporary one");
        }
        repo.git(&["init", "--quiet", "--initial-branch=main"]);
        repo.write("README.md", "temp repo\n");
        repo.commit_all("first commit");
        repo
    }

    pub fn path(&self) -> std::path::PathBuf {
        self.dir.path().join(REPO)
    }

    /// Writes `text` to `relative`, making its directories.
    pub fn write(&self, relative: &str, text: &str) {
        let path = self.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a directory")).expect("the directory");
        std::fs::write(&path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }

    /// Runs git in the repository, isolated, and returns what it printed; a failure stops the run with git's own words.
    pub fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git").args(args).current_dir(self.path()).env_clear().envs(self.git_env()).env("PATH", std::env::var_os("PATH").unwrap_or_default()).output().expect("git starts");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim_end().to_string()
    }

    /// The isolating environment for a process that runs git on this repository, with a home of its own beside the repository.
    pub fn git_env(&self) -> Vec<(String, String)> {
        git_env(&self.dir.path().join(HOME))
    }

    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
    }

    pub fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }
}

impl Default for TempRepo {
    fn default() -> TempRepo {
        TempRepo::new()
    }
}
