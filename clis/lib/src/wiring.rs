//! The composition root: which adapter serves which registry, and where the links come from (the environment, then the vault).
use crate::adapters::bucket_sparse::BucketSparse;
use crate::adapters::cargo_publish::CargoPublish;
use crate::adapters::index_source::PublicIndex;
use crate::domain::libraries::{Catalog, Registry};
use crate::ports::{Failure, IndexSource, Publisher};
use blob_cache::adapters::OciParStore;
use blob_cache::ports::BlobStore;

/// `${name_prefix}-libs-index` of `infra/oracle-cloud/build-cache.tf`: the public bucket of the index and the crates.
pub const LIBS_BUCKET: &str = "mobile-learning-libs-index";

pub fn resolve(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty()).or_else(|| secrets::load_values(&[name]).get(name).cloned().flatten())
}

/// A registry's `url`, with `env:NAME` read from that variable.
pub fn url_of(registry: &Registry) -> Result<Option<String>, Failure> {
    match registry.url.as_deref().map(|u| u.strip_prefix("env:").map_or(Ok(u.to_string()), |var| resolve(var).ok_or(var))) {
        None => Ok(None),
        Some(Ok(url)) => Ok(Some(url)),
        Some(Err(var)) => Err(Failure::new(format!("{var} does not resolve (the environment, then the vault)")).next(format!("set {var}; `wf secret set {var}` stores it"))),
    }
}

fn token_of(registry: &Registry) -> Result<Option<String>, Failure> {
    match registry.token_env.as_deref() {
        None => Ok(None),
        Some(var) => resolve(var).map(Some).ok_or_else(|| Failure::new(format!("{var} does not resolve (the environment, then the vault)")).next(format!("set {var}"))),
    }
}

/// The public libs bucket, index and crates alike: read at the registry's public `url`, written through the registry's token (its
/// write link) when `write`.
pub fn libs_store(registry: &Registry, write: bool) -> Result<Box<dyn BlobStore>, Failure> {
    let link = if write { token_of(registry)? } else { None };
    let read = url_of(registry).ok().flatten().or_else(|| link.clone()).ok_or_else(|| Failure::new(format!("registry {} has no url or write link", registry.alias)).next("set the registry's url in libraries.json, or LIBS_WRITE_PAR_URL"))?;
    Ok(Box::new(OciParStore::new(&read, link.as_deref())))
}

/// The registry's index as readers see it: a public GET at `indexUrl`.
pub fn index_source(registry: &Registry) -> Result<Box<dyn IndexSource>, Failure> {
    let url = registry.index_url.as_deref().ok_or_else(|| Failure::new(format!("registry {} has no indexUrl", registry.alias)).next("set indexUrl to the bucket's public `<base>/index/` in libraries.json"))?;
    Ok(Box::new(PublicIndex::new(Box::new(OciParStore::new(url, None)))))
}

pub fn publisher(registry: &Registry) -> Result<Box<dyn Publisher>, Failure> {
    match registry.publish.as_deref() {
        Some("bucket-sparse") => {
            let index_url = registry.index_url.as_deref().ok_or_else(|| Failure::new(format!("registry {} has no indexUrl", registry.alias)).next("set indexUrl to the bucket's public `<base>/index/` in libraries.json"))?;
            Ok(Box::new(BucketSparse::new(libs_store(registry, true)?, index_source(registry)?, index_url)?))
        }
        Some("cargo-publish") => {
            let token = token_of(registry)?.ok_or_else(|| Failure::new(format!("registry {} names no tokenEnv", registry.alias)))?;
            let index = if registry.kind == "crates-io" { None } else { url_of(registry)? };
            Ok(Box::new(CargoPublish::new(registry.clone(), index, token)))
        }
        Some(other) => Err(Failure::new(format!("the {other} publisher is not built yet (npm registries are a later slice)"))),
        None => Err(Failure::new(format!("registry {} is read-only: libraries.json gives it no publish adapter", registry.alias))),
    }
}

pub fn publisher_can_delete(id: &str) -> bool {
    id == "bucket-sparse"
}

pub fn first_bucket(catalog: &Catalog) -> Option<&Registry> {
    catalog.registries.iter().find(|r| r.publish.as_deref() == Some("bucket-sparse"))
}
