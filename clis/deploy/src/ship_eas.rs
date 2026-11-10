//! `wf ship eas`: publish the app as an EAS Update through its `publish:eas` script. The tier-3 gate of `wf check`
//! publishes through the same function.
use wf_kit::args::parse;
use pipeline::eas::{children_peak_rss_mb, latest_key, latest_pointer, served, MAPS_PREFIX};
use std::path::Path;
use std::time::Duration;

const DEFAULT_FRONTEND: &str = "mla-tamagui";
const TIMEOUT: Duration = Duration::from_secs(20 * 60);

pub(crate) struct Published {
    pub(crate) code: i32,
    /// What the script printed, stderr first when there is any.
    pub(crate) text: String,
}

/// `EXPO_TOKEN` from the vault; the environment only where no vault is configured (a deployed `.env`).
pub(crate) fn token() -> Option<String> {
    secrets::ambient("EXPO_TOKEN").filter(|t| !t.is_empty()).or_else(|| secrets::load_values(&["EXPO_TOKEN"]).get("EXPO_TOKEN").cloned().flatten())
}

pub(crate) fn publish(root: &Path, frontend: &str, branch: &str, route: &str, message: &str) -> Published {
    publish_for(root, frontend, branch, route, message, None)
}

/// `platform` reaches the script only when given, so it keeps its own default (iOS) otherwise.
fn publish_for(root: &Path, frontend: &str, branch: &str, route: &str, message: &str, platform: Option<&str>) -> Published {
    let mut args = ["--filter", frontend, "run", "publish:eas", "--", "--initial-route", route, "--branch", branch, "--message", message].map(str::to_string).to_vec();
    if let Some(p) = platform {
        args.extend(["--platform".to_string(), p.to_string()]);
    }
    let out = wf_kit::proc::capture("pnpm", &args, Some(root), Some(TIMEOUT));
    let text = if out.stderr.is_empty() { out.stdout } else { out.stderr };
    Published { code: out.code, text }
}

const EXPO_PROJECT: &str = "https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates";

fn read_json(path: &Path) -> serde_json::Value {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}









/// The published update as one JSON line, also stored in the verification bucket beside its source maps (so a stack from the
/// device maps back to file:line) as the group's summary and, with `pointer`, as the branch's latest update the e2e tests load.
fn summarize(dist: &Path, branch: &str, route: &str, platform: Option<&str>, pointer: bool, memory: serde_json::Value) -> serde_json::Value {
    let commit = std::process::Command::new("git").args(["rev-parse", "HEAD"]).current_dir(dist).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let updates = read_json(&dist.join("eas-update.json"));
    let group = updates.as_array().and_then(|a| a.iter().find_map(|u| u["group"].as_str())).map(str::to_string);
    let mut platforms: Vec<String> = Vec::new();
    for p in updates.as_array().into_iter().flatten().filter_map(|u| u["platform"].as_str()) {
        if !platforms.iter().any(|q| q == p) {
            platforms.push(p.to_string());
        }
    }
    let export = read_json(&dist.join("eas-update-export.json"));
    let platform = export["platform"].as_str().or(platform);
    let mut summary = serde_json::json!({ "published": true, "branch": branch, "initialRoute": route, "commit": commit, "group": group, "platform": platform, "platforms": platforms,
        "env": export["env"], "hosts": export["hosts"], "memory": memory,
        "timing": { "exportS": export["exportS"], "uploadS": export["uploadS"], "servedS": null } });
    let Some(group) = group else {
        summary["maps"] = serde_json::json!({ "error": "eas update printed no group id (dist/eas-update.json)" });
        return summary;
    };
    summary["link"] = serde_json::json!(format!("{EXPO_PROJECT}/{group}"));
    // a device asks for its own platform: android when the group has it (the CI device), else what was published
    let asked = if platforms.is_empty() || platforms.iter().any(|p| p == "android") { "android" } else { platforms[0].as_str() };
    summary["served"] = served(dist.parent().unwrap_or(dist), &group, asked);
    summary["timing"]["servedS"] = summary["served"]["s"].clone();
    let mut maps = Vec::new();
    collect_maps(dist, dist, &mut maps);
    let uploaded = wf_bucket::artifacts::par_url().and_then(|par| {
        for rel in &maps {
            let body = std::fs::read(dist.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
            wf_bucket::artifacts::put_par(&format!("{par}{MAPS_PREFIX}/{group}/{rel}"), &body, "application/json")?;
        }
        Ok(maps.len())
    });
    let base = format!("{}/{MAPS_PREFIX}/{group}", wf_bucket::artifacts::public_base());
    summary["maps"] = match uploaded {
        Ok(n) => serde_json::json!({ "uploaded": n, "base": base, "files": maps }),
        Err(e) => serde_json::json!({ "error": e, "found": maps.len() }),
    };
    let mut keys = vec![format!("{MAPS_PREFIX}/{group}/summary.json")];
    if pointer {
        keys.push(latest_key(branch));
    }
    let body = summary.to_string();
    let stored = wf_bucket::artifacts::par_url().and_then(|par| keys.iter().try_for_each(|k| wf_bucket::artifacts::put_par(&format!("{par}{k}"), body.as_bytes(), "application/json")));
    let public = wf_bucket::artifacts::public_base();
    summary["pointers"] = match stored {
        Ok(()) => {
            let mut stored = serde_json::json!({ "summary": format!("{public}/{}", keys[0]) });
            if let Some(latest) = keys.get(1) {
                stored["latest"] = serde_json::json!(format!("{public}/{latest}"));
            }
            stored
        }
        Err(e) => serde_json::json!({ "error": e }),
    };
    summary
}



fn collect_maps(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_maps(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "map") {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

fn last_commit_subject(root: &Path) -> String {
    let out = std::process::Command::new("git").args(["log", "-1", "--format=%s"]).current_dir(root).stdin(std::process::Stdio::null()).output().ok();
    out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "wf ship eas".to_string())
}

/// `--latest`: the update CI's pointer names for the branch, as one JSON line whose `link` opens it in Expo Go.
fn latest(branch: &str) -> i32 {
    match latest_pointer(branch) {
        Ok(pointer) => {
            println!("{}", serde_json::json!({ "branch": branch, "commit": pointer["commit"], "group": pointer["group"], "platforms": pointer["platforms"], "link": pointer["link"] }));
            0
        }
        Err(error) => {
            let next = format!("no update of {branch} is stored yet: push an app change to dev (CI's eas-update job stores {}), or publish one by hand with `wf ship eas --initial-route <route>`", latest_key(branch));
            println!("{}", serde_json::json!({ "ok": false, "error": error, "next": next }));
            1
        }
    }
}

pub(crate) fn run(args: &[String]) -> Option<i32> {
    let p = parse(args, &["branch", "initial-route", "message", "frontend", "platform"], &["pointer", "latest"])?;
    if !p.positional.is_empty() {
        return None;
    }
    if p.flags.iter().any(|f| f == "latest") {
        if p.flags.iter().any(|f| f == "pointer") || ["initial-route", "message", "frontend", "platform"].iter().any(|k| p.opts.contains_key(*k)) {
            wf_kit::wf_err!("wf: --latest only reads the pointer: it takes --branch and nothing else");
            return Some(2);
        }
        return Some(latest(p.opts.get("branch").map_or("dev", String::as_str)));
    }
    let platform = p.opts.get("platform").map(String::as_str);
    if let Some(bad) = platform.filter(|p| !["all", "ios", "android"].contains(p)) {
        wf_kit::wf_err!("wf: --platform takes all, ios or android, not {bad:?}");
        return Some(2);
    }
    let pointer = p.flags.iter().any(|f| f == "pointer");
    let root = wf_kit::root::repo_root();
    let frontend = p.opts.get("frontend").map_or(DEFAULT_FRONTEND, String::as_str);
    let branch = p.opts.get("branch").cloned().unwrap_or_else(wf_kit::proc::current_branch);
    let route = p.opts.get("initial-route").map_or("/", String::as_str);
    let message = p.opts.get("message").cloned().unwrap_or_else(|| last_commit_subject(&root));
    let Some(token) = token() else {
        wf_kit::wf_err!("wf: EXPO_TOKEN is not resolvable (`wf secret get EXPO_TOKEN --backend bitwarden` must succeed)");
        return Some(1);
    };
    // the publish script reads the token from its environment
    std::env::set_var("EXPO_TOKEN", token);
    let sampler = wf_vmops::vm_sampler::start_from_env();
    let done = publish_for(&root, frontend, &branch, route, &message, platform);
    let vm = sampler.map(|s| s.finish());
    if done.code == 0 {
        let memory = serde_json::json!({ "peakRssMb": children_peak_rss_mb(), "vm": vm });
        let summary = summarize(&root.join("frontend").join(frontend).join("dist"), &branch, route, platform, pointer, memory);
        println!("{summary}");
        if pointer && summary["pointers"]["latest"].is_null() {
            let why = summary["pointers"]["error"].as_str().or_else(|| summary["maps"]["error"].as_str()).unwrap_or("not stored");
            wf_kit::wf_err!("wf: the update is published, but {} still names an earlier one: {why}", latest_key(&branch));
            return Some(1);
        }
        return Some(0);
    }
    let tail: String = done.text.trim().chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
    wf_kit::wf_err!("wf: eas update failed: exit {}: {tail}", done.code);
    Some(1)
}
