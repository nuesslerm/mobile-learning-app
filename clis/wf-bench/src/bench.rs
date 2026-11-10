//! `wf bench cli`: cold start, wall time and peak RSS per command, one fresh process per run, plus bundle and
//! node_modules footprint. Peak RSS comes from `wait4`, which reports exactly the child it reaped, so no
//! helper interpreter is needed.
use wf_kit::args::{parse_spec, Positional, Spec};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

const STARTUP_TARGETS: &[&[&str]] = &[
    &["--version"],
    &["--help"],
    &["secret", "--help"],
    &["offload", "ask", "--help"],
    &["ship", "--changed", "--help"],
    &["check", "--help"],
];
const LIVE_TARGETS: &[&[&str]] = &[&["offload", "ask", "--stats"], &["ship", "--changed"]];

struct Sample {
    wall_ms: f64,
    max_rss_kb: i64,
    exit_code: i32,
}

fn measure_once(bin: &str, argv: &[&str]) -> Result<Sample, String> {
    let start = Instant::now();
    let child = Command::new(bin)
        .args(argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("cannot run {bin}: {e}"))?;
    let mut status = 0;
    // SAFETY: `rusage` is plain data that `wait4` fills in; zeroed is a valid start.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: waits for the child this function spawned, writing only into the two locals.
    let reaped = unsafe { libc::wait4(child.id() as i32, &mut status, 0, &mut usage) };
    if reaped < 0 {
        return Err(format!("wait4 failed for {bin}"));
    }
    let exited = status & 0x7f == 0;
    Ok(Sample {
        wall_ms: start.elapsed().as_secs_f64() * 1000.0,
        max_rss_kb: usage.ru_maxrss,
        exit_code: if exited {
            (status >> 8) & 0xff
        } else {
            128 + (status & 0x7f)
        },
    })
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    }
}

fn round1(ms: f64) -> f64 {
    (ms * 10.0).round() / 10.0
}

/// A JS number as printed by `String(n)`: whole values carry no decimal point.
fn js_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

struct TargetResult {
    argv: Vec<String>,
    runs: usize,
    first_wall: f64,
    median_wall: f64,
    max_wall: f64,
    median_rss: i64,
    max_rss: i64,
    failures: usize,
}

fn summarize(argv: &[&str], samples: &[Sample]) -> TargetResult {
    let walls: Vec<f64> = samples.iter().map(|s| s.wall_ms).collect();
    let rss: Vec<f64> = samples.iter().map(|s| s.max_rss_kb as f64).collect();
    TargetResult {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        runs: samples.len(),
        first_wall: round1(walls[0]),
        median_wall: round1(median(&walls)),
        max_wall: round1(walls.iter().cloned().fold(f64::MIN, f64::max)),
        median_rss: median(&rss).round() as i64,
        max_rss: samples.iter().map(|s| s.max_rss_kb).max().unwrap_or(0),
        failures: samples.iter().filter(|s| s.exit_code != 0).count(),
    }
}

fn count_entries(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            total += 1;
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(entry.path());
            }
        }
    }
    total
}

fn bundle_bytes(root: &Path) -> Vec<(String, u64)> {
    let mut pkgs: Vec<String> = fs::read_dir(root.join("clis"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    pkgs.sort();
    let mut out = Vec::new();
    for pkg in pkgs {
        let Ok(rd) = fs::read_dir(root.join("clis").join(&pkg).join("bundle")) else {
            continue;
        };
        let mut files: Vec<(String, u64)> = rd
            .flatten()
            .filter_map(|e| {
                let meta = fs::metadata(e.path()).ok()?;
                meta.is_file().then(|| {
                    (
                        format!("{pkg}/{}", e.file_name().to_string_lossy()),
                        meta.len(),
                    )
                })
            })
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

fn format_report(
    bin: &str,
    results: &[TargetResult],
    footprint: Option<&(Vec<(String, u64)>, u64)>,
) -> String {
    let mut lines = vec![
        format!("bin: {bin}"),
        "target | runs | first ms | median ms | max ms | median RSS MB | max RSS MB | failures"
            .to_string(),
    ];
    for r in results {
        lines.push(format!(
            "{} | {} | {} | {} | {} | {:.1} | {:.1} | {}",
            r.argv.join(" "),
            r.runs,
            js_num(r.first_wall),
            js_num(r.median_wall),
            js_num(r.max_wall),
            r.median_rss as f64 / 1024.0,
            r.max_rss as f64 / 1024.0,
            r.failures
        ));
    }
    if let Some((bundles, node_modules)) = footprint {
        for (name, bytes) in bundles {
            lines.push(format!(
                "bundle {name}: {:.2} MB",
                *bytes as f64 / 1024.0 / 1024.0
            ));
        }
        lines.push(format!("node_modules files: {node_modules}"));
    }
    format!("{}\n", lines.join("\n"))
}

fn run_inner(args: &[String]) -> Result<Option<i32>, String> {
    let spec = Spec { values: &["runs", "out"], flags: &["live", "no-footprint"], positional: Positional::None, ..Spec::NONE };
    let Some(p) = parse_spec(args, &spec) else { return Ok(None) };
    let runs_arg = p.val("runs").unwrap_or("5").to_string();
    let (live, no_footprint, out_file) = (p.flag("live"), p.flag("no-footprint"), p.val("out").map(str::to_string));
    let runs: usize = runs_arg
        .parse()
        .ok()
        .filter(|n| *n >= 1)
        .ok_or_else(|| format!("--runs must be a positive integer, got: {runs_arg}"))?;
    let bin = std::env::var("WF_BIN").unwrap_or_else(|_| "wf".into());
    let targets: Vec<&[&str]> = STARTUP_TARGETS
        .iter()
        .chain(if live { LIVE_TARGETS } else { &[] })
        .copied()
        .collect();
    let mut results = Vec::new();
    for argv in &targets {
        let samples = (0..runs)
            .map(|_| measure_once(&bin, argv))
            .collect::<Result<Vec<_>, _>>()?;
        results.push(summarize(argv, &samples));
    }
    let footprint = (!no_footprint).then(|| {
        let root = wf_kit::root::repo_root();
        (
            bundle_bytes(&root),
            count_entries(&root.join("node_modules")),
        )
    });
    if let Some(file) = out_file {
        let report = json!({
            "bin": bin,
            "results": results.iter().map(|r| json!({
                "argv": r.argv, "runs": r.runs, "firstWallMs": r.first_wall, "medianWallMs": r.median_wall,
                "maxWallMs": r.max_wall, "medianRssKb": r.median_rss, "maxRssKb": r.max_rss, "failures": r.failures,
            })).collect::<Vec<Value>>(),
        });
        let mut report = report;
        if let Some((bundles, node_modules)) = &footprint {
            report["footprint"] = json!({
                "bundleBytes": bundles.iter().map(|(n, b)| (n.clone(), json!(b))).collect::<serde_json::Map<_, _>>(),
                "nodeModulesFiles": node_modules,
            });
        }
        fs::write(
            &file,
            format!("{}\n", serde_json::to_string_pretty(&report).expect("json")),
        )
        .map_err(|e| format!("cannot write {file}: {e}"))?;
    }
    print!("{}", format_report(&bin, &results, footprint.as_ref()));
    Ok(Some(0))
}

/// `args` follow `bench`. `None` for a shape it does not serve (unknown options).
pub fn run(args: &[String]) -> Option<i32> {
    match run_inner(args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("Error: {message}");
            Some(1)
        }
    }
}
