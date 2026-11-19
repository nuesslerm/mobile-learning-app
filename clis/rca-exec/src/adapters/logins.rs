//! The agent CLIs' own logins, run at this executor: the CLI's login command started with the executor's home (no workspace), its output kept
//! for `login_status`, a pasted code written to its input. What the CLI writes lands in that home, where its turns read it. Nothing is read
//! of a token: `signed_in` is only whether the credential file is there.
use exec_proc::{LoginProgram, LoginState, CREDENTIAL_FILES, LOGINS_MAX, LOGIN_OUTPUT_MAX};
use exec_proto::{Code, ExecError, ExecResult};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// What a running login has printed: the kept tail, and how many bytes came before it, so an offset stays meaningful as old text is dropped.
#[derive(Default)]
struct Output {
    dropped: u64,
    kept: Vec<u8>,
}

struct Flow {
    program: String,
    child: Child,
    input: Option<ChildStdin>,
    output: Arc<Mutex<Output>>,
}

#[derive(Default)]
pub struct Logins {
    flows: Mutex<BTreeMap<String, Flow>>,
    counter: AtomicU64,
}

fn pump(mut from: impl Read + Send + 'static, into: Arc<Mutex<Output>>) {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = from.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut out = into.lock().unwrap_or_else(|e| e.into_inner());
            out.kept.extend_from_slice(&chunk[..n]);
            if out.kept.len() > LOGIN_OUTPUT_MAX {
                let cut = out.kept.len() - LOGIN_OUTPUT_MAX;
                out.kept.drain(..cut);
                out.dropped += cut as u64;
            }
        }
    });
}

fn io(e: std::io::Error) -> ExecError {
    ExecError::new(Code::Io, e.to_string())
}

fn unknown(flow: &str) -> ExecError {
    ExecError::new(Code::NotFound, format!("no login {flow} is running here"))
}

pub fn signed_in(home: &Path, program: &str) -> bool {
    CREDENTIAL_FILES.iter().find(|(p, _)| *p == program).is_some_and(|(_, file)| home.join(file).is_file())
}

/// The command for `program` with `args`: the CLI found on `search`, the executor's environment, `home` as its home.
fn command(path: &Path, args: &[&str], search: &str, home: &Path) -> Command {
    let mut command = Command::new(path);
    command.args(args).env("PATH", search).env("HOME", home).env("NO_COLOR", "1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    command
}

impl Logins {
    fn locked(&self) -> MutexGuard<'_, BTreeMap<String, Flow>> {
        self.flows.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn start(&self, args: &LoginProgram, path: &Path, search: &str, home: &Path) -> ExecResult<String> {
        let mut flows = self.locked();
        flows.retain(|_, f| f.child.try_wait().map_or(true, |ended| ended.is_none()));
        if flows.len() >= LOGINS_MAX {
            return Err(ExecError::invalid_args(format!("{LOGINS_MAX} logins are running here already")));
        }
        let mut child = command(path, args.login_args(), search, home).spawn().map_err(io)?;
        let output = Arc::new(Mutex::new(Output::default()));
        if let Some(out) = child.stdout.take() {
            pump(out, output.clone());
        }
        if let Some(err) = child.stderr.take() {
            pump(err, output.clone());
        }
        let input = child.stdin.take();
        let id = format!("login-{}", self.counter.fetch_add(1, Ordering::SeqCst) + 1);
        flows.insert(id.clone(), Flow { program: args.program.clone(), child, input, output });
        Ok(id)
    }

    pub fn send(&self, flow: &str, text: &str) -> ExecResult<()> {
        let mut flows = self.locked();
        let running = flows.get_mut(flow).ok_or_else(|| unknown(flow))?;
        if running.child.try_wait().map_err(io)?.is_some() {
            return Err(ExecError::new(Code::NotFound, format!("login {flow} has ended")));
        }
        let input = running.input.as_mut().ok_or_else(|| ExecError::new(Code::Io, "the login takes no input"))?;
        input.write_all(text.as_bytes()).and_then(|()| input.write_all(b"\n")).and_then(|()| input.flush()).map_err(io)
    }

    pub fn status(&self, flow: &str, after: u64, home: &Path) -> ExecResult<LoginState> {
        let mut flows = self.locked();
        let running = flows.get_mut(flow).ok_or_else(|| unknown(flow))?;
        let ended = running.child.try_wait().map_err(io)?;
        // A finished process has closed its pipes, but its last lines may still be on the way from the pump threads
        if ended.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let output = running.output.lock().unwrap_or_else(|e| e.into_inner());
        let from = after.max(output.dropped);
        let skip = (from - output.dropped) as usize;
        let text = String::from_utf8_lossy(output.kept.get(skip..).unwrap_or_default()).into_owned();
        let next = output.dropped + output.kept.len() as u64;
        let ok = ended.map(|s| s.success());
        let error = (ok == Some(false)).then(|| {
            let all = String::from_utf8_lossy(&output.kept);
            let tail: String = all.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
            tail.trim().to_string()
        });
        Ok(LoginState { output: text, next, ended: ended.is_some(), ok, error, signed_in: signed_in(home, &running.program) })
    }

    pub fn cancel(&self, flow: &str) -> bool {
        let Some(mut running) = self.locked().remove(flow) else { return false };
        let pid = running.child.id() as i32;
        // SAFETY: killing the login's own process group, which `process_group(0)` made at its start.
        unsafe { libc::kill(-pid, libc::SIGKILL) };
        let _ = running.child.wait();
        true
    }

    /// The CLI's logout, run to its end (it takes no input), and whether the CLI still holds a login.
    pub fn logout(&self, args: &LoginProgram, path: &Path, search: &str, home: &Path) -> ExecResult<bool> {
        let mut child = command(path, args.logout_args(), search, home).stdin(Stdio::null()).spawn().map_err(io)?;
        let started = std::time::Instant::now();
        loop {
            if child.try_wait().map_err(io)?.is_some() {
                break;
            }
            if started.elapsed() > std::time::Duration::from_secs(30) {
                let pid = child.id() as i32;
                // SAFETY: as in `cancel`.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
                let _ = child.wait();
                return Err(ExecError::new(Code::Io, "the logout did not end in 30 seconds"));
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        // A CLI whose logout leaves its credential file behind has not logged out: the file is what its turns read
        if let Some((_, file)) = CREDENTIAL_FILES.iter().find(|(p, _)| *p == args.program) {
            let _ = std::fs::remove_file(home.join(file));
        }
        Ok(signed_in(home, &args.program))
    }
}

/// The executor's own home, where a CLI keeps its login: the one the executor was given, else the process's.
pub fn home_of(given: Option<&PathBuf>) -> Option<PathBuf> {
    given.cloned().or_else(|| std::env::var_os("HOME").map(PathBuf::from))
}
