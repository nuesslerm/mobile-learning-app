//! The state ref of a test's origin, written with plain git.
/// Writes one file to `refs/wf/state` of the `origin` of the checkout `repo`, with plain git (what `wf bucket state write` does, which
/// the thin binaries of the crates do not carry): fetch the tip, add the blob on top of it, push fast-forward.
pub fn state_put(repo: &std::path::Path, key: &str, body: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let index = repo.join(format!("state-index-{}", std::process::id()));
    let run = |args: &[&str], stdin: Option<&str>| -> (bool, String) {
        let mut child = Command::new("/usr/bin/git")
            .args(args)
            .current_dir(repo)
            .env("GIT_INDEX_FILE", &index)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(text) = stdin {
            child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        (out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let ok = |args: &[&str], stdin: Option<&str>| {
        let (good, out) = run(args, stdin);
        assert!(good, "git {args:?} failed");
        out
    };
    let parent = if run(&["fetch", "--quiet", "origin", "+refs/wf/state:refs/wf/state"], None).0 { Some(ok(&["rev-parse", "refs/wf/state"], None)) } else { None };
    if let Some(parent) = &parent {
        ok(&["read-tree", parent], None);
    }
    let blob = ok(&["hash-object", "-w", "--stdin"], Some(body));
    ok(&["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{key}")], None);
    let tree = ok(&["write-tree"], None);
    let mut args = vec!["commit-tree", tree.as_str(), "-m", "test state"];
    if let Some(parent) = &parent {
        args.extend(["-p", parent]);
    }
    let commit = ok(&args, None);
    ok(&["push", "--quiet", "origin", &format!("{commit}:refs/wf/state")], None);
    let _ = std::fs::remove_file(&index);
}
