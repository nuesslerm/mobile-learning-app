//! `libraries.json`: the one place that names the shared libraries and the registries they publish to and resolve from.
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Library {
    pub name: String,
    pub kind: String,
    pub path: String,
    /// The alias in `registries` this library is published to and consumers resolve it from.
    pub registry: String,
}

/// `kind` is cargo-sparse, crates-io, npm or npm-tarball. `url` may be `env:NAME`, a value read from that variable (the
/// environment, then the vault) because a pre-authenticated link is a credential. `publish` names the adapter, or is null.
/// `index_url` is the read proxy's sparse index base of a bucket registry (no secret, so Cargo.lock may record it); `url` and
/// `token_env` are then only the bucket's read link (a fallback for readers off the tailnet) and write link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    pub alias: String,
    pub kind: String,
    pub url: Option<String>,
    pub token_env: Option<String>,
    pub publish: Option<String>,
    pub index_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    pub registries: Vec<Registry>,
    pub libraries: Vec<Library>,
}

pub const REGISTRY_KINDS: [&str; 4] = ["cargo-sparse", "crates-io", "npm", "npm-tarball"];
pub const PUBLISHERS: [&str; 4] = ["bucket-sparse", "cargo-publish", "npm-publish", "npm-tarball"];

impl Catalog {
    pub fn registry(&self, alias: &str) -> Option<&Registry> {
        self.registries.iter().find(|r| r.alias == alias)
    }

    pub fn library(&self, name: &str) -> Option<&Library> {
        self.libraries.iter().find(|l| l.name == name)
    }
}

pub fn parse(text: &str) -> Result<Catalog, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| format!("libraries.json is not JSON: {error}"))?;
    let rows = |key: &str| value[key].as_array().cloned().ok_or_else(|| format!("libraries.json has no `{key}` array"));
    let text_of = |row: &Value, name: &str| row[name].as_str().map(str::to_string).ok_or_else(|| format!("libraries.json: an entry has no string `{name}`: {row}"));
    let optional = |row: &Value, name: &str| row[name].as_str().map(str::to_string);
    let registries = rows("registries")?
        .iter()
        .map(|row| {
            let registry = Registry { alias: text_of(row, "alias")?, kind: text_of(row, "kind")?, url: optional(row, "url"), token_env: optional(row, "tokenEnv"), publish: optional(row, "publish"), index_url: optional(row, "indexUrl") };
            let bad_publisher = registry.publish.as_deref().is_some_and(|p| !PUBLISHERS.contains(&p));
            if !REGISTRY_KINDS.contains(&registry.kind.as_str()) || bad_publisher {
                return Err(format!("libraries.json: registry {} has kind {:?} and publish {:?}; kind is one of {REGISTRY_KINDS:?}, publish one of {PUBLISHERS:?} or null", registry.alias, registry.kind, registry.publish));
            }
            Ok(registry)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let libraries = rows("libraries")?
        .iter()
        .map(|row| {
            let library = Library { name: text_of(row, "name")?, kind: text_of(row, "kind")?, path: text_of(row, "path")?, registry: text_of(row, "registry")? };
            if !["rust", "npm"].contains(&library.kind.as_str()) || !registries.iter().any(|r| r.alias == library.registry) {
                return Err(format!("libraries.json: {} has kind {:?} and registry {:?}; kind is rust or npm and the registry must be an alias in `registries`", library.name, library.kind, library.registry));
            }
            Ok(library)
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Catalog { registries, libraries })
}

/// The file with `name`'s registry alias replaced, every other entry kept as it was.
pub fn with_registry(text: &str, name: &str, alias: &str) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(text).map_err(|error| format!("libraries.json is not JSON: {error}"))?;
    let row = value["libraries"].as_array_mut().and_then(|rows| rows.iter_mut().find(|row| row["name"] == name)).ok_or_else(|| format!("{name} is not in libraries.json"))?;
    row["registry"] = json!(alias);
    Ok(serde_json::to_string_pretty(&value).map_err(|error| error.to_string())? + "\n")
}
