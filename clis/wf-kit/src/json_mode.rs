//! The repo-wide `--json` convention (`runJsonAware` in the TS CLI): stdout goes to stderr while the command runs, then
//! one JSON line is printed on stdout, `{"ok":true}` or `{"ok":false,"error":"..."}`. Only `pnpm --filter mla-tamagui run build:editor-bundle` takes this path
//! now; `wf browser` and `wf android` print their own envelope (`device::cli`).
use serde_json::{json, Map, Value};
use std::cell::{Cell, RefCell};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;

static RESULT: std::sync::Mutex<Option<serde_json::Value>> = std::sync::Mutex::new(None);

/// A command's result value, printed beside `ok: true` under `--json` (ignored otherwise).
pub fn set_result(value: serde_json::Value) {
    if let Ok(mut r) = RESULT.lock() {
        *r = Some(value);
    }
}

/// Runs `f` with stdout sent to stderr, so a command that streams a long job's output still prints its envelope as the one
/// line on stdout.
pub fn stdout_to_stderr<T>(f: impl FnOnce() -> T) -> T {
    let _ = std::io::stdout().flush();
    // SAFETY: plain fd duplication around `f`; stdout is restored before this returns.
    let saved = unsafe { libc::dup(1) };
    unsafe { libc::dup2(2, 1) };
    let out = f();
    let _ = std::io::stdout().flush();
    unsafe {
        libc::dup2(saved, 1);
        libc::close(saved);
    }
    out
}

/// The descriptors of the redirect `begin` set up: the saved stdout and stderr, and the file stderr goes to meanwhile.
struct Redirect {
    saved_out: i32,
    saved_err: i32,
    captured: std::fs::File,
}

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static ACTIVE: RefCell<Option<Redirect>> = const { RefCell::new(None) };
}

/// Called by a command's parse when the line carries `--json`: inside `run`, stdout goes to stderr and stderr is captured
/// from here on, so the envelope is the one thing on stdout. Anywhere else, or a second time, it does nothing.
pub(crate) fn begin() {
    if !ARMED.with(Cell::get) || ACTIVE.with(|a| a.borrow().is_some()) {
        return;
    }
    let Ok(captured) = tempfile::tempfile() else { return };
    // SAFETY: plain fd duplication around the handler; every descriptor is restored before `run` returns.
    let (saved_out, saved_err) = unsafe { (libc::dup(1), libc::dup(2)) };
    unsafe {
        libc::dup2(2, 1);
        libc::dup2(captured.as_raw_fd(), 2);
    }
    ACTIVE.with(|a| *a.borrow_mut() = Some(Redirect { saved_out, saved_err, captured }));
}

/// Runs `f` (the native handler). When its parse met `--json` (`begin`), the output was redirected and the envelope is
/// printed after it, `{"ok":true}` or `{"ok":false,"error":"..."}`; otherwise the handler ran as it would anywhere. Returns
/// the exit code, or None when the handler declined (its output is replayed to the caller untouched).
pub fn run(args: &[String], f: impl FnOnce(&[String]) -> Option<i32>) -> Option<i32> {
    ARMED.with(|a| a.set(true));
    let code = f(args);
    ARMED.with(|a| a.set(false));
    let Some(Redirect { saved_out, saved_err, mut captured }) = ACTIVE.with(|a| a.borrow_mut().take()) else {
        return code;
    };
    let _ = std::io::stdout().flush();
    unsafe {
        libc::dup2(saved_out, 1);
        libc::dup2(saved_err, 2);
    }
    let mut text = String::new();
    let _ = captured.seek(SeekFrom::Start(0));
    let _ = captured.read_to_string(&mut text);
    let code = code?;
    // Progress lines land on stderr, never in the JSON stream.
    eprint!("{text}");
    let result = RESULT.lock().ok().and_then(|mut r| r.take());
    let payload = if code == 0 {
        match result {
            Some(Value::Object(m)) => {
                let mut out = Map::new();
                out.insert("ok".into(), json!(true));
                out.extend(m);
                Value::Object(out)
            }
            Some(other) => json!({ "ok": true, "result": other }),
            None => json!({ "ok": true }),
        }
    } else {
        let error = text.lines().rev().find(|l| !l.trim().is_empty()).map(|l| l.strip_prefix("wf: ").unwrap_or(l).to_string()).unwrap_or_else(|| format!("exit {code}"));
        json!({ "ok": false, "error": error })
    };
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{payload}");
    let _ = out.flush();
    Some(code)
}
