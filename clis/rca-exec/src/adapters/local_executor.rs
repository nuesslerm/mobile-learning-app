//! `ToolExecutor` on the local disk, rooted at the served directory and, when one is set, its worktree base. A path is relative to the
//! root or absolute under the root or the base; it is confined to that one directory: no `..`, symlinks resolved component by component (a
//! link that leaves it, or dangles, is `PATH_OUTSIDE_ROOT`). Only `browse` reads elsewhere, and only inside its allowlist.
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use crate::domain::ToolExecutor;
use base64::Engine;
use exec_fs::{Browse, BrowseEntry, BrowseResult, ChunkResult, CreateFile, EditFile, EntriesResult, Entry, EntryKind, Found, FoundEntry, ListDir, MakeDir, Match, PutBlob, ReadChunk, ReadFile, ReadResult, Search, SearchEntries, SearchKind, Skill, SkillScan, SkillScope, SkillsFound, Stat, StatResult, WriteFile};
use exec_git::{GitCall, GitDiff, GitOp, Resolution, SetupEvent, SetupResult, SetupStep, WorktreeAdd, WorktreeAdded, WorktreeInfo, WorktreeRemove, WorktreeSetup};
use exec_proc::{CliDetect, CliFound, RunCommand, RunResult};
use exec_pty::{PtyAttach, PtyAttached, PtyClose, PtyClosed, PtyEvent, PtyInfo, PtyOpen, PtyOpened, PtyResize, PtyWrite};
use crate::adapters::git::{Repo, Watches};
use crate::adapters::hosted::Hosted;
use crate::hosted::plan::Paths;
use exec_hosted::{HostedCall, HostedOp};
use crate::adapters::preview::PreviewTable;
use crate::adapters::pty::{PtyTable, Spawn};
use exec_preview::PreviewOp;
use serde_json::Value;
use exec_proto::{Code, ExecError, ExecResult, Stream};

pub const OUTPUT_CAP: usize = 1 << 20;
pub const READ_CAP: usize = 1 << 20;
pub const LIST_CAP: usize = 5000;
pub const MATCH_CAP: usize = 500;
const SEARCH_FILE_CAP: u64 = 2 << 20;
const SCAN_CAP: usize = 200_000;
/// What an entry search does not descend into.
const PRUNED: [&str; 2] = [".git", "node_modules"];

pub struct LocalExecutor {
    root: PathBuf,
    worktree_base: Option<PathBuf>,
    browse_allow: Vec<PathBuf>,
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
    setups: Mutex<HashMap<String, Arc<AtomicBool>>>,
    ptys: PtyTable,
    pty_shell: Option<PathBuf>,
    git_hooks: bool,
    watches: Watches,
    hosted: Hosted,
    previews: PreviewTable,
    cli_path: Option<String>,
    version_timeout: Duration,
    skill_home: Option<PathBuf>,
}

fn io(e: impl std::fmt::Display) -> ExecError {
    ExecError::new(Code::Io, e.to_string())
}

fn io_at(path: &str, e: std::io::Error) -> ExecError {
    match e.kind() {
        std::io::ErrorKind::NotFound => ExecError::new(Code::NotFound, format!("{path} does not exist")),
        _ => ExecError::new(Code::Io, format!("{path}: {e}")),
    }
}

fn outside(path: &str) -> ExecError {
    ExecError::new(Code::PathOutsideRoot, format!("{path} is outside the root"))
}

impl LocalExecutor {
    pub fn new(root: &Path) -> Result<LocalExecutor, String> {
        let root = fs::canonicalize(root).map_err(|e| format!("executor root {}: {e}", root.display()))?;
        Ok(LocalExecutor { root, worktree_base: None, browse_allow: Vec::new(), running: Mutex::new(HashMap::new()), setups: Mutex::new(HashMap::new()), ptys: PtyTable::default(), pty_shell: None, git_hooks: false, watches: Mutex::new(HashMap::new()), hosted: Hosted::default(), previews: PreviewTable::default(), cli_path: None, version_timeout: Duration::from_millis(exec_proc::VERSION_TIMEOUT_MS), skill_home: None })
    }

    /// Widens confinement to the directory worktrees are made in (created when missing).
    pub fn with_worktree_base(mut self, base: &Path) -> Result<LocalExecutor, String> {
        let existed = base.exists();
        fs::create_dir_all(base).map_err(|e| format!("worktree base {}: {e}", base.display()))?;
        let base = fs::canonicalize(base).map_err(|e| format!("worktree base {}: {e}", base.display()))?;
        if base.starts_with(&self.root) || self.root.starts_with(&base) {
            if !existed {
                let _ = fs::remove_dir(&base);
            }
            return Err(format!("worktree base {} and root {} must not contain each other", base.display(), self.root.display()));
        }
        self.worktree_base = Some(base);
        Ok(self)
    }

    /// Directories `browse` may list besides the root and the worktree base.
    pub fn with_browse_allow(mut self, dirs: &[PathBuf]) -> Result<LocalExecutor, String> {
        for dir in dirs {
            self.browse_allow.push(fs::canonicalize(dir).map_err(|e| format!("browse allowlist {}: {e}", dir.display()))?);
        }
        Ok(self)
    }

    /// The shell a pty starts (default: the executor's `$SHELL` when absolute, else `/bin/sh`).
    pub fn with_pty_shell(mut self, shell: &Path) -> Result<LocalExecutor, String> {
        if !shell.is_absolute() || !shell.is_file() {
            return Err(format!("pty shell {} must be an absolute path to a file", shell.display()));
        }
        self.pty_shell = Some(shell.to_path_buf());
        Ok(self)
    }

    /// Lets the repository's own hooks run in the git actions (off by default: a hook is code the repository chooses to run).
    pub fn with_git_hooks(mut self, on: bool) -> LocalExecutor {
        self.git_hooks = on;
        self
    }

    /// The `PATH` the hosted CLIs (`gh`, `glab`) are looked up on, in place of the executor's own.
    pub fn with_hosted_path(mut self, path: &str) -> LocalExecutor {
        self.hosted = Hosted::default().with_path(path);
        self
    }

    /// The `PATH` `cli_detect` looks for the agent CLIs on, in place of the executor's own (tests put their scripted CLIs on it).
    pub fn with_cli_path(mut self, path: &str) -> LocalExecutor {
        self.cli_path = Some(path.to_string());
        self
    }

    /// How long a CLI's `--version` may run before it counts as not answering.
    pub fn with_version_timeout(mut self, limit: Duration) -> LocalExecutor {
        self.version_timeout = limit;
        self
    }

    /// The executor's own home, whose skill directories `skill_scan` reads when asked (and nothing else of it).
    pub fn with_skill_home(mut self, home: Option<&Path>) -> LocalExecutor {
        self.skill_home = home.map(Path::to_path_buf);
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn resolve(&self, path: &str) -> ExecResult<PathBuf> {
        let given = Path::new(path);
        let (jail, rel) = if given.is_absolute() {
            std::iter::once(&self.root).chain(&self.worktree_base).find_map(|j| given.strip_prefix(j).ok().map(|rel| (j, rel))).ok_or_else(|| outside(path))?
        } else {
            (&self.root, given)
        };
        let mut cur = jail.clone();
        for c in rel.components() {
            match c {
                Component::CurDir => {}
                Component::Normal(name) => {
                    cur.push(name);
                    match fs::symlink_metadata(&cur) {
                        Ok(m) if m.file_type().is_symlink() => {
                            cur = fs::canonicalize(&cur).map_err(|_| outside(path))?;
                            if !cur.starts_with(jail) {
                                return Err(outside(path));
                            }
                        }
                        _ => {}
                    }
                }
                _ => return Err(outside(path)),
            }
        }
        Ok(cur)
    }

    fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root).unwrap_or(p).to_string_lossy().into_owned()
    }

    fn stop(&self, id: &str) {
        self.running.lock().unwrap().remove(id);
    }
}

enum Chunk {
    Data(Stream, Vec<u8>),
}

const VERSION_OUTPUT_CAP: usize = 64 << 10;

/// The first file named `program` on `search` that its owner can run; a relative entry of the path is skipped.
fn find_program(program: &str, search: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(search).filter(|dir| dir.is_absolute()).map(|dir| dir.join(program)).find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

/// Runs `<path> --version` with no shell, no stdin and an environment of `PATH` and `HOME` only; its first line of text, or why there is none.
fn probe_version(path: &Path, search: &str, limit: Duration) -> Result<Option<String>, String> {
    let mut cmd = Command::new(path);
    cmd.arg("--version").env_clear().env("PATH", search).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    if let Some(home) = std::env::var_os("HOME") {
        cmd.env("HOME", home);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel();
    if let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) {
        pump(out, Stream::Stdout, tx.clone());
        pump(err, Stream::Stderr, tx);
    }
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let take = |out: &mut Vec<u8>, err: &mut Vec<u8>| {
        while let Ok(Chunk::Data(stream, bytes)) = rx.try_recv() {
            let into = if stream == Stream::Stdout { &mut *out } else { &mut *err };
            let room = VERSION_OUTPUT_CAP.saturating_sub(into.len());
            into.extend_from_slice(&bytes[..bytes.len().min(room)]);
        }
    };
    let deadline = Instant::now() + limit;
    let status = loop {
        take(&mut out, &mut err);
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                kill_group(child.id() as i32);
                let _ = child.wait();
                return Err("timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(e.to_string()),
        }
    };
    // The last bytes may still be in the pipes' readers
    std::thread::sleep(Duration::from_millis(20));
    take(&mut out, &mut err);
    if !status.success() {
        return Err(status.code().map_or_else(|| "ended by a signal".to_string(), |c| format!("exited with {c}")));
    }
    Ok(exec_proc::version_line(&String::from_utf8_lossy(&out)).or_else(|| exec_proc::version_line(&String::from_utf8_lossy(&err))))
}

fn pump(mut from: impl Read + Send + 'static, stream: Stream, tx: mpsc::Sender<Chunk>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = from.read(&mut buf) {
            if n == 0 || tx.send(Chunk::Data(stream, buf[..n].to_vec())).is_err() {
                break;
            }
        }
    });
}

fn kill_group(pid: i32) {
    // SAFETY: a negative pid addresses the process group the child leads; the call only sends a signal.
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

/// Adds the skills below `base`'s skill directories to `found`: a directory per skill holding `SKILL.md`, in directory-name order. A link
/// that leaves `jail` is skipped, so a skill never reads a file the executor was not given. `label` names a path in the answer.
fn scan_skills(base: &Path, jail: &Path, scope: SkillScope, label: &dyn Fn(&Path) -> String, found: &mut SkillsFound) {
    let inside = |p: &Path| fs::canonicalize(p).is_ok_and(|c| c.starts_with(jail));
    for dir in exec_fs::SKILL_DIRS {
        let skills_dir = base.join(dir);
        if !inside(&skills_dir) {
            continue;
        }
        let Ok(read) = fs::read_dir(&skills_dir) else { continue };
        let mut names: Vec<String> = read.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        for name in names {
            let file = skills_dir.join(&name).join(exec_fs::SKILL_FILE);
            if !inside(&file) || !fs::metadata(&file).is_ok_and(|m| m.is_file()) {
                continue;
            }
            let Ok(opened) = fs::File::open(&file) else { continue };
            let mut head = Vec::new();
            if opened.take(exec_fs::SKILL_READ_MAX).read_to_end(&mut head).is_err() {
                continue;
            }
            if found.skills.len() >= exec_fs::SKILLS_MAX {
                found.truncated = true;
                return;
            }
            let (title, description) = exec_fs::front_matter(&String::from_utf8_lossy(&head));
            found.skills.push(Skill { name: title.unwrap_or_else(|| name.chars().take(exec_fs::SKILL_NAME_MAX).collect()), description, scope, dir: dir.to_string(), path: label(&file) });
        }
    }
}

impl ToolExecutor for LocalExecutor {
    fn run_command(&self, id: &str, args: &RunCommand, output: &mut dyn FnMut(Stream, &str)) -> ExecResult<RunResult> {
        let cwd = match &args.cwd {
            Some(c) => self.resolve(c)?,
            None => self.root.clone(),
        };
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&args.command).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
        cmd.envs(args.env.iter().map(|(k, v)| (k, v)));
        let cancelled = Arc::new(AtomicBool::new(false));
        self.running.lock().unwrap().insert(id.to_string(), cancelled.clone());
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.stop(id);
                return Err(io(e));
            }
        };
        let pid = child.id() as i32;
        let (tx, rx) = mpsc::channel();
        pump(child.stdout.take().expect("piped"), Stream::Stdout, tx.clone());
        pump(child.stderr.take().expect("piped"), Stream::Stderr, tx);
        let deadline = Instant::now() + Duration::from_millis(args.timeout_ms);
        let (mut sent, mut truncated, mut timed_out, mut killed) = (0usize, false, false, false);
        let mut forward = |chunk: Chunk, sent: &mut usize, truncated: &mut bool| {
            let Chunk::Data(stream, bytes) = chunk;
            let room = OUTPUT_CAP.saturating_sub(*sent);
            if bytes.len() > room {
                *truncated = true;
            }
            let take = &bytes[..bytes.len().min(room)];
            if !take.is_empty() {
                *sent += take.len();
                output(stream, &String::from_utf8_lossy(take));
            }
        };
        loop {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(chunk) => forward(chunk, &mut sent, &mut truncated),
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if !killed && (cancelled.load(Ordering::SeqCst) || Instant::now() >= deadline) {
                timed_out = !cancelled.load(Ordering::SeqCst);
                killed = true;
                kill_group(pid);
            }
        }
        let status = child.wait().map_err(io)?;
        self.stop(id);
        // A background process that kept the group alive past the pipes must not outlive the action.
        kill_group(pid);
        let exit_code = status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
        Ok(RunResult { exit_code, timed_out, cancelled: cancelled.load(Ordering::SeqCst), truncated })
    }

    fn read_file(&self, args: &ReadFile) -> ExecResult<ReadResult> {
        let path = self.resolve(&args.path)?;
        if path.is_dir() {
            return Err(ExecError::new(Code::NotAFile, format!("{} is a directory", args.path)));
        }
        let bytes = fs::read(&path).map_err(|e| io_at(&args.path, e))?;
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let end = args.limit.map_or(lines.len(), |l| args.offset.saturating_add(l).min(lines.len()));
        let mut content = lines.get(args.offset..end.max(args.offset.min(lines.len()))).unwrap_or(&[]).concat();
        let truncated = content.len() > READ_CAP;
        if truncated {
            let mut cut = READ_CAP;
            while !content.is_char_boundary(cut) {
                cut -= 1;
            }
            content.truncate(cut);
        }
        Ok(ReadResult { content, total_lines: lines.len(), truncated })
    }

    fn write_file(&self, args: &WriteFile) -> ExecResult<usize> {
        let path = self.resolve(&args.path)?;
        if args.create_dirs {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| io_at(&args.path, e))?;
            }
        }
        fs::write(&path, args.content.as_bytes()).map_err(|e| io_at(&args.path, e))?;
        Ok(args.content.len())
    }

    fn edit_file(&self, args: &EditFile) -> ExecResult<usize> {
        if args.old.is_empty() {
            return Err(ExecError::new(Code::InvalidArgs, "`old` must not be empty"));
        }
        let path = self.resolve(&args.path)?;
        let text = fs::read_to_string(&path).map_err(|e| io_at(&args.path, e))?;
        let count = text.matches(&args.old).count();
        if count == 0 {
            return Err(ExecError::new(Code::EditNotFound, format!("`old` does not occur in {}", args.path)));
        }
        if count > 1 && !args.replace_all {
            return Err(ExecError::new(Code::EditAmbiguous, format!("`old` occurs {count} times in {}; add context or set replace_all", args.path)));
        }
        fs::write(&path, text.replace(&args.old, &args.new)).map_err(|e| io_at(&args.path, e))?;
        Ok(count)
    }

    fn git_diff(&self, args: &GitDiff) -> ExecResult<String> {
        let (path, base) = (args.path.as_deref(), args.base.as_deref());
        let dir = match &args.worktree {
            Some(name) => self.worktree_dir(name)?,
            None => self.root.clone(),
        };
        let mut command = Command::new("git");
        command.arg("-C").arg(&dir).args(["diff", "--no-color", "--no-ext-diff", "--no-textconv"]);
        if args.staged {
            command.arg("--cached");
        }
        if let Some(base) = base {
            command.arg(base);
            if let Some(head) = &args.head {
                command.arg(head);
            }
        }
        if let Some(path) = path {
            // A path is held to the tree it names, a worktree's included.
            let given = if args.worktree.is_some() { dir.join(path).to_string_lossy().into_owned() } else { path.to_string() };
            command.arg("--").arg(self.resolve(&given)?);
        }
        let out = command.stdin(Stdio::null()).output().map_err(|e| ExecError::new(Code::Io, format!("git diff: {e}")))?;
        if !out.status.success() {
            return Err(ExecError::new(Code::Io, format!("git diff: {}", String::from_utf8_lossy(&out.stderr).trim())));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn list_dir(&self, args: &ListDir) -> ExecResult<Vec<Entry>> {
        let base = self.resolve(&args.path)?;
        let mut out = Vec::new();
        self.walk(&base, &base, args.depth.max(1), &[".git"], &mut |p, m| {
            if out.len() >= LIST_CAP {
                return false;
            }
            out.push(Entry { name: p.strip_prefix(&base).unwrap_or(p).to_string_lossy().into_owned(), kind: if m.is_dir() { EntryKind::Dir } else { EntryKind::File }, size: m.len() });
            true
        })
        .map_err(|e| io_at(&args.path, e))?;
        Ok(out)
    }

    fn search(&self, args: &Search) -> ExecResult<Found> {
        let base = self.resolve(&args.path)?;
        let glob = args.glob.as_deref().map(glob_regex).transpose()?;
        let mut matches = Vec::new();
        let mut truncated = false;
        let mut files = Vec::new();
        self.walk(&base, &base, usize::MAX, &[".git"], &mut |p, m| {
            if m.is_file() {
                files.push(p.to_path_buf());
            }
            true
        })
        .map_err(|e| io_at(&args.path, e))?;
        let wanted = |p: &Path| glob.as_ref().is_none_or(|g| g.is_match(&p.strip_prefix(&base).unwrap_or(p).to_string_lossy()) || p.file_name().is_some_and(|n| g.is_match(&n.to_string_lossy())));
        match args.kind {
            SearchKind::Files => {
                let pattern = glob_regex(&args.pattern)?;
                for p in files {
                    let shown = p.strip_prefix(&base).unwrap_or(&p).to_string_lossy().into_owned();
                    if (pattern.is_match(&shown) || p.file_name().is_some_and(|n| pattern.is_match(&n.to_string_lossy()))) && wanted(&p) {
                        if matches.len() >= MATCH_CAP {
                            truncated = true;
                            break;
                        }
                        matches.push(Match { path: self.rel(&p), line: None, text: None });
                    }
                }
            }
            SearchKind::Content => {
                let re = Regex::new(&args.pattern).map_err(|e| ExecError::new(Code::InvalidArgs, format!("bad pattern: {e}")))?;
                'files: for p in files.into_iter().filter(|p| wanted(p)) {
                    if fs::metadata(&p).map_or(true, |m| m.len() > SEARCH_FILE_CAP) {
                        continue;
                    }
                    let Ok(text) = fs::read_to_string(&p) else { continue };
                    for (i, line) in text.lines().enumerate() {
                        if re.is_match(line) {
                            if matches.len() >= MATCH_CAP {
                                truncated = true;
                                break 'files;
                            }
                            matches.push(Match { path: self.rel(&p), line: Some(i + 1), text: Some(line.chars().take(500).collect()) });
                        }
                    }
                }
            }
        }
        Ok(Found { matches, truncated })
    }

    fn stat(&self, args: &Stat) -> ExecResult<StatResult> {
        let meta = fs::metadata(self.resolve(&args.path)?).map_err(|e| io_at(&args.path, e))?;
        let modified_ms = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
        Ok(StatResult { kind: if meta.is_dir() { EntryKind::Dir } else { EntryKind::File }, size: meta.len(), modified_ms })
    }

    fn read_chunk(&self, args: &ReadChunk) -> ExecResult<ChunkResult> {
        let path = self.resolve(&args.path)?;
        if path.is_dir() {
            return Err(ExecError::new(Code::NotAFile, format!("{} is a directory", args.path)));
        }
        let mut file = fs::File::open(&path).map_err(|e| io_at(&args.path, e))?;
        let total = file.metadata().map_err(|e| io_at(&args.path, e))?.len();
        let mut bytes = Vec::new();
        if args.offset < total {
            file.seek(SeekFrom::Start(args.offset)).map_err(|e| io_at(&args.path, e))?;
            file.take(args.length).read_to_end(&mut bytes).map_err(|e| io_at(&args.path, e))?;
        }
        let read = bytes.len() as u64;
        Ok(ChunkResult { data: base64::engine::general_purpose::STANDARD.encode(&bytes), offset: args.offset, bytes: read, total_bytes: total, eof: args.offset.saturating_add(read) >= total })
    }

    fn create_file(&self, args: &CreateFile) -> ExecResult<usize> {
        let path = self.resolve(&args.path)?;
        if args.create_dirs {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| io_at(&args.path, e))?;
            }
        }
        // create_new is O_EXCL: it never opens, and never follows a link at, a path that exists.
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => ExecError::new(Code::AlreadyExists, format!("{} already exists", args.path)),
            _ => io_at(&args.path, e),
        })?;
        std::io::Write::write_all(&mut file, args.content.as_bytes()).map_err(|e| io_at(&args.path, e))?;
        Ok(args.content.len())
    }

    fn put_blob(&self, args: &PutBlob) -> ExecResult<u64> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(&args.data).map_err(|_| ExecError::invalid_args("`data` must be base64"))?;
        if bytes.len() as u64 > exec_fs::BLOB_MAX {
            return Err(ExecError::invalid_args(format!("`data` is longer than {} bytes", exec_fs::BLOB_MAX)));
        }
        let path = self.resolve(&args.path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_at(&args.path, e))?;
        }
        // create_new is O_EXCL: it never opens, and never follows a link at, a path that exists.
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => ExecError::new(Code::AlreadyExists, format!("{} already exists", args.path)),
            _ => io_at(&args.path, e),
        })?;
        std::io::Write::write_all(&mut file, &bytes).map_err(|e| io_at(&args.path, e))?;
        Ok(bytes.len() as u64)
    }

    fn make_dir(&self, args: &MakeDir) -> ExecResult<bool> {
        let path = self.resolve(&args.path)?;
        if args.parents && path.is_dir() {
            return Ok(false);
        }
        let made = if args.parents { fs::create_dir_all(&path) } else { fs::create_dir(&path) };
        made.map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => ExecError::new(Code::AlreadyExists, format!("{} already exists", args.path)),
            _ => io_at(&args.path, e),
        })?;
        Ok(true)
    }

    fn browse(&self, args: &Browse) -> ExecResult<BrowseResult> {
        let given = Path::new(&args.path);
        let dir = if given.is_absolute() { fs::canonicalize(given).map_err(|e| io_at(&args.path, e))? } else { self.resolve(&args.path)? };
        let shown = |p: &Path| std::iter::once(&self.root).chain(&self.worktree_base).chain(&self.browse_allow).any(|a| p.starts_with(a));
        if !shown(&dir) {
            return Err(outside(&args.path));
        }
        if !fs::metadata(&dir).map_err(|e| io_at(&args.path, e))?.is_dir() {
            return Err(ExecError::invalid_args(format!("{} is not a directory", args.path)));
        }
        let mut entries: Vec<BrowseEntry> = fs::read_dir(&dir)
            .map_err(|e| io_at(&args.path, e))?
            .flatten()
            .filter(|e| args.show_hidden || !e.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|e| {
                let kind = if fs::metadata(e.path()).ok()?.is_dir() { EntryKind::Dir } else { EntryKind::File };
                Some(BrowseEntry { name: e.file_name().to_string_lossy().into_owned(), kind, path: e.path().to_string_lossy().into_owned() })
            })
            .collect();
        entries.sort_by(|a, b| (a.kind == EntryKind::File, &a.name).cmp(&(b.kind == EntryKind::File, &b.name)));
        entries.truncate(LIST_CAP);
        let parent = dir.parent().filter(|p| shown(p)).map(|p| p.to_string_lossy().into_owned());
        Ok(BrowseResult { path: dir.to_string_lossy().into_owned(), parent, entries })
    }

    fn cli_detect(&self, args: &CliDetect) -> ExecResult<Vec<CliFound>> {
        let search = self.cli_path.clone().or_else(|| std::env::var("PATH").ok()).unwrap_or_default();
        Ok(args
            .programs
            .iter()
            .map(|program| match find_program(program, &search) {
                None => CliFound { program: program.clone(), installed: false, path: None, version: None, error: None },
                Some(path) => {
                    let found = probe_version(&path, &search, self.version_timeout);
                    let (version, error) = match found {
                        Ok(version) => (version, None),
                        Err(error) => (None, Some(error)),
                    };
                    CliFound { program: program.clone(), installed: true, path: Some(path.to_string_lossy().into_owned()), version, error }
                }
            })
            .collect())
    }

    fn skill_scan(&self, args: &SkillScan) -> ExecResult<SkillsFound> {
        let root = self.resolve(&args.root)?;
        if !fs::metadata(&root).map_err(|e| io_at(&args.root, e))?.is_dir() {
            return Err(ExecError::invalid_args(format!("{} is not a directory", args.root)));
        }
        let jail = std::iter::once(&self.root).chain(&self.worktree_base).find(|j| root.starts_with(j)).unwrap_or(&self.root);
        let mut found = SkillsFound { skills: Vec::new(), truncated: false };
        scan_skills(&root, jail, SkillScope::Workspace, &|p| self.rel(p), &mut found);
        if let Some(home) = args.home.then(|| self.skill_home.as_ref().and_then(|h| fs::canonicalize(h).ok())).flatten() {
            scan_skills(&home, &home, SkillScope::Home, &|p| format!("~/{}", p.strip_prefix(&home).unwrap_or(p).to_string_lossy()), &mut found);
        }
        Ok(found)
    }

    fn search_entries(&self, args: &SearchEntries) -> ExecResult<EntriesResult> {
        let query = args.query.to_lowercase();
        if query.is_empty() {
            return Err(ExecError::invalid_args("`query` must not be empty"));
        }
        let (mut hits, mut scanned, mut truncated) = (Vec::new(), 0usize, false);
        let dir = match &args.path {
            Some(path) => self.resolve(path)?,
            None => self.root.clone(),
        };
        self.walk(&dir, &dir, usize::MAX, &PRUNED, &mut |p, m| {
            scanned += 1;
            if scanned > SCAN_CAP {
                truncated = true;
                return false;
            }
            let path = p.strip_prefix(&dir).unwrap_or(p).to_string_lossy().into_owned();
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            let rank = match () {
                _ if name == query => 0,
                _ if name.starts_with(&query) => 1,
                _ if name.contains(&query) => 2,
                _ if path.to_lowercase().contains(&query) => 3,
                _ => return true,
            };
            hits.push((rank, path, if m.is_dir() { EntryKind::Dir } else { EntryKind::File }));
            true
        })
        .map_err(io)?;
        hits.sort_by(|a, b| (a.0, a.1.len(), &a.1).cmp(&(b.0, b.1.len(), &b.1)));
        truncated |= hits.len() > args.limit;
        hits.truncate(args.limit);
        Ok(EntriesResult { entries: hits.into_iter().map(|(_, path, kind)| FoundEntry { path, kind }).collect(), truncated })
    }

    fn git_worktree_add(&self, args: &WorktreeAdd) -> ExecResult<WorktreeAdded> {
        let path = self.worktree_path(&args.name)?;
        if fs::symlink_metadata(&path).is_ok() {
            return Err(ExecError::new(Code::AlreadyExists, format!("worktree {} already exists", args.name)));
        }
        let at = path.to_string_lossy().into_owned();
        match &args.base {
            Some(base) => self.git(&self.root, &["worktree", "add", "-b", &args.branch, &at, base])?,
            None => self.git(&self.root, &["worktree", "add", &at, &args.branch])?,
        };
        let head = self.git(&path, &["rev-parse", "HEAD"])?.trim().to_string();
        Ok(WorktreeAdded { path: at, branch: args.branch.clone(), head })
    }

    fn git_worktree_remove(&self, args: &WorktreeRemove) -> ExecResult<bool> {
        let path = self.worktree_path(&args.name)?;
        let at = path.to_string_lossy().into_owned();
        // Only a worktree git lists is removed, so this can never delete a directory that merely sits in the base.
        if !self.worktrees()?.iter().any(|w| w.path == at && !w.is_main) {
            return Err(ExecError::new(Code::NotFound, format!("worktree {} does not exist", args.name)));
        }
        let mut git_args = vec!["worktree", "remove"];
        if args.force {
            git_args.push("--force");
        }
        git_args.extend(["--", &at]);
        self.git(&self.root, &git_args)?;
        Ok(true)
    }

    fn git_worktree_list(&self) -> ExecResult<Vec<WorktreeInfo>> {
        self.worktrees()
    }

    fn worktree_setup(&self, id: &str, args: &WorktreeSetup, events: &mut dyn FnMut(SetupEvent)) -> ExecResult<SetupResult> {
        let path = self.worktree_path(&args.name)?;
        if !path.is_dir() {
            return Err(ExecError::new(Code::NotFound, format!("worktree {} does not exist", args.name)));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        self.setups.lock().unwrap().insert(id.to_string(), cancelled.clone());
        let deadline = Instant::now() + Duration::from_millis(args.timeout_ms);
        let cwd = path.to_string_lossy().into_owned();
        events(SetupEvent::Started { steps: args.commands.len() });
        let (mut steps, mut timed_out) = (Vec::new(), false);
        for (step, command) in args.commands.iter().enumerate() {
            let left = deadline.saturating_duration_since(Instant::now());
            if cancelled.load(Ordering::SeqCst) || left.is_zero() {
                timed_out = !cancelled.load(Ordering::SeqCst);
                break;
            }
            events(SetupEvent::StepStarted { step, command: command.clone() });
            let run = RunCommand { command: command.clone(), cwd: Some(cwd.clone()), timeout_ms: left.as_millis() as u64, env: vec![] };
            let done = self.run_command(id, &run, &mut |stream, data| events(SetupEvent::StepOutput { step, stream: stream.as_str().into(), data: data.into() }));
            let result = match done {
                Ok(r) => r,
                Err(e) => {
                    self.setups.lock().unwrap().remove(id);
                    return Err(e);
                }
            };
            events(SetupEvent::StepFinished { step, exit_code: result.exit_code });
            steps.push(SetupStep { command: command.clone(), exit_code: result.exit_code });
            timed_out |= result.timed_out;
            if result.exit_code != 0 {
                break;
            }
        }
        self.setups.lock().unwrap().remove(id);
        let ok = !timed_out && !cancelled.load(Ordering::SeqCst) && steps.len() == args.commands.len() && steps.iter().all(|s| s.exit_code == 0);
        events(SetupEvent::Finished { ok });
        Ok(SetupResult { ok, steps, timed_out, cancelled: cancelled.load(Ordering::SeqCst) })
    }

    fn pty_open(&self, args: &PtyOpen) -> ExecResult<PtyOpened> {
        let cwd = match &args.cwd {
            Some(c) => self.resolve(c)?,
            None => self.root.clone(),
        };
        if !cwd.is_dir() {
            return Err(ExecError::new(Code::NotFound, format!("{} is not a directory", cwd.display())));
        }
        let shell = self.pty_shell.clone().or_else(|| std::env::var_os("SHELL").map(PathBuf::from).filter(|s| s.is_absolute() && s.is_file())).unwrap_or_else(|| PathBuf::from("/bin/sh"));
        self.ptys.open(Spawn { pty: &args.pty, shell: &shell, cwd, cols: args.cols, rows: args.rows, env: &args.env })
    }

    fn pty_write(&self, args: &PtyWrite) -> ExecResult<usize> {
        self.ptys.write(&args.pty, args.data.as_bytes())
    }

    fn pty_resize(&self, args: &PtyResize) -> ExecResult<()> {
        self.ptys.resize(&args.pty, args.cols, args.rows)
    }

    fn pty_close(&self, args: &PtyClose) -> ExecResult<PtyClosed> {
        self.ptys.close(args)
    }

    fn pty_list(&self) -> ExecResult<Vec<PtyInfo>> {
        Ok(self.ptys.list())
    }

    fn pty_attach(&self, id: &str, args: &PtyAttach, output: &mut dyn FnMut(u64, &[u8]), events: &mut dyn FnMut(PtyEvent)) -> ExecResult<PtyAttached> {
        self.ptys.attach(id, args, output, events)
    }

    fn git_op(&self, id: &str, op: &GitOp, events: &mut dyn FnMut(Value)) -> ExecResult<Value> {
        let dir = match (&op.call, &op.worktree) {
            // The repository is made where the request says, and each first file is held to the tree like any other path.
            (GitCall::Init { path, files, .. }, _) => {
                let dir = self.resolve(path)?;
                for file in files {
                    self.resolve(&dir.join(&file.path).to_string_lossy())?;
                }
                dir
            }
            (_, Some(name)) => self.worktree_dir(name)?,
            (_, None) => self.root.clone(),
        };
        // A file the action writes is held to the tree like any other path.
        let confined = match &op.call {
            GitCall::ConflictResolve { path, resolution: Resolution::Content, .. } => Some(self.resolve(&dir.join(path).to_string_lossy())?),
            _ => None,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        self.watches.lock().unwrap().insert(id.to_string(), cancelled.clone());
        let done = Repo::new(dir, self.git_hooks).call(&op.call, confined.as_deref(), &cancelled, events);
        self.watches.lock().unwrap().remove(id);
        done
    }

    fn preview_op(&self, op: &PreviewOp) -> ExecResult<Value> {
        match op {
            PreviewOp::Start(start) => {
                let dir = match &start.cwd {
                    Some(c) => self.resolve(c)?,
                    None => self.root.clone(),
                };
                if !dir.is_dir() {
                    return Err(ExecError::new(Code::NotFound, format!("{} is not a directory", start.cwd.as_deref().unwrap_or("."))));
                }
                let shown = self.rel(&dir);
                let shown = if shown.is_empty() { ".".to_string() } else { shown };
                Ok(exec_proto::to_value(&self.previews.start(&dir, &shown, start)?))
            }
            PreviewOp::Stop(stop) => Ok(exec_proto::to_value(&self.previews.stop(&stop.id))),
            PreviewOp::List => Ok(exec_proto::to_value(&self.previews.list())),
        }
    }

    fn hosted_op(&self, id: &str, op: &HostedOp) -> ExecResult<Value> {
        // A directory a clone or a publish names is held to the root and the worktree base like any other path.
        let mut paths = Paths::default();
        match &op.call {
            HostedCall::RepoClone { dest } => {
                let target = self.resolve(dest)?;
                let empty = match fs::read_dir(&target) {
                    Ok(mut entries) => entries.next().is_none(),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
                    Err(_) => false,
                };
                if !empty {
                    return Err(ExecError::new(Code::AlreadyExists, format!("{dest} exists and is not an empty directory")));
                }
                paths.dest = Some(target);
            }
            HostedCall::RepoPublish { dir, .. } => {
                let target = self.resolve(dir)?;
                if !target.join(".git").exists() {
                    return Err(ExecError::new(Code::NotFound, format!("{dir} is not a git repository")));
                }
                paths.dir = Some(target);
            }
            _ => {}
        }
        let mut done = self.hosted.call(id, op, &paths, &self.root)?;
        if let (HostedCall::RepoClone { .. }, Some(target), Some(map)) = (&op.call, &paths.dest, done.as_object_mut()) {
            map.insert("path".into(), Value::String(self.rel(target)));
        }
        Ok(done)
    }

    fn cancel(&self, id: &str) -> bool {
        if self.ptys.cancel_attach(id) || self.hosted.cancel(id) {
            return true;
        }
        if let Some(flag) = self.watches.lock().unwrap().get(id) {
            flag.store(true, Ordering::SeqCst);
            return true;
        }
        let setup = self.setups.lock().unwrap().get(id).map(|flag| flag.store(true, Ordering::SeqCst)).is_some();
        match self.running.lock().unwrap().get(id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => setup,
        }
    }
}

impl LocalExecutor {
    /// `<worktree base>/<name>`; the name is one plain segment (checked by the contract), so the path stays under the base.
    fn worktree_path(&self, name: &str) -> ExecResult<PathBuf> {
        let base = self.worktree_base.as_ref().ok_or_else(|| ExecError::new(Code::Io, "this executor has no worktree base (start it with --worktree-base)"))?;
        Ok(base.join(exec_git::worktree_name(name)?))
    }

    /// The directory of an existing worktree of the base, for the actions that work in one.
    fn worktree_dir(&self, name: &str) -> ExecResult<PathBuf> {
        let path = self.worktree_path(name)?;
        if !path.is_dir() {
            return Err(ExecError::new(Code::NotFound, format!("worktree {name} does not exist")));
        }
        Ok(path)
    }

    /// One git invocation in `dir`: no prompt, no hooks of the repository, stdout on success and git's own message on failure.
    fn git(&self, dir: &Path, args: &[&str]) -> ExecResult<String> {
        let out = Command::new("git")
            .args(if self.git_hooks { ["-c", "core.fsmonitor=false"] } else { ["-c", "core.hooksPath=/dev/null"] })
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .output()
            .map_err(|e| ExecError::new(Code::Io, format!("git {}: {e}", args[0])))?;
        if !out.status.success() {
            return Err(ExecError::new(Code::Io, format!("git {}: {}", args[..2.min(args.len())].join(" "), String::from_utf8_lossy(&out.stderr).trim())));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn worktrees(&self) -> ExecResult<Vec<WorktreeInfo>> {
        let listing = self.git(&self.root, &["worktree", "list", "--porcelain"])?;
        let mut out: Vec<WorktreeInfo> = Vec::new();
        for line in listing.lines() {
            match line.split_once(' ') {
                Some(("worktree", path)) => out.push(WorktreeInfo { path: path.to_string(), head: String::new(), branch: None, is_main: out.is_empty() }),
                Some(("HEAD", head)) => out.last_mut().map(|w| w.head = head.to_string()).unwrap_or_default(),
                Some(("branch", name)) => out.last_mut().map(|w| w.branch = Some(name.strip_prefix("refs/heads/").unwrap_or(name).to_string())).unwrap_or_default(),
                _ => {}
            }
        }
        Ok(out)
    }

    /// Depth-first, sorted, never following a symlink and skipping the entries named in `skip`; `visit` returns false to stop.
    fn walk(&self, base: &Path, dir: &Path, depth: usize, skip: &[&str], visit: &mut dyn FnMut(&Path, &fs::Metadata) -> bool) -> std::io::Result<bool> {
        let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if skip.iter().any(|p| e.file_name() == *p) {
                continue;
            }
            let path = e.path();
            let m = fs::symlink_metadata(&path)?;
            if !visit(&path, &m) {
                return Ok(false);
            }
            if m.is_dir() && path.strip_prefix(base).is_ok_and(|r| r.components().count() < depth) && !self.walk(base, &path, depth, skip, visit)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// `*` within a segment, `**` across segments, `?` one character; everything else literal.
fn glob_regex(glob: &str) -> ExecResult<Regex> {
    let mut re = String::from("^");
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    re.push_str("(?:.*/)?");
                } else {
                    re.push_str(".*");
                }
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    Regex::new(&re).map_err(|e| ExecError::new(Code::InvalidArgs, format!("bad glob: {e}")))
}
