use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

const MAGIC: &[u8] = b"WFEASMAP1\n";

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn filename(group: &str, platform: &str) -> Result<String, String> {
    if !valid_id(group) || !matches!(platform, "ios" | "android") {
        return Err("give an EAS group id and either the ios or android platform".into());
    }
    Ok(format!("{platform}.map.sealed"))
}

fn associated_data(group: &str, platform: &str) -> String {
    format!("wf:eas-map:v1:{group}:{platform}")
}

pub fn key() -> Result<[u8; 32], String> {
    crate::ci_db::key().or_else(|original| {
        let text = secrets::secret_from("CI_DB_KEY", "bitwarden")?.ok_or(original)?;
        crate::ci_db::decode_key(&text)
    })
}

pub fn write(
    dir: &Path,
    key: &[u8; 32],
    group: &str,
    platform: &str,
    update_id: &str,
    map: &[u8],
) -> Result<(), String> {
    let file = filename(group, platform)?;
    if !valid_id(update_id) {
        return Err(format!("missing or invalid EAS update id for {platform}"));
    }
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    writeln!(encoder, "{update_id}")
        .and_then(|()| encoder.write_all(map))
        .map_err(|e| e.to_string())?;
    let data = encoder.finish().map_err(|e| e.to_string())?;
    let aad = associated_data(group, platform);
    let body = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &data,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| "could not seal EAS source map")?;
    let dir = dir.join(group);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut tmp = tempfile::NamedTempFile::new_in(&dir).map_err(|e| e.to_string())?;
    tmp.write_all(MAGIC)
        .and_then(|()| tmp.write_all(&nonce))
        .and_then(|()| tmp.write_all(&body))
        .map_err(|e| e.to_string())?;
    tmp.persist(dir.join(file)).map_err(|e| e.to_string())?;
    Ok(())
}

pub struct Map {
    pub update_id: String,
    pub bytes: Vec<u8>,
}

fn open(bytes: &[u8], key: &[u8; 32], group: &str, platform: &str) -> Result<Map, String> {
    let data = bytes
        .strip_prefix(MAGIC)
        .filter(|b| b.len() > 24)
        .ok_or("invalid sealed EAS source map")?;
    let (nonce, body) = data.split_at(24);
    let aad = associated_data(group, platform);
    let compressed = XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(nonce), Payload { msg: body, aad: aad.as_bytes() })
        .map_err(|_| "cannot authenticate the EAS source map with CI_DB_KEY for this update group and platform")?;
    let mut plain = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .read_to_end(&mut plain)
        .map_err(|e| format!("invalid compressed EAS source map: {e}"))?;
    let at = plain
        .iter()
        .position(|&b| b == b'\n')
        .ok_or("sealed EAS source map has no update id")?;
    let update_id = std::str::from_utf8(&plain[..at])
        .ok()
        .filter(|id| valid_id(id))
        .ok_or("invalid EAS update id in source map")?
        .to_string();
    Ok(Map {
        update_id,
        bytes: plain.split_off(at + 1),
    })
}

pub fn read(group: &str, platform: &str) -> Result<Map, String> {
    let file = filename(group, platform)?;
    let key = key()?;
    let bytes = match std::env::var_os("WF_EAS_MAPS_DIR") {
        Some(dir) if !dir.is_empty() => std::fs::read(Path::new(&dir).join(group).join(file))
            .map_err(|e| format!("cannot read the local map for {group}/{platform}: {e}"))?,
        _ => download(group, &file)?,
    };
    open(&bytes, &key, group, platform)
}

fn gh(args: &[&str], failure: &str) -> Result<String, String> {
    let args = args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    let out = wf_kit::proc::capture("gh", &args, None, Some(Duration::from_secs(60)));
    if out.code == 0 {
        return Ok(out.stdout.trim().to_string());
    }
    Err(format!(
        "{failure}: {}",
        out.stderr.lines().next().unwrap_or("gh failed")
    ))
}

fn download(group: &str, file: &str) -> Result<Vec<u8>, String> {
    let name = format!("eas-maps-{group}");
    let endpoint = format!("repos/{{owner}}/{{repo}}/actions/artifacts?name={name}");
    let run = gh(
        &[
            "api",
            &endpoint,
            "--jq",
            "[.artifacts[] | select(.expired == false) | .workflow_run.id][0] // empty",
        ],
        "cannot find the sealed source map on GitHub",
    )?;
    if !run.bytes().all(|b| b.is_ascii_digit()) || run.is_empty() {
        return Err(format!(
            "no retained source map artifact for update group {group}"
        ));
    }
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let dest = dir
        .path()
        .to_str()
        .ok_or("source map temp directory is not UTF-8")?;
    gh(
        &["run", "download", &run, "--name", &name, "--dir", dest],
        "cannot download the sealed source map",
    )?;
    std::fs::read(dir.path().join(file))
        .map_err(|e| format!("artifact {name} has no readable {file}: {e}"))
}
