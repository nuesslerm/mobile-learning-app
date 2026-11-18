//! The driver of a target: a child process that speaks `mimic-driver/1` (`stdio`) or the step interpreter (`script`), the one place that
//! names the concrete adapters of the interpreter's libraries.
use super::{StdioDriver, StdioSpec};
use crate::config::{ScriptSpec, Target};
use mimic_core::ports::Driver;
use mimic_script_core::case::Case;
use mimic_script_core::Policy;
use mimic_script_host::pin::Pin;
use mimic_script_host::provision::{ensure_runtime, CurlGet};
use mimic_script_host::target::{SpawnLaunch, UrlLaunch};
use mimic_script_orca::OrcaLaunch;
use mimic_script_host::{Launch, ScriptDriver};
use std::path::PathBuf;

fn platform() -> String {
    format!("linux-{}", if std::env::consts::ARCH == "aarch64" { "arm64" } else { "x64" })
}

fn cache(spec: &ScriptSpec) -> PathBuf {
    spec.cache.clone().unwrap_or_else(|| PathBuf::from(std::env::var("T3_RELEASE_CACHE").or_else(|_| std::env::var("HOME").map(|h| format!("{h}/.cache/mimic-t3"))).unwrap_or_else(|_| ".mimic-cache".into())))
}

/// What the sidecar gets: a fixed few variables, the ones the set names, and nothing else of the caller's environment. The scripted agent CLIs
/// (`cli_dir`) come first on its `PATH`, so the host it starts sees the same CLIs the replayed chain does.
fn sidecar_env(spec: &ScriptSpec) -> Vec<(String, String)> {
    let fixed = [("TZ", "UTC".to_string()), ("LANG", "C.UTF-8".to_string())];
    let inherited = ["PATH", "HOME", "MIMIC_FIXED_PORT"].into_iter().chain(spec.env_pass.iter().map(String::as_str)).filter_map(|n| Some((n.to_string(), std::env::var(n).ok()?)));
    let mut env: Vec<(String, String)> = fixed.into_iter().map(|(k, v)| (k.to_string(), v)).chain(inherited).collect();
    for (name, value) in &spec.env_set {
        env.retain(|(k, _)| k != name);
        env.push((name.clone(), value.clone()));
    }
    if let Some(dir) = &spec.cli_dir {
        match env.iter_mut().find(|(k, _)| k == "PATH") {
            Some((_, path)) => *path = format!("{}:{path}", dir.display()),
            None => env.push(("PATH".into(), dir.display().to_string())),
        }
    }
    env
}

fn launch(target: &Target, spec: &ScriptSpec) -> Result<Box<dyn Launch>, String> {
    if spec.orca {
        return Ok(Box::new(OrcaLaunch { sidecar: spec.sidecar.clone(), env: sidecar_env(spec), cwd: Some(spec.dir.clone()) }));
    }
    if spec.spawn {
        let pin_path = target.pin.as_ref().ok_or("a spawned script target needs a pin file (the release it runs)")?;
        let pin = Pin::parse(&std::fs::read_to_string(pin_path).map_err(|e| format!("{}: {e}", pin_path.display()))?)?;
        let runtime = ensure_runtime(&pin, &platform(), &cache(spec), &CurlGet)?;
        Ok(Box::new(SpawnLaunch { runtime: runtime.dir, ready_ms: spec.ready_ms, cli_dir: spec.cli_dir.clone() }))
    } else {
        let addr = spec.addr.as_deref().ok_or("a url target needs addr and workspace (mimic.toml, or --addr and --workspace of the verb)")?.parse().map_err(|e| format!("script addr: {e}"))?;
        let workspace = spec.workspace.clone().ok_or("a url target needs addr and workspace (mimic.toml, or --addr and --workspace of the verb)")?;
        Ok(Box::new(UrlLaunch { addr, workspace, mint: spec.mint.clone(), cli_dir: spec.cli_dir.clone() }))
    }
}

/// The driver `target` names: opening it starts nothing; a spawned server starts with the first case. `command` is the child process of a
/// `stdio` target as the caller wants it run (the recorder adds its concurrency to the environment).
pub fn open_driver(target: &Target, command: StdioSpec) -> Result<Box<dyn Driver>, String> {
    let Some(spec) = &target.script else {
        return Ok(Box::new(StdioDriver::new(command)));
    };
    let mut policy = Policy::default();
    policy.settle_ms = spec.settle_ms.unwrap_or(policy.settle_ms);
    policy.first_answer_ms = spec.first_answer_ms.unwrap_or(policy.first_answer_ms);
    policy.quiet_cap_ms = spec.quiet_cap_ms;
    let prelude = match &spec.prelude {
        Some(path) => {
            let entries: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?).map_err(|e| format!("{}: {e}", path.display()))?;
            Case::entries_of(&entries).map_err(|e| format!("{}: {e}", path.display()))?
        }
        None => Vec::new(),
    };
    let version = target.provenance().ok().and_then(|p| p.version);
    Ok(Box::new(ScriptDriver::new(launch(target, spec)?, policy, prelude, spec.session, version)))
}
