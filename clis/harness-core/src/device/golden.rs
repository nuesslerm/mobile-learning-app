//! `assert_screenshot`: the screen as a small PNG plus a sidecar JSON, checked against
//! `<goldens>/<name>.png`.
use super::{Device, Level};
use crate::golden::{self, Mode, Outcome, Tolerance};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Goldens {
    pub dir: PathBuf,
    pub mode: Mode,
    /// The run's folder: captures go to `<run>/goldens/`, the gallery to `<run>/goldens.html`.
    pub run_dir: PathBuf,
    started: Instant,
    steps: RefCell<usize>,
    last_action: RefCell<Option<String>>,
    pub outcomes: RefCell<Vec<Outcome>>,
}

pub struct Opts {
    pub tolerance: Option<f64>,
    pub threshold: Option<f64>,
    pub max_width: Option<usize>,
    /// Rectangles (left, top, right, bottom) in screen pixels left out of the comparison.
    pub ignore: Vec<(i64, i64, i64, i64)>,
}

impl Goldens {
    pub fn new(dir: PathBuf, mode: Mode, run_dir: PathBuf) -> Self {
        Goldens { dir, mode, run_dir, started: Instant::now(), steps: RefCell::new(0), last_action: RefCell::new(None), outcomes: RefCell::new(vec![]) }
    }

    /// Counts a device call; input calls also become the last action a golden names.
    pub fn note(&self, action: Option<String>) {
        *self.steps.borrow_mut() += 1;
        if action.is_some() {
            *self.last_action.borrow_mut() = action;
        }
    }

    /// A capture in `mode` whatever the run's (assert_screenshot compares on every run).
    pub fn capture_as(&self, device: &dyn Device, name: &str, opts: &Opts, mode: Mode) -> Result<Outcome, String> {
        if !golden::valid_name(name) {
            return Err(format!("'{name}' is not a golden name (letters, digits, '.', '_' and '-'; no paths)"));
        }
        // a plain run keeps its screenshots beside the report; only a golden run writes the comparison folder
        let out = self.run_dir.join(if mode == Mode::Save { "shots" } else { "goldens" });
        std::fs::create_dir_all(&out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
        let screen = out.join(format!("{name}.screen.png"));
        let t0 = Instant::now();
        device.take_screenshot(&screen.display().to_string())?;
        let capture_ms = t0.elapsed().as_millis() as u64;
        let bytes = std::fs::read(&screen).map_err(|e| format!("cannot read the screenshot {}: {e}", screen.display()))?;
        let _ = std::fs::remove_file(&screen);
        let full = golden::decode(&bytes).map_err(|e| format!("the screenshot is not a PNG: {e}"))?;
        let img = golden::downscale(&full, opts.max_width.unwrap_or(golden::MAX_WIDTH));
        let scale = img.width as f64 / full.width.max(1) as f64;
        let px = |v: i64| (v.max(0) as f64 * scale).round() as usize;
        let tol = Tolerance {
            threshold: opts.threshold.unwrap_or(golden::THRESHOLD),
            max_ratio: opts.tolerance.unwrap_or(golden::MAX_RATIO),
            ignore: opts.ignore.iter().map(|&(l, t, r, b)| (px(l), px(t), px(r), px(b))).collect(),
        };
        let warnings = device.get_logs(Level::Warn).unwrap_or_default();
        let meta = json!({
            "platform": device.platform(),
            "step": *self.steps.borrow(),
            "last_action": *self.last_action.borrow(),
            "warnings": warnings,
            "summary": device.summary().unwrap_or(Value::Null),
            "screen": { "width": full.width, "height": full.height },
            "timings": { "since_start_ms": self.started.elapsed().as_millis() as u64, "capture_ms": capture_ms },
            "tolerance": { "threshold": tol.threshold, "max_ratio": tol.max_ratio, "ignore": tol.ignore },
        });
        if mode == Mode::Save {
            return golden::save(name, &img, &out.join(name), meta);
        }
        let o = golden::check(name, &img, &self.dir.join(format!("{name}.png")), &out.join(name), mode == Mode::Update, &tol, meta)?;
        self.outcomes.borrow_mut().push(o.clone());
        Ok(o)
    }

    /// Writes the gallery when the run took goldens; returns its line for the run's output.
    pub fn finish(&self) -> Option<String> {
        let outcomes = self.outcomes.borrow();
        if outcomes.is_empty() {
            return None;
        }
        let count = |v: &str| outcomes.iter().filter(|o| o.verdict == v).count();
        let line = match golden::write_report("run goldens", Path::new(&self.run_dir), &outcomes) {
            Ok(p) => format!("goldens: {} ({} match, {} updated, {} mismatch, {} missing)", p.display(), count("match"), count("updated"), count("mismatch"), count("missing")),
            Err(e) => format!("goldens: cannot write the gallery: {e}"),
        };
        Some(line)
    }
}
