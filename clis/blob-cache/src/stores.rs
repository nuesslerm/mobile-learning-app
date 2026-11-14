//! Which blob stores a role uses, chosen in config. A value names one store by URL scheme (`par+https://`, `dav+https://`,
//! `s3://`, `file://`); a comma list names several, and every one is a real member: reads try them in order and return the
//! first hit, writes go to all of them. It is not a fallback chain: a declared store that cannot answer is an error that names it.
//! A store written with a leading `-` is disabled, which is the same as not declared. Credentials never go in a URL: an S3 pair
//! and a bearer token come as separate values. Pure parsing here, the adapters are built from `adapters`.
use crate::adapters::{Auth, DirStore, HttpStore, S3Location, Side};
use crate::domain::Key;
use crate::ports::{BlobStore, PutOutcome, StoreError};
use cli_http::Via;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const S3_READ_CREDENTIAL: &str = "BLOB_S3_READ_CREDENTIAL";
pub const S3_WRITE_CREDENTIAL: &str = "BLOB_S3_WRITE_CREDENTIAL";
pub const DAV_READ_TOKEN: &str = "BLOB_DAV_READ_TOKEN";
pub const DAV_WRITE_TOKEN: &str = "BLOB_DAV_WRITE_TOKEN";

/// The config keys of one role: the store list of each plane, and the legacy single-link key each list falls back to.
pub struct Role {
    pub name: &'static str,
    pub stores: [&'static str; 2],
    pub legacy: [&'static str; 2],
}

pub const BUILD_CACHE: Role = Role {
    name: "build-cache",
    stores: ["BUILD_CACHE_READ_STORES", "BUILD_CACHE_WRITE_STORES"],
    legacy: ["BUILD_CACHE_READ_PAR_URL", "BUILD_CACHE_WRITE_PAR_URL"],
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Plane {
    Read,
    Write,
}

impl Plane {
    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        if self == Plane::Read { "read" } else { "write" }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// A key the role needs does not resolve; each item names the key and what else would do.
    Missing(Vec<String>),
    /// A value is set and wrong. The text never repeats a value: it can hold a link.
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing(keys) => write!(f, "{} does not resolve", keys.join(" and ")),
            ConfigError::Invalid(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Every key `open` may read for this plane, for a caller that loads values from the environment and the vault in one call.
pub fn keys(role: &Role, write: bool) -> Vec<&'static str> {
    let mut keys = vec![role.stores[0], role.legacy[0], S3_READ_CREDENTIAL, DAV_READ_TOKEN];
    if write {
        keys.extend([role.stores[1], role.legacy[1], S3_WRITE_CREDENTIAL, DAV_WRITE_TOKEN]);
    }
    keys
}

#[derive(Clone, PartialEq, Eq, Debug)]
enum Spec {
    Par(String),
    Dav(String),
    S3 { bucket: String, prefix: String, endpoint: String, region: String, path_style: bool, conditional: bool },
    File(PathBuf),
}

impl Spec {
    fn scheme(&self) -> &'static str {
        match self {
            Spec::Par(_) => "par",
            Spec::Dav(_) => "dav",
            Spec::S3 { .. } => "s3",
            Spec::File(_) => "file",
        }
    }

    /// Where the store is, with no path, query or credential: what an error may say.
    fn label(&self) -> String {
        match self {
            Spec::Par(url) => format!("par+{}", origin(url)),
            Spec::Dav(url) => format!("dav+{}", origin(url)),
            Spec::S3 { bucket, endpoint, .. } => format!("s3://{bucket}@{}", authority(endpoint)),
            Spec::File(dir) => format!("file://{}", dir.display()),
        }
    }
}

fn authority(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?']).next().unwrap_or_default()
}

fn origin(url: &str) -> String {
    let scheme = url.split_once("://").map_or("https", |(scheme, _)| scheme);
    format!("{scheme}://{}", authority(url))
}

fn loopback(url: &str) -> bool {
    let host = authority(url);
    let host = host.strip_prefix('[').and_then(|h| h.split(']').next()).unwrap_or_else(|| host.rsplit_once(':').map_or(host, |(h, _)| h));
    host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// https, or plain http on this machine (the in-process doubles): a token or a signature never crosses a network in the clear.
fn transport_ok(url: &str) -> Result<(), String> {
    match url.split_once("://") {
        Some(("https", _)) => Ok(()),
        Some(("http", _)) if loopback(url) => Ok(()),
        Some(("http", _)) => Err("http is only accepted for a loopback host".into()),
        _ => Err("the address must start with https://".into()),
    }
}

fn percent_decode(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or("a bad %-escape in a query value")?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "a query value is not UTF-8".into())
}

fn no_userinfo(url: &str) -> Result<(), String> {
    if authority(url).contains('@') {
        return Err("a credential in the URL is refused: give it as a separate value".into());
    }
    Ok(())
}

fn parse_spec(raw: &str) -> Result<Spec, String> {
    let Some((scheme, rest)) = raw.split_once("://") else { return Err("no scheme (par+https://, dav+https://, s3://, file://)".into()) };
    match scheme {
        "par+https" | "par+http" => {
            let url = format!("{}://{rest}", &scheme[4..]);
            transport_ok(&url)?;
            no_userinfo(&url)?;
            Ok(Spec::Par(url))
        }
        "dav+https" | "dav+http" => {
            let url = format!("{}://{rest}", &scheme[4..]);
            transport_ok(&url)?;
            no_userinfo(&url)?;
            if rest.contains('?') {
                return Err("a dav address takes no query".into());
            }
            Ok(Spec::Dav(url))
        }
        "file" => {
            let dir = rest.split('?').next().unwrap_or_default();
            if !dir.starts_with('/') || dir.len() < 2 {
                return Err("a file address is file:///absolute/dir".into());
            }
            Ok(Spec::File(PathBuf::from(dir)))
        }
        "s3" => parse_s3(rest),
        other if !other.is_empty() && other.len() <= 16 && other.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)) => Err(format!("unknown scheme {other}")),
        _ => Err("unknown scheme".into()),
    }
}

fn parse_s3(rest: &str) -> Result<Spec, String> {
    // a secret key holds slashes, so the whole address is checked, not just its authority
    if rest.split('?').next().unwrap_or_default().contains('@') {
        return Err("a credential in the URL is refused: give the S3 key pair as a separate value".into());
    }
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (bucket, prefix) = path.split_once('/').unwrap_or((path, ""));
    if bucket.is_empty() {
        return Err("an s3 address is s3://bucket/prefix?endpoint=https://host".into());
    }
    let (mut endpoint, mut region, mut path_style, mut conditional) = (None, "auto".to_string(), false, false);
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let flag = |v: &str| match v {
            "1" | "true" => Ok(true),
            "0" | "false" => Ok(false),
            _ => Err(format!("{name} is 1 or 0")),
        };
        match name {
            "endpoint" => endpoint = Some(percent_decode(value)?),
            "region" => region = percent_decode(value)?,
            "path_style" => path_style = flag(value)?,
            "conditional" => conditional = flag(value)?,
            name if name.len() <= 24 && name.chars().all(|c| c.is_ascii_lowercase() || c == '_') => return Err(format!("unknown option {name} (endpoint, region, path_style, conditional)")),
            _ => return Err("unknown option".into()),
        }
    }
    let endpoint = endpoint.filter(|e| !e.is_empty()).ok_or("an s3 address needs ?endpoint=https://host")?;
    transport_ok(&endpoint)?;
    no_userinfo(&endpoint)?;
    if region.is_empty() {
        return Err("region is empty".into());
    }
    Ok(Spec::S3 { bucket: bucket.to_string(), prefix: prefix.to_string(), endpoint, region, path_style, conditional })
}

/// One item of a store list: `enabled` is false for an entry written with a leading `-`.
struct Item {
    enabled: bool,
    spec: Spec,
}

fn parse_list(list: &str, plane: Plane) -> Result<Vec<Item>, ConfigError> {
    let mut items = Vec::new();
    for (i, entry) in list.split(',').map(str::trim).filter(|e| !e.is_empty()).enumerate() {
        let (enabled, raw) = match entry.strip_prefix('-') {
            Some(rest) => (false, rest.trim_start()),
            None => (true, entry),
        };
        let spec = parse_spec(raw).map_err(|why| ConfigError::Invalid(format!("the {} store list, entry {}: {why}", plane.name(), i + 1)))?;
        items.push(Item { enabled, spec });
    }
    Ok(items)
}

fn nonempty(lookup: &dyn Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    lookup(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// The plane's list from `role.stores`, else the legacy link as a one-element `par+https` list, else `None`.
fn items_of(role: &Role, plane: Plane, lookup: &dyn Fn(&str) -> Option<String>) -> Result<Option<Vec<Item>>, ConfigError> {
    let i = plane.index();
    if let Some(list) = nonempty(lookup, role.stores[i]) {
        return parse_list(&list, plane).map(Some);
    }
    Ok(nonempty(lookup, role.legacy[i]).map(|url| vec![Item { enabled: true, spec: Spec::Par(url) }]))
}

fn missing(role: &Role, plane: Plane) -> ConfigError {
    let i = plane.index();
    ConfigError::Missing(vec![format!("{} (or {})", role.legacy[i], role.stores[i])])
}

fn s3_pair(lookup: &dyn Fn(&str) -> Option<String>, plane: Plane, index: usize) -> Result<(String, String), ConfigError> {
    let key = if plane == Plane::Read { S3_READ_CREDENTIAL } else { S3_WRITE_CREDENTIAL };
    let value = nonempty(lookup, key).ok_or_else(|| ConfigError::Missing(vec![key.to_string()]))?;
    match value.split_once(':') {
        Some((access, secret)) if !access.is_empty() && !secret.is_empty() => Ok((access.to_string(), secret.to_string())),
        _ => Err(ConfigError::Invalid(format!("{key} is not ACCESS:SECRET (store {})", index + 1))),
    }
}

struct Member {
    label: String,
    store: Box<dyn BlobStore>,
}

/// The stores of one role and plane pair, used as one `BlobStore`.
#[derive(Clone)]
pub struct Stores {
    members: Arc<Vec<Member>>,
}

impl fmt::Debug for Stores {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.labels()).finish()
    }
}

impl Stores {
    /// Where each store is, as an error would say it.
    pub fn labels(&self) -> Vec<String> {
        self.members.iter().map(|m| m.label.clone()).collect()
    }

    /// Each store with its label, for a check that must reach every one (a plain read stops at the first hit).
    pub fn members(&self) -> impl Iterator<Item = (&str, &dyn BlobStore)> {
        self.members.iter().map(|m| (m.label.as_str(), m.store.as_ref()))
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// A single store as a one-member set, for a caller or a test that already holds one.
    pub fn single(label: &str, store: impl BlobStore + 'static) -> Stores {
        Stores { members: Arc::new(vec![Member { label: label.to_string(), store: Box::new(store) }]) }
    }

    fn named(label: &str, error: StoreError) -> StoreError {
        match error {
            StoreError::Unreachable(why) => StoreError::Unreachable(format!("store {label}: {why}")),
            StoreError::Credential(why) => StoreError::Credential(format!("store {label}: {why}")),
            StoreError::Rejected(why) => StoreError::Rejected(format!("store {label}: {why}")),
        }
    }
}

/// Builds the stores of `role`: `Ok(None)` when no store is declared (every entry disabled), which is a run with no shared cache.
/// With `write` the write plane is resolved too and its list must line up with the read list, entry for entry.
pub fn open(role: &Role, write: bool, lookup: &dyn Fn(&str) -> Option<String>, via: Via) -> Result<Option<Stores>, ConfigError> {
    let Some(reads) = items_of(role, Plane::Read, lookup)? else { return Err(missing(role, Plane::Read)) };
    let writes = if write {
        match items_of(role, Plane::Write, lookup)? {
            Some(items) => Some(items),
            None if reads.iter().any(|i| i.enabled) => return Err(missing(role, Plane::Write)),
            None => None,
        }
    } else {
        None
    };
    if let Some(writes) = &writes {
        let enabled = |items: &[Item]| items.iter().map(|i| i.enabled).collect::<Vec<_>>();
        if enabled(&reads) != enabled(writes) {
            return Err(ConfigError::Invalid(format!("the {} read list has {} entries and its write list {}, or they enable different stores: they must line up entry for entry", role.name, reads.len(), writes.len())));
        }
    }
    let mut members = Vec::new();
    for (i, read) in reads.iter().enumerate().filter(|(_, r)| r.enabled) {
        let write = writes.as_ref().map(|w| &w[i].spec);
        if let Some(write) = write.filter(|w| w.scheme() != read.spec.scheme()) {
            return Err(ConfigError::Invalid(format!("store {}: read is {} but write is {}", i + 1, read.spec.scheme(), write.scheme())));
        }
        members.push(Member { label: read.spec.label(), store: build(i, &read.spec, write, lookup, via.clone())? });
    }
    Ok((!members.is_empty()).then(|| Stores { members: Arc::new(members) }))
}

fn build(i: usize, read: &Spec, write: Option<&Spec>, lookup: &dyn Fn(&str) -> Option<String>, via: Via) -> Result<Box<dyn BlobStore>, ConfigError> {
    let token = |key: &str| nonempty(lookup, key).ok_or_else(|| ConfigError::Missing(vec![key.to_string()]));
    Ok(match (read, write) {
        (Spec::Par(r), w) => {
            let write = w.map(|w| if let Spec::Par(url) = w { Side::new(url, Auth::None) } else { unreachable!("schemes were checked") });
            Box::new(HttpStore::new(Side::new(r, Auth::None), write).via(via))
        }
        (Spec::Dav(r), w) => {
            let write = match w {
                Some(Spec::Dav(url)) => Some(Side::new(url, Auth::Bearer(token(DAV_WRITE_TOKEN)?))),
                _ => None,
            };
            Box::new(HttpStore::new(Side::new(r, Auth::Bearer(token(DAV_READ_TOKEN)?)), write).via(via))
        }
        (Spec::File(r), w) => {
            if w.is_some_and(|w| w != read) {
                return Err(ConfigError::Invalid(format!("store {}: a file store has one directory, the read and write lists name two", i + 1)));
            }
            Box::new(DirStore::new(r))
        }
        (read @ Spec::S3 { .. }, w) => {
            let side = |spec: &Spec, plane: Plane| -> Result<Side, ConfigError> {
                let Spec::S3 { bucket, prefix, endpoint, region, path_style, .. } = spec else { unreachable!("schemes were checked") };
                let (access, secret) = s3_pair(lookup, plane, i)?;
                Ok(Side::s3(&S3Location { endpoint, bucket, prefix, virtual_host: !path_style }, Auth::s3(&access, &secret, region)))
            };
            let conditional = matches!(w.unwrap_or(read), Spec::S3 { conditional: true, .. });
            let write = w.map(|w| side(w, Plane::Write)).transpose()?;
            Box::new(HttpStore::new(side(read, Plane::Read)?, write).conditional_put(conditional).via(via))
        }
    })
}

impl BlobStore for Stores {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>, StoreError> {
        for member in self.members.iter() {
            if let Some(body) = member.store.get(key).map_err(|e| Stores::named(&member.label, e))? {
                return Ok(Some(body));
            }
        }
        Ok(None)
    }

    fn exists(&self, key: &Key) -> Result<bool, StoreError> {
        for member in self.members.iter() {
            if member.store.exists(key).map_err(|e| Stores::named(&member.label, e))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn put(&self, key: &Key, body: &[u8], if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.write_all(|store| store.put(key, body, if_absent))
    }

    fn get_file(&self, key: &Key, dest: &Path) -> Result<bool, StoreError> {
        for member in self.members.iter() {
            if member.store.get_file(key, dest).map_err(|e| Stores::named(&member.label, e))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn put_file(&self, key: &Key, source: &Path, if_absent: bool) -> Result<PutOutcome, StoreError> {
        self.write_all(|store| store.put_file(key, source, if_absent))
    }

    fn requests_per_put(&self, if_absent: bool) -> u64 {
        self.members.iter().map(|m| m.store.requests_per_put(if_absent)).sum()
    }
}

impl Stores {
    /// Writes to every store, so one that is down does not stop the others from being filled; the first failure is the answer.
    fn write_all(&self, mut write: impl FnMut(&dyn BlobStore) -> Result<PutOutcome, StoreError>) -> Result<PutOutcome, StoreError> {
        let mut outcome = PutOutcome::Exists;
        let mut first: Option<StoreError> = None;
        for member in self.members.iter() {
            match write(member.store.as_ref()) {
                Ok(PutOutcome::Stored) => outcome = PutOutcome::Stored,
                Ok(PutOutcome::Exists) => {}
                Err(error) => {
                    first.get_or_insert(Stores::named(&member.label, error));
                }
            }
        }
        first.map_or(Ok(outcome), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_list_when_parsed_then_a_leading_dash_disables_and_a_credential_in_the_url_is_refused() {
        let items = parse_list("par+https://h/p/SECRET/o/, -s3://b/p?endpoint=https://e&region=r&path_style=1", Plane::Read).unwrap();
        assert_eq!(items.iter().map(|i| i.enabled).collect::<Vec<_>>(), [true, false]);
        let refused = parse_list("s3://AKIA:topsecret@b/p?endpoint=https://e", Plane::Read).err().unwrap().to_string();
        assert!(refused.contains("credential") && !refused.contains("topsecret") && !refused.contains("AKIA"), "{refused}");
        let query = parse_list("s3://b/p?endpoint=https://e&secret_key=topsecret", Plane::Read).err().unwrap().to_string();
        assert!(!query.contains("topsecret"), "{query}");
    }

    #[test]
    fn given_plain_http_when_the_host_is_not_loopback_then_it_is_refused() {
        assert!(parse_spec("dav+http://127.0.0.1:1/o/").is_ok());
        assert!(parse_spec("dav+http://box.example:1/o/").is_err());
        assert!(parse_spec("s3://b?endpoint=http://s3.example").is_err());
        assert!(parse_spec("dav+http://127.evil.example:1/o/").is_err());
        assert!(parse_spec("s3://b?endpoint=http://127.0.0.1.evil.example").is_err());
        assert!(parse_spec("dav+http://[::1]:1/o/").is_ok());
    }
}
