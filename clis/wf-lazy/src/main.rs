//! `wf-lazy <verb> ...`: runs the thin binary of the verb's crate, built on first use, and the full `wf` for any other word. The build is
//! cargo's own freshness check (about 0.3 s when current), under a lock so two first calls build once.
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{exit, Command};

/// verb, package, binary: the verbs whose crate ships a thin binary that serves them like the router does.
pub const TABLE: &[(&str, &str, &str)] = &[
    ("offload", "offload", "offload"),
    ("knowledge", "knowledge", "knowledge"),
    ("parity", "parity", "wf-parity"),
    ("lib", "wf-lib", "wf-lib"),
    ("bench", "wf-bench", "wf-bench"),
    ("cache", "wf-bucket", "wf-bucket"),
    ("bucket", "wf-bucket", "wf-bucket"),
    ("ship", "deploy", "deploy"),
    ("vm", "deploy", "deploy"),
];

fn target_of(args: &[String]) -> (&'static str, &'static str) {
    args.first()
        .and_then(|verb| TABLE.iter().find(|(v, _, _)| v == verb))
        .map_or(("wf", "wf"), |(_, package, bin)| (package, bin))
}

fn repo_root() -> PathBuf {
    let out = Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok().filter(|o| o.status.success());
    out.map_or_else(|| PathBuf::from("."), |o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (package, bin) = target_of(&args);
    let root = repo_root();
    let manifest = root.join("clis/Cargo.toml");
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("clis/target"), PathBuf::from);
    let cargo = std::env::var("WF_LAZY_CARGO").unwrap_or_else(|_| "cargo".to_string());
    let build = ["build", "--locked", "-p", package, "--bin", bin, "--manifest-path", &manifest.display().to_string()];
    let status = Command::new("flock")
        .arg(target.join(".wf-lazy.lock"))
        .arg(&cargo)
        .args(build)
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_TARGET_DIR", &target)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("wf-lazy: the build failed; rerun it with: {cargo} {}", build.join(" "));
            exit(s.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("wf-lazy: cannot run flock {cargo}: {e}");
            exit(2);
        }
    }
    let err = Command::new(target.join("debug").join(bin)).args(&args).exec();
    eprintln!("wf-lazy: cannot run {bin}: {err}");
    exit(2);
}
