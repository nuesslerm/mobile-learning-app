//! `wf browser check-taps`: whether a recording shows every logged tap. Some frame within 1500 ms after each tap (the marker lingers 1800 ms, so a loaded recorder that drops frames still shows it) has the
//! tap indicator's blue within 20 px of its position. ffmpeg decodes (`FFMPEG_PATH`, else `ffmpeg` on PATH).
use serde_json::{json, Value};
use std::process::{Command, Stdio};

/// Inside the marker's linger (`lingerMs` 1800 in `cdp/src/attached.rs`): a recorder that drops frames under load must not fail a tap it shows.
const WINDOW_MS: f64 = 1500.0;
const RADIUS_PX: i64 = 20;
const MIN_DOT_PIXELS: usize = 12;

/// The indicator is #2f7dff at 85 % opacity: strongly blue, and far bluer than red or green.
fn is_dot_blue(r: u8, g: u8, b: u8) -> bool {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    b >= 170 && b - r >= 100 && b - g >= 50
}

fn dot_near(frame: &[u8], width: usize, x: f64, y: f64) -> bool {
    let height = frame.len() / 3 / width;
    let (cx, cy) = (x.round() as i64, y.round() as i64);
    let mut found = 0;
    for dy in -RADIUS_PX..=RADIUS_PX {
        for dx in -RADIUS_PX..=RADIUS_PX {
            if dx * dx + dy * dy > RADIUS_PX * RADIUS_PX {
                continue;
            }
            let (px, py) = (cx + dx, cy + dy);
            if px < 0 || py < 0 || px >= width as i64 || py >= height as i64 {
                continue;
            }
            let at = (py as usize * width + px as usize) * 3;
            if is_dot_blue(frame[at], frame[at + 1], frame[at + 2]) {
                found += 1;
                if found >= MIN_DOT_PIXELS {
                    return true;
                }
            }
        }
    }
    false
}

fn ffmpeg() -> String {
    match std::env::var("FFMPEG_PATH") {
        Ok(p) if std::path::Path::new(&p).exists() => p,
        _ => "ffmpeg".into(),
    }
}

fn run(args: &[String]) -> Result<(Vec<u8>, String), String> {
    let out = Command::new(ffmpeg())
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| "video-check needs ffmpeg: install it or set FFMPEG_PATH".to_string())?;
    Ok((out.stdout, String::from_utf8_lossy(&out.stderr).into_owned()))
}

fn video_size(video: &str) -> Result<(usize, usize), String> {
    // ffmpeg with no output names the streams on stderr, then exits non-zero.
    let (_, stderr) = run(&["-hide_banner".into(), "-i".into(), video.into()])?;
    let line = stderr.lines().find(|l| l.contains("Video: ")).ok_or_else(|| format!("video-check: no video stream in {video}"))?;
    let after = &line[line.find("Video: ").unwrap_or(0)..];
    for token in after.split([' ', ',', '[']) {
        if let Some((w, h)) = token.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse::<usize>(), h.parse::<usize>()) {
                if (10..100_000).contains(&w) && (10..100_000).contains(&h) {
                    return Ok((w, h));
                }
            }
        }
    }
    Err(format!("video-check: no video stream in {video}"))
}

pub fn input_log_path_for(video: &str) -> String {
    let stem = video.strip_suffix(".webm").or_else(|| video.strip_suffix(".mp4")).unwrap_or(video);
    format!("{stem}.inputs.jsonl")
}

pub fn check_video_taps(video: &str, inputs: &str) -> Result<Value, String> {
    for path in [video, inputs] {
        if !std::path::Path::new(path).exists() {
            return Err(format!("video-check: no file at {path}"));
        }
    }
    let taps: Vec<(f64, f64, f64)> = std::fs::read_to_string(inputs)
        .map_err(|e| e.to_string())?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|e| e["kind"] == "tap")
        .map(|e| (e["t"].as_f64().unwrap_or(0.0), e["x"].as_f64().unwrap_or(-1.0), e["y"].as_f64().unwrap_or(-1.0)))
        .collect();
    let (width, height) = video_size(video)?;
    let frame_bytes = width * height * 3;
    let mut missing: Vec<Value> = Vec::new();
    for (t, x, y) in &taps {
        let (raw, _) = run(&[
            "-hide_banner".into(),
            "-ss".into(),
            format!("{:.3}", t / 1000.0),
            "-i".into(),
            video.into(),
            "-t".into(),
            format!("{:.3}", WINDOW_MS / 1000.0),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "rgb24".into(),
            "pipe:1".into(),
        ])?;
        let shown = raw.chunks_exact(frame_bytes).any(|frame| dot_near(frame, width, *x, *y));
        if !shown {
            missing.push(json!(t));
        }
    }
    Ok(json!({ "taps": taps.len(), "dotsFound": taps.len() - missing.len(), "pass": missing.is_empty(), "missing": missing }))
}
