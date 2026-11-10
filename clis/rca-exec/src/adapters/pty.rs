//! The executor's ptys: a shell on a pseudo-terminal opened with `openpty(3)`, its output kept in a bounded ring numbered by byte offset so
//! any number of attaches can read it from any retained offset. Every pty is its own session and process group (killed as one on close);
//! the shell starts with a cleared environment holding only `exec_pty::INHERITED_ENV` of the executor's and the allowed requests.
use exec_proto::{Code, ExecError, ExecResult};
use exec_pty::{PtyAttach, PtyAttached, PtyClose, PtyClosed, PtyEvent, PtyInfo, PtyOpened};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const POLL_MS: i32 = 100;
const WRITE_DEADLINE: Duration = Duration::from_secs(2);
const DRAIN_GRACE: Duration = Duration::from_millis(300);

fn io(e: impl std::fmt::Display) -> ExecError {
    ExecError::new(Code::Io, e.to_string())
}

#[derive(Default)]
struct State {
    buf: VecDeque<u8>,
    /// Offset of `buf[0]`.
    base: u64,
    exit_code: Option<i32>,
    reader_done: bool,
    closed: bool,
}

impl State {
    fn total(&self) -> u64 {
        self.base + self.buf.len() as u64
    }
}

struct Pty {
    pid: u32,
    cwd: String,
    master: File,
    size: Mutex<(u16, u16)>,
    state: Mutex<State>,
    changed: Condvar,
}

/// The executor's open ptys.
#[derive(Default)]
pub struct PtyTable {
    ptys: Mutex<HashMap<String, Arc<Pty>>>,
    attaches: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

pub struct Spawn<'a> {
    pub pty: &'a str,
    pub shell: &'a PathBuf,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    pub env: &'a [(String, String)],
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

fn poll(fd: i32, events: i16) -> bool {
    let mut p = libc::pollfd { fd, events, revents: 0 };
    // SAFETY: one valid pollfd for the length passed; poll only reads and writes it.
    unsafe { libc::poll(&mut p, 1, POLL_MS) > 0 }
}

fn set_flag(fd: i32, get: i32, set: i32, flag: i32) -> std::io::Result<()> {
    // SAFETY: fcntl on a descriptor this function's caller owns.
    unsafe {
        let current = libc::fcntl(fd, get);
        if current < 0 || libc::fcntl(fd, set, current | flag) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

fn signal_group(pid: u32, signal: i32) {
    // SAFETY: a negative pid addresses the process group the shell leads; the call only sends a signal.
    unsafe {
        libc::kill(-(pid as i32), signal);
    }
}

/// Kills everything left in the shell's session: an interactive shell puts each background job in a group of its own, so the group kill misses
/// them. `/proc` names each process's session; where there is none only the group is killed.
fn kill_session(sid: u32) {
    signal_group(sid, libc::SIGKILL);
    let Ok(procs) = std::fs::read_dir("/proc") else { return };
    for entry in procs.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<i32>().ok()) else { continue };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else { continue };
        // After the command name (which may hold spaces and parentheses): state, ppid, pgrp, session.
        let in_session = stat.rsplit_once(')').and_then(|(_, rest)| rest.split_whitespace().nth(3).map(|s| s == sid.to_string())).unwrap_or(false);
        if in_session {
            // SAFETY: the call only sends a signal to a process of the session being closed.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

fn reader(pty: Arc<Pty>) {
    let mut master = &pty.master;
    let mut chunk = [0u8; 8192];
    loop {
        if pty.state.lock().unwrap().closed {
            break;
        }
        if !poll(master.as_raw_fd(), libc::POLLIN) {
            continue;
        }
        match master.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let mut st = pty.state.lock().unwrap();
                st.buf.extend(&chunk[..n]);
                while st.buf.len() > exec_pty::RETAIN {
                    let over = st.buf.len() - exec_pty::RETAIN;
                    st.buf.drain(..over);
                    st.base += over as u64;
                }
                pty.changed.notify_all();
            }
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {}
            Err(_) => break,
        }
    }
    pty.state.lock().unwrap().reader_done = true;
    pty.changed.notify_all();
}

fn waiter(pty: Arc<Pty>, mut child: std::process::Child) {
    let code = child.wait().map(|s| s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0))).unwrap_or(-1);
    let until = Instant::now() + DRAIN_GRACE;
    let mut st = pty.state.lock().unwrap();
    while !st.reader_done && Instant::now() < until {
        st = pty.changed.wait_timeout(st, Duration::from_millis(20)).unwrap().0;
    }
    st.exit_code = Some(code);
    pty.changed.notify_all();
    drop(st);
    // Whatever the shell left running in its session must not outlive it.
    kill_session(pty.pid);
}

impl PtyTable {
    pub fn open(&self, spawn: Spawn) -> ExecResult<PtyOpened> {
        let mut ptys = self.ptys.lock().unwrap();
        if ptys.contains_key(spawn.pty) {
            return Err(ExecError::new(Code::AlreadyExists, format!("pty {} exists", spawn.pty)));
        }
        if ptys.len() >= exec_pty::MAX_PTYS {
            return Err(ExecError::new(Code::Io, format!("{} ptys are open", ptys.len())));
        }
        let (mut master, mut slave) = (-1, -1);
        let size = winsize(spawn.cols, spawn.rows);
        // SAFETY: both out-pointers are valid; the name and termios are null (defaults); the winsize outlives the call.
        let made = unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &size as *const libc::winsize as _) };
        if made != 0 {
            return Err(io(format!("openpty: {}", std::io::Error::last_os_error())));
        }
        // SAFETY: openpty returned two fresh descriptors that nothing else owns.
        let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        set_flag(master.as_raw_fd(), libc::F_GETFD, libc::F_SETFD, libc::FD_CLOEXEC).map_err(io)?;
        set_flag(master.as_raw_fd(), libc::F_GETFL, libc::F_SETFL, libc::O_NONBLOCK).map_err(io)?;
        let mut cmd = Command::new(spawn.shell);
        cmd.arg("-i").current_dir(&spawn.cwd).env_clear();
        for name in exec_pty::INHERITED_ENV {
            if let Some(v) = std::env::var_os(name) {
                cmd.env(name, v);
            }
        }
        cmd.env("TERM", "xterm-256color").envs(spawn.env.iter().map(|(k, v)| (k, v)));
        cmd.stdin(Stdio::from(slave.try_clone().map_err(io)?)).stdout(Stdio::from(slave.try_clone().map_err(io)?)).stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls (setsid, ioctl) between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| io(format!("{}: {e}", spawn.shell.display())))?;
        drop(cmd);
        let pty = Arc::new(Pty { pid: child.id(), cwd: spawn.cwd.to_string_lossy().into_owned(), master: File::from(master), size: Mutex::new((spawn.cols, spawn.rows)), state: Mutex::new(State::default()), changed: Condvar::new() });
        std::thread::spawn({ let p = pty.clone(); move || reader(p) });
        std::thread::spawn({ let p = pty.clone(); move || waiter(p, child) });
        let opened = PtyOpened { pty: spawn.pty.to_string(), pid: pty.pid, cwd: pty.cwd.clone(), cols: spawn.cols, rows: spawn.rows };
        ptys.insert(spawn.pty.to_string(), pty);
        Ok(opened)
    }

    fn get(&self, name: &str) -> ExecResult<Arc<Pty>> {
        self.ptys.lock().unwrap().get(name).cloned().ok_or_else(|| ExecError::new(Code::NotFound, format!("pty {name} does not exist")))
    }

    pub fn write(&self, name: &str, data: &[u8]) -> ExecResult<usize> {
        let pty = self.get(name)?;
        if pty.state.lock().unwrap().exit_code.is_some() {
            return Err(ExecError::new(Code::Io, format!("pty {name} has exited")));
        }
        let (mut master, deadline, mut sent) = (&pty.master, Instant::now() + WRITE_DEADLINE, 0);
        while sent < data.len() {
            match master.write(&data[sent..]) {
                Ok(n) => sent += n,
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted) => {
                    if Instant::now() >= deadline {
                        return Err(io(format!("pty {name} is not reading its input")));
                    }
                    poll(master.as_raw_fd(), libc::POLLOUT);
                }
                Err(e) => return Err(io(e)),
            }
        }
        Ok(sent)
    }

    pub fn resize(&self, name: &str, cols: u16, rows: u16) -> ExecResult<()> {
        let pty = self.get(name)?;
        let size = winsize(cols, rows);
        // SAFETY: TIOCSWINSZ reads one winsize from a valid pointer on the pty master.
        if unsafe { libc::ioctl(pty.master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) } != 0 {
            return Err(io(format!("resize: {}", std::io::Error::last_os_error())));
        }
        *pty.size.lock().unwrap() = (cols, rows);
        Ok(())
    }

    pub fn close(&self, args: &PtyClose) -> ExecResult<PtyClosed> {
        let Some(pty) = self.ptys.lock().unwrap().remove(&args.pty) else {
            return Ok(PtyClosed { closed: false, exit_code: None });
        };
        pty.state.lock().unwrap().closed = true;
        let exited = |p: &Pty| p.state.lock().unwrap().exit_code;
        let wait = |until: Duration| {
            let end = Instant::now() + until;
            let mut st = pty.state.lock().unwrap();
            while st.exit_code.is_none() && Instant::now() < end {
                st = pty.changed.wait_timeout(st, Duration::from_millis(20)).unwrap().0;
            }
        };
        signal_group(pty.pid, libc::SIGHUP);
        wait(Duration::from_millis(300));
        if exited(&pty).is_none() {
            kill_session(pty.pid);
            wait(Duration::from_secs(2));
        }
        kill_session(pty.pid);
        pty.changed.notify_all();
        Ok(PtyClosed { closed: true, exit_code: exited(&pty) })
    }

    pub fn list(&self) -> Vec<PtyInfo> {
        let mut out: Vec<PtyInfo> = self
            .ptys
            .lock()
            .unwrap()
            .iter()
            .map(|(name, p)| {
                let st = p.state.lock().unwrap();
                let (cols, rows) = *p.size.lock().unwrap();
                PtyInfo { pty: name.clone(), pid: p.pid, cwd: p.cwd.clone(), cols, rows, offset: st.total(), base_offset: st.base, exited: st.exit_code.is_some(), exit_code: st.exit_code }
            })
            .collect();
        out.sort_by(|a, b| a.pty.cmp(&b.pty));
        out
    }

    pub fn cancel_attach(&self, id: &str) -> bool {
        self.attaches.lock().unwrap().get(id).map(|flag| flag.store(true, Ordering::SeqCst)).is_some()
    }

    pub fn attach(&self, id: &str, args: &PtyAttach, output: &mut dyn FnMut(u64, &[u8]), events: &mut dyn FnMut(PtyEvent)) -> ExecResult<PtyAttached> {
        let pty = self.get(&args.pty)?;
        let detach = Arc::new(AtomicBool::new(false));
        self.attaches.lock().unwrap().insert(id.to_string(), detach.clone());
        let requested = args.after_offset.unwrap_or(0);
        let mut st = pty.state.lock().unwrap();
        let mut cursor = requested.clamp(st.base, st.total());
        events(PtyEvent::Attached { pty: args.pty.clone(), requested_offset: requested, start_offset: cursor, reset: cursor != requested, offset: st.total() });
        let outcome = loop {
            if detach.load(Ordering::SeqCst) {
                break (st.exit_code, true);
            }
            if cursor < st.base {
                cursor = st.base;
            }
            if cursor < st.total() {
                let from = (cursor - st.base) as usize;
                let take = (st.buf.len() - from).min(exec_pty::FRAME_DATA_CAP);
                let chunk: Vec<u8> = st.buf.range(from..from + take).copied().collect();
                drop(st);
                output(cursor, &chunk);
                cursor += chunk.len() as u64;
                st = pty.state.lock().unwrap();
                continue;
            }
            if st.exit_code.is_some() {
                break (st.exit_code, false);
            }
            st = pty.changed.wait_timeout(st, Duration::from_millis(50)).unwrap().0;
        };
        drop(st);
        self.attaches.lock().unwrap().remove(id);
        Ok(PtyAttached { pty: args.pty.clone(), offset: cursor, exited: outcome.0.is_some(), exit_code: outcome.0, detached: outcome.1 })
    }
}

impl Drop for PtyTable {
    fn drop(&mut self) {
        for pty in self.ptys.lock().unwrap().values() {
            pty.state.lock().unwrap().closed = true;
            kill_session(pty.pid);
        }
    }
}
