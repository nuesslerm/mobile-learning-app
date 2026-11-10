//! The manifest a bundle's `manifest.json` holds, checked as the pinned `MobileWebBundleManifestSchema` checks it.
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const MAX_ASSETS: usize = 256;
const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_ASSET_BYTES: u64 = 10 * 1024 * 1024;
const ENTRYPOINT: &str = "index.html";

#[derive(Debug, Clone)]
pub(crate) struct Asset {
    pub path: String,
    pub sha256: String,
    pub byte_length: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct Manifest {
    pub raw: Value,
    pub build_id: String,
    pub assets: Vec<Asset>,
}

pub(crate) fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Segments of `[A-Za-z0-9._-]`, never `.`/`..`, a trailing dot or a Windows device name: the names every platform keeps.
pub(crate) fn is_asset_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 255 {
        return false;
    }
    path.split('/').all(|seg| {
        let device = {
            let stem = seg.split('.').next().unwrap_or("").to_ascii_lowercase();
            matches!(stem.as_str(), "con" | "prn" | "aux" | "nul") || (stem.len() == 4 && (stem.starts_with("com") || stem.starts_with("lpt")) && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        };
        !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) && seg != "." && seg != ".." && !seg.ends_with('.') && !device
    })
}

fn is_content_type(t: &str) -> bool {
    let (essence, charset) = match t.split_once("; charset=") {
        Some((e, c)) => (e, Some(c)),
        None => (t, None),
    };
    let token = |s: &str| !s.is_empty() && s.as_bytes()[0].is_ascii_alphanumeric() && s.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'+' | b'-'));
    t.len() <= 128
        && essence.split_once('/').is_some_and(|(a, b)| token(a) && token(b))
        && charset.is_none_or(|c| !c.is_empty() && c.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-')))
}

/// The id the phone recomputes: the sha-256 of the path-sorted assets, fixed key order, no whitespace.
fn build_id(assets: &[Asset], types: &[String]) -> String {
    let mut rows: Vec<(&Asset, &String)> = assets.iter().zip(types).collect();
    rows.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    let list: Vec<Value> = rows.iter().map(|(a, t)| json!({"path": a.path, "sha256": a.sha256, "byteLength": a.byte_length, "contentType": t})).collect();
    Sha256::digest(Value::Array(list).to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn exact_keys(o: &Map<String, Value>, required: &[&str], optional: &[&str]) -> Result<(), String> {
    if let Some(k) = required.iter().find(|k| !o.contains_key(**k)) {
        return Err(format!("`{k}` is missing"));
    }
    match o.keys().find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str())) {
        Some(k) => Err(format!("unrecognized key `{k}`")),
        None => Ok(()),
    }
}

fn uint(v: &Value, what: &str) -> Result<u64, String> {
    v.as_u64().ok_or_else(|| format!("`{what}` must be a non-negative integer"))
}

pub(crate) fn parse(raw: Value) -> Result<Manifest, String> {
    let o = raw.as_object().ok_or("the manifest is not an object")?;
    exact_keys(o, &["schemaVersion", "buildId", "desktopVersion", "minCompatibleRuntimeProtocolVersion", "runtimeProtocolVersion", "entrypoint", "totalBytes", "assets", "routes"], &["pageVersion"])?;
    if o["schemaVersion"] != 1 {
        return Err("`schemaVersion` must be 1".into());
    }
    let build = o["buildId"].as_str().filter(|b| is_sha256(b)).ok_or("`buildId` must be a sha-256 hex")?.to_string();
    if !o["desktopVersion"].as_str().is_some_and(|v| (1..=64).contains(&v.len())) {
        return Err("`desktopVersion` must be 1 to 64 characters".into());
    }
    let (min, now) = (uint(&o["minCompatibleRuntimeProtocolVersion"], "minCompatibleRuntimeProtocolVersion")?, uint(&o["runtimeProtocolVersion"], "runtimeProtocolVersion")?);
    if min > now {
        return Err("the protocol window is inverted".into());
    }
    if o["entrypoint"] != ENTRYPOINT {
        return Err("`entrypoint` must be index.html".into());
    }
    if !o["routes"].is_array() || o["routes"].as_array().is_some_and(|r| r.len() > 64) {
        return Err("`routes` must be a list of at most 64".into());
    }
    let list = o["assets"].as_array().filter(|a| !a.is_empty() && a.len() <= MAX_ASSETS).ok_or("`assets` must hold 1 to 256 entries")?;
    let (mut assets, mut types, mut sum) = (Vec::new(), Vec::new(), 0u64);
    for a in list {
        let ao = a.as_object().ok_or("an asset is not an object")?;
        exact_keys(ao, &["path", "sha256", "byteLength", "contentType"], &[])?;
        let path = ao["path"].as_str().filter(|p| is_asset_path(p)).ok_or("an asset path is not portable")?;
        let sha256 = ao["sha256"].as_str().filter(|s| is_sha256(s)).ok_or("an asset sha256 is not a sha-256 hex")?;
        let byte_length = uint(&ao["byteLength"], "byteLength").ok().filter(|n| *n <= MAX_ASSET_BYTES).ok_or("an asset is over 10 MiB")?;
        let content_type = ao["contentType"].as_str().filter(|t| is_content_type(t)).ok_or("an asset content type is not canonical")?;
        if assets.last().is_some_and(|p: &Asset| path <= p.path.as_str()) {
            return Err("assets must be sorted by path and unique".into());
        }
        sum += byte_length;
        assets.push(Asset { path: path.into(), sha256: sha256.into(), byte_length });
        types.push(content_type.to_string());
    }
    let mut folded: Vec<String> = assets.iter().map(|a| a.path.to_lowercase()).collect();
    folded.sort();
    if folded.windows(2).any(|w| w[0] == w[1]) {
        return Err("asset paths must not collide when case is folded".into());
    }
    if sum != uint(&o["totalBytes"], "totalBytes")? || sum > MAX_TOTAL_BYTES {
        return Err("`totalBytes` must equal the sum of the asset sizes, at most 32 MiB".into());
    }
    if !assets.iter().any(|a| a.path == ENTRYPOINT) {
        return Err("the entrypoint is not a listed asset".into());
    }
    if build != build_id(&assets, &types) {
        return Err("`buildId` is not the content hash of the asset list".into());
    }
    Ok(Manifest { raw, build_id: build, assets })
}
