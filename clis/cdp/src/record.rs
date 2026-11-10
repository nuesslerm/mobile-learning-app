//! Session video: the page is captured about twelve times a second (`Page.captureScreenshot`, which works whether or not
//! the tab is composited, unlike the screencast) and the JPEGs are piped into ffmpeg, which writes a constant-rate
//! H.264 `.mp4`. Frames carry wall-clock timestamps, so a still page plays at its real speed.
use crate::attached::Attached;
use base64::Engine as _;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const FRAME_GAP: Duration = Duration::from_millis(60);
/// Chromium 151 never answers the first capture of a fresh page (CI, 18 Oct): an unanswered capture is given up so the
/// next one can land, instead of holding the recording empty for the 60 s call timeout.
const CAPTURE_WAIT: Duration = Duration::from_secs(5);

pub struct Recording {
    child: Child,
    capture: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    frames: Arc<AtomicU64>,
    /// Captures dropped as tiled, and captures that failed with the last error: why a recording came out empty.
    dropped: Arc<AtomicU64>,
    failed: Arc<Mutex<(u64, String)>>,
    pub path: PathBuf,
    /// When the first frame was captured: the video's time zero, which the input log is measured from.
    pub first_frame: Arc<Mutex<Option<Instant>>>,
    inputs: Arc<Mutex<Inputs>>,
}

/// The input log: lines waiting for a frame captured after their moment, and the latest frames handed to ffmpeg.
#[derive(Default)]
struct Inputs {
    log: Option<PathBuf>,
    pending: VecDeque<(Instant, Value)>,
    /// When each frame's capture was asked for, and its time in the video (ms), oldest first.
    frames: VecDeque<(Instant, u64)>,
}

/// Frames kept to place a late line: 20 s at the usual pace, more on a loaded host.
const FRAME_HISTORY: usize = 330;

impl Inputs {
    /// Writes the waiting lines whose frame has been captured, in the order they were logged.
    fn resolve(&mut self) {
        while let Some((since, _)) = self.pending.front() {
            let Some(&(_, ms)) = self.frames.iter().find(|(began, _)| began >= since) else { break };
            if let Some((_, entry)) = self.pending.pop_front() {
                self.write(entry, ms);
            }
        }
    }

    fn write(&self, mut entry: Value, t: u64) {
        entry["t"] = json!(t);
        if let Some(mut f) = self.log.as_ref().and_then(|log| std::fs::OpenOptions::new().append(true).open(log).ok()) {
            let _ = writeln!(f, "{entry}");
        }
    }

    /// Lines no frame came for (captures stalled, ffmpeg gone) are placed at their own moment on the video's clock.
    fn flush(&mut self, zero: Option<Instant>) {
        self.resolve();
        for (since, entry) in std::mem::take(&mut self.pending) {
            self.write(entry, zero.map_or(0, |z| since.saturating_duration_since(z).as_millis() as u64));
        }
    }
}

impl Recording {
    /// Milliseconds into the video (0 until the first frame).
    pub fn elapsed_ms(&self) -> u64 {
        self.first_frame.lock().ok().and_then(|f| *f).map_or(0, |t| t.elapsed().as_millis() as u64)
    }

    /// Starts the input log at `log` (JSON lines, emptied now).
    pub fn log_inputs_to(&self, log: PathBuf) -> Result<(), String> {
        std::fs::write(&log, "").map_err(|e| e.to_string())?;
        self.inputs.lock().map_err(|_| "the input log is poisoned".to_string())?.log = Some(log);
        Ok(())
    }

    /// Appends `entry` to the input log with `t`, the video time of the first frame whose capture was asked for after `since`,
    /// so the video at `t` shows the page as it was then (a tap's dot) however slow captures are. It never waits for that
    /// frame: the line is written, in order, once the frame is captured, at the latest when the recording stops.
    pub fn log_input(&self, since: Instant, entry: Value) {
        if let Ok(mut inputs) = self.inputs.lock() {
            inputs.pending.push_back((since, entry));
            inputs.resolve();
        }
    }

    fn flush_inputs(&self) {
        let zero = self.first_frame.lock().ok().and_then(|f| *f);
        if let Ok(mut inputs) = self.inputs.lock() {
            inputs.flush(zero);
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        self.flush_inputs();
    }
}

fn ffmpeg() -> String {
    match std::env::var("FFMPEG_PATH") {
        Ok(p) if std::path::Path::new(&p).exists() => p,
        _ => "ffmpeg".into(),
    }
}

impl Attached {
    /// Starts recording the page to `path` (`.mp4`), `width` by `height` CSS pixels.
    pub fn start_recording(&mut self, path: PathBuf, width: i64, height: i64) -> Result<Recording, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut child = Command::new(ffmpeg())
            .args(["-y", "-loglevel", "error", "-f", "image2pipe", "-use_wallclock_as_timestamps", "1", "-c:v", "mjpeg", "-i", "-"])
            .args(["-vf", &format!("scale={w}:{h}:force_original_aspect_ratio=decrease,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=white,setsar=1,fps=25,tpad=stop_mode=clone:stop_duration=1", w = width / 2 * 2, h = height / 2 * 2), "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(path.with_extension("ffmpeg.log")).map(Stdio::from).unwrap_or_else(|_| Stdio::null()))
            .spawn()
            .map_err(|_| "video recording needs ffmpeg: install it or set FFMPEG_PATH".to_string())?;
        let mut stdin = child.stdin.take().ok_or("ffmpeg has no stdin")?;
        let first_frame: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (cdp, session) = (self.cdp_handle().clone(), self.session_id());
        let frames = Arc::new(AtomicU64::new(0));
        let (dropped, failed) = (Arc::new(AtomicU64::new(0)), Arc::new(Mutex::new((0, String::new()))));
        let (first, halt, count) = (Arc::clone(&first_frame), Arc::clone(&stop), Arc::clone(&frames));
        let (tiled, errors) = (Arc::clone(&dropped), Arc::clone(&failed));
        let inputs: Arc<Mutex<Inputs>> = Arc::default();
        let landed = Arc::clone(&inputs);
        let capture = std::thread::spawn(move || {
            while !halt.load(Ordering::Relaxed) {
                let began = Instant::now();
                let shot = match cdp.call_within("Page.captureScreenshot", json!({ "format": "jpeg", "quality": 70 }), Some(&session), CAPTURE_WAIT) {
                    Ok(r) => r["data"].as_str().and_then(|d| crate::BASE64.decode(d).ok()),
                    Err(e) => {
                        if let Ok(mut f) = errors.lock() {
                            *f = (f.0 + 1, format!("{e} after {} ms", began.elapsed().as_millis()));
                        }
                        None
                    }
                };
                // a tiled capture is dropped: ffmpeg's wall-clock timestamps hold the previous frame over the gap
                let shot = shot.filter(|f| {
                    let keep = !crate::tiled::jpeg_is_tiled(f);
                    if !keep {
                        tiled.fetch_add(1, Ordering::Relaxed);
                    }
                    keep
                });
                if let Some(frame) = shot {
                    // ffmpeg stamps a frame when it reads it, which is no earlier than this
                    let at = Instant::now();
                    let zero = first.lock().map(|mut f| *f.get_or_insert(at)).unwrap_or(at);
                    if stdin.write_all(&frame).is_err() {
                        break;
                    }
                    count.fetch_add(1, Ordering::Relaxed);
                    if let Ok(mut i) = landed.lock() {
                        if i.frames.len() == FRAME_HISTORY {
                            i.frames.pop_front();
                        }
                        i.frames.push_back((began, at.duration_since(zero).as_millis() as u64));
                        i.resolve();
                    }
                }
                if let Some(rest) = FRAME_GAP.checked_sub(began.elapsed()) {
                    std::thread::sleep(rest);
                }
            }
        });
        Ok(Recording { child, capture: Some(capture), stop, frames, dropped, failed, path, first_frame, inputs })
    }

    /// Stops capturing, lets ffmpeg finish the file, and returns its size in bytes.
    pub fn stop_recording(&mut self, mut recording: Recording) -> Result<u64, String> {
        // The last input's marker and a settling animation must reach the file: at least 400 ms and two frames captured after
        // this call, however slow a capture is on a loaded host (3 s cap). A recording that holds no frame yet waits out the unanswered first
        // capture and one more (two CAPTURE_WAITs), or it would end empty.
        let (asked, from) = (Instant::now(), recording.frames.load(Ordering::Relaxed));
        let cap = |frames: u64| if frames == 0 { CAPTURE_WAIT * 2 } else { Duration::from_secs(3) };
        while (asked.elapsed() < Duration::from_millis(400) || recording.frames.load(Ordering::Relaxed) < from + 2) && asked.elapsed() < cap(recording.frames.load(Ordering::Relaxed)) {
            std::thread::sleep(Duration::from_millis(20));
        }
        recording.stop.store(true, Ordering::Relaxed);
        if let Some(t) = recording.capture.take() {
            let _ = t.join();
        }
        recording.flush_inputs();
        let status = recording.child.wait().map_err(|e| e.to_string())?;
        let log = recording.path.with_extension("ffmpeg.log");
        let size = std::fs::metadata(&recording.path).map(|m| m.len()).map_err(|e| {
            let said = std::fs::read_to_string(&log).unwrap_or_default();
            let tail: Vec<&str> = said.lines().rev().take(5).collect();
            let (failures, last) = recording.failed.lock().map(|f| f.clone()).unwrap_or_default();
            format!(
                "the recording was not written: {e}; {} frames captured, {} dropped as tiled, {failures} captures failed{}; ffmpeg {status}: {}",
                recording.frames.load(Ordering::Relaxed),
                recording.dropped.load(Ordering::Relaxed),
                if last.is_empty() { String::new() } else { format!(" (last: {last})") },
                tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
            )
        });
        if size.is_ok() {
            let _ = std::fs::remove_file(&log);
        }
        size
    }
}
