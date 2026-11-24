//! Runs the shell of a workflow or action step the way the runner does (`bash -e`), with stubs for what it calls, so a step's behaviour is tested and not its text.
use crate::support::repo_root;
use std::path::PathBuf;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

pub fn read(path: &str) -> String {
    std::fs::read_to_string(repo_root().join(path)).unwrap_or_else(|e| panic!("{path} is readable: {e}"))
}

/// The `run: |` script of the step called `name`, dedented, from the file's text.
pub fn step_script(text: &str, name: &str) -> String {
    let head = text.find(&format!("- name: {name}\n")).unwrap_or_else(|| panic!("no step {name}"));
    let mut lines = text[head..].split_inclusive('\n');
    let step_indent = text[..head].rsplit('\n').next().map_or(0, |l| l.len());
    let first = lines.next().unwrap();
    let _ = first;
    let mut out = String::new();
    let mut indent = None;
    let mut in_run = false;
    for line in lines {
        let lead = line.chars().take_while(|c| *c == ' ').count();
        if !in_run {
            if !line.trim().is_empty() && lead <= step_indent {
                panic!("step {name} has no run block");
            }
            in_run = line.trim_end() == format!("{}run: |", " ".repeat(lead)) && lead > step_indent;
            continue;
        }
        let want = *indent.get_or_insert(lead);
        if !line.trim().is_empty() && lead < want {
            break;
        }
        out.push_str(line.get(want..).unwrap_or("\n"));
    }
    out
}

/// A scratch dir with a `bin` of stubs put first on PATH.
pub struct Scratch {
    pub root: PathBuf,
    pub bin: PathBuf,
}

impl Scratch {
    pub fn new() -> Self {
        let root = tempfile::tempdir().expect("a scratch dir").keep();
        // an unprivileged user must reach everything in it
        std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o777)).unwrap();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let temp = root.join("temp");
        std::fs::create_dir_all(&temp).unwrap();
        std::fs::set_permissions(&temp, std::os::unix::fs::PermissionsExt::from_mode(0o777)).unwrap();
        Self { root, bin }
    }

    pub fn stub(&self, name: &str, body: &str) {
        let path = self.bin.join(name);
        std::fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// Runs `script` under `bash -e` as the runner does for a step with no shell set (as root, or as `nobody` when `as_user` and the test runs as root). The step is the
    /// only holder of the pipes it prints into, as on the runner, and a step still running after 90 s is killed with its process group: no test expects that.
    pub fn run(&self, script: &str, env: &[(&str, &str)], as_user: bool, path_only: bool) -> Output {
        let file = self.root.join("step.sh");
        std::fs::write(&file, script).unwrap();
        std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let path = if path_only { "/usr/bin:/bin".to_string() } else { format!("{}:/usr/bin:/bin", self.bin.display()) };
        let mut cmd = if as_user && running_as_root() {
            let mut c = Command::new("setpriv");
            c.args(["--reuid=65534", "--regid=65534", "--clear-groups", "/bin/bash"]);
            c
        } else {
            Command::new("/bin/bash")
        };
        cmd.arg("-e").arg(&file);
        cmd.env_clear().env("PATH", path).env("RUNNER_TEMP", self.path("temp")).env("HOME", &self.root).envs(env.iter().copied());
        cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
        let mut child = cmd.spawn().expect("bash runs");
        let pgid = child.id();
        let read = |mut pipe: Box<dyn Read + Send>| std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        });
        let out = read(Box::new(child.stdout.take().unwrap()));
        let err = read(Box::new(child.stderr.take().unwrap()));
        let deadline = Instant::now() + Duration::from_secs(90);
        let status = loop {
            if let Some(status) = child.try_wait().expect("the step can be waited for") {
                break status;
            }
            if Instant::now() > deadline {
                let _ = Command::new("kill").args(["-KILL", "--", &format!("-{pgid}")]).status();
                break child.wait().expect("the killed step is reaped");
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        Output { status, stdout: out.join().unwrap(), stderr: err.join().unwrap() }
    }
}

pub fn running_as_root() -> bool {
    Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0").unwrap_or(false)
}
