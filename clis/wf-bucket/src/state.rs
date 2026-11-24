//! The state ref: small cross-run files (the EAS pointer, the run index) kept as an orphan history under `refs/wf/state` of the
//! repository's `origin`. A ref outside `refs/heads` and `refs/tags` starts no workflow when pushed. Reads need only git, never
//! the Actions API: they fetch just the tip commit and its trees (no history, no blobs) into a scratch repository and let `git show`
//! pull the one blob asked for, because the ref's history holds a sealed database per commit and a plain fetch downloads all of
//! them. A write fetches the tip the same way, builds one parentless commit over its tree with plumbing (no checkout) and pushes it
//! over the tip it read, guarded by a lease, so the ref is always a single snapshot: the history never holds more than one sealed
//! database, and a job that lost a race fetches again and retries.
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const REF: &str = "refs/wf/state";
const ATTEMPTS: u32 = 5;
const READ_TIMEOUT: Duration = Duration::from_secs(90);

/// Seconds one git call of a read may run (`WF_STATE_READ_TIMEOUT_S`), so a hung remote is reported, not waited on.
fn read_timeout() -> Duration {
    std::env::var("WF_STATE_READ_TIMEOUT_S").ok().and_then(|s| s.trim().parse::<u64>().ok()).filter(|s| *s > 0).map_or(READ_TIMEOUT, Duration::from_secs)
}

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
    command.args(args).envs(envs.iter().map(|(k, v)| (*k, v.as_str()))).env("GIT_TERMINAL_PROMPT", "0").env("LC_ALL", "C");
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
    let out = git(base, &["fetch", "--quiet", "--no-tags", "--depth=1", "--filter=blob:none", "origin", &spec], &[], None)?;
    if !out.ok {
        return if out.stderr.contains("couldn't find remote ref") { Ok(None) } else { Err(format!("git fetch origin {REF} failed: {}", out.stderr)) };
    }
    Ok(Some(text(run(base, &["rev-parse", "--verify", &format!("{REF}^{{commit}}")], &[], None)?)))
}

/// Runs git with a deadline; a call that outlives it is killed and reported as an error naming the time it was given.
fn git_timed(args: &[String], envs: &[(String, String)], limit: Duration) -> Result<Out, String> {
    let mut command = Command::new("git");
    command.args(args).envs(envs.iter().map(|(k, v)| (k, v))).env("GIT_TERMINAL_PROMPT", "0").env("LC_ALL", "C").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let mut child = command.spawn().map_err(|e| format!("cannot run git: {e}"))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        })
    };
    let (stdout, stderr) = (drain(child.stdout.take().map(|p| Box::new(p) as _)), drain(child.stderr.take().map(|p| Box::new(p) as _)));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| format!("git did not finish: {e}"))? {
            break status;
        }
        if started.elapsed() >= limit {
            // git's transport helper holds the pipes open, so the whole group goes and the readers are not waited for
            let _ = Command::new("kill").args(["-KILL", &format!("-{}", child.id())]).status();
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("git did not finish within {} s (the remote did not answer in time): {}", limit.as_secs(), args.last().map_or("", String::as_str)));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let (stdout, stderr) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    Ok(Out { ok: status.success(), stdout, stderr: String::from_utf8_lossy(&stderr).trim().to_string() })
}

/// The checkout's `http.*` and `credential.*` settings (the checkout action's auth header, a proxy, a CA bundle) as `-c` pairs,
/// so the scratch repository authenticates exactly as the checkout does; passed as `GIT_CONFIG_*` variables so a token never shows in argv.
fn transport_config() -> Vec<(String, String)> {
    let out = git(None, &["config", "--get-regexp", r"^(http|credential)\."], &[], None).ok().filter(|o| o.ok);
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let pairs: Vec<(&str, &str)> = text.lines().filter_map(|l| l.split_once(' ')).collect();
    let mut envs = vec![("GIT_CONFIG_COUNT".to_string(), pairs.len().to_string())];
    for (i, (k, v)) in pairs.iter().enumerate() {
        envs.push((format!("GIT_CONFIG_KEY_{i}"), k.to_string()));
        envs.push((format!("GIT_CONFIG_VALUE_{i}"), v.to_string()));
    }
    envs
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Reads files of the state ref's tip without touching the checkout: a depth-1, blob-less fetch of the ref into a throwaway bare
/// repository (the tip commit and its small trees), then `git show` per path, which fetches only that blob from the remote.
fn read_files(paths: &[String]) -> Result<Vec<Option<Vec<u8>>>, String> {
    let limit = read_timeout();
    let url = text(run(None, &["remote", "get-url", "origin"], &[], None)?);
    let dir = std::env::temp_dir().join(format!("wf-state-read-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())));
    let _scratch = Scratch(dir.clone());
    let git_dir = format!("--git-dir={}", dir.display());
    let transport = transport_config();
    let step = |rest: &[&str]| -> Result<Out, String> {
        let mut args = vec![git_dir.clone()];
        args.extend(rest.iter().map(|s| s.to_string()));
        git_timed(&args, &transport, limit)
    };
    let init = git_timed(&["init".to_string(), "--bare".to_string(), "--quiet".to_string(), dir.display().to_string()], &[], limit)?;
    if !init.ok {
        return Err(format!("git init of the scratch repository failed: {}", init.stderr));
    }
    let added = step(&["remote", "add", "origin", &url])?;
    if !added.ok {
        return Err(format!("git remote add failed: {}", added.stderr));
    }
    let spec = format!("+{REF}:{REF}");
    let fetched = step(&["fetch", "--quiet", "--no-tags", "--depth=1", "--filter=blob:none", "origin", &spec])?;
    if !fetched.ok {
        return if fetched.stderr.contains("couldn't find remote ref") { Ok(vec![None; paths.len()]) } else { Err(format!("git fetch origin {REF} failed: {}", fetched.stderr)) };
    }
    paths
        .iter()
        .map(|path| {
            let spec = format!("{REF}:{path}");
            let out = step(&["show", &spec])?;
            if out.ok {
                return Ok(Some(out.stdout));
            }
            if out.stderr.contains("does not exist") || out.stderr.contains("exists on disk, but not in") {
                return Ok(None);
            }
            Err(format!("git show {spec} failed: {}", out.stderr))
        })
        .collect()
}

/// A file of the state ref: no ref yet, or no such file, is `Ok(None)`; any other failure is an error, so a caller never
/// replaces an index it could not read.
pub fn read(path: &str) -> Result<Option<String>, String> {
    checked(path)?;
    Ok(read_files(&[path.to_string()])?.remove(0).map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// A file of the state ref as its exact bytes (a sealed database), with `read`'s answers for a missing ref or file.
pub fn read_bytes(path: &str) -> Result<Option<Vec<u8>>, String> {
    checked(path)?;
    Ok(read_files(&[path.to_string()])?.remove(0))
}

/// Several files of one fetched tip, in the order asked: a missing one is `None`.
pub fn read_many(paths: &[String]) -> Result<Vec<Option<String>>, String> {
    for path in paths {
        checked(path)?;
    }
    Ok(read_files(paths)?.into_iter().map(|b| b.map(|b| String::from_utf8_lossy(&b).into_owned())).collect())
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
        let tree = text(run(base, &["write-tree", "--missing-ok"], &env, None)?);
        Ok(text(run(base, &["commit-tree", tree.as_str(), "-m", message], &env, None)?))
    })();
    let _ = std::fs::remove_file(&index);
    result
}

/// Commit `files` onto the state ref and push it. A push another writer beat is fetched and redone on the new tip, so no
/// writer's files are lost; the commit that landed is returned.
pub fn write(files: &[(&str, &[u8])], message: &str) -> Result<String, String> {
    write_in(None, files, message)
}

/// Seconds each phase of a `write` took, summed over its attempts, so a slow push names the phase that was slow.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    pub attempts: u32,
    pub fetch_s: f64,
    pub commit_s: f64,
    pub push_s: f64,
}

/// `write` with git running in exactly `base` as its environment (nothing of the caller's reaches it), for a verb that keeps its own variables out of every child.
pub fn write_in(base: Base, files: &[(&str, &[u8])], message: &str) -> Result<String, String> {
    write_timed(base, files, message).map(|(commit, _)| commit)
}

/// `write_in` that also returns how long its fetch, commit and push took.
pub fn write_timed(base: Base, files: &[(&str, &[u8])], message: &str) -> Result<(String, Timing), String> {
    for (path, _) in files {
        checked(path)?;
    }
    let mut last = String::new();
    let mut timing = Timing::default();
    for attempt in 0..ATTEMPTS {
        timing.attempts = attempt + 1;
        let started = Instant::now();
        let parent = fetch(base)?;
        timing.fetch_s += started.elapsed().as_secs_f64();
        let started = Instant::now();
        let commit = commit_files(base, parent.as_deref(), files, message)?;
        timing.commit_s += started.elapsed().as_secs_f64();
        let started = Instant::now();
        let target = format!("{commit}:{REF}");
        let lease = parent.as_deref().map(|p| format!("--force-with-lease={REF}:{p}"));
        let mut args = vec!["push", "--quiet"];
        args.extend(lease.as_deref());
        args.extend(["origin", target.as_str()]);
        let out = git(base, &args, &[], None)?;
        timing.push_s += started.elapsed().as_secs_f64();
        if out.ok {
            return Ok((commit, timing));
        }
        last = out.stderr;
        std::thread::sleep(std::time::Duration::from_millis(150 * u64::from(attempt + 1) + u64::from(std::process::id() % 100)));
    }
    Err(format!("the push to {REF} was refused {ATTEMPTS} times: {last}"))
}
