//! `wf secret login <tool>` by the tool's own link: the login command runs here, the person opens the link it prints on any device,
//! and the credential it leaves (a file in a private directory, or a token it prints) goes to the vault. Nothing reaches the shared
//! desktop's listener. A login that needs a code pasted back (`claude setup-token`) takes it on stdin when run interactively, or from
//! a second `wf secret login <tool> --code <code>` through a private control file.
use crate::auth::{store_files, vault_store};
use crate::vnc::descriptor::{strip_terminal_codes, Descriptor, LinkSpec, LOGIN_DIR_VAR};
use base64::Engine as _;
use serde_json::json;
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(100);
const CODE_PICKUP: Duration = Duration::from_secs(10);
/// How long a command that printed its credential gets to exit by itself before it is stopped.
const EXIT_GRACE: Duration = Duration::from_secs(2);

/// Where the private directories live: `wf-login` under `WF_LOGIN_RUNTIME_DIR`, else `/dev/shm` (memory, never a disk) when there is one.
fn runtime_dir() -> PathBuf {
    let base = std::env::var_os("WF_LOGIN_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| Path::new("/dev/shm").is_dir().then(|| PathBuf::from("/dev/shm")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("wf-login")
}

/// An owner-only directory; one that exists already must be ours, which `set_permissions` proves by failing on anyone else's.
fn private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    file.write_all(bytes)
}

/// The directory one login's credential file lands in, removed when the login ends however it ends.
struct RunDir(PathBuf);

impl RunDir {
    fn create(runtime: &Path) -> std::io::Result<RunDir> {
        private_dir(runtime)?;
        let dir = runtime.join(format!("run-{}", uuid::Uuid::new_v4().simple()));
        private_dir(&dir)?;
        Ok(RunDir(dir))
    }
}

impl Drop for RunDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The control files of a login that waits for a code: a marker holding its pid, and the file the code arrives in.
#[derive(Clone)]
struct Control {
    waiting: PathBuf,
    code: PathBuf,
}

impl Control {
    fn of(runtime: &Path, tool: &str) -> Control {
        Control { waiting: runtime.join(format!("{tool}.waiting")), code: runtime.join(format!("{tool}.code")) }
    }

    fn is_waiting(&self) -> bool {
        std::fs::read_to_string(&self.waiting).ok().and_then(|pid| pid.trim().parse::<u32>().ok()).is_some_and(alive)
    }
}

struct Waiting(Control);

impl Drop for Waiting {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0.waiting);
        let _ = std::fs::remove_file(&self.0.code);
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// Stops the command and everything it started (it runs in its own process group).
fn kill_group(child: &mut Child) {
    let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", child.id())]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    let _ = child.kill();
    let _ = child.wait();
}

fn pump(mut pipe: impl Read + Send + 'static, tx: mpsc::Sender<Vec<u8>>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
}

/// A code is typed into the command's terminal, so it must be one word of printable characters.
fn valid_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 512 && code.chars().all(|c| !c.is_control() && !c.is_whitespace())
}

fn first_group(pattern: &str, text: &str) -> Option<String> {
    regex::Regex::new(pattern).ok()?.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn say(text: &str) {
    print!("{text}");
    let _ = std::io::stdout().flush();
}

fn duration_text(d: Duration) -> String {
    if d.as_secs() >= 120 {
        format!("{} minutes", d.as_secs() / 60)
    } else {
        format!("{:.1} seconds", d.as_secs_f64())
    }
}

/// The command's last words, for a failure that must say what the command said; every secret is blanked.
fn tail(text: &str, secrets: &[&str]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    let keep = lines.len().saturating_sub(8);
    lines.drain(..keep);
    let mut joined = lines.join("\n  ");
    for secret in secrets.iter().filter(|s| !s.is_empty()) {
        joined = joined.replace(secret, "<redacted>");
    }
    joined
}

/// How a link login ended, for the caller that stores what it produced.
enum Ended {
    /// The command printed its credential (`outputSecret`) and was stopped.
    Printed(String),
    /// The command exited 0.
    Exited,
}

struct Run<'a> {
    tool: &'a Descriptor,
    spec: &'a LinkSpec,
    child: Child,
    control: Control,
}

/// `wf secret login <tool>` for a tool with a link spec.
pub(crate) fn login(tool: &Descriptor, spec: &LinkSpec, copy: bool) -> Result<(), i32> {
    let runtime = runtime_dir();
    let control = Control::of(&runtime, &tool.id);
    let run_dir = RunDir::create(&runtime).map_err(|e| {
        eprintln!("{}: cannot make a private directory under {}: {e}", tool.id, runtime.display());
        1
    })?;
    if spec.needs_code && control.is_waiting() {
        eprintln!("{}: another `wf secret login {}` is already waiting for its code; send it with --code, or stop it first", tool.id, tool.id);
        return Err(1);
    }
    let dir = run_dir.0.display().to_string();
    let mut command = Command::new("sh");
    command.args(["-c", &spec.command.replace(LOGIN_DIR_VAR, &dir)]);
    for (key, value) in &spec.env {
        command.env(key, value.replace(LOGIN_DIR_VAR, &dir));
    }
    command.process_group(0).stdin(if spec.needs_code { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        eprintln!("{}: cannot start `{}`: {e}", tool.id, spec.command.replace(LOGIN_DIR_VAR, "<dir>"));
        1
    })?;
    let (tx, rx) = mpsc::channel();
    pump(child.stdout.take().expect("piped"), tx.clone());
    pump(child.stderr.take().expect("piped"), tx);
    let mut run = Run { tool, spec, child, control };
    wf_kit::stop::claim();
    let ended = run.drive(&rx);
    wf_kit::stop::release();
    let ended = ended?;
    drop(run);
    store(tool, spec, ended, &run_dir, copy)
}

impl Run<'_> {
    /// Streams the command's link and code to stdout, hands it the code when it needs one, and returns once it finished or failed.
    fn drive(&mut self, rx: &mpsc::Receiver<Vec<u8>>) -> Result<Ended, i32> {
        let (tool, spec) = (self.tool, self.spec);
        let deadline = Instant::now() + std::env::var("WF_LOGIN_TIMEOUT_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()).map_or(Duration::from_secs(spec.timeout_secs), Duration::from_millis);
        let started = Instant::now();
        let secret = tool.output_secret.as_ref();
        let (mut raw, mut text) = (Vec::new(), String::new());
        let (mut link_shown, mut code_shown, mut instructed) = (false, false, false);
        let mut pasted = String::new();
        let typed = (spec.needs_code && std::io::stdin().is_terminal()).then(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let mut line = String::new();
                while std::io::stdin().read_line(&mut line).is_ok_and(|n| n > 0) {
                    if tx.send(line.trim().to_string()).is_err() {
                        break;
                    }
                    line.clear();
                }
            });
            rx
        });
        let mut exited: Option<Instant> = None;
        let mut guard: Option<Waiting> = None;
        loop {
            let mut fresh = false;
            match rx.recv_timeout(TICK) {
                Ok(bytes) => {
                    raw.extend(bytes);
                    fresh = true;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    let code = match self.child.wait() {
                        Ok(status) => status.code().unwrap_or(-1),
                        Err(_) => -1,
                    };
                    return self.finished(code, &text, &pasted);
                }
            }
            if fresh {
                text = strip_terminal_codes(&String::from_utf8_lossy(&raw));
            }
            if !link_shown {
                if let Some(link) = first_group(&spec.link_pattern, &text) {
                    if spec.needs_code {
                        let _ = private_dir(self.control.waiting.parent().expect("a file in the runtime dir"));
                        let _ = write_private(&self.control.waiting, std::process::id().to_string().as_bytes());
                        guard.get_or_insert_with(|| Waiting(self.control.clone()));
                    }
                    say(&format!("{}: open this link on any device and sign in:\n{link}\n", tool.id));
                    link_shown = true;
                }
            }
            if link_shown && !code_shown {
                if let Some(code) = spec.code_pattern.as_deref().and_then(|p| first_group(p, &text)) {
                    say(&format!("one-time code:\n{code}\n"));
                    code_shown = true;
                }
            }
            if link_shown && !instructed && (spec.code_pattern.is_none() || code_shown) {
                instructed = true;
                if spec.needs_code {
                    say(&format!(
                        "then send the code the page shows:\n  wf secret login {} --code <code>\n{}",
                        tool.id,
                        if typed.is_some() { "or paste it here.\n" } else { "" }
                    ));
                } else {
                    say("Waiting for the login to finish...\n");
                }
            }
            if spec.needs_code && link_shown && pasted.is_empty() {
                let arrived = typed.as_ref().and_then(|t| t.try_recv().ok()).or_else(|| take_code_file(&self.control.code));
                if let Some(code) = arrived {
                    if !valid_code(&code) {
                        eprintln!("{}: that code has spaces or control characters in it; send it exactly as the page shows it", tool.id);
                    } else if self.send_code(&code) {
                        pasted = code;
                        say("code received, finishing the login...\n");
                    } else {
                        eprintln!("{}: the login command no longer takes input", tool.id);
                        kill_group(&mut self.child);
                        return Err(1);
                    }
                }
            }
            if let Some(found) = secret.and_then(|s| s.find(&text)) {
                let until = Instant::now() + EXIT_GRACE;
                while Instant::now() < until && self.child.try_wait().ok().flatten().is_none() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                kill_group(&mut self.child);
                return Ok(Ended::Printed(found));
            }
            if let Some(message) = spec.error_pattern.as_deref().and_then(|p| first_group(p, &text)) {
                kill_group(&mut self.child);
                eprintln!("{}: login failed: {}", tool.id, tail(&message, &[&pasted]));
                return Err(1);
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                let at = *exited.get_or_insert_with(Instant::now);
                if at.elapsed() > Duration::from_millis(500) {
                    return self.finished(status.code().unwrap_or(-1), &text, &pasted);
                }
            }
            if wf_kit::stop::requested() {
                kill_group(&mut self.child);
                eprintln!("{}: login stopped; nothing was stored", tool.id);
                return Err(143);
            }
            if Instant::now() >= deadline {
                kill_group(&mut self.child);
                let waited = duration_text(started.elapsed());
                if spec.needs_code && link_shown && pasted.is_empty() {
                    eprintln!("{}: no code arrived within {waited} -- run the same command again for a fresh link, then send the code with `wf secret login {} --code <code>`", tool.id, tool.id);
                } else {
                    eprintln!("{}: login did not complete within the timeout ({waited}) -- run the same command again for a fresh link", tool.id);
                    let said = tail(&text, &[&pasted]);
                    if !said.is_empty() {
                        eprintln!("the command's last output:\n  {said}");
                    }
                }
                return Err(1);
            }
        }
    }

    /// The command ended on its own: success is exit 0 (the credential is read afterwards), anything else says what it said.
    fn finished(&mut self, code: i32, text: &str, pasted: &str) -> Result<Ended, i32> {
        let secrets: Vec<String> = self.tool.output_secret.as_ref().and_then(|s| s.find(text)).into_iter().chain([pasted.to_string()]).collect();
        let refs: Vec<&str> = secrets.iter().map(String::as_str).collect();
        if code == 0 {
            if let Some(found) = self.tool.output_secret.as_ref().and_then(|s| s.find(text)) {
                return Ok(Ended::Printed(found));
            }
            if self.tool.output_secret.is_some() {
                eprintln!("{}: the login command finished but printed no token\n  {}", self.tool.id, tail(text, &refs));
                return Err(1);
            }
            return Ok(Ended::Exited);
        }
        eprintln!("{}: login failed (exit {code}): {}", self.tool.id, tail(text, &refs));
        Err(1)
    }

    fn send_code(&mut self, code: &str) -> bool {
        let Some(stdin) = self.child.stdin.as_mut() else { return false };
        stdin.write_all(format!("{code}\r").as_bytes()).and_then(|()| stdin.flush()).is_ok()
    }
}

/// The code a second invocation left, taken out of the file so it is used once.
fn take_code_file(path: &Path) -> Option<String> {
    let code = std::fs::read_to_string(path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(code.trim().to_string())
}

/// Stores what the finished login produced: the credential file (read out of the private directory) or the printed token.
fn store(tool: &Descriptor, spec: &LinkSpec, ended: Ended, run_dir: &RunDir, copy: bool) -> Result<(), i32> {
    let (key, value) = match (ended, &spec.credential_file, &tool.vault) {
        (Ended::Printed(token), _, _) => match &tool.output_secret {
            Some(output) => (output.key.clone(), token),
            None => return Err(1),
        },
        (Ended::Exited, Some(file), Some(vault)) => {
            let bytes = match std::fs::read(run_dir.0.join(file)) {
                Ok(bytes) if !bytes.is_empty() => bytes,
                _ => {
                    eprintln!("{}: the login finished but left no {file} to store", tool.id);
                    return Err(1);
                }
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            if copy {
                if let Err(e) = store_files(&json!([{ "path": vault.file, "content": encoded }])) {
                    eprintln!("{}: the credential arrived but could not be stored: {e}", tool.id);
                    return Err(1);
                }
            }
            (vault.key.clone(), encoded)
        }
        (Ended::Exited, _, _) => return Err(1),
    };
    vault_store(&tool.id, &key, &value).map_err(|_| 1)?;
    println!("{}: authenticated ✓ (vault {key}{})", tool.id, if copy { "; credential copied to this machine" } else { "" });
    Ok(())
}

/// `wf secret login <tool> --code <code>`: hands the code to the login waiting for it, and says whether it took it.
pub(crate) fn deliver_code(tool: &Descriptor, spec: &LinkSpec, code: &str) -> Result<(), i32> {
    if !spec.needs_code {
        eprintln!("{}: this login takes no code", tool.id);
        return Err(1);
    }
    if !valid_code(code) {
        eprintln!("{}: the code must be one word without spaces or control characters, exactly as the page shows it", tool.id);
        return Err(1);
    }
    let control = Control::of(&runtime_dir(), &tool.id);
    if !control.is_waiting() {
        eprintln!("{}: no `wf secret login {}` is waiting for a code -- start it first, it prints the link", tool.id, tool.id);
        return Err(1);
    }
    let staged = control.code.with_extension(format!("code.{}", std::process::id()));
    write_private(&staged, code.as_bytes()).and_then(|()| std::fs::rename(&staged, &control.code)).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        eprintln!("{}: could not hand over the code: {e}", tool.id);
        1
    })?;
    let until = Instant::now() + CODE_PICKUP;
    while Instant::now() < until {
        if !control.code.exists() {
            println!("{}: code handed to the waiting login", tool.id);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(&control.code);
    eprintln!("{}: the waiting login did not pick the code up within {}", tool.id, duration_text(CODE_PICKUP));
    Err(1)
}
