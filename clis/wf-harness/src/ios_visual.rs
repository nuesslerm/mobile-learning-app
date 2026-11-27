use harness_core::golden::{self, Outcome, Tolerance};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};

pub fn run(args: &[String]) -> Option<i32> {
    let args = parse_spec(
        args,
        &Spec {
            values: &["device", "expect", "out", "ignore", "reference", "max-diff-ratio", "capture"],
            flags: &["update-goldens", "exact"],
            positional: Positional::Exactly(1),
            ..Spec::NONE
        },
    )?;
    Some(match capture(&args) {
        Ok(outcome) => {
            println!("{}", outcome.json());
            i32::from(!outcome.ok())
        }
        Err(error) => {
            println!("{}", json!({ "ok": false, "error": error }));
            2
        }
    })
}

fn capture(args: &Parsed) -> Result<Outcome, String> {
    let name = &args.positional[0];
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err("use letters, digits, dashes or underscores for the screenshot name".into());
    }
    let reference = args.val("reference").map(Path::new);
    if reference.is_some() && args.flag("update-goldens") {
        return Err("--reference compares against a supplied image and cannot be used with --update-goldens".into());
    }
    if args.flag("exact") && reference.is_none() {
        return Err("--exact requires --reference".into());
    }
    let ratio = args.val("max-diff-ratio");
    if (ratio.is_some() || args.val("capture").is_some()) && reference.is_none() {
        return Err("--max-diff-ratio and --capture require --reference".into());
    }
    // refused before the capture; compare_reference reads it again
    ratio.map(parse_ratio).transpose()?;
    let (bytes, meta) = if let Some(path) = args.val("capture") {
        if args.val("device").is_some() || args.val("expect").is_some() {
            return Err("--capture replaces --device and --expect".into());
        }
        (std::fs::read(path).map_err(|err| format!("cannot read captured frame {path}: {err}"))?, json!({ "platform": "ios", "capturedFrame": path }))
    } else {
        let device = args.val("device").ok_or("give --device and --expect to capture the simulator")?;
        let expected_id = args.val("expect").ok_or("give --device and --expect to capture the simulator")?;
        let tree: Vec<Value> = serde_json::from_slice(&idb(device, &["ui", "describe-all", "--json"])?)
            .map_err(|err| format!("cannot read idb accessibility tree: {err}"))?;
        if !tree.iter().any(|element| element["AXUniqueId"].as_str() == Some(expected_id)) {
            return Err(format!("'{expected_id}' is missing from the simulator's accessibility tree"));
        }
        (idb(device, &["screenshot", "-"])?, json!({ "platform": "ios", "expectedTestID": expected_id }))
    };
    let img = golden::decode(&bytes)?;
    if img.width >= img.height {
        return Err("rotate the simulator to portrait before capturing".into());
    }
    let root = wf_kit::root::repo_root();
    let baseline = root.join("docs/runbooks/goldens/ios").join(format!("{name}.png"));
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|err| err.to_string())?.as_millis();
    let out = args.val("out").map(PathBuf::from).unwrap_or_else(|| root.join(".agents/tmp/runs").join(format!("ios-visual-{stamp}-{}", std::process::id())));
    if let Some(reference) = reference {
        let run = out.join(format!("{name}-{stamp}-{}", std::process::id()));
        return compare_reference(args, name, reference, &run, &bytes, &img, meta);
    }
    let mut ignore = vec![(0, 0, img.width, img.height * 7 / 100)];
    ignore.extend(ignore_rectangles(args, &img)?);
    let tol = Tolerance {
        ignore,
        ..Tolerance::default()
    };
    golden::check(name, &img, &baseline, &out.join(name), args.flag("update-goldens"), &tol, meta)
}

fn parse_ratio(value: &str) -> Result<f64, String> {
    value.parse().ok().filter(|ratio| (0.0..=1.0).contains(ratio))
        .ok_or_else(|| "--max-diff-ratio expects a value from 0 to 1".into())
}

fn compare_reference(args: &Parsed, name: &str, reference: &Path, run: &Path, bytes: &[u8], native: &golden::Image, mut meta: Value) -> Result<Outcome, String> {
    let max_ratio = args.val("max-diff-ratio").map(parse_ratio).transpose()?;
    let reference_image = golden::decode(&std::fs::read(reference).map_err(|e| format!("cannot read {}: {e}", reference.display()))?)?;
    let aspect_ratio = reference_image.width as f64 * native.height as f64 / (reference_image.height as f64 * native.width as f64);
    if !(0.99..=1.01).contains(&aspect_ratio) {
        return Err(format!(
            "reference must have the same portrait aspect ratio as the capture (reference {}x{}, capture {}x{})",
            reference_image.width, reference_image.height, native.width, native.height,
        ));
    }
    let ignore = ignore_rectangles(args, &reference_image)?;
    let tol = if args.flag("exact") {
        Tolerance { exact: true, threshold: 0.0, max_ratio: max_ratio.unwrap_or(0.0), ignore }
    } else {
        Tolerance { ignore, max_ratio: max_ratio.unwrap_or(golden::MAX_RATIO), ..Tolerance::default() }
    };
    if let Some(parent) = run.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::create_dir(run).map_err(|e| format!("cannot create {}: {e}", run.display()))?;
    let raw = run.join("capture.png");
    let normalized = run.join("capture-srgb.png");
    let target = run.join("reference-srgb.png");
    std::fs::write(&raw, bytes).map_err(|e| format!("cannot write {}: {e}", raw.display()))?;
    normalize(&raw, &normalized, &reference_image)?;
    normalize(reference, &target, &reference_image)?;
    let actual = golden::decode(&std::fs::read(&normalized).map_err(|e| format!("cannot read {}: {e}", normalized.display()))?)?;
    meta["reference"] = json!(reference);
    meta["nativeCapture"] = json!({ "path": raw, "width": native.width, "height": native.height });
    meta["comparisonColorSpace"] = json!("sRGB");
    golden::check_reference(name, &actual, &target, &run.join(name), &tol, meta)
}

fn normalize(source: &Path, out: &Path, size: &golden::Image) -> Result<(), String> {
    let output = Command::new("sips")
        .args(["--matchTo", "/System/Library/ColorSync/Profiles/sRGB Profile.icc", "--resampleHeightWidth"])
        .arg(size.height.to_string())
        .arg(size.width.to_string())
        .arg(source)
        .arg("--out")
        .arg(out)
        .output()
        .map_err(|e| format!("cannot run sips to normalize the iOS capture or reference: {e}"))?;
    if !output.status.success() {
        return Err(format!("cannot normalize {}: {}", source.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(())
}

fn ignore_rectangles(args: &Parsed, image: &golden::Image) -> Result<Vec<(usize, usize, usize, usize)>, String> {
    args.values("ignore").into_iter().map(|area| rect(area, image.width, image.height)).collect()
}

fn rect(area: &str, width: usize, height: usize) -> Result<(usize, usize, usize, usize), String> {
    let coords = area.split(',').map(str::parse::<usize>).collect::<Result<Vec<_>, _>>();
    if let Ok(coords) = coords {
        if let &[left, top, right, bottom] = coords.as_slice() {
            if left < right && top < bottom && right <= width && bottom <= height {
                return Ok((left, top, right, bottom));
            }
        }
    }
    Err(format!("invalid --ignore rectangle '{area}': use left,top,right,bottom in screenshot pixels"))
}

fn idb(device: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("idb")
        .args(args)
        .args(["--udid", device])
        .output()
        .map_err(|err| format!("cannot run idb, the local iOS simulator driver: {err}"))?;
    if !output.status.success() {
        return Err(format!("idb {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output.stdout)
}
