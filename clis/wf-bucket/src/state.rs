//! The state ref: small cross-run files (the EAS pointer, the run index) kept as an orphan history under `refs/wf/state` of the
//! repository's `origin`. A ref outside `refs/heads` and `refs/tags` starts no workflow when pushed. Reads fetch the ref and
//! `git show` a path, so they need only git, never the Actions API; writes build the commit with plumbing (no checkout) and push
//! it fast-forward only, fetching and retrying when another job moved the ref first.
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const REF: &str = "refs/wf/state";
const ATTEMPTS: u32 = 5;

struct Out {
    ok: bool,
    stdout: Vec<u8>,
    stderr: String,
}

/// The whole environment git runs in, when the caller owns it (a verb that must keep its own variables out of git); `None` inherits.
pub type Base<'a> = Option<&'a [(String, String)]>;

fn git(base: Base, args: &[&str], envs: &[(&str, String)], stdin: Option<&[u8]>) -> Result<Out, String> {
    let mut command = Command::new("git");
    if let Some(base) = base {
        command.env_clear().envs(base.iter().map(|(k, v)| (k, v)));
    }
    command.args(args).envs(envs.iter().map(|(k, v)| (*k, v.as_str()))).env("GIT_TERMINAL_PROMPT", "0");
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| format!("cannot run git: {e}"))?;
    if let (Some(body), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(body).map_err(|e| format!("cannot feed git {}: {e}", args[0]))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("git {} did not finish: {e}", args[0]))?;
    Ok(Out { ok: out.status.success(), stdout: out.stdout, stderr: String::from_utf8_lossy(&out.stderr).trim().to_string() })
}

fn run(base: Base, args: &[&str], envs: &[(&str, String)], stdin: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let out = git(base, args, envs, stdin)?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(format!("git {} failed: {}", args.join(" "), out.stderr))
    }
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).trim().to_string()
}

fn checked(path: &str) -> Result<(), String> {
    let bad = path.is_empty() || path.starts_with('/') || path.starts_with('-') || path.split('/').any(|s| s.is_empty() || s == "." || s == ".." || s == ".git");
    if bad {
        return Err(format!("{path:?} is not a relative path inside the state ref"));
    }
    Ok(())
}

/// The remote's tip of the state ref, now in the local `refs/wf/state`: none when the remote has no such ref yet.
fn fetch(base: Base) -> Result<Option<String>, String> {
    let spec = format!("+{REF}:{REF}");
    let out = git(base, &["fetch", "--quiet", "--no-tags", "origin", &spec], &[], None)?;
    if !out.ok {
        return if out.stderr.contains("couldn't find remote ref") { Ok(None) } else { Err(format!("git fetch origin {REF} failed: {}", out.stderr)) };
    }
    Ok(Some(text(run(base, &["rev-parse", "--verify", &format!("{REF}^{{commit}}")], &[], None)?)))
}

fn show(base: Base, tip: &str, path: &str) -> Result<Option<String>, String> {
    Ok(show_bytes(base, tip, path)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

fn show_bytes(base: Base, tip: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
    let spec = format!("{tip}:{path}");
    let out = git(base, &["show", &spec], &[], None)?;
    if out.ok {
        return Ok(Some(out.stdout));
    }
    if out.stderr.contains("does not exist") || out.stderr.contains("exists on disk, but not in") {
        return Ok(None);
    }
    Err(format!("git show {spec} failed: {}", out.stderr))
}

/// A file of the state ref: no ref yet, or no such file, is `Ok(None)`; any other failure is an error, so a caller never
/// replaces an index it could not read.
pub fn read(path: &str) -> Result<Option<String>, String> {
    checked(path)?;
    let Some(tip) = fetch(None)? else { return Ok(None) };
    show(None, &tip, path)
}

/// A file of the state ref as its exact bytes (a sealed database), with `read`'s answers for a missing ref or file.
pub fn read_bytes(path: &str) -> Result<Option<Vec<u8>>, String> {
    checked(path)?;
    let Some(tip) = fetch(None)? else { return Ok(None) };
    show_bytes(None, &tip, path)
}

/// Several files of one fetched tip, in the order asked: a missing one is `None`.
pub fn read_many(paths: &[String]) -> Result<Vec<Option<String>>, String> {
    for path in paths {
        checked(path)?;
    }
    let Some(tip) = fetch(None)? else { return Ok(vec![None; paths.len()]) };
    paths.iter().map(|p| show(None, &tip, p)).collect()
}

fn commit_files(base: Base, parent: Option<&str>, files: &[(&str, &[u8])], message: &str) -> Result<String, String> {
    let git_dir = PathBuf::from(text(run(base, &["rev-parse", "--absolute-git-dir"], &[], None)?));
    let index = git_dir.join(format!("wf-state-index-{}", std::process::id()));
    let _ = std::fs::remove_file(&index);
    let result = (|| {
        let env = [
            ("GIT_INDEX_FILE", index.display().to_string()),
            ("GIT_AUTHOR_NAME", "wf state".to_string()),
            ("GIT_AUTHOR_EMAIL", "wf-state@users.noreply.github.com".to_string()),
            ("GIT_COMMITTER_NAME", "wf state".to_string()),
            ("GIT_COMMITTER_EMAIL", "wf-state@users.noreply.github.com".to_string()),
        ];
        if let Some(parent) = parent {
            run(base, &["read-tree", parent], &env, None)?;
        }
        for (path, body) in files {
            let blob = text(run(base, &["hash-object", "-w", "--stdin"], &env, Some(body))?);
            run(base, &["update-index", "--add", "--cacheinfo", &format!("100644,{blob},{path}")], &env, None)?;
        }
        let tree = text(run(base, &["write-tree"], &env, None)?);
        let mut args = vec!["commit-tree", tree.as_str(), "-m", message];
        if let Some(parent) = parent {
            args.extend(["-p", parent]);
        }
        Ok(text(run(base, &args, &env, None)?))
    })();
    let _ = std::fs::remove_file(&index);
    result
}

/// Commit `files` onto the state ref and push it. A push another writer beat is fetched and redone on the new tip, so no
/// writer's files are lost; the commit that landed is returned.
pub fn write(files: &[(&str, &[u8])], message: &str) -> Result<String, String> {
    write_in(None, files, message)
}

/// `write` with git running in exactly `base` as its environment (nothing of the caller's reaches it), for a verb that keeps its own variables out of every child.
pub fn write_in(base: Base, files: &[(&str, &[u8])], message: &str) -> Result<String, String> {
    for (path, _) in files {
        checked(path)?;
    }
    let mut last = String::new();
    for attempt in 0..ATTEMPTS {
        let parent = fetch(base)?;
        let commit = commit_files(base, parent.as_deref(), files, message)?;
        let out = git(base, &["push", "--quiet", "origin", &format!("{commit}:{REF}")], &[], None)?;
        if out.ok {
            return Ok(commit);
        }
        last = out.stderr;
        std::thread::sleep(std::time::Duration::from_millis(150 * u64::from(attempt + 1) + u64::from(std::process::id() % 100)));
    }
    Err(format!("the push to {REF} was refused {ATTEMPTS} times: {last}"))
}
