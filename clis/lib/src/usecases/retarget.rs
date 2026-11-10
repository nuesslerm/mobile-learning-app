use crate::domain::libraries::{with_registry, Catalog, Registry};
use crate::domain::manifest::{retarget, with_registry_index, Target, CRATES_IO};
use crate::ports::{Checkout, Failure};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The sparse index a registry is read from, or the variable that carries it when it is a credential and so cannot be written down.
/// A public `indexUrl` is never a credential, so it wins over a secret `url`.
enum Index {
    Literal(String),
    Env(String),
}

fn index_of(registry: &Registry) -> Result<Index, Failure> {
    if let Some(public) = registry.index_url.as_deref() {
        return Ok(Index::Literal(if public.starts_with("sparse+") { public.to_string() } else { format!("sparse+{public}") }));
    }
    let url = registry.url.as_deref().ok_or_else(|| Failure::new(format!("registry {} has no url in libraries.json", registry.alias)))?;
    Ok(match url.strip_prefix("env:") {
        Some(var) => Index::Env(var.to_string()),
        None if url.starts_with("sparse+") || registry.kind != "cargo-sparse" => Index::Literal(url.to_string()),
        None => Index::Literal(format!("sparse+{url}")),
    })
}

/// The nearest directory at or above the manifest whose Cargo.toml has a `[workspace]`, else the manifest's own.
fn workspace_dir(root: &Path, manifest: &str) -> PathBuf {
    let mut dir = root.join(manifest).parent().map(Path::to_path_buf).unwrap_or_else(|| root.to_path_buf());
    let own = dir.clone();
    loop {
        let declares = std::fs::read_to_string(dir.join("Cargo.toml")).ok().and_then(|t| t.parse::<toml::Table>().ok()).is_some_and(|t| t.contains_key("workspace"));
        if declares {
            return dir;
        }
        if dir == root || !dir.pop() {
            return own;
        }
    }
}

/// Moves every consumer's dependency line on `name` to `alias`, regenerates the `[registries]` entries of the workspaces that changed
/// and records the alias in `libraries.json`.
pub fn retarget_library(root: &Path, catalog: &Catalog, checkout: &dyn Checkout, name: &str, alias: &str) -> Result<Value, Failure> {
    let library = catalog.library(name).ok_or_else(|| Failure::new(format!("{name} is not in libraries.json")))?;
    if library.kind != "rust" {
        return Err(Failure::new(format!("{name} is an npm library; only Rust libraries are retargeted in this slice")));
    }
    let registry = catalog.registry(alias).ok_or_else(|| Failure::new(format!("{alias} is not in the registries of libraries.json")).next(format!("add it there; the aliases are {:?}", catalog.registries.iter().map(|r| &r.alias).collect::<Vec<_>>())))?;
    let target = if registry.kind == "crates-io" || alias == CRATES_IO { Target::CratesIo } else { Target::Alias(alias.to_string()) };
    let own = format!("{}/Cargo.toml", library.path);
    let (mut changed, mut skipped, mut workspaces) = (Vec::new(), Vec::new(), BTreeSet::new());
    for (path, text) in checkout.files("Cargo.toml")?.into_iter().filter(|(p, _)| *p != own) {
        let done = retarget(&text, name, &target).map_err(|e| Failure::new(format!("{path}: {e}")))?;
        skipped.extend(done.skipped.iter().map(|s| json!({ "manifest": path, "table": s.table, "why": s.why })));
        if done.changed.is_empty() {
            continue;
        }
        std::fs::write(root.join(&path), &done.text).map_err(|e| Failure::new(format!("{path}: {e}")))?;
        changed.extend(done.changed.iter().map(|c| json!({ "manifest": path, "table": c.table, "from": c.from, "to": c.to })));
        workspaces.insert(workspace_dir(root, &path));
    }
    let mut configs = Vec::new();
    if target != Target::CratesIo {
        let index = index_of(registry)?;
        for dir in workspaces {
            let shown = dir.strip_prefix(root).unwrap_or(&dir).display().to_string();
            match &index {
                Index::Env(var) => configs.push(json!({ "workspace": shown, "alias": alias, "written": false, "why": format!("the index is a credential: build with CARGO_REGISTRIES_{}_INDEX=sparse+${var}", alias.to_ascii_uppercase().replace('-', "_")) })),
                Index::Literal(url) => {
                    let file = dir.join(".cargo/config.toml");
                    let current = std::fs::read_to_string(&file).unwrap_or_default();
                    let text = with_registry_index(&current, alias, url).map_err(|e| Failure::new(format!("{}: {e}", file.display())))?;
                    std::fs::create_dir_all(dir.join(".cargo")).map_err(|e| Failure::new(e.to_string()))?;
                    std::fs::write(&file, text).map_err(|e| Failure::new(format!("{}: {e}", file.display())))?;
                    configs.push(json!({ "workspace": shown, "alias": alias, "written": true, "file": format!("{}{}.cargo/config.toml", shown, if shown.is_empty() { "" } else { "/" }) }));
                }
            }
        }
    }
    let file = root.join("libraries.json");
    let text = std::fs::read_to_string(&file).map_err(|e| Failure::new(format!("libraries.json: {e}")))?;
    std::fs::write(&file, with_registry(&text, name, alias)?).map_err(|e| Failure::new(format!("libraries.json: {e}")))?;
    Ok(json!({ "ok": true, "library": name, "registry": alias, "previous": library.registry, "changed": changed, "skipped": skipped, "configs": configs }))
}
