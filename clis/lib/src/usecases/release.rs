use crate::domain::libraries::Catalog;
use crate::domain::manifest::bump_version;
use crate::domain::version::Bump;
use crate::ports::{Failure, Published, Publisher, Release};
use serde_json::{json, Value};
use std::path::Path;

/// Bumps the library's manifest, hands it to the registry's publisher and, when that fails, puts the manifest back.
pub fn release(root: &Path, catalog: &Catalog, name: &str, how: Bump, publisher: &dyn Publisher) -> Result<Value, Failure> {
    let library = catalog.library(name).ok_or_else(|| Failure::new(format!("{name} is not in libraries.json")).next("add it there, or name one of the libraries `wf lib status` lists"))?;
    if library.kind != "rust" {
        return Err(Failure::new(format!("{name} is an npm library; only Rust libraries are released in this slice")));
    }
    let manifest = root.join(&library.path).join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| Failure::new(format!("{}: {e}", manifest.display())))?;
    let (previous, version, bumped) = bump_version(&text, how)?;
    if publisher.already_released(name, &version)? {
        return Err(Failure::new(format!("{name} {version} is already released; versions are immutable")).next(format!("set the manifest version to the latest released one, or release a higher bump than {}", how.name())));
    }
    std::fs::write(&manifest, &bumped).map_err(|e| Failure::new(format!("{}: {e}", manifest.display())))?;
    let published = publisher.publish(&Release { library, manifest: &manifest, version: &version }).inspect_err(|_| {
        let _ = std::fs::write(&manifest, &text);
    })?;
    let Published { objects, checksum, bytes } = published;
    Ok(json!({
        "ok": true,
        "crate": name,
        "previous": previous,
        "version": version,
        "bump": how.name(),
        "registry": library.registry,
        "publisher": publisher.id(),
        "objects": objects,
        "checksum": checksum,
        "bytes": bytes,
        "warnings": ["cargo-semver-checks is not run yet: a breaking change must be given as major by the caller"],
    }))
}
