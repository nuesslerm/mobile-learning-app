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
    ("secret", "secrets", "secrets"),
    ("agent-config", "agent-config-sync", "agent-config"),
    ("flow", "harness-flow", "harness-flow"),
];

/// Shapes of a tabled verb that only the router serves: `secret login` (the login capture) and `flow --publish` (the bucket client).
fn router_only(verb: &str, rest: &[String]) -> bool {
    match verb {
        "secret" => rest.first().is_some_and(|a| a == "login"),
        "flow" => rest.iter().any(|a| a == "--publish"),
        _ => false,
    }
}

/// The `check` shapes the pipeline crate's thin binary serves like the router: a gate mode of its own and none of the router-only ones.
/// `--dead-refs` is served alone or with `--staged` only, exactly as the router reads it.
fn check_is_thin(rest: &[String]) -> bool {
    const ROUTER_ONLY: &[&str] = &["--health", "--visual", "--drift", "--deployed", "--arch", "--dry-run"];
    const GATES: &[&str] = &["--staged", "--pushed", "--budget", "--unused-workspace-deps", "--unused-deps", "--workspace-crossings", "--ts-packages", "--cargo-inputs"];
    if rest.iter().any(|a| ROUTER_ONLY.contains(&a.as_str())) {
        return false;
    }
    if rest.iter().any(|a| a == "--dead-refs") {
        let others: Vec<&str> = rest.iter().map(String::as_str).filter(|a| *a != "--dead-refs").collect();
        return rest.iter().filter(|a| *a == "--dead-refs").count() == 1 && (others.is_empty() || others == ["--staged"]);
    }
    rest.iter().any(|a| GATES.contains(&a.as_str()))
}

fn target_of(args: &[String]) -> (&'static str, &'static str) {
    match args.first().map(String::as_str) {
        Some("check") if check_is_thin(&args[1..]) => ("pipeline", "pipeline"),
        Some(verb) if router_only(verb, &args[1..]) => ("wf", "wf"),
        Some(verb) => TABLE.iter().find(|(v, _, _)| *v == verb).map_or(("wf", "wf"), |(_, package, bin)| (package, bin)),
        None => ("wf", "wf"),
    }
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
