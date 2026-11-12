//! Cargo's sparse registry layout, exactly: `config.json` at the root, one file of JSON lines per crate at a path made of its name,
//! and the `.crate` files where `config.json`'s `dl` says. In the public libs bucket the index lives under `index/` and the crates under
//! `crates/`, both readable by anyone and nothing else in the bucket. https://doc.rust-lang.org/cargo/reference/registry-index.html
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// Where `config.json` is stored in the bucket.
pub const CONFIG: &str = "index/config.json";
pub const CRATES_IO_INDEX: &str = "https://github.com/rust-lang/crates.io-index";

/// A dependency as the index records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dep {
    /// The name the manifest uses (the rename, when there is one).
    pub name: String,
    pub req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    /// `normal` or `build`; dev dependencies are not recorded.
    pub kind: String,
    /// `Some(index url)` for a registry other than this one; `None` for this one.
    pub registry: Option<String>,
    /// The package's real name when `name` is a rename.
    pub package: Option<String>,
}

/// What a release records about one version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub vers: String,
    pub deps: Vec<Dep>,
    pub cksum: String,
    pub features: BTreeMap<String, Vec<String>>,
    pub links: Option<String>,
    pub rust_version: Option<String>,
}

/// `1/a`, `2/ab`, `3/a/abc`, `se/rv/service-kit`: where a crate's file lives under the index root.
pub fn path(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    match lower.len() {
        0 => String::new(),
        1 => format!("1/{lower}"),
        2 => format!("2/{lower}"),
        3 => format!("3/{}/{lower}", &lower[..1]),
        _ => format!("{}/{}/{lower}", &lower[..2], &lower[2..4]),
    }
}

/// The storage key of a sparse path (`index/se/rv/service-kit`).
pub fn key(sparse_path: &str) -> String {
    format!("index/{sparse_path}")
}

pub fn crate_path(name: &str, version: &str) -> String {
    format!("crates/{name}/{version}.crate")
}

/// The bucket's public base (`https://objectstorage.<region>.oraclecloud.com/n/<ns>/b/<bucket>/o`) from the registry's `indexUrl`
/// (`[sparse+]<base>/index/`).
pub fn public_base(index_url: &str) -> Result<String, String> {
    let url = index_url.strip_prefix("sparse+").unwrap_or(index_url).trim_end_matches('/');
    match url.strip_suffix("/index") {
        Some(base) if base.starts_with("http") => Ok(base.to_string()),
        _ => Err(format!("indexUrl {index_url:?} is not `<public bucket base>/index/`")),
    }
}

/// `config.json`: downloads come straight from the public bucket, `<base>/crates/<name>/<version>.crate`.
pub fn config(index_url: &str) -> Result<String, String> {
    let base = public_base(index_url)?;
    Ok(format!("{}\n", json!({ "dl": format!("{base}/crates/{{crate}}/{{version}}.crate") })))
}

fn dep_json(dep: &Dep) -> Value {
    json!({
        "name": dep.name,
        "req": dep.req,
        "features": dep.features,
        "optional": dep.optional,
        "default_features": dep.default_features,
        "target": dep.target,
        "kind": dep.kind,
        "registry": dep.registry,
        "package": dep.package,
    })
}

/// The index line of one version. A feature that uses `dep:` or `?/` goes in `features2` with `v: 2`, as Cargo writes it.
pub fn line(entry: &Entry) -> String {
    let modern = |values: &[String]| values.iter().any(|v| v.starts_with("dep:") || v.contains("?/"));
    let (mut plain, mut extended) = (Map::new(), Map::new());
    for (name, values) in &entry.features {
        let target = if modern(values) { &mut extended } else { &mut plain };
        target.insert(name.clone(), json!(values));
    }
    let mut object = Map::new();
    object.insert("name".into(), json!(entry.name));
    object.insert("vers".into(), json!(entry.vers));
    object.insert("deps".into(), Value::Array(entry.deps.iter().map(dep_json).collect()));
    object.insert("cksum".into(), json!(entry.cksum));
    object.insert("features".into(), Value::Object(plain));
    object.insert("yanked".into(), json!(false));
    if let Some(links) = &entry.links {
        object.insert("links".into(), json!(links));
    }
    if !extended.is_empty() {
        object.insert("features2".into(), Value::Object(extended));
        object.insert("v".into(), json!(2));
    }
    if let Some(rust_version) = &entry.rust_version {
        object.insert("rust_version".into(), json!(rust_version));
    }
    Value::Object(object).to_string()
}

fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines().filter(|l| !l.trim().is_empty())
}

/// The versions a crate's file lists, with whether each is yanked.
pub fn versions(text: &str) -> Vec<(String, bool)> {
    lines(text)
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| Some((v["vers"].as_str()?.to_string(), v["yanked"].as_bool().unwrap_or(false))))
        .collect()
}

/// `text` with the entry's line added at the end; a version already listed is an error, never a second line.
pub fn append(text: &str, entry: &Entry) -> Result<String, String> {
    if versions(text).iter().any(|(v, _)| *v == entry.vers) {
        return Err(format!("{} {} is already in the index", entry.name, entry.vers));
    }
    let mut out: String = lines(text).map(|l| format!("{l}\n")).collect();
    out.push_str(&line(entry));
    out.push('\n');
    Ok(out)
}

/// `text` with the given versions yanked: still resolvable for a lockfile that names them, never chosen by a new resolution.
pub fn yank(text: &str, doomed: &[String]) -> String {
    lines(text)
        .map(|l| match serde_json::from_str::<Value>(l) {
            Ok(mut v) if v["vers"].as_str().is_some_and(|vers| doomed.iter().any(|d| d == vers)) => {
                v["yanked"] = json!(true);
                format!("{v}\n")
            }
            _ => format!("{l}\n"),
        })
        .collect()
}
