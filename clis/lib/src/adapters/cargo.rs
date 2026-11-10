//! `cargo package` and `cargo metadata` for one crate: the `.crate` bytes and what the index records about the version.
use crate::domain::index::{Dep, Entry, CRATES_IO_INDEX};
use crate::ports::Failure;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct Packaged {
    pub bytes: Vec<u8>,
    pub entry: Entry,
}

fn run(args: &[&str], scratch: &Path) -> Result<String, Failure> {
    let out = Command::new("cargo").args(args).env("CARGO_TARGET_DIR", scratch).stdin(Stdio::null()).output().map_err(|e| Failure::new(format!("cargo {}: {e}", args[0])))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let said = String::from_utf8_lossy(&out.stderr);
    let tail: Vec<&str> = said.lines().rev().take(12).collect();
    Err(Failure::new(format!("cargo {} failed: {}", args[0], tail.into_iter().rev().collect::<Vec<_>>().join(" | "))).next("fix what cargo reports; the manifest version was restored"))
}

/// Metadata and a `.crate` for the crate whose manifest is `manifest`. `--allow-dirty` because the release has just bumped it.
pub fn package(manifest: &Path, scratch: &Path) -> Result<Packaged, Failure> {
    let manifest_arg = manifest.to_string_lossy();
    let meta = run(&["metadata", "--no-deps", "--format-version", "1", "--manifest-path", &manifest_arg], scratch)?;
    let meta: Value = serde_json::from_str(&meta).map_err(|e| Failure::new(format!("cargo metadata: {e}")))?;
    let canonical = manifest.canonicalize().unwrap_or_else(|_| manifest.to_path_buf());
    let package = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["manifest_path"].as_str().is_some_and(|m| Path::new(m) == canonical))
        .ok_or_else(|| Failure::new(format!("cargo metadata lists no package at {}", manifest.display())))?;
    let (name, vers) = (package["name"].as_str().unwrap_or_default().to_string(), package["version"].as_str().unwrap_or_default().to_string());
    run(&["package", "--allow-dirty", "--quiet", "--manifest-path", &manifest_arg], scratch)?;
    let file = scratch.join("package").join(format!("{name}-{vers}.crate"));
    let bytes = std::fs::read(&file).map_err(|e| Failure::new(format!("{}: {e}", file.display())))?;
    let features: BTreeMap<String, Vec<String>> = package["features"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| (k.clone(), v.as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_string)).collect()))
        .collect();
    let deps = package["dependencies"].as_array().into_iter().flatten().filter(|d| d["kind"] != "dev").map(dep).collect();
    let entry = Entry {
        name,
        vers,
        deps,
        cksum: hex::encode(Sha256::digest(&bytes)),
        features,
        links: package["links"].as_str().map(str::to_string),
        rust_version: package["rust_version"].as_str().map(str::to_string),
    };
    Ok(Packaged { bytes, entry })
}

/// A dependency with no `registry` key is, once packaged, a crates.io one; any other registry is read as this one.
fn dep(d: &Value) -> Dep {
    let real = d["name"].as_str().unwrap_or_default().to_string();
    let renamed = d["rename"].as_str().map(str::to_string);
    Dep {
        name: renamed.clone().unwrap_or_else(|| real.clone()),
        req: d["req"].as_str().unwrap_or("*").to_string(),
        features: d["features"].as_array().into_iter().flatten().filter_map(|f| f.as_str().map(str::to_string)).collect(),
        optional: d["optional"].as_bool().unwrap_or(false),
        default_features: d["uses_default_features"].as_bool().unwrap_or(true),
        target: d["target"].as_str().map(str::to_string),
        kind: if d["kind"] == "build" { "build" } else { "normal" }.to_string(),
        registry: if d["registry"].is_null() { Some(CRATES_IO_INDEX.to_string()) } else { None },
        package: renamed.map(|_| real),
    }
}
