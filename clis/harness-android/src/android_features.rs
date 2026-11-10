//! The slim redroid's package catalog: every package of the stock image is core (always kept), part of a named feature
//! (kept only when asked for with `--with`) or removed. `android_features.json` is the one place to read what the
//! emulator can and cannot do; `wf android list-devices` shows it against a running device.
use serde_json::{json, Value};
use std::collections::HashSet;

const DATA: &str = include_str!("android_features.json");

pub(crate) struct Feature {
    pub name: String,
    pub what: String,
    pub packages: Vec<String>,
}

pub(crate) struct Catalog {
    pub core: Vec<String>,
    pub features: Vec<Feature>,
    pub removed: Vec<String>,
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(|s| s.as_str().map(str::to_string)).collect()
}

pub(crate) fn catalog() -> Catalog {
    let v: Value = serde_json::from_str(DATA).expect("android_features.json is valid JSON");
    Catalog {
        core: strings(&v["core"]),
        features: v["features"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| Feature { name: f["name"].as_str().unwrap_or("").to_string(), what: f["what"].as_str().unwrap_or("").to_string(), packages: strings(&f["packages"]) })
            .collect(),
        removed: strings(&v["removed"]),
    }
}

/// Splits `--with camera,files` into feature names, refusing a name the catalog does not know.
pub(crate) fn parse_with(raw: &str, cat: &Catalog) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for name in raw.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        if !cat.features.iter().any(|f| f.name == name) {
            let known: Vec<&str> = cat.features.iter().map(|f| f.name.as_str()).collect();
            return Err(format!("unknown feature '{name}' (known: {})", known.join(", ")));
        }
        out.push(name.to_string());
    }
    Ok(out)
}

/// (packages to disable, packages to enable) for a slim device with `with` features on. Core is never touched.
pub(crate) fn plan(cat: &Catalog, with: &[String]) -> (Vec<String>, Vec<String>) {
    let mut disable = cat.removed.clone();
    let mut enable = Vec::new();
    for f in &cat.features {
        if with.contains(&f.name) {
            enable.extend(f.packages.iter().cloned());
        } else {
            disable.extend(f.packages.iter().cloned());
        }
    }
    (disable, enable)
}

/// Every non-core package enabled, for `--stock`: undoes a slim device whose `/data` volume kept the disabled state.
pub(crate) fn restore_all(cat: &Catalog) -> Vec<String> {
    let mut all = cat.removed.clone();
    all.extend(cat.features.iter().flat_map(|f| f.packages.iter().cloned()));
    all
}

/// Each feature as `on` (all packages enabled), `off` (all disabled) or `partial`, from the device's disabled packages.
pub(crate) fn report(cat: &Catalog, disabled: &HashSet<String>) -> Value {
    let state = |pkgs: &[String]| {
        let off = pkgs.iter().filter(|p| disabled.contains(*p)).count();
        if off == 0 {
            "on"
        } else if off == pkgs.len() {
            "off"
        } else {
            "partial"
        }
    };
    let features: Vec<Value> = cat.features.iter().map(|f| json!({ "feature": f.name, "state": state(&f.packages), "what": f.what, "packages": f.packages.len() })).collect();
    json!({
        "core": { "state": state(&cat.core), "packages": cat.core.len() },
        "removed": { "state": state(&cat.removed), "packages": cat.removed.len() },
        "features": features,
        "disabledTotal": disabled.len(),
    })
}
