//! The doubles of the flow and integration tests: the shared test kit, which runs `knowledge` (or `WF_BIN`) with stubs first on its PATH,
//! and `Gem`, a repository with the stub `pnpm` and `wf` that the Gem sync needs.
pub use wf_testkit::*;
use std::path::PathBuf;
use std::time::Duration;

/// A repository with a stub `pnpm` (repomix: the compressed run writes "skeleton", the full one the text of the `pack` file, "PACK" at
/// first, and reports three files) and a stub `wf` whose `offload ask` answers from ANSWERS (one line per attempt) and counts its calls.
pub struct Gem {
    pub dir: tempfile::TempDir,
    pub repo: PathBuf,
    pub bin: PathBuf,
}

// the flow tests and the integration tests are two binaries that each use part of this fixture
#[allow(dead_code)]
impl Gem {
    pub fn new(answers: &[&str]) -> Gem {
        let dir = tempfile::Builder::new().prefix("wf-flow-gemini-").tempdir().unwrap();
        let (repo, bin) = (dir.path().join("repo"), dir.path().join("bin"));
        std::fs::create_dir_all(repo.join("docs/gemini-context")).unwrap();
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(repo.join("CLAUDE.md"), "layering rules\n").unwrap();
        std::fs::write(repo.join("CONCEPTS.md"), "vocabulary\n").unwrap();
        std::fs::write(repo.join("docs/gemini-context/architecture-prompt.md"), "Write ARCHITECTURE.md.\n").unwrap();
        let pack = dir.path().join("pack");
        std::fs::write(&pack, "PACK").unwrap();
        create_sh_stub(&bin.join("pnpm"), &format!("out=\"\"; mode=pack\nwhile [ $# -gt 0 ]; do case \"$1\" in --output) out=\"$2\";; --compress) mode=skeleton;; esac; shift; done\nif [ $mode = skeleton ]; then printf 'skeleton' > \"$out\"; else printf '## File: a.ts\\n%s\\n' \"$(cat {pack})\" > \"$out\"; fi\necho \"Total Files: 3\"", pack = pack.display()));
        let answers_file = dir.path().join("answers");
        std::fs::write(&answers_file, answers.join("\n")).unwrap();
        let calls = dir.path().join("calls");
        create_sh_stub(&bin.join("wf"), &format!("for last; do :; done; echo \"$@\" >> {calls}; n=$(wc -l < {calls}); grep -q -- '--agent read-only --shape document --prompt-file' {calls} || exit 8; sed -n \"${{n}}p\" {answers} | sed 's/\\\\n/\\n/g'; grep -q 'Write ARCHITECTURE.md' \"$last\" && grep -q '===== skeleton' \"$last\" && grep -q '===== CLAUDE.md' \"$last\" || exit 9", calls = calls.display(), answers = answers_file.display()));
        Gem { dir, repo, bin }
    }

    pub fn index(&self, args: &[&str]) -> RunResult {
        self.index_with_env(args, &[])
    }

    /// `knowledge index --gemini` with extra environment, as the workflow gives it (the destination and the job summary file).
    pub fn index_with_env(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        let mut full = vec!["knowledge", "index", "--gemini"];
        full.extend_from_slice(args);
        let opts = env.iter().fold(RunOpts::default().bin_dir(&self.bin).cwd(&self.repo).clean_env().timeout(Duration::from_secs(60)), |opts, (key, value)| opts.env(key, value));
        run_wf(&full, &opts)
    }

    /// What the next full pack will contain.
    pub fn set_pack(&self, text: &str) {
        std::fs::write(self.dir.path().join("pack"), text).unwrap();
    }

    pub fn asks(&self) -> usize {
        std::fs::read_to_string(self.dir.path().join("calls")).map_or(0, |c| c.lines().count())
    }

    pub fn cached(&self, name: &str) -> String {
        std::fs::read_to_string(self.repo.join(".gemini-context-cache").join(name)).unwrap_or_default()
    }
}
