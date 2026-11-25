use exec_proc::SessionOpen;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Component, Path, PathBuf};

const MAX_FRAME: usize = 1 << 20;
const MAX_OUTPUT: usize = 32 << 10;

pub(super) struct ClaudeTaskOutput {
    session: String,
    base: PathBuf,
    canonical_base: PathBuf,
    uid: u32,
    running: HashSet<(String, Option<String>)>,
    line: String,
    oversized: bool,
}

impl ClaudeTaskOutput {
    pub(super) fn for_session(args: &SessionOpen) -> Option<Self> {
        if args.program != "claude" {
            return None;
        }
        let session = args.args.chunks_exact(2).find(|pair| matches!(pair[0].as_str(), "--session-id" | "--resume"))?[1].clone();
        let base = std::env::var_os("CLAUDE_CODE_TMPDIR").map(PathBuf::from).unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                PathBuf::from("/tmp")
            } else {
                std::env::temp_dir()
            }
        });
        if !base.is_absolute() {
            return None;
        }
        let canonical_base = base.canonicalize().ok()?;
        // SAFETY: geteuid takes no arguments.
        let uid = unsafe { libc::geteuid() };
        Some(Self { session, base, canonical_base, uid, running: HashSet::new(), line: String::new(), oversized: false })
    }

    pub(super) fn forward(&mut self, text: &str, emit: &mut dyn FnMut(&str)) {
        for part in text.split_inclusive('\n') {
            let ends = part.ends_with('\n');
            if self.oversized || self.line.len() + part.len() > MAX_FRAME {
                self.flush(emit);
                emit(part);
                self.oversized = !ends;
                continue;
            }
            self.line.push_str(part);
            if ends {
                let mut line = std::mem::take(&mut self.line);
                match self.enrich(&line) {
                    Some(frame) => {
                        emit(&frame);
                        emit("\n");
                    }
                    None => emit(&line),
                }
                line.clear();
                self.line = line;
            }
        }
    }

    pub(super) fn flush(&mut self, emit: &mut dyn FnMut(&str)) {
        if !self.line.is_empty() {
            emit(&self.line);
            self.line.clear();
        }
    }

    fn enrich(&mut self, line: &str) -> Option<String> {
        if !line.contains("\"task_started\"") && !line.contains("\"task_notification\"") {
            return None;
        }
        let mut frame: Value = serde_json::from_str(line).ok()?;
        if frame["type"] != "system" {
            return None;
        }
        let subtype = frame["subtype"].as_str()?;
        let same_session = frame["session_id"].as_str() == Some(&self.session);
        let key = frame["task_id"].as_str().filter(|id| valid_task_id(id)).map(|id| {
            (id.to_string(), frame["run_id"].as_str().map(str::to_string))
        });
        if subtype == "task_started" && same_session && frame["is_backgrounded"] == true && frame["task_type"] == "local_bash" {
            self.running.extend(key);
            return None;
        }
        if subtype != "task_notification" {
            return None;
        }
        let terminal = matches!(frame["status"].as_str(), Some("completed" | "failed" | "stopped"));
        let output = key.filter(|key| same_session && terminal && self.running.remove(key)).and_then(|(task, _)| {
            self.read_output(Path::new(frame["output_file"].as_str()?), &task)
        });
        let fields = frame.as_object_mut()?;
        let replaced = fields.remove("rca_task_stdout").is_some();
        if let Some(output) = output {
            fields.insert("rca_task_stdout".to_string(), output);
        } else if !replaced {
            return None;
        }
        Some(frame.to_string())
    }

    fn read_output(&self, path: &Path, task: &str) -> Option<Value> {
        let relative = path.strip_prefix(&self.canonical_base).or_else(|_| path.strip_prefix(&self.base)).ok()?;
        let names: Vec<_> = relative.components().map(|component| match component {
            Component::Normal(name) => Some(name),
            _ => None,
        }).collect::<Option<_>>()?;
        let [user, cwd, session, tasks, leaf] = names.as_slice() else { return None };
        if *user != OsStr::new(&format!("claude-{}", self.uid)) || *session != OsStr::new(&self.session)
            || *tasks != OsStr::new("tasks") || *leaf != OsStr::new(&format!("{task}.output"))
        {
            return None;
        }
        let mut dir = File::open(&self.canonical_base).ok()?;
        for name in [user, cwd, session, tasks] {
            dir = open_child(&dir, name, libc::O_DIRECTORY)?;
            if dir.metadata().ok()?.uid() != self.uid {
                return None;
            }
        }
        let file = open_child(&dir, leaf, 0)?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() || meta.uid() != self.uid || meta.nlink() != 1 {
            return None;
        }
        let mut bytes = Vec::new();
        file.take((MAX_OUTPUT + 1) as u64).read_to_end(&mut bytes).ok()?;
        let truncated = bytes.len() > MAX_OUTPUT;
        bytes.truncate(MAX_OUTPUT);
        Some(json!({"text": String::from_utf8_lossy(&bytes), "truncated": truncated}))
    }
}

fn valid_task_id(task: &str) -> bool {
    !task.is_empty() && task.len() <= 128 && task.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn open_child(dir: &File, name: &OsStr, flags: libc::c_int) -> Option<File> {
    let name = CString::new(name.as_bytes()).ok()?;
    // SAFETY: the directory fd and C string are live. This call does not create a file.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags | libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK) };
    if fd < 0 {
        return None;
    }
    // SAFETY: a successful openat returns a new fd owned by this caller.
    Some(unsafe { File::from_raw_fd(fd) })
}
