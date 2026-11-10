//! The temporary repository, with real git.
use exec_testkit::TempRepo;

#[test]
fn a_new_repo_is_on_main_with_the_one_first_commit_and_a_clean_tree() {
    // Given a new temporary repository
    let repo = TempRepo::new();
    // Then it is on main, has exactly one commit, holds the README it committed, and has nothing to commit
    assert_eq!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(repo.git(&["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(repo.git(&["log", "--format=%s"]), "first commit");
    assert_eq!(std::fs::read_to_string(repo.path().join("README.md")).unwrap(), "temp repo\n");
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

#[test]
fn git_in_the_repo_reads_no_configuration_but_the_repos_own_and_commits_as_the_fixed_author() {
    // Given a repository
    let repo = TempRepo::new();
    // When every configuration entry git sees is listed with the file it comes from
    let listed = repo.git(&["config", "--list", "--show-origin"]);
    // Then each comes from the repository's own file: nothing global, nothing of the system
    assert!(!listed.is_empty() && listed.lines().all(|l| l.starts_with("file:.git/config")), "{listed}");
    // And the commit is the fixed author's
    assert_eq!(repo.git(&["log", "-1", "--format=%an <%ae> / %cn <%ce>"]), "Test Author <author@example.test> / Test Committer <committer@example.test>");
    // And git started with the repo's isolation environment alone (the PATH aside) runs there too
    let status = std::process::Command::new("git").args(["status", "--porcelain"]).current_dir(repo.path()).env_clear().envs(repo.git_env()).env("PATH", std::env::var_os("PATH").unwrap()).status().unwrap();
    assert!(status.success());
}

#[test]
fn a_file_written_to_the_repo_shows_as_untracked_until_it_is_committed_and_the_commit_moves_head() {
    // Given a repository and its first commit
    let repo = TempRepo::new();
    let first = repo.head();
    // When a file in a new directory is written
    repo.write("src/a.txt", "one\n");
    // Then git lists it as untracked
    assert_eq!(repo.git(&["status", "--porcelain", "--untracked-files=all"]), "?? src/a.txt");
    // And after the commit the tree is clean and head is another commit
    repo.commit_all("add a");
    assert_eq!((repo.git(&["status", "--porcelain"]), repo.head() != first, repo.git(&["rev-list", "--count", "HEAD"])), (String::new(), true, "2".to_string()));
}

#[test]
fn two_repos_are_two_directories_that_do_not_see_each_others_files() {
    // Given two repositories, one with a file the other lacks
    let (a, b) = (TempRepo::new(), TempRepo::new());
    a.write("only-a.txt", "a\n");
    // When each is read
    // Then they are different directories, and the file is only in the first
    assert_ne!(a.path(), b.path());
    assert!(a.path().join("only-a.txt").exists() && !b.path().join("only-a.txt").exists());
}
