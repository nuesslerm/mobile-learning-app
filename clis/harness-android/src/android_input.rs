//! The Android device's work as typed functions: look an element up by the Maestro selector (on the device's UI server, or in
//! a uiautomator dump with `WF_ANDROID_UIA=dump`), tap, swipe, type, press keys, draw strokes, read the keyboard and the JS
//! log, feed the microphone and record the screen. Every call is plain `adb` with the serial given, or the UI server
//! (`android_u2`). The device driver (`device/android.rs`) calls them in process, for the Lua binder and the CLI alike.
use crate::android_u2 as u2;
use harness_core::device::Sel;
use harness_core::trace::{decode_rgba, guide_walk, strokes_from_rgba, Select};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const UI_DUMP: &str = "/sdcard/wf-ui.xml";
const RECORD_PREFIX: &str = "/sdcard/wf-record";
const RECORD_FLAG: &str = "/sdcard/wf-record.on";
const FRAME_SLEEP: &str = "0.016";
const ASK_AGAIN: Duration = Duration::from_millis(100);
const KEY_PAUSE: &str = "0.08";
const EDIT_TEXT: &str = "android.widget.EditText";

const CACHES: &str = "/data/user/0/host.exp.exponent/cache /data/user/0/com.nuesslerm.mla_tamagui/cache";
const CLIP: &str = "/data/local/tmp/clip.wav";
const CLIP_SOURCE: &str = "/data/local/tmp/clip.src";
const MARKER: &str = "/data/local/tmp/inj.marker";

/// The on-device loop that swaps each fresh recording for the prepared clip: the emulator has no microphone.
const INJECTOR: &str = r#"for d in /data/user/0/host.exp.exponent/cache /data/user/0/com.nuesslerm.mla_tamagui/cache; do mkdir -p $d 2>/dev/null; done
touch /data/local/tmp/inj.marker
while true; do
  for d in /data/user/0/host.exp.exponent/cache /data/user/0/com.nuesslerm.mla_tamagui/cache; do
    for f in $(find $d -type f \( -name '*.m4a' -o -name '*.mp4' -o -name '*.3gp' -o -name '*.aac' \) -newer /data/local/tmp/inj.marker 2>/dev/null); do
      if [ ! -e "$f.swapped" ]; then
        cp /data/local/tmp/clip.wav "$f.new"; chmod 666 "$f.new"; chown $(stat -c %u:%g $d) "$f.new"; mv "$f.new" "$f"; touch "$f.swapped"
      fi
    done
  done
  sleep 0.2
done
"#;

/// One node of the screen, as a lookup found it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Node {
    /// The whole box, and its on-screen part, in screen px.
    pub bounds: [i64; 4],
    pub visible: [i64; 4],
    pub text: String,
    pub desc: String,
    pub class: String,
    pub id: String,
    /// The text of the EditText a web view hosts (the code editor's buffer).
    pub inner: Option<String>,
    /// The UI server's (selector, instance) that found it, so a tap lands on that same match.
    pub at: Option<(usize, usize)>,
}

impl Node {
    pub fn shows(&self) -> bool {
        self.visible[2] > self.visible[0] && self.visible[3] > self.visible[1]
    }

    /// Today's text of a node: its text, else its content description.
    pub fn label(&self) -> &str {
        if self.text.is_empty() {
            &self.desc
        } else {
            &self.text
        }
    }

    /// The element's text: a web view's field value, an EditText's value (its text), else the text, else the label.
    pub fn value(&self) -> Option<String> {
        match &self.inner {
            Some(t) => Some(t.clone()),
            _ if self.class == EDIT_TEXT => Some(self.text.clone()),
            _ => Some(self.label().to_string()).filter(|t| !t.is_empty()),
        }
    }

    fn matches(&self, sel: &Sel) -> bool {
        sel.id.as_ref().is_none_or(|p| p.is_id_match(&self.id)) && sel.text.as_ref().is_none_or(|p| p.is_match(&self.text) || p.is_match(&self.desc))
    }
}

/// The deepest match, as the browser picks it: of the matches in tree order, the first with no other match inside it (a
/// node's subtree follows it, so only later matches can be inside it).
pub(crate) fn deepest<T>(found: Vec<T>, inside: impl Fn(&T, &T) -> bool) -> Option<T> {
    let i = (0..found.len()).find(|&i| !found[i + 1..].iter().any(|n| inside(n, &found[i])))?;
    found.into_iter().nth(i)
}

fn adb_name(tail: &[&str]) -> String {
    match tail {
        ["shell", cmd, ..] => format!("adb shell {}", cmd.split_whitespace().next().unwrap_or("")),
        [first, ..] => format!("adb {first}"),
        [] => "adb".to_string(),
    }
}

struct Ran {
    stdout: Vec<u8>,
    stderr: String,
    code: i32,
}

/// One adb call as an inner span: inside a device command it nests under that command's span.
fn adb_raw(serial: &str, tail: &[&str]) -> Result<Ran, String> {
    let mut span = harness_core::spans::begin("inner", &adb_name(tail), None);
    let o = Command::new("adb").arg("-s").arg(serial).args(tail).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).output();
    let ran = o.map(|o| Ran { stdout: o.stdout, stderr: String::from_utf8_lossy(&o.stderr).trim().to_string(), code: o.status.code().unwrap_or(1) }).map_err(|e| format!("cannot run adb: {e}"));
    if !ran.as_ref().is_ok_and(|r| r.code == 0) {
        span.outcome = "error";
    }
    ran
}

fn adb_output(serial: &str, tail: &[&str]) -> Result<Vec<u8>, String> {
    let r = adb_raw(serial, tail)?;
    if r.code == 0 {
        return Ok(r.stdout);
    }
    let shown = tail.join(" ");
    let why = if r.stderr.is_empty() { String::from_utf8_lossy(&r.stdout).trim().to_string() } else { r.stderr };
    Err(format!("adb {:.80} failed (exit {}): {why:.300}", shown, r.code))
}

/// The call's stdout; a non-zero exit is an error naming it.
pub(crate) fn adb_text(serial: &str, tail: &[&str]) -> Result<String, String> {
    adb_output(serial, tail).map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub(crate) fn adb_run(serial: &str, tail: &[&str]) -> Result<(), String> {
    adb_output(serial, tail).map(|_| ())
}

/// A shell command's stdout and exit code: a non-zero exit is an answer; only adb's own failure is an error.
pub(crate) fn adb_shell(serial: &str, cmd: &str) -> Result<(String, i32), String> {
    let r = adb_raw(serial, &["shell", cmd])?;
    if r.code != 0 && r.stdout.is_empty() && (r.stderr.starts_with("error:") || r.stderr.starts_with("adb: ")) {
        return Err(format!("adb shell: {}", r.stderr));
    }
    Ok((String::from_utf8_lossy(&r.stdout).into_owned(), r.code))
}

/// A word the device's shell reads as itself: quoted only when it holds more than `[A-Za-z0-9_@%+=:,./-]`.
pub(crate) fn sh_word(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&#10;", "\n").replace("&amp;", "&")
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(unescape(&tag[start..end]))
}

fn parse_bounds(s: &str) -> Option<[i64; 4]> {
    let nums: Vec<i64> = s.split(|c: char| !c.is_ascii_digit() && c != '-').filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect();
    <[i64; 4]>::try_from(nums).ok()
}

/// The nodes of a uiautomator dump in tree order, each with the index just past its subtree.
fn parse_dump(xml: &str) -> Vec<(Node, usize)> {
    let mut out: Vec<(Node, usize)> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find('<') {
        rest = &rest[at..];
        if let Some(tail) = rest.strip_prefix("</node>") {
            if let Some(i) = open.pop() {
                out[i].1 = out.len();
            }
            rest = tail;
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        let Some(body) = tag.strip_prefix("<node ") else { continue };
        let tag = format!(" {body}");
        let s = |k: &str| attr(&tag, k).unwrap_or_default();
        let bounds = parse_bounds(&s("bounds")).unwrap_or_default();
        out.push((Node { bounds, visible: bounds, text: s("text"), desc: s("content-desc"), class: s("class"), id: s("resource-id"), inner: None, at: None }, 0));
        let i = out.len() - 1;
        if body.ends_with('/') {
            out[i].1 = out.len();
        } else {
            open.push(i);
        }
    }
    for i in open {
        out[i].1 = out.len();
    }
    out
}

/// The element a selector names in a dump: of the visible matches in tree order, Maestro's deepest.
fn find_in(xml: &str, sel: &Sel) -> Option<Node> {
    let nodes = parse_dump(xml);
    let found: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].0.shows() && nodes[i].0.matches(sel)).collect();
    let i = deepest(found, |&a, &b| b < a && a < nodes[b].1)?;
    let mut n = nodes[i].0.clone();
    if n.class.contains("WebView") {
        n.inner = nodes[i + 1..nodes[i].1].iter().find(|(c, _)| c.class == EDIT_TEXT).map(|(c, _)| c.text.clone());
    }
    Some(n)
}

fn dump_ui(serial: &str) -> Result<String, String> {
    adb_output(serial, &["shell", "uiautomator", "dump", UI_DUMP])?;
    let bytes = adb_output(serial, &["exec-out", "cat", UI_DUMP])?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn u2_or_dump<T>(serial: &str, server: impl FnOnce() -> Result<T, String>, dump: impl FnOnce(&str) -> T) -> Result<T, String> {
    if u2::enabled() {
        match server() {
            Ok(v) => return Ok(v),
            Err(e) => wf_kit::wf_err!("wf: the on-device UI server is not usable ({e}); falling back to uiautomator dump"),
        }
    }
    Ok(dump(&dump_ui(serial)?))
}

/// The value of the first EditText drawn inside `b`: on a device the editor's testID sits on a FrameLayout around the web
/// view, and the field is not its child in the tree.
pub(crate) fn field_inside(serial: &str, b: [i64; 4]) -> Result<Option<String>, String> {
    u2_or_dump(serial, || u2::field_inside(serial, b), |xml| parse_dump(xml).into_iter().map(|(n, _)| n).find(|n| n.class == EDIT_TEXT && centre_in(&n.bounds, &b)).map(|n| n.text))
}

pub(crate) fn centre_in(a: &[i64; 4], b: &[i64; 4]) -> bool {
    let (x, y) = ((a[0] + a[2]) / 2, (a[1] + a[3]) / 2);
    x >= b[0] && x < b[2] && y >= b[1] && y < b[3]
}

/// The element a selector names, a UI server's miss checked against a dead server.
pub(crate) fn locate(serial: &str, sel: &Sel) -> Result<Option<Node>, String> {
    u2_or_dump(serial, || u2::locate(serial, sel), |xml| find_in(xml, sel))
}

/// One look, for a poll that checks for a dead server only where a miss decides it.
fn look(serial: &str, sel: &Sel) -> Result<Option<Node>, String> {
    u2_or_dump(serial, || u2::look(serial, sel), |xml| find_in(xml, sel))
}

/// How many nodes match, visible or not.
pub(crate) fn count(serial: &str, sel: &Sel) -> Result<usize, String> {
    u2_or_dump(serial, || u2::count(serial, sel), |xml| parse_dump(xml).iter().filter(|(n, _)| n.matches(sel)).count())
}

/// The screen's elements in tree order, those with an id, a text, a label or a field (as the browser's): for evidence and
/// for writing a selector. A UI server is never swapped for a dump here: the dump would take its automation connection.
pub(crate) fn hierarchy(serial: &str) -> Result<Value, String> {
    let xml = if u2::enabled() { u2::hierarchy(serial)? } else { dump_ui(serial)? };
    let elements: Vec<Value> = parse_dump(&xml)
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| !n.id.is_empty() || !n.text.is_empty() || !n.desc.is_empty() || n.class == EDIT_TEXT)
        .take(500)
        .map(|n| {
            let [left, top, right, bottom] = n.visible;
            json!({ "id": Some(n.id).filter(|v| !v.is_empty()), "label": Some(n.desc).filter(|v| !v.is_empty()), "text": n.text.chars().take(80).collect::<String>(), "class": n.class, "bounds": { "left": left, "top": top, "right": right, "bottom": bottom }, "frame": "native" })
        })
        .collect();
    Ok(json!({ "platform": "android", "elements": elements }))
}

/// Whether the element's visibility becomes `want` within `timeout`, asking again 100 ms after each answer: the look that met
/// it (the node, when it was to show), else None. A miss that decides the wait (it is gone, or it never showed) is first
/// checked against a dead UI server, which misses everything.
pub(crate) fn wait_visible(serial: &str, sel: &Sel, want: bool, timeout: Duration) -> Result<Option<Option<Node>>, String> {
    let dead = || if u2::enabled() { u2::revived(serial) } else { Ok(false) };
    let end = Instant::now() + timeout;
    loop {
        harness_core::run_stop::check()?;
        let seen = look(serial, sel)?;
        if seen.is_some() == want && (want || !dead()?) {
            return Ok(Some(seen));
        }
        if Instant::now() >= end {
            return Ok(if want && dead()? { look(serial, sel)?.map(Some) } else { None });
        }
        std::thread::sleep(ASK_AGAIN);
    }
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// One JSON line per input in `--log <file>` (or `WF_ANDROID_INPUT_LOG`): the evidence that a run used input only.
pub(crate) fn log_input(path: Option<&str>, verb: &str, args: &[String]) {
    let Some(path) = path.map(str::to_string).or_else(|| std::env::var("WF_ANDROID_INPUT_LOG").ok()).filter(|p| !p.is_empty()) else { return };
    let line = json!({ "at": now_ms(), "verb": verb, "args": args }).to_string();
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => {
            let _ = writeln!(f, "{line}");
        }
        Err(e) => wf_kit::wf_err!("wf: cannot write the input log {path}: {e}"),
    }
}

fn coord(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// A VIEW intent for the link: Expo Go opens exp:// links and the app's own.
pub(crate) fn open_link(serial: &str, link: &str) -> Result<(), String> {
    adb_run(serial, &["shell", "am", "start", "-a", "android.intent.action.VIEW", "-d", &sh_word(link)])
}

/// A tap at a point: the UI server's click, else `input tap`.
pub(crate) fn tap_point(serial: &str, x: f64, y: f64, log: Option<&str>) -> Result<(), String> {
    let (cx, cy) = (coord(x), coord(y));
    log_input(log, "tap", &[cx.clone(), cy.clone()]);
    if u2::enabled() && u2::click(serial, x, y).is_ok() {
        return Ok(());
    }
    adb_run(serial, &["shell", "input", "tap", &cx, &cy])
}

/// A tap on the element `sel` names: the UI server finds and clicks it in one call (the match `at` when a lookup gave one),
/// else `input tap` at `point`.
pub(crate) fn tap_node(serial: &str, sel: &Sel, at: Option<(usize, usize)>, point: (f64, f64), log: Option<&str>) -> Result<(), String> {
    let (x, y) = (coord(point.0), coord(point.1));
    log_input(log, "tap", &[x.clone(), y.clone()]);
    if u2::enabled() && u2::click_target(serial, sel, at) == Ok(true) {
        return Ok(());
    }
    adb_run(serial, &["shell", "input", "tap", &x, &y])
}

/// One finger from `from` to `to`: the UI server's swipe, else `input swipe` (with the duration when one is given).
pub(crate) fn swipe(serial: &str, from: (f64, f64), to: (f64, f64), duration_ms: Option<u64>, log: Option<&str>) -> Result<(), String> {
    let words = [coord(from.0), coord(from.1), coord(to.0), coord(to.1)];
    log_input(log, "swipe", &words);
    if u2::enabled() && u2::swipe(serial, from, to, duration_ms.unwrap_or(300)).is_ok() {
        return Ok(());
    }
    let ms = duration_ms.map(|d| d.to_string());
    let mut tail = vec!["shell", "input", "swipe", &words[0], &words[1], &words[2], &words[3]];
    tail.extend(ms.as_deref());
    adb_run(serial, &tail)
}

/// The screen as PNG bytes.
pub(crate) fn screenshot(serial: &str) -> Result<Vec<u8>, String> {
    adb_output(serial, &["exec-out", "screencap", "-p"])
}

/// The screen size in px (`wm size`, the override when there is one).
pub(crate) fn screen_size(serial: &str) -> Result<(u64, u64), String> {
    let size = adb_text(serial, &["shell", "wm size"])?;
    let wh = size.lines().rev().find_map(|l| l.split_once("size:").map(|(_, v)| v.trim().to_string())).ok_or_else(|| format!("wm size: unexpected output: {}", size.trim()))?;
    wh.split_once('x').and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?))).ok_or_else(|| format!("wm size: unexpected size {wh}"))
}

/// `per_segment` evenly spaced samples along every segment of the polyline, ending exactly on its last point.
fn straight_samples(points: &[(f64, f64)], per_segment: usize) -> Vec<(f64, f64)> {
    let mut out = vec![points[0]];
    for w in points.windows(2) {
        for i in 1..=per_segment {
            let t = i as f64 / per_segment as f64;
            out.push((w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
        }
    }
    out
}

/// One `adb shell` script: press at the first sample, move through the rest a frame apart, lift at the last.
pub(crate) fn stroke_script(samples: &[(f64, f64)]) -> String {
    let at = |(x, y): (f64, f64)| format!("{} {}", coord(x), coord(y));
    let mut parts = vec![format!("input motionevent DOWN {}", at(samples[0]))];
    for s in &samples[1..samples.len() - 1] {
        parts.push(format!("input motionevent MOVE {}", at(*s)));
        parts.push(format!("sleep {FRAME_SLEEP}"));
    }
    parts.push(format!("input motionevent UP {}", at(samples[samples.len() - 1])));
    parts.join("; ")
}

/// Touch down, move through `samples`, lift: on the device's UI server when it answers, else one `adb shell` script.
fn draw(serial: &str, samples: &[(f64, f64)]) -> Result<(), String> {
    if u2::enabled() && u2::swipe_points(serial, samples).is_ok() {
        return Ok(());
    }
    adb_run(serial, &["shell", &stroke_script(samples)])
}

/// The strokes a drawing pad shows inside `area` (screen px), read off a screenshot. The lesson pad marks one active stroke
/// with a start dot and an arrow: that one is followed. Without a dot, every saturated line is read.
pub(crate) fn read_strokes(serial: &str, area: [i64; 4], select: Select, what: &str) -> Result<Vec<Vec<(f64, f64)>>, String> {
    let (rgba, width, height) = decode_rgba(&screenshot(serial)?).map_err(|e| format!("cannot read the screenshot: {e}"))?;
    let (x0, y0) = (area[0].clamp(0, width as i64) as usize, area[1].clamp(0, height as i64) as usize);
    let (x1, y1) = (area[2].clamp(0, width as i64) as usize, area[3].clamp(0, height as i64) as usize);
    if x1 <= x0 || y1 <= y0 {
        return Err(format!("{what} is not on the screen"));
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut crop = Vec::with_capacity(cw * ch * 4);
    for y in y0..y1 {
        crop.extend_from_slice(&rgba[(y * width + x0) * 4..(y * width + x1) * 4]);
    }
    let read = if select == Select::Guide { guide_walk(&crop, cw, ch).map(|path| vec![path]) } else { None };
    let read = read.unwrap_or_else(|| strokes_from_rgba(&crop, cw, ch, select));
    let strokes: Vec<Vec<(f64, f64)>> = read.into_iter().map(|s| s.into_iter().map(|(x, y)| (x + x0 as f64, y + y0 as f64)).collect()).collect();
    if strokes.is_empty() {
        return Err(format!("no strokes read off {what}"));
    }
    Ok(strokes)
}

/// Draws each stroke by touch, a pause apart, as a hand follows a guide (a one-point stroke is a tap); returns the points.
pub(crate) fn draw_strokes(serial: &str, strokes: &[Vec<(f64, f64)>], human: bool, log: Option<&str>) -> Result<usize, String> {
    let mut points = 0;
    for stroke in strokes {
        points += stroke.len();
        log_input(log, "stroke", &stroke.iter().map(|(x, y)| format!("{},{}", coord(*x), coord(*y))).collect::<Vec<_>>());
        if stroke.len() < 2 {
            adb_run(serial, &["shell", "input", "tap", &coord(stroke[0].0), &coord(stroke[0].1)])?;
        } else {
            let samples = if human { cdp::attached::human_stroke_samples(stroke)? } else { straight_samples(stroke, 2) };
            draw(serial, &samples)?;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    Ok(points)
}

/// Starts recording the screen. screenrecord stops by itself at its time limit, so a long run records back-to-back segments
/// from a device-side loop. `note` (the local path the stop pulls to) is kept in the loop's flag file.
pub(crate) fn record_start(serial: &str, note: Option<&str>) -> Result<(), String> {
    let flag = note.map_or_else(|| format!("touch {RECORD_FLAG}"), |n| format!("printf '%s' {} > {RECORD_FLAG}", sh_word(n)));
    adb_run(serial, &["shell", &format!("rm -f {RECORD_PREFIX}-*.mp4; {flag}; nohup sh -c 'i=0; while [ -e {RECORD_FLAG} ]; do i=$((i+1)); screenrecord --time-limit 170 {RECORD_PREFIX}-$i.mp4; done' >/dev/null 2>&1 &")])
}

/// Whether a recording runs, and the note it started with.
pub(crate) fn recording(serial: &str) -> Result<Option<String>, String> {
    let out = adb_text(serial, &["shell", &format!("[ -e {RECORD_FLAG} ] && echo \"wf-recording=$(cat {RECORD_FLAG})\"; true")])?;
    Ok(out.lines().find_map(|l| l.strip_prefix("wf-recording=")).map(|n| n.trim().to_string()))
}

/// Stops the recording and pulls its segments in order, the first to `out` and the rest beside it; none when nothing recorded.
pub(crate) fn record_stop(serial: &str, out: &str) -> Result<Vec<String>, String> {
    // screenrecord writes the mp4 trailer on SIGINT, then exits: the pull waits for that exit, at most 5 s (an image without
    // pidof waits the old 2 s). A loop already gone (between segments, or killed) is not a failure.
    adb_run(serial, &["shell", &format!("rm -f {RECORD_FLAG}; pkill -INT screenrecord; command -v pidof >/dev/null || sleep 2; i=0; while pidof screenrecord >/dev/null && [ $i -lt 50 ]; do sleep 0.1; i=$((i+1)); done; true")])?;
    let listing = adb_text(serial, &["shell", "ls", &format!("{RECORD_PREFIX}-*.mp4")]).unwrap_or_default();
    let mut segments: Vec<(u32, String)> = listing
        .lines()
        .map(str::trim)
        .filter_map(|l| {
            let n = l.strip_prefix(RECORD_PREFIX)?.strip_prefix('-')?.strip_suffix(".mp4")?.parse().ok()?;
            Some((n, l.to_string()))
        })
        .collect();
    segments.sort();
    let base = out.strip_suffix(".mp4").unwrap_or(out);
    let mut files = Vec::new();
    for (i, (_, remote)) in segments.iter().enumerate() {
        let local = if i == 0 { out.to_string() } else { format!("{base}-{}.mp4", i + 1) };
        adb_run(serial, &["pull", remote, &local])?;
        files.push(local);
    }
    Ok(files)
}

pub(crate) fn keyboard_shown(serial: &str) -> Result<bool, String> {
    Ok(adb_text(serial, &["shell", "dumpsys", "input_method"])?.contains("mInputShown=true"))
}

/// Where the soft keyboard is drawn: the IME window's frame (`dumpsys window <name>` dumps only the windows so named).
pub(crate) fn keyboard_bounds(serial: &str) -> Result<Option<Value>, String> {
    Ok(ime_frame(&adb_text(serial, &["shell", "dumpsys window"])?))
}

/// Where the keyboard is drawn: the IME's inset source, `type=ime frame=[l,t][r,b]`. Its window is taller (on the CI device
/// it spans the screen from the status bar down), so the window's own frame is only the fallback.
fn ime_frame(dump: &str) -> Option<Value> {
    if let Some(r) = dump.lines().find_map(|l| l.find("type=ime frame=[").and_then(|at| rect_after(&l[at..]))) {
        return Some(r);
    }
    let mut in_ime = false;
    for line in dump.lines() {
        if line.contains("InputMethod") || line.contains("type=ime") || line.contains("ty=INPUT_METHOD") {
            in_ime = true;
        }
        let at = line.find("frame=[").or_else(|| line.find("mFrame=["));
        if let (true, Some(at)) = (in_ime, at) {
            if let Some(r) = rect_after(&line[at..]) {
                return Some(r);
            }
        }
    }
    None
}

fn rect_after(text: &str) -> Option<Value> {
    let nums: Vec<i64> = text.split(|c: char| !c.is_ascii_digit() && c != '-').filter_map(|t| t.parse().ok()).take(4).collect();
    match nums[..] {
        [l, t, r, b] => Some(json!({ "left": l, "top": t, "right": r, "bottom": b })),
        _ => None,
    }
}

/// The JS runtime's log lines (`ReactNativeJS`) at priority `min` (V, D, I, W or E) or above, as logcat prints them. The read
/// is bounded: a bare `logcat -d` never returns on the emulator.
pub(crate) fn js_log(serial: &str, min: char) -> Result<Vec<String>, String> {
    let filter = format!("ReactNativeJS:{min}");
    let out = adb_text(serial, &["logcat", "-d", "-t", "5000", "-v", "brief", &filter, "*:S"])?;
    let levels = &"VDIWEF"["VDIWEF".find(min).unwrap_or(0)..];
    Ok(out.lines().filter(|l| l.split_once('/').is_some_and(|(p, rest)| p.len() == 1 && levels.contains(p) && rest.starts_with("ReactNativeJS"))).map(str::to_string).collect())
}

/// Empties the device's log buffers; returns how many JS lines they held.
pub(crate) fn js_log_clear(serial: &str) -> Result<usize, String> {
    let out = adb_text(serial, &["shell", "n=$(logcat -d -t 5000 -v brief 'ReactNativeJS:V' '*:S' | grep -c /ReactNativeJS); logcat -c; echo \"wf-cleared=$n\""])?;
    Ok(out.lines().find_map(|l| l.strip_prefix("wf-cleared=")).and_then(|n| n.trim().parse().ok()).unwrap_or(0))
}

fn is_root(serial: &str) -> bool {
    adb_text(serial, &["shell", "id -u"]).is_ok_and(|o| o.trim() == "0")
}

/// adbd as root: the injector swaps files in another app's cache, which only root can do, and a fresh adb session starts as
/// the shell user. A TCP serial drops when adbd restarts as root, so this reconnects until it answers as root.
pub(crate) fn ensure_root(serial: &str) {
    if is_root(serial) {
        return;
    }
    let _ = adb_run(serial, &["root"]);
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(500));
        if serial.contains(':') {
            let _ = adb_run(serial, &["connect", serial]);
        }
        if is_root(serial) {
            return;
        }
    }
}

/// Sets the clip the app's microphone hears at its next recording, over the one set before: pushed with the injector loop,
/// which swaps it into each fresh recording file. Refused while a recording runs as the shell user: going root kills it.
pub(crate) fn set_microphone(serial: &str, wav: &str) -> Result<(), String> {
    if !is_root(serial) && recording(serial)?.is_some() {
        return Err("refused: a screen recording runs as the shell user, and switching adbd to root for the clip would end it -- stop it, or start it with start-recording, which goes root first".into());
    }
    ensure_root(serial);
    let script = std::env::temp_dir().join(format!("wf-inject-{}.sh", std::process::id()));
    std::fs::write(&script, format!("printf '%s' {} > {CLIP_SOURCE}\n{INJECTOR}", sh_word(wav))).map_err(|e| format!("cannot write the injector: {e}"))?;
    let pushed = adb_run(serial, &["push", wav, CLIP]).and_then(|()| adb_run(serial, &["push", &script.display().to_string(), "/data/local/tmp/wf-inject.sh"]));
    let _ = std::fs::remove_file(&script);
    pushed?;
    // two calls: the pattern in a command line that also names the script would match (and kill) its own shell
    let _ = adb_run(serial, &["shell", "pkill -f 'wf-injec[t].sh'"]);
    adb_run(serial, &["shell", "nohup sh /data/local/tmp/wf-inject.sh >/dev/null 2>&1 &"])
}

/// The clip set last and whether a recorder took it: the injector marks each recording it swapped with a `.swapped` file
/// newer than its start marker.
pub(crate) fn microphone(serial: &str) -> Result<Value, String> {
    let out = adb_text(serial, &["shell", &format!("echo \"wf-input=$(cat {CLIP_SOURCE} 2>/dev/null)\"; find {CACHES} -name '*.swapped' -newer {MARKER} 2>/dev/null; true")])?;
    let input = out.lines().find_map(|l| l.strip_prefix("wf-input=")).map(str::trim).filter(|v| !v.is_empty());
    Ok(json!({ "input": input, "consumed": out.lines().any(|l| l.trim().ends_with(".swapped")) }))
}

fn key_script(keys: &[String]) -> String {
    keys.join(&format!("; sleep {KEY_PAUSE}; "))
}

/// Sends the text, a run of printable characters per `input` process (each process is a JVM start, so a key per process
/// costs seconds a line); newline, tab and quote go as keys. The dispatcher's input_text refuses it while the keyboard is
/// down, so a run cannot pass with the keys going nowhere visible.
pub(crate) fn type_text(serial: &str, text: &str, log: Option<&str>) -> Result<(), String> {
    let mut keys: Vec<String> = Vec::new();
    let mut run = String::new();
    let flush = |run: &mut String, keys: &mut Vec<String>| {
        if !run.is_empty() {
            keys.push(format!("input text '{run}'"));
            run.clear();
        }
    };
    for c in text.chars() {
        match c {
            ' ' => run.push_str("%s"),
            '\n' | '\t' | '\'' => {
                flush(&mut run, &mut keys);
                keys.push(format!("input keyevent {}", match c { '\n' => 66, '\t' => 61, _ => 75 }));
            }
            '%' | '\\' => return Err(format!("cannot send {c:?} through input text; tap the syntax toolbar or the keyboard's own key for it")),
            c if c.is_ascii_graphic() => run.push(c),
            c => return Err(format!("only ASCII can be typed, not {c:?}")),
        }
    }
    flush(&mut run, &mut keys);
    log_input(log, "type", &[text.to_string()]);
    adb_run(serial, &["shell", &key_script(&keys)])
}

fn keycode(name: &str) -> Option<u32> {
    Some(match name {
        "back" => 4,
        "enter" => 66,
        "backspace" => 67,
        "delete" => 112,
        "tab" => 61,
        "space" => 62,
        "escape" => 111,
        "left" => 21,
        "up" => 19,
        "right" => 22,
        "down" => 20,
        "home" => 122,
        "end" => 123,
        _ => return None,
    })
}

/// Single keys by name (back, enter, backspace, delete, tab, space, escape, the arrows, home, end), paced.
pub(crate) fn press_keys(serial: &str, names: &[String], log: Option<&str>) -> Result<(), String> {
    let codes: Vec<u32> = names.iter().map(|n| keycode(n).ok_or_else(|| format!("unknown key {n:?}"))).collect::<Result<_, _>>()?;
    log_input(log, "press", names);
    if u2::enabled() {
        let pause = Duration::from_secs_f64(KEY_PAUSE.parse().unwrap_or(0.08));
        // a key that was sent before a failure would be sent twice by the fallback, so it stops here
        return codes.iter().enumerate().try_for_each(|(i, code)| {
            if i > 0 {
                std::thread::sleep(pause);
            }
            u2::press_key(serial, *code)
        });
    }
    let keys: Vec<String> = codes.iter().map(|c| format!("input keyevent {c}")).collect();
    adb_run(serial, &["shell", &key_script(&keys)])
}

/// Keys held together, by keycode (Ctrl+End is 113 123).
pub(crate) fn press_combination(serial: &str, codes: &[u32], name: &str, log: Option<&str>) -> Result<(), String> {
    log_input(log, "press", &[name.to_string()]);
    let codes: Vec<String> = codes.iter().map(u32::to_string).collect();
    adb_run(serial, &["shell", &format!("input keycombination {}", codes.join(" "))])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NESTED: &str = r#"<hierarchy rotation="0"><node index="0" text="" resource-id="Card" content-desc="Two Sum, Easy" class="android.view.ViewGroup" bounds="[0,100][400,300]"><node index="0" text="Two Sum" resource-id="" class="android.widget.TextView" bounds="[10,110][200,150]" /><node index="1" text="" resource-id="CodeEditorInput" class="android.webkit.WebView" bounds="[0,160][400,300]"><node index="0" text="def f():&#10;    pass" resource-id="" class="android.widget.EditText" bounds="[0,160][400,300]" /></node></node><node index="1" text="Two Sum again" resource-id="" class="android.widget.TextView" bounds="[0,0][0,0]" /></hierarchy>"#;

    fn sel(v: Value) -> Sel {
        Sel::from_json(&v).unwrap()
    }

    #[test]
    fn given_a_dump_when_a_selector_is_looked_up_then_the_deepest_visible_match_wins_and_a_web_view_reads_its_field() {
        let label = find_in(NESTED, &sel(json!({ "text": ".*Two Sum.*" }))).unwrap();
        assert_eq!((label.text.as_str(), label.bounds), ("Two Sum", [10, 110, 200, 150]), "the TextView inside the card that also matches by its label");
        let editor = find_in(NESTED, &sel(json!({ "id": "CodeEditorInput" }))).unwrap();
        assert_eq!((editor.value(), editor.class.as_str()), (Some("def f():\n    pass".to_string()), "android.webkit.WebView"));
        assert_eq!(find_in(NESTED, &sel(json!({ "text": "Two Sum again" }))), None, "a node with no on-screen part is not visible");
        assert_eq!(parse_dump(NESTED).iter().filter(|(n, _)| n.matches(&sel(json!({ "text": ".*two sum.*" })))).count(), 3);
        assert_eq!(parse_dump(NESTED).iter().map(|(_, end)| *end).collect::<Vec<_>>(), [4, 2, 4, 4, 5]);
        assert_eq!(find_in(NESTED, &sel(json!({ "id": "Card", "text": "Two Sum, Easy" }))).map(|n| n.bounds), Some([0, 100, 400, 300]), "every field must hold");
        let chain = r#"<hierarchy><node resource-id="A" class="V" bounds="[0,200][400,600]"><node resource-id="B" class="V" bounds="[0,200][400,400]"><node resource-id="C" class="V" bounds="[0,200][200,300]" /></node><node resource-id="D" class="V" bounds="[0,400][400,600]" /></node></hierarchy>"#;
        assert_eq!(find_in(chain, &sel(json!({ "id": "[A-D]" }))).map(|n| n.id), Some("C".to_string()), "the first match with no match inside it, as the browser picks");
    }

    #[test]
    fn given_the_editor_wrapper_of_a_device_when_its_field_is_drawn_inside_it_but_not_its_child_then_that_field_is_its_text() {
        let dump = r#"<hierarchy><node resource-id="CodeEditorInput" class="android.widget.FrameLayout" text="" bounds="[17,186][463,524]" /><node resource-id="" class="android.widget.EditText" text="class Solution:" bounds="[54,170][465,525]" /><node resource-id="ProblemEditorTestInput" class="android.widget.EditText" text="" bounds="[17,751][463,999]" /></hierarchy>"#;
        let mut host = find_in(dump, &sel(json!({ "id": "CodeEditorInput" }))).unwrap();
        assert_eq!(host.value(), None, "a wrapper has no text of its own");
        host.inner = parse_dump(dump).into_iter().map(|(n, _)| n).find(|n| n.class == EDIT_TEXT && centre_in(&n.bounds, &host.bounds)).map(|n| n.text);
        assert_eq!(host.value().as_deref(), Some("class Solution:"), "the field drawn over it, not the test input below");
    }

    #[test]
    fn given_shell_words_and_logcat_levels_when_built_then_only_unsafe_words_are_quoted() {
        assert_eq!(sh_word("exp://u.expo.dev/p/group/g/--/leetcode"), "exp://u.expo.dev/p/group/g/--/leetcode");
        assert_eq!(sh_word("exp://h/x?a=1&b='2'"), "'exp://h/x?a=1&b='\\''2'\\'''");
        assert_eq!(deepest(vec![1, 2, 3, 9], |a, b| a / 4 == b / 4), Some(3));
        assert_eq!(deepest(Vec::<u8>::new(), |_, _| true), None);
    }

    #[test]
    fn given_an_ime_window_in_dumpsys_when_read_then_its_frame_is_the_keyboard_bounds() {
        let dump = "  Window #3 Window{a u0 InputMethod}:\n    mAttrs={(0,0)(fillxfill) gr=BOTTOM ty=INPUT_METHOD}\n    frame=[0,610][393,852] last=[0,0][0,0]\n";
        assert_eq!(ime_frame(dump), Some(json!({ "left": 0, "top": 610, "right": 393, "bottom": 852 })));
        assert_eq!(ime_frame("  Window #1 Window{b u0 StatusBar}:\n    frame=[0,0][393,40]\n"), None);
        // the CI device: the IME window spans the screen below the status bar, its inset source is where the keys are
        let device = "  Window #5 Window{c u0 InputMethod}:\n    mAttrs={(0,0)(fillxfill) gr=BOTTOM ty=INPUT_METHOD}\n    frame=[0,32][480,976]\n  InsetsSource type=ime frame=[0,626][480,976] visible=true\n";
        assert_eq!(ime_frame(device), Some(json!({ "left": 0, "top": 626, "right": 480, "bottom": 976 })));
    }
}
