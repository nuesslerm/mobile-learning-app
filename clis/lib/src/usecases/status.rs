use crate::domain::index;
use crate::domain::libraries::{Catalog, Library};
use crate::domain::lockfile::{pins, Source};
use crate::domain::version::{latest, newer};
use crate::ports::{Checkout, Failure, IndexSource};
use serde_json::{json, Value};

/// Per library: what the registry holds and what each lockfile of the checkout pins. `source` is the registry's index through the read proxy, `None`
/// when none can be built; a registry this slice cannot read gets `latest: null` and a note.
pub fn status(catalog: &Catalog, checkout: &dyn Checkout, source: Option<&dyn IndexSource>) -> Result<Value, Failure> {
    let locks = checkout.files("Cargo.lock")?;
    let parsed: Vec<(String, Vec<_>)> = locks.iter().map(|(path, text)| pins(text).map(|p| (path.clone(), p)).map_err(|e| Failure::new(format!("{path}: {e}")))).collect::<Result<_, _>>()?;
    let mut store_error: Option<String> = None;
    let libraries: Vec<Value> = catalog
        .libraries
        .iter()
        .map(|library| {
            let registry = catalog.registry(&library.registry);
            let readable = registry.is_some_and(|r| r.publish.as_deref() == Some("bucket-sparse"));
            let (released, note) = if library.kind != "rust" {
                (Vec::new(), Some("npm libraries are not read by status yet".to_string()))
            } else if !readable {
                (Vec::new(), Some(format!("registry {} is not read by status", library.registry)))
            } else {
                match source.map(|s| released(s, &library.name)) {
                    Some(Ok(found)) => (found, None),
                    Some(Err(why)) => {
                        store_error.get_or_insert(why.clone());
                        (Vec::new(), Some(why))
                    }
                    None => {
                        let why = "no index source can be built (the registry needs an indexUrl)".to_string();
                        store_error.get_or_insert(why.clone());
                        (Vec::new(), Some(why))
                    }
                }
            };
            let live: Vec<&str> = released.iter().filter(|(_, yanked)| !yanked).map(|(v, _)| v.as_str()).collect();
            let newest = latest(live.iter().copied());
            json!({
                "name": library.name,
                "kind": library.kind,
                "path": library.path,
                "registry": library.registry,
                "latest": newest,
                "released": released.iter().map(|(v, _)| v).collect::<Vec<_>>(),
                "note": note,
                "pins": pins_of(library, &parsed, &live),
            })
        })
        .collect();
    Ok(json!({ "ok": store_error.is_none(), "error": store_error, "libraries": libraries }))
}

fn released(source: &dyn IndexSource, name: &str) -> Result<Vec<(String, bool)>, String> {
    let text = source.file(&index::path(name)).map_err(|f| f.error)?;
    Ok(text.map(|t| index::versions(&t)).unwrap_or_default())
}

/// A lockfile in the library's own directory (its own workspace) is the library, not a consumer.
fn pins_of(library: &Library, locks: &[(String, Vec<crate::domain::lockfile::Pin>)], live: &[&str]) -> Vec<Value> {
    let mut rows = Vec::new();
    for (path, found) in locks {
        let dir = path.strip_suffix("Cargo.lock").unwrap_or("").trim_end_matches('/');
        if !dir.is_empty() && (dir == library.path || dir.starts_with(&format!("{}/", library.path))) {
            continue;
        }
        for pin in found.iter().filter(|p| p.name == library.name) {
            let behind = (pin.source == Source::Mla).then(|| live.iter().filter(|v| newer(v, &pin.version)).count());
            rows.push(json!({ "service": if dir.is_empty() { "." } else { dir }, "lockfile": path, "version": pin.version, "source": pin.source.name(), "behind": behind }));
        }
    }
    rows
}
