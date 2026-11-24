use harness_core::golden::{self, Outcome, Tolerance};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use wf_kit::args::{parse_spec, Parsed, Positional, Spec};

pub fn run(args: &[String]) -> Option<i32> {
    let args = parse_spec(
        args,
        &Spec {
            values: &["device", "expect", "out", "ignore"],
            required: &["device", "expect"],
            flags: &["update-goldens"],
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
    let device = args.val("device").expect("required by parse_spec");
    let expected_id = args.val("expect").expect("required by parse_spec");
    let tree: Vec<Value> = serde_json::from_slice(&idb(device, &["ui", "describe-all", "--json"])?)
        .map_err(|err| format!("cannot read idb accessibility tree: {err}"))?;
    if !tree.iter().any(|element| element["AXUniqueId"].as_str() == Some(expected_id)) {
        return Err(format!("'{expected_id}' is missing from the simulator's accessibility tree"));
    }

    let img = golden::decode(&idb(device, &["screenshot", "-"])?)?;
    if img.width >= img.height {
        return Err("rotate the simulator to portrait before capturing".into());
    }
    let root = wf_kit::root::repo_root();
    let baseline = root.join("docs/runbooks/goldens/ios").join(format!("{name}.png"));
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|err| err.to_string())?.as_millis();
    let out = args.val("out").map(PathBuf::from).unwrap_or_else(|| root.join(".agents/tmp/runs").join(format!("ios-visual-{stamp}-{}", std::process::id())));
    let mut ignore = vec![(0, 0, img.width, img.height * 7 / 100)];
    for area in args.values("ignore") {
        ignore.push(rect(area, img.width, img.height)?);
    }
    let tol = Tolerance {
        ignore,
        ..Tolerance::default()
    };
    let meta = json!({ "platform": "ios", "expectedTestID": expected_id });
    golden::check(name, &img, &baseline, &out.join(name), args.flag("update-goldens"), &tol, meta)
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
