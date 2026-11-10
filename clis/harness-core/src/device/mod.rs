//! One script API over the device drivers. A `Device` is what a test script drives (`wf android test`, `wf browser test`): the Android emulator
//! over adb and its UI server, the browser over the browser daemon's `/call`, both in process.
//!
//! The command table (`api.rs`) and its dispatcher (`commands.rs`) hold the policy; the drivers do platform work only.
pub mod api;
pub mod cli;
pub mod commands;
pub mod golden;
pub mod lua;
pub mod sel;

pub use sel::Sel;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl Rect {
    pub fn json(&self) -> Value {
        json!({ "left": self.left, "top": self.top, "right": self.right, "bottom": self.bottom, "cx": (self.left + self.right) / 2, "cy": (self.top + self.bottom) / 2 })
    }
}

/// What a lookup found: its on-screen part in tap units and its text (a field's value, the editor's buffer, else the label).
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub bounds: Rect,
    /// The whole box, on screen or not: with `bounds` it tells how much of the element shows.
    pub full: Rect,
    pub text: Option<String>,
    /// `app`, `editor` or `native`.
    pub frame: &'static str,
}

impl Element {
    pub fn json(&self) -> Value {
        json!({ "bounds": self.bounds.json(), "text": self.text })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Part {
    Screen,
    Keyboard,
    Microphone,
}

impl Part {
    pub const ALL: [Part; 3] = [Part::Screen, Part::Keyboard, Part::Microphone];

    pub fn name(self) -> &'static str {
        match self {
            Part::Screen => "screen",
            Part::Keyboard => "keyboard",
            Part::Microphone => "microphone",
        }
    }

    pub fn parse(s: &str) -> Option<Part> {
        Part::ALL.into_iter().find(|p| p.name() == s)
    }
}

/// Maestro's pressKey names, plus Ctrl+End.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Enter,
    Backspace,
    Tab,
    Left,
    Right,
    Up,
    Down,
    CtrlEnd,
}

impl Key {
    pub const ALL: [Key; 8] = [Key::Enter, Key::Backspace, Key::Tab, Key::Left, Key::Right, Key::Up, Key::Down, Key::CtrlEnd];

    pub fn name(self) -> &'static str {
        match self {
            Key::Enter => "Enter",
            Key::Backspace => "Backspace",
            Key::Tab => "Tab",
            Key::Left => "Remote Dpad Left",
            Key::Right => "Remote Dpad Right",
            Key::Up => "Remote Dpad Up",
            Key::Down => "Remote Dpad Down",
            Key::CtrlEnd => "Ctrl+End",
        }
    }

    pub fn parse(s: &str) -> Result<Key, String> {
        Key::ALL.into_iter().find(|k| k.name().eq_ignore_ascii_case(s.trim())).ok_or_else(|| format!("unknown key '{s}' ({}); letters go through input_text", Key::ALL.map(Key::name).join(", ")))
    }

    /// The key name `android_input::press_keys` takes.
    pub fn short(self) -> &'static str {
        match self {
            Key::Enter => "enter",
            Key::Backspace => "backspace",
            Key::Tab => "tab",
            Key::Left => "left",
            Key::Right => "right",
            Key::Up => "up",
            Key::Down => "down",
            Key::CtrlEnd => "ctrl-end",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "debug" => Some(Level::Debug),
            "info" => Some(Level::Info),
            "warn" => Some(Level::Warn),
            "error" => Some(Level::Error),
            _ => None,
        }
    }
}

/// A logpoint: `file:line[:condition]`, with the expressions each hit captures. Deferred, it waits for a script whose map
/// knows the file (test --logpoint arms before the first page); otherwise an unknown file fails at once.
#[derive(Clone, Debug, PartialEq)]
pub struct Logpoint {
    pub file: String,
    pub line: u32,
    pub condition: Option<String>,
    pub capture: Vec<String>,
    pub defer: bool,
}

/// A PNG's size from its header; (0, 0) when the file is no PNG (a driver's raw capture is kept as it came).
pub fn png_size(path: &str) -> Result<(u32, u32), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let be = |at: usize| bytes.get(at..at + 4).map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    Ok(if bytes.starts_with(b"\x89PNG\r\n\x1a\n") { (be(16), be(20)) } else { (0, 0) })
}

/// What a test drives (docs/plans/harness-api/contract.md): platform work only, no waiting, evidence or option policy.
pub trait Device {
    fn platform(&self) -> &'static str;
    /// Before a command that changes the device: whether this process may drive it now (Android's lease, renewed here).
    fn guard(&self) -> Result<(), String> {
        Ok(())
    }
    fn open_link(&self, link: &str) -> Result<(), String>;
    /// A tap at a point in tap units.
    fn tap_at(&self, x: f64, y: f64) -> Result<(), String>;
    /// A tap inside the element's on-screen part; returns the point tapped.
    fn tap_element(&self, el: &Element, sel: &Sel) -> Result<(f64, f64), String>;
    fn swipe(&self, from: (f64, f64), to: (f64, f64), duration_ms: u64) -> Result<(), String>;
    /// Types at the caret of the focused field (the dispatcher refuses it while the keyboard is down, as press_key).
    fn input_text(&self, text: &str) -> Result<(), String>;
    fn press_key(&self, key: Key) -> Result<(), String>;
    /// The system Back key (Android; the dispatcher refuses it while the keyboard is up).
    fn back(&self) -> Result<(), String>;
    fn draw_strokes_on(&self, el: &Element, sel: &Sel, source: &str, human: bool) -> Result<Value, String>;
    fn set_microphone_input(&self, path: &str) -> Result<Value, String>;
    /// A shell command on the device: its stdout and exit code.
    fn run_shell(&self, cmd: &str) -> Result<(String, i32), String>;
    /// The screen as a PNG at `path`; returns its size in pixels.
    fn take_screenshot(&self, path: &str) -> Result<(u32, u32), String>;
    fn start_recording(&self, path: &str) -> Result<(), String>;
    /// Ends the recording; returns the files written (the first at the path it started with, more beside it when segmented).
    fn stop_recording(&self) -> Result<Vec<String>, String>;
    /// One look: the element when it is on screen, else None.
    fn get_element(&self, sel: &Sel) -> Result<Option<Element>, String>;
    /// A look for `sel` that also looks for `also`, while `sel` shows less than `pct` percent of its box, where the same look
    /// can (the browser: one page evaluation); the second is None when the driver did not look for it.
    fn get_element_with(&self, sel: &Sel, _also: &Sel, _pct: f64) -> Result<(Option<Element>, Option<Option<Element>>), String> {
        Ok((self.get_element(sel)?, None))
    }
    /// Maestro's settle before an action: from `seen` (a look already made), else a first look of its own (None when that
    /// finds nothing), the element looked at every 100 ms until two looks in a row agree on its box, at most `ms`.
    fn settled(&self, sel: &Sel, seen: Option<Element>, ms: u64) -> Result<Option<Element>, String> {
        settle_by_looks(self, sel, seen, ms)
    }
    /// How many elements match, on screen or not (for a failure's evidence).
    fn count_matches(&self, sel: &Sel) -> Result<usize, String>;
    /// What the device holds; only the parts asked for are read.
    fn get_device_state(&self, parts: &[Part]) -> Result<Value, String>;
    /// `get_device_state('keyboard').visible`, for a driver that reads it cheaper alone (Android: without the bounds).
    fn keyboard_visible(&self) -> Result<bool, String> {
        Ok(self.get_device_state(&[Part::Keyboard])?["keyboard"]["visible"] == true)
    }
    /// The app's console lines at `level` or above, `'<level>: <message>'`.
    fn get_logs(&self, level: Level) -> Result<Vec<String>, String>;
    /// Drops the buffered lines; returns how many there were.
    fn clear_logs(&self) -> Result<usize, String>;
    /// The screen's elements, for a failure's evidence and for writing a selector.
    fn hierarchy(&self) -> Result<Value, String>;
    /// Resolves when the selector's visibility equals `want`, or after `ms`; event-driven where the platform can.
    fn wait_visibility(&self, sel: &Sel, want: bool, ms: u64) -> Result<bool, String>;
    /// Between a condition's checks: at most `ms`, ended sooner by a change since the last check began `look_ms` ago where
    /// the driver sees one (the browser: a DOM mutation). False when it cannot watch and waited for nothing, so the caller
    /// pauses instead.
    fn wait_for_change(&self, _ms: u64, _look_ms: u64) -> Result<bool, String> {
        Ok(false)
    }
    /// A visible wait that answers the element too: Some once it shows (holding the element, None if gone by the read), None
    /// when it never did. A driver whose wait looks itself answers with the look that met it.
    fn wait_shown(&self, sel: &Sel, ms: u64) -> Result<Option<Option<Element>>, String> {
        Ok(if self.wait_visibility(sel, true, ms)? { Some(self.get_element(sel)?) } else { None })
    }
    /// `{method, pattern, status, body | json_body | body_file, headers, delay}`: answers matching requests for the rest of
    /// the run; unmatched requests still reach the network.
    fn add_network_stub(&self, spec: &Value) -> Result<Value, String>;
    /// The stubbed requests answered so far: {method, url, status, pattern, at}.
    fn get_network_stub_hits(&self) -> Result<Vec<Value>, String>;
    fn get_debug_hits(&self) -> Result<Vec<Value>, String>;
    /// Arms source-mapped logpoints (test --logpoint): hits are recorded with stack and locals.
    fn arm_logpoints(&self, points: &[Logpoint]) -> Result<(), String>;
    /// How stubbed requests were routed, for a stub that saw no hit (Android: the stub rules with their packet counts).
    fn stub_routing(&self) -> Option<Value> {
        None
    }
    /// A cheap description of the screen for a golden's sidecar (browser: url, title, visible test ids, text); None if costly.
    fn summary(&self) -> Option<Value> {
        None
    }
    /// Releases what the run opened (a browser session) whether or not it was recorded; called once at the end of every run.
    fn shutdown(&self) {}
}

/// The settle as separate looks 100 ms apart: a driver's own, or the browser's where the page cannot run it.
pub fn settle_by_looks<D: Device + ?Sized>(dev: &D, sel: &Sel, seen: Option<Element>, ms: u64) -> Result<Option<Element>, String> {
    let mut el = match seen {
        Some(el) => el,
        None => match dev.get_element(sel)? {
            Some(el) => el,
            None => return Ok(None),
        },
    };
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        crate::run_stop::check()?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        match dev.get_element(sel)? {
            Some(next) if next.bounds == el.bounds && next.full == el.full => return Ok(Some(next)),
            Some(next) => el = next,
            None => break,
        }
    }
    Ok(Some(el))
}

/// This binary, even when a rebuild replaced the file on disk while a run was in progress (`/proc/self/exe` keeps the inode).
pub fn self_exe() -> Result<std::path::PathBuf, String> {
    let proc_exe = std::path::PathBuf::from("/proc/self/exe");
    if proc_exe.exists() {
        return Ok(proc_exe);
    }
    std::env::current_exe().map_err(|e| format!("cannot find the wf binary: {e}"))
}

/// How long a wait may take before it is a failure: 10 s while the suites are being tuned, `WF_WAIT_BUDGET_MS` to tighten
/// it (the target is 2 s: slower than that is a bug or a slow app, and the timeout report says which).
pub fn wait_budget_ms() -> u64 {
    std::env::var("WF_WAIT_BUDGET_MS").ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(10_000)
}

/// The run's speed: a user's gestures and typing at that many times their pace. `WF_TEST_SPEED` (`--speed` sets it), a
/// number from 1 to 4, default 1; a test refuses any other value when it starts, and outside a test it reads as 1.
pub fn test_speed() -> Result<f64, String> {
    match std::env::var("WF_TEST_SPEED") {
        Ok(v) if !v.trim().is_empty() => parse_speed(&v).ok_or_else(|| format!("WF_TEST_SPEED is a number from 1 to 4, got '{v}'")),
        _ => Ok(1.0),
    }
}

pub fn parse_speed(v: &str) -> Option<f64> {
    v.trim().parse::<f64>().ok().filter(|s| (1.0..=4.0).contains(s))
}

pub fn speed() -> f64 {
    test_speed().unwrap_or(1.0)
}

/// A gesture's duration at the run's speed. A drag stays a drag: the browser daemon flings a move shorter than 250 ms.
pub fn paced(ms: u64) -> u64 {
    pace(ms, speed())
}

fn pace(ms: u64, speed: f64) -> u64 {
    if ms < 250 {
        return ms;
    }
    ((ms as f64 / speed).round() as u64).max(250)
}

/// The default timeout of an optional lookup, as Maestro's own shorter one: `WF_OPTIONAL_LOOKUP_MS`, 1 s.
/// How long an action waits for its element to hold still before it acts anyway (`WF_SETTLE_MS`).
pub fn settle_ms() -> u64 {
    std::env::var("WF_SETTLE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000)
}

pub fn optional_lookup_ms() -> u64 {
    std::env::var("WF_OPTIONAL_LOOKUP_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(1_000)
}

/// How much longer a timed-out wait keeps looking, only to tell slow from broken: `WF_WAIT_DIAGNOSE_MS`, 8 s.
pub fn wait_diagnose_ms() -> u64 {
    std::env::var("WF_WAIT_DIAGNOSE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(8_000)
}

/// With `WF_RUN_TIMING=<file>` every browser daemon call becomes a span (`crate::spans`); the Android driver's adb and
/// UI-server calls open their own.
pub fn log_timing(name: &str, took: std::time::Duration) {
    if std::env::var_os("WF_RUN_TIMING").is_none() {
        return;
    }
    // inside a script's device call it is a part of that call; on its own (a CLI verb run by hand) it is the call
    let kind = if crate::spans::current().is_some() { "inner" } else { "call" };
    crate::spans::record(kind, name, took.as_millis() as u64, "ok");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_maestros_key_names_when_parsed_then_case_is_ignored_and_letters_are_refused() {
        assert_eq!((Key::parse("remote dpad left"), Key::parse(" ctrl+end ")), (Ok(Key::Left), Ok(Key::CtrlEnd)));
        assert!(Key::parse("left").unwrap_err().contains("Remote Dpad Left") && Key::parse("a").unwrap_err().contains("input_text"));
        assert_eq!(Key::ALL.map(Key::short), ["enter", "backspace", "tab", "left", "right", "up", "down", "ctrl-end"]);
    }

    #[test]
    fn given_a_run_speed_when_a_gesture_is_paced_then_it_shortens_but_never_below_a_drag() {
        // Given speeds inside and outside 1 to 4, Then only the inside ones parse
        assert_eq!(["1", "2", " 1.5 ", "4"].map(parse_speed), [Some(1.0), Some(2.0), Some(1.5), Some(4.0)]);
        assert_eq!(["9", "abc", "0.5", "NaN", "inf", ""].map(parse_speed), [None; 6]);
        // When durations are paced, Then 600 halves at 2, 400 stops at the 250 ms drag floor, a fling stays as it was
        assert_eq!([pace(600, 2.0), pace(400, 2.0), pace(600, 1.5), pace(600, 4.0), pace(120, 4.0), pace(400, 1.0)], [300, 250, 400, 250, 120, 400]);
    }
}
