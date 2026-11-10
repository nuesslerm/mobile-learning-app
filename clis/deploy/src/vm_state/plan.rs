//! What `vm-apps.json` says to back up, as the caller reads it and hands it to agent-shell (which has no repository checkout).
use serde_json::{json, Value};

/// What the live repository may hold (restic's own count), as a multiple of the sum of the volumes' `maxBytes`: restic keeps each volume
/// once (deduplicated and compressed) plus 14 days of change, so every volume at its cap twice over is a ceiling a healthy repository stays
/// far under.
pub const REPOSITORY_FACTOR: u64 = 2;

/// What the bucket may hold, every object version included: the live repository plus the packs a prune deleted that the previous-version
/// lifecycle rule (one day, `vm-state.tf`) has not yet removed. A prune can delete at most what the repository held, so the allowance is
/// one more repository. The backup counts the bucket's versions and refuses to start a run that could take it past this.
pub const BUCKET_FACTOR: u64 = 4;

/// agent-shell's running `wf` lives here: a backup never reads it and a restore never writes over it (an executable in use cannot be replaced).
pub const WORKSPACE_NEVER: &str = "./home/wf-bin";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Volume {
    pub name: String,
    /// Where agent-shell mounts it.
    pub path: String,
    pub excludes: Vec<String>,
    pub max_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPlan {
    pub app: String,
    pub volumes: Vec<Volume>,
}

/// Where agent-shell mounts a volume: its own workspace at `/workspace`, any other project's under `/state/<app>/<volume>`
/// (`infra/oracle-cloud/agent-shell/docker-compose.prod.yml`).
pub fn mount_of(app: &str, volume: &str) -> String {
    if app == "agent-shell" && volume == "agent-workspace" {
        "/workspace".into()
    } else {
        format!("/state/{app}/{volume}")
    }
}

fn keep_wf_bin_out(path: &str, excludes: &mut Vec<String>) {
    if path == "/workspace" && !excludes.iter().any(|e| e == WORKSPACE_NEVER) {
        excludes.push(WORKSPACE_NEVER.into());
    }
}

/// The apps that declare `stateVolumes`, in file order.
pub fn from_apps(text: &str) -> Result<Vec<AppPlan>, String> {
    let listed: Value = serde_json::from_str(text).map_err(|e| format!("vm-apps.json is not JSON: {e}"))?;
    let mut plans = Vec::new();
    for entry in listed.as_array().ok_or("vm-apps.json is not a list")? {
        let app = entry["name"].as_str().ok_or("an entry of vm-apps.json has no name")?;
        let Some(volumes) = entry["stateVolumes"].as_array() else { continue };
        let mut found = Vec::new();
        for v in volumes {
            let name = v["name"].as_str().filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))).ok_or_else(|| format!("{app}: a stateVolumes entry needs a plain name"))?;
            let mut excludes: Vec<String> = v["excludes"].as_array().into_iter().flatten().filter_map(|e| e.as_str().map(String::from)).collect();
            let max_bytes = v["maxBytes"].as_u64().filter(|m| *m > 0).ok_or_else(|| format!("{app}/{name}: a stateVolumes entry declares its maxBytes (the size, measured before anything is read, above which the backup refuses the volume)"))?;
            keep_wf_bin_out(&mount_of(app, name), &mut excludes);
            found.push(Volume { name: name.into(), path: mount_of(app, name), excludes, max_bytes });
        }
        if !found.is_empty() {
            plans.push(AppPlan { app: app.into(), volumes: found });
        }
    }
    Ok(plans)
}

fn caps_sum(plans: &[AppPlan]) -> u64 {
    plans.iter().flat_map(|p| &p.volumes).map(|v| v.max_bytes).sum()
}

/// What the live repository may hold: `vmStateRepositoryBytes` of the committed budget, `vm_state_repository_bytes` of `vm-state.tf`.
pub fn repository_budget_bytes(plans: &[AppPlan]) -> u64 {
    REPOSITORY_FACTOR * caps_sum(plans)
}

/// What the bucket may hold, previous versions included: `vmStateBytes` of the committed budget, `vm_state_budget_bytes` of `vm-state.tf`.
pub fn bucket_budget_bytes(plans: &[AppPlan]) -> u64 {
    BUCKET_FACTOR * caps_sum(plans)
}

/// The most one backup of these volumes can add to the bucket: every volume at its cap, incompressible and changed entirely.
pub fn growth_bytes(plans: &[AppPlan]) -> u64 {
    caps_sum(plans)
}

/// A volume's excludes as restic reads them: `./x` is anchored at the volume's mount, a plain name matches at any depth.
pub fn restic_excludes(volume: &Volume) -> Vec<String> {
    volume.excludes.iter().map(|e| e.strip_prefix("./").map_or_else(|| e.clone(), |rest| format!("{}/{rest}", volume.path.trim_end_matches('/')))).collect()
}

pub fn to_json(plans: &[AppPlan]) -> Value {
    Value::Array(
        plans
            .iter()
            .map(|p| json!({ "app": p.app, "volumes": p.volumes.iter().map(|v| json!({ "name": v.name, "path": v.path, "excludes": v.excludes, "maxBytes": v.max_bytes })).collect::<Vec<_>>() }))
            .collect(),
    )
}

/// The plan as one hex word, so it travels in a shell line without quoting.
pub fn encode(plans: &[AppPlan]) -> String {
    hex::encode(to_json(plans).to_string())
}

pub fn decode(word: &str) -> Result<Vec<AppPlan>, String> {
    let bytes = hex::decode(word).map_err(|e| format!("--plan is not hex: {e}"))?;
    let listed: Value = serde_json::from_slice(&bytes).map_err(|e| format!("--plan is not JSON: {e}"))?;
    let mut plans = Vec::new();
    for p in listed.as_array().ok_or("--plan is not a list")? {
        let volumes = p["volumes"].as_array().into_iter().flatten();
        let volumes = volumes
            .map(|v| {
                let path: String = v["path"].as_str()?.into();
                let mut excludes: Vec<String> = v["excludes"].as_array()?.iter().filter_map(|e| e.as_str().map(String::from)).collect();
                keep_wf_bin_out(&path, &mut excludes);
                Some(Volume { name: v["name"].as_str()?.into(), path, excludes, max_bytes: v["maxBytes"].as_u64()? })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or("--plan has a malformed volume")?;
        plans.push(AppPlan { app: p["app"].as_str().ok_or("--plan has an app without a name")?.into(), volumes });
    }
    Ok(plans)
}
