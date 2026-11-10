//! One chunk or range window: the params as the pinned strict schema takes them and the reply's body.
use crate::manifest::{self, MAX_ASSET_BYTES};
use flate2::write::GzEncoder;
use flate2::Compression;
use orca_core::wire::Failure;
use serde_json::Value;
use std::io::Write;

pub(crate) struct Params {
    pub build_id: String,
    pub path: String,
    pub offset: u64,
}

pub(crate) fn params(v: &Value) -> Result<Params, Failure> {
    let invalid = |m: &str| Failure::invalid(m.to_string());
    let o = v.as_object().ok_or_else(|| invalid("Invalid input: expected object"))?;
    if o.keys().any(|k| !matches!(k.as_str(), "buildId" | "path" | "offset")) {
        return Err(invalid("Unrecognized key in bundle window params"));
    }
    let build_id = o.get("buildId").and_then(Value::as_str).filter(|b| manifest::is_sha256(b)).ok_or_else(|| invalid("Invalid buildId: expected a sha-256 hex"))?;
    let path = o.get("path").and_then(Value::as_str).filter(|p| manifest::is_asset_path(p)).ok_or_else(|| invalid("Invalid path: expected a portable asset path"))?;
    // JSON has one number type: 2.0 is the integer 2 to the phone
    let offset = o.get("offset").and_then(|n| n.as_u64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0).map(|f| f as u64))).filter(|n| *n <= MAX_ASSET_BYTES);
    Ok(Params { build_id: build_id.into(), path: path.into(), offset: offset.ok_or_else(|| invalid("Invalid offset: expected an integer from 0 to 10485760"))? })
}

/// gzip when it shrinks the window, else the bytes as they are: a reply never outgrows its frame bound.
pub(crate) fn encode_range(data: &[u8]) -> (&'static str, Vec<u8>) {
    if data.is_empty() {
        return ("identity", Vec::new());
    }
    let mut gz = GzEncoder::new(Vec::new(), Compression::new(6));
    match gz.write_all(data).and_then(|()| gz.finish()) {
        Ok(zipped) if zipped.len() < data.len() => ("gzip", zipped),
        _ => ("identity", data.to_vec()),
    }
}
