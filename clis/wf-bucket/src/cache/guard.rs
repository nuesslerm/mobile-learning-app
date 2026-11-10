//! `wf cache guard`: keeps the tenancy's Object Storage under its cap (`docs/solutions/ci/cache-layers.md`). The CI runner holds no OCI
//! credential: it asks agent-shell, which does, to run `wf cache guard --local` through vm-ops-api's exec route, the way a deploy leg
//! reaches the VM. Every path prints one JSON line and exits 0, so the plan job that runs it never fails or waits on it.
use blob_cache::adapters::OciTenancy;
use blob_cache::domain::Budget;
use blob_cache::storage_guard::{guard, sanitize, tail, Report};
use serde_json::Value;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use wf_kit::args::{Parsed, Positional, Spec};

pub const GUARD: Spec = Spec { flags: &["local", "dry-run"], positional: Positional::None, ..Spec::NONE };

/// What the runner waits for agent-shell, what the script there may run, and when the guard stops starting work: the plan job runs
/// `wf cache guard` under `timeout 60`, so the runner's wait ends first and cancels the remote job, and the remote guard stops its own
/// calls well before its shell's `timeout 40` kills it, so the line that says what was deleted is always printed.
const RELAY_LIMIT_MS: u64 = 45_000;
const LOCAL_LIMIT_MS: u64 = 30_000;
const SCRIPT: &str = "W=/workspace/home/wf-bin/wf; [ -x $W ] || W=wf\nWF_SITE=vm timeout 40 $W cache guard --local";

fn limit(var: &str, default_ms: u64) -> Duration {
    Duration::from_millis(std::env::var(var).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default_ms))
}
const REQUIRED: [&str; 5] = ["OCI_USER_OCID", "OCI_API_KEY_FINGERPRINT", "OCI_TENANCY_OCID", "OCI_REGION", "OCI_API_PRIVATE_KEY"];

pub fn run(p: &Parsed) -> Option<i32> {
    let dry_run = p.flag("dry-run");
    let line = if p.flag("local") { local(dry_run).to_json() } else { relay(dry_run) };
    println!("{line}");
    Some(0)
}

/// On agent-shell: measure and evict with the `oci` CLI. A missing `~/.oci/config` is written once from the vault secrets that
/// `wf vm creds sync` reads, then the run is repeated.
fn local(dry_run: bool) -> Report {
    let cap = Budget::committed().tenancy_stored_bytes;
    let tenancy = OciTenancy::default();
    let deadline = Instant::now() + limit("WF_GUARD_LOCAL_LIMIT_MS", LOCAL_LIMIT_MS);
    let run = || guard(&tenancy, cap, wf_kit::time::now_ms(), deadline, dry_run);
    let first = run();
    if first.skip_code() != Some("unmeasurable") || !first.skip_reason().is_some_and(|why| why.contains("ConfigFileNotFound") || why.contains("Could not find config file")) {
        return first;
    }
    match write_oci_config() {
        Ok(()) => run(),
        Err(why) => Report::skipped(cap, "config-missing", format!("no ~/.oci/config and it could not be written: {why}")),
    }
}

/// `~/.oci/config` as the vault's OCI secrets describe it, written when it is not there yet (agent-shell holds no key until first use).
pub fn ensure_oci_config() -> Result<(), String> {
    let config = std::env::var_os("HOME").map(PathBuf::from).map(|home| home.join(".oci/config"));
    if config.is_some_and(|path| path.exists()) {
        return Ok(());
    }
    write_oci_config()
}

fn write_oci_config() -> Result<(), String> {
    let mut keys = REQUIRED.to_vec();
    keys.push("OCI_API_PRIVATE_KEY_PASSPHRASE");
    let found = secrets::load_values(&keys);
    let value = |key: &str| found.get(key).cloned().flatten().filter(|v| !v.is_empty());
    let missing: Vec<&str> = REQUIRED.into_iter().filter(|key| value(key).is_none()).collect();
    if !missing.is_empty() {
        return Err(format!("{} do not resolve from the vault", missing.join(", ")));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("HOME is not set")?;
    let dir = home.join(".oci");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let private = |path: &PathBuf, text: &str| {
        let mut file = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        file.write_all(text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
    };
    let pem = dir.join("oci_api_key.pem");
    private(&pem, &format!("{}\n", value("OCI_API_PRIVATE_KEY").unwrap_or_default().trim()))?;
    let mut config = vec![
        "[DEFAULT]".to_string(),
        format!("user={}", value("OCI_USER_OCID").unwrap_or_default()),
        format!("fingerprint={}", value("OCI_API_KEY_FINGERPRINT").unwrap_or_default()),
        format!("tenancy={}", value("OCI_TENANCY_OCID").unwrap_or_default()),
        format!("region={}", value("OCI_REGION").unwrap_or_default()),
        format!("key_file={}", pem.display()),
    ];
    if let Some(passphrase) = value("OCI_API_PRIVATE_KEY_PASSPHRASE") {
        config.push(format!("pass_phrase={passphrase}"));
    }
    private(&dir.join("config"), &format!("{}\n", config.join("\n")))
}

/// On the runner: the line agent-shell printed, or a skip that says why there is none.
fn relay(dry_run: bool) -> Value {
    let cap = Budget::committed().tenancy_stored_bytes;
    let skipped = |code: &'static str, why: String| Report::skipped(cap, code, why).to_json();
    let script = format!("{SCRIPT}{}\n", if dry_run { " --dry-run" } else { "" });
    match wf_vmops::vm_exec::capture_within(&script, limit("WF_GUARD_RELAY_LIMIT_MS", RELAY_LIMIT_MS)) {
        Err(why) if why.contains("did not finish within") => skipped("relay-timeout", why),
        Err(why) => skipped("relay-unreachable", why),
        Ok((code, output)) => output
            .lines()
            .rev()
            .find_map(|line| serde_json::from_str::<Value>(line.trim()).ok().filter(|v| v["event"] == "storage-guard"))
            .unwrap_or_else(|| skipped("relay-no-result", format!("agent-shell printed no guard result (exit {code}): {}", tail(&sanitize(&output), 300)))),
    }
}
