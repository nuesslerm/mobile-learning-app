//! `mobileWeb.bundle.manifest`, `.chunk` and `.range`: the host serves the one bundle it holds, whole-asset hash checked, in windows on the
//! advertised grid. The bundle is a directory the host does not build (`manifest.json` and the assets it lists); a host without one answers
//! the pinned `mobile_web_bundle_unavailable` and does not advertise the capability.
use crate::manifest::{self, Manifest};
use crate::window;
use crate::{BROTLI_RANGE, CHUNK, LOCALE_CATALOG, MANIFEST, RANGE, WEB_BUNDLE_CAPABILITY, WINDOW_BYTES};
use orca_core::wire::Failure;
use orca_core::{Area, Ctx, Kind, Method};
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::HashMap;
use std::sync::Mutex;

/// Where the bundle's bytes are. The adapter owns the files; the area owns what they must be.
pub trait BundleSource: Send + Sync {
    /// The parsed `manifest.json`, or none when the host holds no bundle.
    fn manifest(&self) -> Result<Option<Value>, String>;
    /// The asset's size and sha-256 as the file holds them now.
    fn digest(&self, path: &str) -> Result<(u64, String), String>;
    /// Up to `len` bytes of the asset from `offset`.
    fn read(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, String>;
}

/// The source of a host with no bundle.
pub struct NoBundle;

impl BundleSource for NoBundle {
    fn manifest(&self) -> Result<Option<Value>, String> {
        Ok(None)
    }

    fn digest(&self, path: &str) -> Result<(u64, String), String> {
        Err(format!("no bundle holds {path}"))
    }

    fn read(&self, path: &str, _offset: u64, _len: usize) -> Result<Vec<u8>, String> {
        Err(format!("no bundle holds {path}"))
    }
}

const METHODS: &[Method] = &[Method { name: MANIFEST, kind: Kind::Unary }, Method { name: CHUNK, kind: Kind::Unary }, Method { name: RANGE, kind: Kind::Unary }, Method { name: BROTLI_RANGE, kind: Kind::Unary }, Method { name: LOCALE_CATALOG, kind: Kind::Unary }];
const CAPABILITIES: &[&str] = &[WEB_BUNDLE_CAPABILITY];

pub struct MobileWebArea {
    source: Box<dyn BundleSource>,
    manifest: Option<Manifest>,
    problem: Option<String>,
    /// One verdict per build and asset: the hash is read once, and a failed read is no verdict.
    verdicts: Mutex<HashMap<(String, String), bool>>,
}

impl MobileWebArea {
    /// Loads the manifest once: a bundle is immutable until the host restarts. One that fails its checks is held as absent, with `problem()` saying why.
    pub fn new(source: Box<dyn BundleSource>) -> Self {
        let (manifest, problem) = match source.manifest().and_then(|m| m.map(manifest::parse).transpose()) {
            Ok(m) => (m, None),
            Err(e) => (None, Some(e)),
        };
        Self { source, manifest, problem, verdicts: Mutex::new(HashMap::new()) }
    }

    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    pub fn held(&self) -> bool {
        self.manifest.is_some()
    }

    fn bundle_error(code: &str) -> Failure {
        Failure::invalid(code)
    }

    fn manifest(&self) -> Result<&Manifest, Failure> {
        self.manifest.as_ref().ok_or_else(|| Self::bundle_error("mobile_web_bundle_unavailable"))
    }

    fn verified(&self, build_id: &str, path: &str, expect: &manifest::Asset) -> Result<bool, Failure> {
        let key = (build_id.to_string(), path.to_string());
        if let Some(v) = self.verdicts.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Ok(*v);
        }
        let (len, sha) = self.source.digest(path).map_err(|_| Self::bundle_error("mobile_web_bundle_asset_changed"))?;
        let ok = len == expect.byte_length && sha == expect.sha256;
        self.verdicts.lock().unwrap_or_else(|e| e.into_inner()).insert(key, ok);
        Ok(ok)
    }

    /// Build, then member, then alignment, then the asset's hash, then the window: the order of the pinned host, so a phone reads the same code.
    fn window(&self, params: &Value, brotli: bool) -> Result<(Value, Vec<u8>), Failure> {
        let p = window::params(params)?;
        let bundle = self.manifest()?;
        if p.build_id != bundle.build_id {
            return Err(Self::bundle_error("mobile_web_bundle_build_changed"));
        }
        let asset = bundle.assets.iter().find(|a| a.path == p.path).ok_or_else(|| Self::bundle_error("mobile_web_bundle_asset_unknown"))?;
        // a brotli window is cut from the asset's copy, which only an asset the manifest gives one has
        let (source_path, source_len) = match (brotli, asset.brotli_len) {
            (false, _) => (p.path.clone(), asset.byte_length),
            (true, Some(len)) => (format!("{}.br", p.path), len),
            (true, None) => return Err(Self::bundle_error("mobile_web_bundle_asset_unknown")),
        };
        let grid = WINDOW_BYTES as u64;
        if p.offset % grid != 0 || (p.offset > 0 && p.offset >= source_len) {
            return Err(Self::bundle_error("mobile_web_bundle_offset_invalid"));
        }
        if !self.verified(&p.build_id, &p.path, asset)? {
            return Err(Self::bundle_error("mobile_web_bundle_asset_changed"));
        }
        let want = (source_len - p.offset).min(grid) as usize;
        let data = self.source.read(&source_path, p.offset, want).ok().filter(|d| d.len() == want).ok_or_else(|| Self::bundle_error("mobile_web_bundle_asset_changed"))?;
        let header = json!({"buildId": p.build_id, "path": p.path, "offset": p.offset, "assetByteLength": asset.byte_length, "sha256": asset.sha256, "eof": p.offset + data.len() as u64 >= source_len});
        Ok((header, data))
    }

    /// `{locale, knownSha256?}`: the catalog of the build, or none when the phone already holds this copy; the file is read whole, hash checked.
    fn locale_catalog(&self, params: &Value) -> Result<Value, Failure> {
        let invalid = |m: &str| Failure::invalid(m.to_string());
        let o = params.as_object().ok_or_else(|| invalid("Invalid input: expected object"))?;
        if o.keys().any(|k| !matches!(k.as_str(), "locale" | "knownSha256")) {
            return Err(invalid("Unrecognized key in locale catalog params"));
        }
        let locale = window::typed_string(o.get("locale"))?.filter(|l| manifest::is_ui_locale(l)).ok_or_else(|| invalid("Invalid locale: expected a UI locale"))?;
        let known = match o.get("knownSha256") {
            None => None,
            Some(Value::String(s)) if manifest::is_sha256(s) => Some(s.as_str()),
            Some(_) => return Err(invalid("Invalid knownSha256: expected a sha-256 hex")),
        };
        let bundle = self.manifest()?;
        let entry = bundle.catalogs.iter().find(|c| c.locale == locale).ok_or_else(|| Self::bundle_error("mobile_web_bundle_asset_unknown"))?;
        if known == Some(entry.sha256.as_str()) {
            return Ok(json!({"locale": entry.locale, "sha256": entry.sha256, "catalog": null}));
        }
        let changed = || Self::bundle_error("mobile_web_bundle_asset_changed");
        let bytes = self.source.read(&entry.path, 0, entry.byte_length as usize).ok().filter(|b| b.len() as u64 == entry.byte_length).ok_or_else(changed)?;
        let sha: String = sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
        if sha != entry.sha256 {
            return Err(changed());
        }
        let catalog: Value = serde_json::from_slice(&bytes).ok().filter(Value::is_object).ok_or_else(changed)?;
        Ok(json!({"locale": entry.locale, "sha256": entry.sha256, "catalog": catalog}))
    }
}

fn base64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

impl Area for MobileWebArea {
    fn name(&self) -> &'static str {
        "mobile-web"
    }

    fn methods(&self) -> &'static [Method] {
        METHODS
    }

    fn capabilities(&self) -> &'static [&'static str] {
        if self.held() {
            CAPABILITIES
        } else {
            &[]
        }
    }

    fn call(&self, _ctx: &mut Ctx, method: &str, params: &Value) -> Result<Value, Failure> {
        match method {
            MANIFEST => Ok(json!({"manifest": self.manifest()?.raw, "chunkBytes": WINDOW_BYTES, "rangeBytes": WINDOW_BYTES})),
            CHUNK => {
                let (mut header, data) = self.window(params, false)?;
                header["dataBase64"] = base64(&data).into();
                Ok(header)
            }
            RANGE => {
                let (mut header, data) = self.window(params, false)?;
                let (encoding, bytes) = window::encode_range(&data);
                header["encoding"] = encoding.into();
                header["dataBase64"] = base64(&bytes).into();
                Ok(header)
            }
            BROTLI_RANGE => {
                let (mut header, data) = self.window(params, true)?;
                header["dataBase64"] = base64(&data).into();
                Ok(header)
            }
            LOCALE_CATALOG => self.locale_catalog(params),
            other => Err(Failure::new(orca_core::wire::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
        }
    }
}
