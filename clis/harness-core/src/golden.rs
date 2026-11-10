//! Golden images, shared by `device:assert_screenshot` (the harness runs) and `wf flow goldens` (the flow harness): a capture is
//! downscaled to a small PNG and compared with the committed golden by pixelmatch. It matches when the share of differing
//! pixels is at most `max_ratio`; a mismatch leaves actual, expected and diff PNGs beside the run, `update` rewrites the golden.
use crate::pixelmatch::diff_mask;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Goldens are stored at most this wide (px); taller pages keep their aspect ratio.
pub const MAX_WIDTH: usize = 360;
/// pixelmatch's per-pixel YIQ colour threshold (0..1): below it two pixels count as equal.
pub const THRESHOLD: f64 = 0.1;
/// The share of differing pixels a match may have: 0.2 %.
pub const MAX_RATIO: f64 = 0.002;

#[derive(Clone)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

#[derive(Clone)]
pub struct Tolerance {
    pub threshold: f64,
    pub max_ratio: f64,
    /// Rectangles (left, top, right, bottom) in golden pixels that never count, such as a status-bar clock.
    pub ignore: Vec<(usize, usize, usize, usize)>,
}

impl Default for Tolerance {
    fn default() -> Self {
        Tolerance { threshold: THRESHOLD, max_ratio: MAX_RATIO, ignore: vec![] }
    }
}

#[derive(Clone)]
pub struct Outcome {
    pub name: String,
    /// `match`, `updated`, `mismatch` or `missing` (no golden yet).
    pub verdict: &'static str,
    pub message: String,
    pub golden: PathBuf,
    pub actual: PathBuf,
    pub expected: Option<PathBuf>,
    pub diff: Option<PathBuf>,
    pub differing: usize,
    pub ratio: f64,
}

impl Outcome {
    pub fn ok(&self) -> bool {
        matches!(self.verdict, "match" | "updated" | "saved")
    }

    pub fn json(&self) -> Value {
        let p = |p: &Option<PathBuf>| p.as_ref().map(|p| p.display().to_string());
        json!({
            "name": self.name, "verdict": self.verdict, "ok": self.ok(), "message": self.message,
            "golden": self.golden.display().to_string(), "actual": self.actual.display().to_string(),
            "expected": p(&self.expected), "diff": p(&self.diff), "differing_pixels": self.differing, "ratio": self.ratio,
        })
    }
}

pub fn decode(bytes: &[u8]) -> Result<Image, String> {
    let (rgba, width, height) = crate::trace::decode_rgba(bytes)?;
    Ok(Image { width, height, rgba })
}

pub fn encode(img: &Image) -> Result<Vec<u8>, String> {
    let opaque = img.rgba.chunks(4).all(|p| p[3] == 255);
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, img.width as u32, img.height as u32);
        e.set_color(if opaque { png::ColorType::Rgb } else { png::ColorType::Rgba });
        e.set_depth(png::BitDepth::Eight);
        e.set_compression(png::Compression::Best);
        e.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
        let mut w = e.write_header().map_err(|e| e.to_string())?;
        let data: Vec<u8> = if opaque { img.rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect() } else { img.rgba.clone() };
        w.write_image_data(&data).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Area-averaged downscale to at most `max_width` pixels wide.
pub fn downscale(img: &Image, max_width: usize) -> Image {
    if max_width == 0 || img.width <= max_width {
        return img.clone();
    }
    let (w, h) = (max_width, ((img.height * max_width) as f64 / img.width as f64).round().max(1.0) as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for dy in 0..h {
        let (y0, y1) = (dy * img.height / h, ((dy + 1) * img.height / h).max(dy * img.height / h + 1));
        for dx in 0..w {
            let (x0, x1) = (dx * img.width / w, ((dx + 1) * img.width / w).max(dx * img.width / w + 1));
            let mut sum = [0u64; 4];
            for y in y0..y1.min(img.height) {
                for x in x0..x1.min(img.width) {
                    let i = (y * img.width + x) * 4;
                    (0..4).for_each(|c| sum[c] += img.rgba[i + c] as u64);
                }
            }
            let n = ((y1.min(img.height) - y0) * (x1.min(img.width) - x0)).max(1) as u64;
            let o = (dy * w + dx) * 4;
            (0..4).for_each(|c| rgba[o + c] = (sum[c] / n) as u8);
        }
    }
    Image { width: w, height: h, rgba }
}

fn pad(img: &Image, w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    for y in 0..img.height {
        out[y * w * 4..(y * w + img.width) * 4].copy_from_slice(&img.rgba[y * img.width * 4..(y + 1) * img.width * 4]);
    }
    out
}

/// Differing pixels on the canvas both images fit in (anything outside the smaller one differs), ignored rectangles excluded.
pub fn compare(actual: &Image, expected: &Image, tol: &Tolerance) -> (Vec<bool>, usize, usize) {
    let (w, h) = (actual.width.max(expected.width), actual.height.max(expected.height));
    let (a, e) = (pad(actual, w, h), pad(expected, w, h));
    let mut mask = diff_mask(&a, &e, w, h, tol.threshold);
    for (i, m) in mask.iter_mut().enumerate() {
        let (x, y) = (i % w, i / w);
        if x >= actual.width.min(expected.width) || y >= actual.height.min(expected.height) {
            *m = true;
        }
        if tol.ignore.iter().any(|&(l, t, r, b)| x >= l && x < r && y >= t && y < b) {
            *m = false;
        }
    }
    (mask, w, h)
}

/// Red where pixels differ, a faded copy of the golden elsewhere.
fn diff_image(expected: &Image, mask: &[bool], w: usize, h: usize) -> Image {
    let mut rgba = vec![255u8; w * h * 4];
    for (i, differs) in mask.iter().enumerate() {
        let o = i * 4;
        if *differs {
            rgba[o..o + 3].copy_from_slice(&[255, 0, 0]);
            continue;
        }
        let (x, y) = (i % w, i / w);
        if x < expected.width && y < expected.height {
            let p = (y * expected.width + x) * 4;
            let gray = 0.299 * expected.rgba[p] as f64 + 0.587 * expected.rgba[p + 1] as f64 + 0.114 * expected.rgba[p + 2] as f64;
            let v = (255.0 - (255.0 - gray) * 0.15) as u8;
            rgba[o..o + 3].copy_from_slice(&[v, v, v]);
        }
    }
    Image { width: w, height: h, rgba }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    std::fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

fn sibling(stem: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{suffix}", stem.display()))
}

/// Checks `img` against `golden` (a .png; its sidecar is the .json beside it). The run's copies go to `<out_stem>.actual.png`,
/// `.expected.png`, `.diff.png` and `.json`; `meta` is the sidecar body. With `update` the golden and its sidecar are rewritten.
/// What a screenshot does with its golden: nothing (the run only keeps it), compare with it, or replace it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Save,
    Compare,
    Update,
}

/// A screenshot kept without a golden: `<out_stem>.png` and its sidecar `<out_stem>.json`; nothing is compared.
pub fn save(name: &str, img: &Image, out_stem: &Path, meta: Value) -> Result<Outcome, String> {
    let shot = sibling(out_stem, ".png");
    write(&shot, &encode(img)?)?;
    let mut meta = meta;
    meta["name"] = json!(name);
    meta["image"] = json!({ "width": img.width, "height": img.height });
    write(&sibling(out_stem, ".json"), format!("{}\n", serde_json::to_string_pretty(&meta).unwrap_or_default()).as_bytes())?;
    Ok(Outcome { name: name.into(), verdict: "saved", message: format!("screenshot '{name}' saved to {}", shot.display()), golden: PathBuf::new(), actual: shot, expected: None, diff: None, differing: 0, ratio: 0.0 })
}

pub fn check(name: &str, img: &Image, golden: &Path, out_stem: &Path, update: bool, tol: &Tolerance, meta: Value) -> Result<Outcome, String> {
    let bytes = encode(img)?;
    let actual = sibling(out_stem, ".actual.png");
    write(&actual, &bytes)?;
    let mut meta = meta;
    meta["name"] = json!(name);
    meta["image"] = json!({ "width": img.width, "height": img.height });
    meta["tolerance"] = json!({ "threshold": tol.threshold, "max_ratio": tol.max_ratio, "ignore": tol.ignore });
    let mut o = Outcome { name: name.into(), verdict: "match", message: String::new(), golden: golden.into(), actual, expected: None, diff: None, differing: 0, ratio: 0.0 };
    if update {
        write(golden, &bytes)?;
        write(&golden.with_extension("json"), format!("{}\n", serde_json::to_string_pretty(&meta).unwrap_or_default()).as_bytes())?;
        o.verdict = "updated";
        o.message = format!("golden '{name}' written to {}", golden.display());
    } else if !golden.exists() {
        o.verdict = "missing";
        o.message = format!("golden '{name}': no golden at {} (run with --update-goldens to record it); actual: {}", golden.display(), o.actual.display());
    } else {
        let want = std::fs::read(golden).map_err(|e| format!("cannot read {}: {e}", golden.display())).and_then(|b| decode(&b).map_err(|e| format!("golden {}: {e}", golden.display())))?;
        let (mask, w, h) = compare(img, &want, tol);
        o.differing = mask.iter().filter(|m| **m).count();
        o.ratio = o.differing as f64 / (w * h).max(1) as f64;
        if o.ratio > tol.max_ratio {
            let (expected, diff) = (sibling(out_stem, ".expected.png"), sibling(out_stem, ".diff.png"));
            write(&expected, &encode(&want)?)?;
            write(&diff, &encode(&diff_image(&want, &mask, w, h))?)?;
            o.verdict = "mismatch";
            let size = if (img.width, img.height) != (want.width, want.height) { format!(" (size {}x{}, golden {}x{})", img.width, img.height, want.width, want.height) } else { String::new() };
            o.message = format!(
                "golden '{name}' differs: {} pixels ({:.3}%) over the {:.3}% tolerance{size}; expected {}, actual {}, diff {}",
                o.differing,
                o.ratio * 100.0,
                tol.max_ratio * 100.0,
                expected.display(),
                o.actual.display(),
                diff.display()
            );
            o.expected = Some(expected);
            o.diff = Some(diff);
        } else {
            o.message = format!("golden '{name}' matches ({} pixels differ)", o.differing);
        }
    }
    let mut sidecar = meta;
    sidecar["result"] = o.json();
    write(&sibling(out_stem, ".json"), format!("{}\n", serde_json::to_string_pretty(&sidecar).unwrap_or_default()).as_bytes())?;
    Ok(o)
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn rel(base: &Path, p: &Path) -> String {
    let abs = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (b, t) = (abs(base), abs(p));
    match t.strip_prefix(&b) {
        Ok(r) => r.display().to_string(),
        Err(_) => format!("file://{}", t.display()),
    }
}

/// A self-contained page listing each golden with its verdict and images (paths relative to `base`, the page's folder).
pub fn gallery(title: &str, base: &Path, outcomes: &[Outcome]) -> String {
    let failed = outcomes.iter().filter(|o| !o.ok()).count();
    let mut cards = String::new();
    for o in outcomes {
        let img = |label: &str, p: &Path| format!("<figure><img src=\"{}\" alt=\"{} {label}\" loading=\"lazy\"><figcaption>{label}</figcaption></figure>", esc(&rel(base, p)), esc(&o.name));
        let mut figs = String::new();
        if let Some(e) = &o.expected {
            figs.push_str(&img("expected", e));
        }
        figs.push_str(&img("actual", &o.actual));
        if let Some(d) = &o.diff {
            figs.push_str(&img("diff", d));
        }
        cards.push_str(&format!(
            "<section class=\"card {v}\"><header><h2>{n}</h2><span class=\"badge\">{v}</span></header><p>{m}</p><div class=\"figs\">{figs}</div></section>\n",
            v = o.verdict,
            n = esc(&o.name),
            m = esc(&o.message)
        ));
    }
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{t}</title><style>\
:root{{--bg:#f7f7f8;--fg:#1c1c1f;--card:#fff;--line:#ddd;--ok:#1a7f37;--bad:#c62828;--new:#8250df}}\
@media (prefers-color-scheme:dark){{:root{{--bg:#111214;--fg:#e8e8ea;--card:#1b1c1f;--line:#333;--ok:#3fb950;--bad:#ff6b6b;--new:#b392f0}}}}\
body{{margin:0;padding:16px;background:var(--bg);color:var(--fg);font:14px/1.4 system-ui,sans-serif}}h1{{font-size:20px}}\
.card{{background:var(--card);border:1px solid var(--line);border-radius:8px;padding:12px;margin:0 0 16px}}\
.card header{{display:flex;justify-content:space-between;gap:8px;align-items:center}}h2{{font-size:16px;margin:0;word-break:break-all}}\
.badge{{font-weight:600;padding:2px 8px;border-radius:99px;border:1px solid currentColor}}.match .badge{{color:var(--ok)}}.updated .badge{{color:var(--new)}}\
.mismatch .badge,.missing .badge{{color:var(--bad)}}.figs{{display:flex;gap:12px;overflow-x:auto}}figure{{margin:0}}\
img{{max-width:min(360px,80vw);border:1px solid var(--line);display:block}}figcaption{{font-size:12px;opacity:.7}}\
</style></head><body><h1>{t}</h1><p>{n} goldens, {failed} failing</p>\n{cards}</body></html>\n",
        t = esc(title),
        n = outcomes.len()
    )
}

/// Writes `goldens.html` and `goldens.json` into `dir`; returns the page's path.
pub fn write_report(title: &str, dir: &Path, outcomes: &[Outcome]) -> Result<PathBuf, String> {
    let page = dir.join("goldens.html");
    write(&page, gallery(title, dir, outcomes).as_bytes())?;
    let list: Vec<Value> = outcomes.iter().map(Outcome::json).collect();
    write(&dir.join("goldens.json"), format!("{}\n", serde_json::to_string_pretty(&json!({ "ok": outcomes.iter().all(Outcome::ok), "goldens": list })).unwrap_or_default()).as_bytes())?;
    Ok(page)
}

/// `[A-Za-z0-9][A-Za-z0-9._-]*`: a golden name is a file name, never a path.
pub fn valid_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}
