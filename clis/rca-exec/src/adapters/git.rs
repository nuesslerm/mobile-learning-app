//! The repository actions of the git family on the local `git` binary (`exec_git::GitOp`). One `Repo` is one working tree: every command runs
//! there with no prompt, no pager, no editor, every pathspec literal and git's own repository discovery stopped at the tree (a root inside
//! another repository never acts on the outer one). The repository's hooks run only when the executor was started with `--git-hooks`.
//! Push, pull and fetch go to a remote the repository has configured, with whatever credentials this machine's git finds.
use base64::Engine;
use exec_git::{
    BlobResult, Checkpointed, CommitDone, CommitInfo, CompareResult, DiffFile, DiscardDone, DiscardScope, FastForwarded, Fetched, FetchedRef, GitCall, GitEvent, GitStatus, HeadMoved, InitDone, InitFile, Integrated, LogResult, MergeStrategy, PathsDone, PullStrategy, Pushed,
    RefCreated, Refspec, RefInfo, RefKind, RefsResult, Resolution, Resolved, Step, StatusFile, UpstreamStatus, Watched, BLOB_MAX, STATUS_FILES_CAP,
};
use exec_proto::{to_value, Code, ExecError, ExecResult};
use serde_json::Value;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const LOCAL_TIMEOUT: Duration = Duration::from_secs(60);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(300);
const OUT_CAP: usize = 16 << 20;
const ERR_CAP: usize = 1 << 20;

pub struct Out {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Out {
    fn ok(&self) -> bool {
        self.code == 0
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// What git said on failure: stderr, else stdout (`git commit` reports "nothing to commit" there).
    fn said(&self) -> String {
        let err = self.stderr.trim();
        if err.is_empty() { self.text().trim().to_string() } else { err.to_string() }
    }
}

struct Opts<'a> {
    stdin: Option<&'a [u8]>,
    timeout: Duration,
    max_out: usize,
    env: &'a [(&'a str, &'a str)],
}

const PLAIN: Opts = Opts { stdin: None, timeout: LOCAL_TIMEOUT, max_out: OUT_CAP, env: &[] };

pub struct Repo {
    dir: PathBuf,
    hooks: bool,
}

fn kill_group(pid: i32) {
    // SAFETY: a negative pid addresses the process group the child leads; the call only sends a signal.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

fn subcommand<'a>(args: &[&'a str]) -> &'a str {
    args.iter().find(|a| !a.starts_with('-')).copied().unwrap_or("git")
}

fn fail(args: &[&str], out: &Out) -> ExecError {
    let said = out.said();
    let lower = said.to_lowercase();
    let code = if lower.contains("unknown revision") || lower.contains("bad revision") || lower.contains("not a valid object name") || lower.contains("invalid reference") || lower.contains("did not match any") || lower.contains("couldn't find remote ref") {
        Code::NotFound
    } else if lower.contains("already exists") {
        Code::AlreadyExists
    } else {
        Code::Io
    };
    ExecError::new(code, format!("git {}: {said}", subcommand(args)))
}

fn io(what: &str, e: impl std::fmt::Display) -> ExecError {
    ExecError::new(Code::Io, format!("{what}: {e}"))
}

fn count(s: &str) -> u32 {
    s.trim().parse().unwrap_or(0)
}

fn lines(out: &Out) -> Vec<String> {
    out.text().lines().map(str::to_string).collect()
}

fn nul_tokens(out: &Out) -> Vec<String> {
    out.text().split('\0').filter(|t| !t.is_empty()).map(str::to_string).collect()
}

impl Repo {
    pub fn new(dir: PathBuf, hooks: bool) -> Repo {
        Repo { dir, hooks }
    }

    fn run_with(&self, args: &[&str], o: &Opts) -> ExecResult<Out> {
        let mut cmd = Command::new("git");
        if !self.hooks {
            cmd.args(["-c", "core.hooksPath=/dev/null"]);
        }
        cmd.args(["-c", "core.fsmonitor=false", "-c", "protocol.ext.allow=never", "-c", "color.ui=false", "-C"]).arg(&self.dir).args(args);
        cmd.env("GIT_TERMINAL_PROMPT", "0").env("GIT_EDITOR", "true").env("GIT_SEQUENCE_EDITOR", "true").env("GIT_PAGER", "cat").env("GIT_OPTIONAL_LOCKS", "0").env("GIT_LITERAL_PATHSPECS", "1").env("LC_ALL", "C");
        if let Some(parent) = self.dir.parent() {
            cmd.env("GIT_CEILING_DIRECTORIES", parent);
        }
        cmd.envs(o.env.iter().copied());
        if std::env::var_os("GIT_SSH_COMMAND").is_none() {
            cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
        }
        cmd.stdin(if o.stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
        let mut child = cmd.spawn().map_err(|e| io(&format!("git {}", subcommand(args)), e))?;
        let pid = child.id() as i32;
        if let (Some(data), Some(mut stdin)) = (o.stdin, child.stdin.take()) {
            let data = data.to_vec();
            std::thread::spawn(move || {
                let _ = stdin.write_all(&data);
            });
        }
        let (mut stdout, mut stderr) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
        let max_out = o.max_out;
        let reader = std::thread::spawn(move || {
            let (mut kept, mut buf) = (Vec::new(), [0u8; 16 * 1024]);
            while let Ok(n) = stdout.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let room = max_out.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
                if kept.len() >= max_out {
                    kill_group(pid);
                    break;
                }
            }
            kept
        });
        let err_reader = std::thread::spawn(move || {
            let (mut kept, mut buf) = (Vec::new(), [0u8; 4096]);
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let room = ERR_CAP.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
            kept
        });
        let deadline = Instant::now() + o.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    kill_group(pid);
                    let _ = child.wait();
                    return Err(ExecError::new(Code::Io, format!("git {} timed out after {} s", subcommand(args), o.timeout.as_secs())));
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                Err(e) => return Err(io("git wait", e)),
            }
        };
        kill_group(pid);
        let stdout = reader.join().unwrap_or_default();
        let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
        Ok(Out { code: status.code().unwrap_or(-1), stdout, stderr })
    }

    fn run(&self, args: &[&str]) -> ExecResult<Out> {
        self.run_with(args, &PLAIN)
    }

    /// stdout of a command that must succeed.
    fn ok(&self, args: &[&str]) -> ExecResult<String> {
        let out = self.run(args)?;
        if out.ok() { Ok(out.text()) } else { Err(fail(args, &out)) }
    }

    /// The one line a query prints, or `None` when git says no (a missing upstream, a detached head, an unborn branch).
    fn query(&self, args: &[&str]) -> ExecResult<Option<String>> {
        let out = self.run(args)?;
        Ok(out.ok().then(|| out.text().trim().to_string()).filter(|s| !s.is_empty()))
    }

    fn head_sha(&self) -> ExecResult<Option<String>> {
        self.query(&["rev-parse", "--verify", "-q", "HEAD"])
    }

    fn current_branch(&self) -> ExecResult<Option<String>> {
        self.query(&["symbolic-ref", "--short", "-q", "HEAD"])
    }

    fn remotes(&self) -> ExecResult<Vec<String>> {
        Ok(lines(&self.run(&["remote"])?))
    }

    fn conflicts(&self) -> ExecResult<Vec<String>> {
        Ok(nul_tokens(&self.run(&["diff", "--name-only", "--diff-filter=U", "-z"])?))
    }

    /// `left` ahead of and behind `right`.
    fn ahead_behind(&self, left: &str, right: &str) -> ExecResult<(u32, u32)> {
        let spec = format!("{left}...{right}");
        let out = self.run(&["rev-list", "--left-right", "--count", &spec])?;
        let text = out.text();
        let mut parts = text.split_whitespace();
        Ok(if out.ok() { (count(parts.next().unwrap_or("0")), count(parts.next().unwrap_or("0"))) } else { (0, 0) })
    }

    fn operation(&self) -> ExecResult<Option<String>> {
        let Some(git_dir) = self.query(&["rev-parse", "--absolute-git-dir"])? else { return Ok(None) };
        let git_dir = Path::new(&git_dir);
        let found = [("MERGE_HEAD", "merge"), ("rebase-merge", "rebase"), ("rebase-apply", "rebase"), ("CHERRY_PICK_HEAD", "cherry-pick"), ("REVERT_HEAD", "revert")];
        Ok(found.iter().find(|(file, _)| git_dir.join(file).exists()).map(|(_, name)| name.to_string()))
    }

    pub fn status(&self) -> ExecResult<GitStatus> {
        let out = self.run(&["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=normal"])?;
        if !out.ok() {
            return Err(fail(&["status"], &out));
        }
        let mut status = GitStatus { branch: None, head: None, detached: false, upstream: None, ahead: 0, behind: 0, operation: None, files: Vec::new(), truncated: false, clean: true };
        let tokens: Vec<String> = out.text().split('\0').map(str::to_string).collect();
        let mut i = 0;
        while i < tokens.len() {
            let token = &tokens[i];
            i += 1;
            if let Some(header) = token.strip_prefix("# ") {
                match header.split_once(' ') {
                    Some(("branch.oid", oid)) => status.head = Some(oid.to_string()).filter(|o| o != "(initial)"),
                    Some(("branch.head", "(detached)")) => status.detached = true,
                    Some(("branch.head", name)) => status.branch = Some(name.to_string()),
                    Some(("branch.upstream", up)) => status.upstream = Some(up.to_string()),
                    Some(("branch.ab", ab)) => {
                        let mut parts = ab.split_whitespace();
                        status.ahead = count(parts.next().unwrap_or("+0").trim_start_matches('+'));
                        status.behind = count(parts.next().unwrap_or("-0").trim_start_matches('-'));
                    }
                    _ => {}
                }
                continue;
            }
            let file = match token.split_at_checked(2) {
                Some(("1 ", rest)) => split_entry(rest, 8).map(|(xy, path)| (xy, path, None, false)),
                Some(("2 ", rest)) => {
                    let orig = tokens.get(i).cloned();
                    i += 1;
                    split_entry(rest, 9).map(|(xy, path)| (xy, path, orig, false))
                }
                Some(("u ", rest)) => split_entry(rest, 10).map(|(xy, path)| (xy, path, None, true)),
                Some(("? ", path)) => Some(("??".to_string(), path.to_string(), None, false)),
                _ => None,
            };
            if let Some((xy, path, orig_path, conflicted)) = file {
                if status.files.len() >= STATUS_FILES_CAP {
                    status.truncated = true;
                    continue;
                }
                let mut cols = xy.chars();
                let (index, worktree) = (cols.next().unwrap_or('.').to_string(), cols.next().unwrap_or('.').to_string());
                status.files.push(StatusFile { path, orig_path, index, worktree, conflicted });
            }
        }
        status.clean = status.files.is_empty();
        status.operation = self.operation()?;
        Ok(status)
    }

    pub fn refs(&self, kind: Option<RefKind>, query: Option<&str>, limit: usize) -> ExecResult<RefsResult> {
        let patterns: &[&str] = match kind {
            Some(RefKind::Branch) => &["refs/heads"],
            Some(RefKind::Remote) => &["refs/remotes"],
            Some(RefKind::Tag) => &["refs/tags"],
            None => &["refs/heads", "refs/remotes", "refs/tags"],
        };
        let format = "--format=%(refname)%09%(if)%(*objectname)%(then)%(*objectname)%(else)%(objectname)%(end)%09%(upstream:short)%09%(HEAD)";
        let mut args = vec!["for-each-ref", "--sort=-committerdate", format];
        args.extend(patterns);
        let current = self.current_branch()?;
        let needle = query.map(str::to_lowercase);
        let mut refs = Vec::new();
        for line in self.ok(&args)?.lines() {
            let mut cols = line.split('\t');
            let (full, sha, upstream, head) = (cols.next().unwrap_or(""), cols.next().unwrap_or(""), cols.next().unwrap_or(""), cols.next().unwrap_or(""));
            let (kind, name) = match (full.strip_prefix("refs/heads/"), full.strip_prefix("refs/remotes/"), full.strip_prefix("refs/tags/")) {
                (Some(n), _, _) => (RefKind::Branch, n),
                (_, Some(n), _) if !n.ends_with("/HEAD") => (RefKind::Remote, n),
                (_, _, Some(n)) => (RefKind::Tag, n),
                _ => continue,
            };
            if needle.as_ref().is_some_and(|q| !name.to_lowercase().contains(q)) {
                continue;
            }
            refs.push(RefInfo { name: name.to_string(), full: full.to_string(), kind: kind.as_str().to_string(), sha: sha.to_string(), upstream: Some(upstream.to_string()).filter(|u| !u.is_empty()), current: head.trim() == "*" });
        }
        let truncated = refs.len() > limit;
        refs.truncate(limit);
        Ok(RefsResult { detached: current.is_none() && self.head_sha()?.is_some(), current, refs, truncated })
    }

    fn commits(&self, rev: Option<&str>, path: Option<&str>, limit: usize, skip: usize) -> ExecResult<(Vec<CommitInfo>, bool)> {
        let format = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%s%x1f%b%x1e";
        let (n, s) = (format!("-n{}", limit + 1), format!("--skip={skip}"));
        let mut args = vec!["log", "--no-show-signature", &n, &s, format];
        if let Some(rev) = rev {
            args.push(rev);
        }
        args.push("--");
        if let Some(path) = path {
            args.push(path);
        }
        let out = self.run(&args)?;
        if !out.ok() {
            // A repository without commits has an empty history, not an error.
            if rev.is_none() && self.head_sha()?.is_none() {
                return Ok((Vec::new(), false));
            }
            return Err(fail(&args, &out));
        }
        let mut commits: Vec<CommitInfo> = out
            .text()
            .split('\x1e')
            .filter_map(|record| {
                let mut f = record.trim_start_matches('\n').split('\x1f');
                let sha = f.next().filter(|s| !s.is_empty())?.to_string();
                let parents = f.next().unwrap_or("").split_whitespace().map(str::to_string).collect();
                let (author_name, author_email) = (f.next().unwrap_or("").to_string(), f.next().unwrap_or("").to_string());
                let authored_ms = f.next().unwrap_or("0").parse::<i64>().unwrap_or(0) * 1000;
                let (subject, body) = (f.next().unwrap_or("").to_string(), f.next().unwrap_or("").trim_end().to_string());
                Some(CommitInfo { short: sha.chars().take(7).collect(), sha, parents, author_name, author_email, authored_ms, subject, body })
            })
            .collect();
        let truncated = commits.len() > limit;
        commits.truncate(limit);
        Ok((commits, truncated))
    }

    pub fn log(&self, rev: Option<&str>, path: Option<&str>, limit: usize, skip: usize) -> ExecResult<LogResult> {
        let (commits, truncated) = self.commits(rev, path, limit, skip)?;
        Ok(LogResult { commits, truncated })
    }

    pub fn compare(&self, base: &str, head: &str, limit: usize) -> ExecResult<CompareResult> {
        for rev in [base, head] {
            let spec = format!("{rev}^{{commit}}");
            if self.query(&["rev-parse", "--verify", "-q", &spec])?.is_none() {
                return Err(ExecError::new(Code::NotFound, format!("git compare: {rev} is not a commit")));
            }
        }
        let merge_base = self.query(&["merge-base", base, head])?;
        let (behind, ahead) = self.ahead_behind(base, head)?;
        let range = format!("{base}..{head}");
        let (commits, truncated) = self.commits(Some(&range), None, limit, 0)?;
        let three = format!("{base}...{head}");
        let spec: Vec<&str> = if merge_base.is_some() { vec![three.as_str()] } else { vec![base, head] };
        let mut names = vec!["diff", "--name-status", "-z", "-M", "--no-ext-diff", "--no-textconv"];
        names.extend(&spec);
        let mut nums = vec!["diff", "--numstat", "-z", "-M", "--no-ext-diff", "--no-textconv"];
        nums.extend(&spec);
        let named = self.ok(&names)?;
        let numbered = self.ok(&nums)?;
        let (named, numbered): (Vec<&str>, Vec<&str>) = (named.split('\0').filter(|t| !t.is_empty()).collect(), numbered.split('\0').collect());
        let (mut files, mut n, mut m) = (Vec::new(), 0, 0);
        while n < named.len() {
            let status = named[n];
            let renamed = status.starts_with('R') || status.starts_with('C');
            let (orig_path, path) = if renamed { (named.get(n + 1).map(|s| s.to_string()), named.get(n + 2).unwrap_or(&"").to_string()) } else { (None, named.get(n + 1).unwrap_or(&"").to_string()) };
            n += if renamed { 3 } else { 2 };
            // numstat: `adds\tdels\tpath`, or `adds\tdels\t` followed by the two paths for a rename
            let stat = numbered.get(m).copied().unwrap_or("");
            m += if renamed { 3 } else { 1 };
            let mut cols = stat.splitn(3, '\t');
            let (adds, dels) = (cols.next().unwrap_or("-"), cols.next().unwrap_or("-"));
            let binary = adds == "-";
            files.push(DiffFile {
                path,
                orig_path,
                status: match status.chars().next() {
                    Some('A') => "added",
                    Some('D') => "deleted",
                    Some('R') => "renamed",
                    Some('C') => "copied",
                    Some('T') => "type_changed",
                    _ => "modified",
                }
                .to_string(),
                additions: adds.parse().ok(),
                deletions: dels.parse().ok(),
                binary,
            });
        }
        let files_truncated = files.len() > STATUS_FILES_CAP;
        files.truncate(STATUS_FILES_CAP);
        Ok(CompareResult { base: base.to_string(), head: head.to_string(), merge_base, ahead, behind, commits, files, truncated: truncated || files_truncated })
    }

    pub fn upstream(&self) -> ExecResult<UpstreamStatus> {
        let branch = self.current_branch()?;
        let remotes = self.remotes()?;
        let mut status = UpstreamStatus { branch: branch.clone(), has_upstream: false, upstream: None, remote: None, ahead: 0, behind: 0, remotes };
        let Some(branch) = branch else { return Ok(status) };
        let heads = format!("refs/heads/{branch}");
        let line = self.query(&["for-each-ref", "--format=%(upstream:short)%09%(upstream:remotename)", &heads])?.unwrap_or_default();
        let (up, remote) = line.split_once('\t').unwrap_or(("", ""));
        if !up.is_empty() {
            let spec = format!("{branch}@{{upstream}}");
            let (ahead, behind) = self.ahead_behind(&format!("refs/heads/{branch}"), &spec)?;
            status = UpstreamStatus { has_upstream: true, upstream: Some(up.to_string()), remote: Some(remote.to_string()).filter(|r| !r.is_empty()), ahead, behind, ..status };
        }
        Ok(status)
    }

    /// Feeds the paths to git on stdin, so a long list never meets the limit of an argument vector.
    fn with_paths(&self, args: &[&str], paths: &[String]) -> ExecResult<Out> {
        let mut input = Vec::new();
        for p in paths {
            input.extend_from_slice(p.as_bytes());
            input.push(0);
        }
        let mut all = args.to_vec();
        all.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
        let out = self.run_with(&all, &Opts { stdin: Some(&input), ..PLAIN })?;
        if out.ok() { Ok(out) } else { Err(fail(args, &out)) }
    }

    pub fn stage(&self, paths: &[String]) -> ExecResult<PathsDone> {
        self.with_paths(&["add", "-A"], paths)?;
        Ok(PathsDone { paths: paths.len() })
    }

    pub fn unstage(&self, paths: &[String]) -> ExecResult<PathsDone> {
        if self.head_sha()?.is_some() {
            self.with_paths(&["restore", "--staged"], paths)?;
        } else {
            self.with_paths(&["rm", "--cached", "-r", "-q", "--ignore-unmatch"], paths)?;
        }
        Ok(PathsDone { paths: paths.len() })
    }

    pub fn discard(&self, paths: &[String], scope: DiscardScope, untracked: bool, source: Option<&str>) -> ExecResult<DiscardDone> {
        let rev = source.or((scope == DiscardScope::All).then_some("HEAD"));
        let mut args = vec!["ls-files", "-z", "--"];
        args.extend(paths.iter().map(String::as_str));
        let tracked = nul_tokens(&self.run(&args)?);
        let restored = match rev {
            Some(rev) => {
                let tree = format!("{rev}^{{tree}}");
                if self.query(&["rev-parse", "--verify", "-q", &tree])?.is_none() {
                    return Err(ExecError::new(Code::NotFound, format!("git discard: {rev} has no tree to restore from")));
                }
                let mut args = vec!["ls-tree", "-r", "-z", "--name-only", rev, "--"];
                args.extend(paths.iter().map(String::as_str));
                let in_rev = nul_tokens(&self.run(&args)?);
                let known: HashSet<&String> = in_rev.iter().collect();
                let remove: Vec<String> = tracked.iter().filter(|f| !known.contains(f)).cloned().collect();
                if !in_rev.is_empty() {
                    self.with_paths(&["restore", "--staged", "--worktree", "--source", rev], &in_rev)?;
                }
                if !remove.is_empty() {
                    self.with_paths(&["rm", "-f", "-q"], &remove)?;
                }
                in_rev.len() + remove.len()
            }
            None => {
                if !tracked.is_empty() {
                    self.with_paths(&["restore", "--worktree"], &tracked)?;
                }
                tracked.len()
            }
        };
        let mut cleaned = 0;
        if untracked {
            let mut dry = vec!["clean", "-n", "-d", "--"];
            dry.extend(paths.iter().map(String::as_str));
            cleaned = lines(&self.run(&dry)?).len();
            let mut real = vec!["clean", "-f", "-d", "-q", "--"];
            real.extend(paths.iter().map(String::as_str));
            let out = self.run(&real)?;
            if !out.ok() {
                return Err(fail(&real, &out));
            }
        }
        Ok(DiscardDone { restored, cleaned })
    }

    /// Commits the working tree as it is (untracked files too, ignored ones not) under the turn's checkpoint ref, on a private copy of the index:
    /// the repository's own index, HEAD, branch and files are never touched, and no hook runs.
    pub fn checkpoint(&self, thread: &str, turn: &str) -> ExecResult<Checkpointed> {
        let name = exec_git::checkpoint_ref(thread, turn);
        let head = self.head_sha()?;
        let real = PathBuf::from(self.ok(&["rev-parse", "--path-format=absolute", "--git-path", "index"])?.trim());
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let private = real.with_file_name(format!("index.rca-checkpoint-{}-{nanos}", std::process::id()));
        let done = self.snapshot(&name, head.as_deref(), &real, &private);
        let _ = std::fs::remove_file(&private);
        done
    }

    fn snapshot(&self, name: &str, head: Option<&str>, real: &Path, private: &Path) -> ExecResult<Checkpointed> {
        if real.is_file() {
            std::fs::copy(real, private).map_err(|e| io("git checkpoint", e))?;
        }
        let path = private.to_string_lossy().into_owned();
        let env = [("GIT_INDEX_FILE", path.as_str()), ("GIT_AUTHOR_NAME", "rca"), ("GIT_AUTHOR_EMAIL", "rca@localhost"), ("GIT_COMMITTER_NAME", "rca"), ("GIT_COMMITTER_EMAIL", "rca@localhost")];
        let opts = Opts { env: &env, timeout: NETWORK_TIMEOUT, ..PLAIN };
        let step = |args: &[&str]| -> ExecResult<String> {
            let out = self.run_with(args, &opts)?;
            if out.ok() { Ok(out.text().trim().to_string()) } else { Err(fail(args, &out)) }
        };
        step(&["add", "-A", "--", "."])?;
        let tree = step(&["write-tree"])?;
        let message = format!("rca checkpoint {name}");
        let mut commit_args = vec!["commit-tree", tree.as_str(), "-m", message.as_str()];
        commit_args.extend(head.into_iter().flat_map(|h| ["-p", h]));
        let commit = step(&commit_args)?;
        step(&["update-ref", name, &commit])?;
        Ok(Checkpointed { name: name.to_string(), commit, head: head.map(str::to_string) })
    }

    pub fn commit(&self, message: &str, amend: bool, all: bool, allow_empty: bool) -> ExecResult<CommitDone> {
        let mut args = vec!["commit", "-F", "-"];
        for (on, flag) in [(amend, "--amend"), (all, "-a"), (allow_empty, "--allow-empty")] {
            if on {
                args.push(flag);
            }
        }
        let out = self.run_with(&args, &Opts { stdin: Some(message.as_bytes()), ..PLAIN })?;
        if !out.ok() {
            return Err(fail(&args, &out));
        }
        let sha = self.ok(&["rev-parse", "HEAD"])?.trim().to_string();
        let subject = self.ok(&["log", "-1", "--format=%s"])?.trim().to_string();
        Ok(CommitDone { short: sha.chars().take(7).collect(), sha, subject })
    }

    /// `git init` here (this directory must exist), then the first files written as new files, staged and committed. A commit that fails is
    /// the result's `commit_error`: the repository and its files exist either way.
    pub fn init(&self, path: &str, branch: Option<&str>, files: &[InitFile], message: &str) -> ExecResult<InitDone> {
        if !self.dir.is_dir() {
            return Err(if self.dir.exists() { ExecError::invalid_args(format!("{path} is not a directory")) } else { ExecError::new(Code::NotFound, format!("{path} does not exist")) });
        }
        let branch = match branch {
            Some(branch) => branch.to_string(),
            None => self.query(&["config", "--get", "init.defaultBranch"])?.unwrap_or_else(|| "main".to_string()),
        };
        self.ok(&["init", "--quiet", &format!("--initial-branch={branch}")])?;
        // A repository that was there keeps the branch it was on: git ignores the one asked for
        let branch = self.current_branch()?.unwrap_or(branch);
        for file in files {
            let target = self.dir.join(&file.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| io(&file.path, e))?;
            }
            // create_new is O_EXCL: it never opens, and never follows a link at, a path that exists.
            let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| match e.kind() {
                std::io::ErrorKind::AlreadyExists => ExecError::new(Code::AlreadyExists, format!("{} already exists", file.path)),
                _ => io(&file.path, e),
            })?;
            out.write_all(file.content.as_bytes()).map_err(|e| io(&file.path, e))?;
        }
        let (commit, commit_error) = if files.is_empty() { (None, None) } else { self.first_commit(files, message)? };
        Ok(InitDone { path: path.to_string(), branch, commit, commit_error })
    }

    /// Stages the files by name (forced, so no ignore rule drops one) and commits them: the commit's sha, or why there is none.
    fn first_commit(&self, files: &[InitFile], message: &str) -> ExecResult<(Option<String>, Option<String>)> {
        let mut add = vec!["add", "--force", "--"];
        add.extend(files.iter().map(|f| f.path.as_str()));
        let added = self.run(&add)?;
        if !added.ok() {
            return Ok((None, Some(commit_failure(&added.said()))));
        }
        let committed = self.run(&["commit", "--quiet", &format!("--message={message}")])?;
        if !committed.ok() {
            return Ok((None, Some(commit_failure(&committed.said()))));
        }
        Ok((self.head_sha()?, None))
    }

    fn head_moved(&self) -> ExecResult<HeadMoved> {
        let branch = self.current_branch()?;
        Ok(HeadMoved { detached: branch.is_none(), branch, head: self.ok(&["rev-parse", "HEAD"])?.trim().to_string() })
    }

    pub fn checkout(&self, target: &str, detach: bool, track: bool, force: bool) -> ExecResult<HeadMoved> {
        let mut args = vec!["switch"];
        for (on, flag) in [(detach, "--detach"), (track, "--track"), (force, "--force")] {
            if on {
                args.push(flag);
            }
        }
        args.push(target);
        self.ok(&args)?;
        self.head_moved()
    }

    pub fn create_ref(&self, name: &str, kind: RefKind, from: Option<&str>, checkout: bool, message: Option<&str>) -> ExecResult<RefCreated> {
        let prefix = if kind == RefKind::Tag { "refs/tags" } else { "refs/heads" };
        let full = format!("{prefix}/{name}");
        if !self.run(&["check-ref-format", &full])?.ok() {
            return Err(ExecError::invalid_args("`name` must be a branch or tag name"));
        }
        let mut args: Vec<&str> = match (kind, checkout) {
            (RefKind::Tag, _) => vec!["tag"],
            (_, true) => vec!["switch", "-c"],
            _ => vec!["branch"],
        };
        if let Some(message) = message {
            args.extend(["-a", "-m", message]);
        }
        args.push(name);
        args.extend(from);
        self.ok(&args)?;
        let commit = format!("{full}^{{commit}}");
        Ok(RefCreated { name: name.to_string(), kind: kind.as_str().to_string(), sha: self.ok(&["rev-parse", &commit])?.trim().to_string() })
    }

    /// The remote a request names, which must be one the repository has configured; with none named, the current branch's upstream remote,
    /// else `origin`, else the only one.
    fn remote(&self, requested: Option<&str>) -> ExecResult<String> {
        let remotes = self.remotes()?;
        if let Some(name) = requested {
            return remotes.iter().find(|r| *r == name).cloned().ok_or_else(|| ExecError::invalid_args(format!("`remote` {name} is not a configured remote of this repository")));
        }
        if let Some(branch) = self.current_branch()? {
            let key = format!("branch.{branch}.remote");
            if let Some(remote) = self.query(&["config", "--get", &key])?.filter(|r| remotes.contains(r)) {
                return Ok(remote);
            }
        }
        match remotes.as_slice() {
            [] => Err(ExecError::invalid_args("this repository has no remote configured")),
            [only] => Ok(only.clone()),
            many if many.iter().any(|r| r == "origin") => Ok("origin".to_string()),
            _ => Err(ExecError::invalid_args("`remote` is required: the repository has several remotes and none is the branch's upstream")),
        }
    }

    pub fn fetch(&self, remote: Option<&str>, prune: bool, refspecs: &[Refspec]) -> ExecResult<Fetched> {
        let remote = self.remote(remote)?;
        let mut args = vec!["fetch", "--no-recurse-submodules"];
        if prune {
            args.push("--prune");
        }
        if !refspecs.is_empty() {
            args.extend(["--no-tags", "--quiet"]);
        }
        args.push(&remote);
        let specs: Vec<String> = refspecs.iter().map(Refspec::arg).collect();
        args.extend(specs.iter().map(String::as_str));
        let out = self.run_with(&args, &Opts { timeout: NETWORK_TIMEOUT, ..PLAIN })?;
        if !out.ok() {
            return Err(fail(&args, &out));
        }
        let mut refs = Vec::new();
        for spec in refspecs {
            let landed = format!("{}^{{commit}}", spec.destination);
            let sha = self.query(&["rev-parse", "--verify", "--quiet", &landed])?.ok_or_else(|| ExecError::new(Code::Io, format!("git fetched {} but {} names no commit", spec.source, spec.destination)))?;
            refs.push(FetchedRef { name: spec.destination.clone(), sha });
        }
        Ok(Fetched { remote: remote.clone(), output: out.said(), refs })
    }

    /// The branch of `remote` the current branch follows: its configured merge ref when that is on this remote, else its own name.
    fn remote_branch(&self, remote: &str) -> ExecResult<Option<String>> {
        let Some(branch) = self.current_branch()? else { return Ok(None) };
        let (remote_key, merge_key) = (format!("branch.{branch}.remote"), format!("branch.{branch}.merge"));
        let follows = self.query(&["config", "--get", &remote_key])?.as_deref() == Some(remote);
        let merge = if follows { self.query(&["config", "--get", &merge_key])? } else { None };
        Ok(Some(merge.and_then(|m| m.strip_prefix("refs/heads/").map(str::to_string)).unwrap_or(branch)))
    }

    /// Turns the end of a command that integrates commits into its result: a stop on conflicts is a result, any other failure an error.
    fn integrated(&self, args: &[&str], before: Option<String>, out: Out) -> ExecResult<Integrated> {
        let conflicts = self.conflicts()?;
        if !out.ok() && conflicts.is_empty() {
            return Err(fail(args, &out));
        }
        let head = self.head_sha()?.unwrap_or_default();
        Ok(Integrated { ok: out.ok() && conflicts.is_empty(), updated: Some(&head) != before.as_ref(), head, conflicted: !conflicts.is_empty(), conflicts, operation: self.operation()?, output: out.said() })
    }

    pub fn pull(&self, remote: Option<&str>, branch: Option<&str>, strategy: PullStrategy) -> ExecResult<Integrated> {
        let remote = self.remote(remote)?;
        let branch = match branch {
            Some(b) => b.to_string(),
            None => self.remote_branch(&remote)?.ok_or_else(|| ExecError::invalid_args("detached HEAD: name the remote `branch` to pull"))?,
        };
        let mut args = vec!["pull", "--no-edit", "--no-recurse-submodules"];
        args.push(match strategy {
            PullStrategy::FfOnly => "--ff-only",
            PullStrategy::Merge => "--no-rebase",
            PullStrategy::Rebase => "--rebase",
        });
        args.extend([remote.as_str(), branch.as_str()]);
        let before = self.head_sha()?;
        let out = self.run_with(&args, &Opts { timeout: NETWORK_TIMEOUT, ..PLAIN })?;
        self.integrated(&args, before, out)
    }

    pub fn push(&self, remote: Option<&str>, branch: Option<&str>, set_upstream: bool, force_with_lease: bool) -> ExecResult<Pushed> {
        let remote = self.remote(remote)?;
        let branch = match branch {
            Some(b) => b.to_string(),
            None => self.current_branch()?.ok_or_else(|| ExecError::invalid_args("detached HEAD: name the local `branch` to push"))?,
        };
        let local = format!("refs/heads/{branch}");
        if self.query(&["rev-parse", "--verify", "-q", &local])?.is_none() {
            return Err(ExecError::new(Code::NotFound, format!("git push: branch {branch} does not exist")));
        }
        let refspec = format!("{local}:{local}");
        let mut args = vec!["push", "--no-recurse-submodules"];
        for (on, flag) in [(set_upstream, "--set-upstream"), (force_with_lease, "--force-with-lease")] {
            if on {
                args.push(flag);
            }
        }
        args.extend([remote.as_str(), refspec.as_str()]);
        let out = self.run_with(&args, &Opts { timeout: NETWORK_TIMEOUT, ..PLAIN })?;
        if !out.ok() {
            return Err(fail(&args, &out));
        }
        Ok(Pushed { remote: remote.clone(), branch, set_upstream, output: out.said() })
    }

    pub fn fast_forward(&self, branch: Option<&str>, to: Option<&str>) -> ExecResult<FastForwarded> {
        let current = self.current_branch()?;
        let branch = match branch.map(str::to_string).or_else(|| current.clone()) {
            Some(b) => b,
            None => return Err(ExecError::invalid_args("detached HEAD: name the `branch` to fast-forward")),
        };
        let local = format!("refs/heads/{branch}");
        let from = self.query(&["rev-parse", "--verify", "-q", &local])?.ok_or_else(|| ExecError::new(Code::NotFound, format!("git fast-forward: branch {branch} does not exist")))?;
        let target = match to {
            Some(t) => t.to_string(),
            None => format!("{branch}@{{upstream}}"),
        };
        let commit = format!("{target}^{{commit}}");
        let to_sha = self.query(&["rev-parse", "--verify", "-q", &commit])?.ok_or_else(|| ExecError::invalid_args(if to.is_some() { "`to` is not a commit" } else { "the branch has no upstream: give `to`" }))?;
        if current.as_deref() == Some(branch.as_str()) {
            self.ok(&["merge", "--ff-only", &to_sha])?;
        } else {
            let elsewhere = self.ok(&["worktree", "list", "--porcelain"])?.lines().any(|l| l == format!("branch {local}"));
            if elsewhere {
                return Err(ExecError::invalid_args(format!("branch {branch} is checked out in another worktree")));
            }
            if !self.run(&["merge-base", "--is-ancestor", &from, &to_sha])?.ok() {
                return Err(ExecError::new(Code::Io, format!("git fast-forward: {branch} cannot be fast-forwarded to {target}")));
            }
            self.ok(&["update-ref", &local, &to_sha, &from])?;
        }
        let head = self.ok(&["rev-parse", &local])?.trim().to_string();
        Ok(FastForwarded { updated: head != from, branch, from, head })
    }

    pub fn merge(&self, step: Step, target: Option<&str>, strategy: MergeStrategy, message: Option<&str>) -> ExecResult<Integrated> {
        let mut args = vec!["merge"];
        match step {
            Step::Abort => args.push("--abort"),
            Step::Continue => args.push("--continue"),
            Step::Start => {
                args.push("--no-edit");
                args.extend(match strategy {
                    MergeStrategy::Default => None,
                    MergeStrategy::FfOnly => Some("--ff-only"),
                    MergeStrategy::NoFf => Some("--no-ff"),
                    MergeStrategy::Squash => Some("--squash"),
                });
                if let Some(message) = message {
                    args.extend(["-m", message]);
                }
                args.extend(target);
            }
        }
        let before = self.head_sha()?;
        let out = self.run(&args)?;
        self.integrated(&args, before, out)
    }

    pub fn rebase(&self, step: Step, onto: Option<&str>) -> ExecResult<Integrated> {
        let mut args = vec!["-c", "rebase.autoStash=false", "rebase"];
        match step {
            Step::Abort => args.push("--abort"),
            Step::Continue => args.push("--continue"),
            Step::Start => args.extend(onto),
        }
        let before = self.head_sha()?;
        let out = self.run(&args)?;
        self.integrated(&args, before, out)
    }

    /// `confined` is the file's resolved path, for the resolution that writes it.
    pub fn conflict_resolve(&self, path: &str, resolution: Resolution, content: Option<&str>, confined: Option<&Path>) -> ExecResult<Resolved> {
        if !self.conflicts()?.iter().any(|c| c == path) {
            return Err(ExecError::invalid_args(format!("`path` {path} is not a conflicted file")));
        }
        match resolution {
            Resolution::Ours | Resolution::Theirs => {
                let side = if resolution == Resolution::Ours { "--ours" } else { "--theirs" };
                // A side that deleted the file has nothing to check out: its resolution is the removal.
                if self.run(&["checkout", side, "--", path])?.ok() {
                    self.ok(&["add", "--", path])?;
                } else {
                    self.ok(&["rm", "-q", "-f", "--", path])?;
                }
            }
            Resolution::Content => {
                let target = confined.ok_or_else(|| ExecError::invalid_args("`content` needs a path inside the tree"))?;
                std::fs::write(target, content.unwrap_or("")).map_err(|e| io(path, e))?;
                self.ok(&["add", "--", path])?;
            }
            Resolution::Resolved => {
                self.ok(&["add", "--", path])?;
            }
        }
        Ok(Resolved { path: path.to_string(), remaining: self.conflicts()? })
    }

    pub fn blob(&self, rev: &str, path: &str, max_bytes: u64) -> ExecResult<BlobResult> {
        let spec = format!("{rev}:{path}");
        let kind = self.run(&["cat-file", "-t", &spec])?;
        if !kind.ok() {
            return Err(ExecError::new(Code::NotFound, format!("{path} does not exist at {rev}")));
        }
        if kind.text().trim() != "blob" {
            return Err(ExecError::new(Code::NotAFile, format!("{path} is not a file at {rev}")));
        }
        let size: u64 = self.ok(&["cat-file", "-s", &spec])?.trim().parse().unwrap_or(0);
        let args = ["cat-file", "blob", spec.as_str()];
        let out = self.run_with(&args, &Opts { max_out: max_bytes.min(BLOB_MAX) as usize, ..PLAIN })?;
        if !out.ok() && (out.stdout.len() as u64) < size.min(max_bytes) {
            return Err(fail(&args, &out));
        }
        let binary = out.stdout.iter().take(8000).any(|b| *b == 0);
        Ok(BlobResult { rev: rev.to_string(), path: path.to_string(), size, truncated: size > out.stdout.len() as u64, binary, data: base64::engine::general_purpose::STANDARD.encode(&out.stdout) })
    }

    /// Looks at the status every `interval` and sends it when it differs from the last one sent, until `cancelled` or `max_events` were sent.
    pub fn watch(&self, interval: Duration, max_events: Option<u64>, cancelled: &AtomicBool, events: &mut dyn FnMut(Value)) -> ExecResult<Watched> {
        let (mut sent, mut last) = (0u64, None);
        while !cancelled.load(Ordering::SeqCst) {
            let status = self.status()?;
            if last.as_ref() != Some(&status) {
                events(to_value(&GitEvent::Status { status: status.clone() }));
                sent += 1;
                last = Some(status);
                if max_events.is_some_and(|m| sent >= m) {
                    return Ok(Watched { events: sent, cancelled: false });
                }
            }
            let until = Instant::now() + interval;
            while Instant::now() < until && !cancelled.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(Watched { events: sent, cancelled: true })
    }

    pub fn call(&self, call: &GitCall, confined: Option<&Path>, cancelled: &Arc<AtomicBool>, events: &mut dyn FnMut(Value)) -> ExecResult<Value> {
        Ok(match call {
            GitCall::Status => to_value(&self.status()?),
            GitCall::Refs { kind, query, limit } => to_value(&self.refs(*kind, query.as_deref(), *limit)?),
            GitCall::Log { rev, path, limit, skip } => to_value(&self.log(rev.as_deref(), path.as_deref(), *limit, *skip)?),
            GitCall::Compare { base, head, limit } => to_value(&self.compare(base, head, *limit)?),
            GitCall::Upstream => to_value(&self.upstream()?),
            GitCall::Stage { paths } => to_value(&self.stage(paths)?),
            GitCall::Unstage { paths } => to_value(&self.unstage(paths)?),
            GitCall::Discard { paths, scope, untracked, source } => to_value(&self.discard(paths, *scope, *untracked, source.as_deref())?),
            GitCall::Commit { message, amend, all, allow_empty } => to_value(&self.commit(message, *amend, *all, *allow_empty)?),
            GitCall::Checkout { target, detach, track, force } => to_value(&self.checkout(target, *detach, *track, *force)?),
            GitCall::CreateRef { name, kind, from, checkout, message } => to_value(&self.create_ref(name, *kind, from.as_deref(), *checkout, message.as_deref())?),
            GitCall::Fetch { remote, prune, refspecs } => to_value(&self.fetch(remote.as_deref(), *prune, refspecs)?),
            GitCall::Pull { remote, branch, strategy } => to_value(&self.pull(remote.as_deref(), branch.as_deref(), *strategy)?),
            GitCall::Push { remote, branch, set_upstream, force_with_lease } => to_value(&self.push(remote.as_deref(), branch.as_deref(), *set_upstream, *force_with_lease)?),
            GitCall::FastForward { branch, to } => to_value(&self.fast_forward(branch.as_deref(), to.as_deref())?),
            GitCall::Merge { step, target, strategy, message } => to_value(&self.merge(*step, target.as_deref(), *strategy, message.as_deref())?),
            GitCall::Rebase { step, onto } => to_value(&self.rebase(*step, onto.as_deref())?),
            GitCall::ConflictResolve { path, resolution, content } => to_value(&self.conflict_resolve(path, *resolution, content.as_deref(), confined)?),
            GitCall::Blob { rev, path, max_bytes } => to_value(&self.blob(rev, path, *max_bytes)?),
            GitCall::Watch { interval_ms, max_events } => to_value(&self.watch(Duration::from_millis(*interval_ms), *max_events, cancelled, events)?),
            GitCall::Checkpoint { thread, turn } => to_value(&self.checkpoint(thread, turn)?),
            GitCall::Init { path, initial_branch, files, message } => to_value(&self.init(path, initial_branch.as_deref(), files, message)?),
        })
    }
}

/// Why a first commit did not happen, in a sentence a person can act on: git's own last line, or for a machine that has no identity what to set.
fn commit_failure(said: &str) -> String {
    let lower = said.to_lowercase();
    if ["identity unknown", "tell me who you are", "no name was given", "no email was given"].iter().any(|text| lower.contains(text)) {
        return "Git has no name or email on this machine. Set user.name and user.email, then commit.".to_string();
    }
    said.lines().map(str::trim).rfind(|line| !line.is_empty()).unwrap_or("Git could not make the first commit.").to_string()
}

/// `rest` of a porcelain v2 entry after its type: `fields` space-separated columns, then the path (which may hold spaces); the first is the XY pair.
fn split_entry(rest: &str, fields: usize) -> Option<(String, String)> {
    let mut parts = rest.splitn(fields, ' ');
    let xy = parts.next()?.to_string();
    let path = parts.last()?.to_string();
    Some((xy, path))
}

/// Registry of the watches that run, so a `cancel` reaches them.
pub type Watches = Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>;

#[cfg(test)]
mod tests {
    use super::commit_failure;

    #[test]
    fn given_what_git_says_when_a_first_commit_fails_then_a_missing_identity_says_what_to_set_and_anything_else_is_its_last_line() {
        // Given git's refusal for a machine with no name or email, in the three wordings git has used, and an unrelated failure
        let identity = ["Author identity unknown\n\n*** Please tell me who you are.\n\nfatal: unable to auto-detect email address", "fatal: empty ident name (for <>) not allowed\nno name was given", "*** Please tell me who you are"];
        let other = "error: gpg failed to sign the data\nfatal: failed to write commit object\n\n";
        // Then each identity wording is the same sentence, and the other is its last line
        for said in identity {
            assert_eq!(commit_failure(said), "Git has no name or email on this machine. Set user.name and user.email, then commit.", "{said}");
        }
        assert_eq!(commit_failure(other), "fatal: failed to write commit object");
        assert_eq!(commit_failure(""), "Git could not make the first commit.");
    }
}
