//! The pinned release as something a driver can run: the tarball the pin names is refused unless both the pin and the release's
//! `SHA256SUMS` give the hash of its bytes, then unpacked once per cache.
use crate::pin::{parse_sha256_sums, sha256_hex, Pin};
use std::path::{Path, PathBuf};

/// How bytes are fetched; the adapter is `curl`, which honours the proxy and CA settings of the environment.
pub trait Get {
    fn get(&self, url: &str) -> Result<Vec<u8>, String>;
}

pub struct CurlGet;

impl Get for CurlGet {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        let out = crate::child_env::command("curl").args(["-fsSL", "--retry", "3", url]).output().map_err(|e| format!("curl: {e}"))?;
        if out.status.success() { Ok(out.stdout) } else { Err(format!("GET {url}: {}", String::from_utf8_lossy(&out.stderr).trim())) }
    }
}

#[derive(Debug, PartialEq)]
pub struct Runtime {
    pub dir: PathBuf,
    pub downloaded: bool,
}

/// The verified runtime of `pin` under `cache`: the tarball is fetched once, and unpacked once.
pub fn ensure_runtime(pin: &Pin, platform: &str, cache: &Path, get: &dyn Get) -> Result<Runtime, String> {
    let want = pin.sha256(platform).ok_or_else(|| format!("the pin has no platform {platform}"))?;
    let name = pin.asset(platform);
    let tarball = cache.join(&name);
    let mut downloaded = false;
    if !std::fs::read(&tarball).is_ok_and(|bytes| sha256_hex(&bytes) == want) {
        let _ = std::fs::remove_file(&tarball);
        let sums = parse_sha256_sums(&String::from_utf8_lossy(&get.get(&format!("{}/SHA256SUMS", pin.release))?));
        if sums.iter().find(|(n, _)| *n == name).map(|(_, s)| s.as_str()) != Some(want) {
            return Err(format!("SHA256SUMS of the release disagrees with the pin for {name}"));
        }
        let bytes = get.get(&format!("{}/{name}", pin.release))?;
        if sha256_hex(&bytes) != want {
            return Err(format!("sha256 of {name} does not match the pin"));
        }
        std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
        std::fs::write(&tarball, bytes).map_err(|e| e.to_string())?;
        downloaded = true;
    }
    let dir = cache.join(format!("runtime-{}-{platform}", pin.version));
    if !dir.exists() {
        let scratch = tempfile_in(cache)?;
        let status = crate::child_env::command("tar").arg("xzf").arg(&tarball).arg("-C").arg(&scratch).arg("--strip-components=1").output().map_err(|e| format!("tar: {e}"))?;
        if !status.status.success() {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(format!("tar failed: {}", String::from_utf8_lossy(&status.stderr).trim()));
        }
        std::fs::rename(&scratch, &dir).map_err(|e| e.to_string())?;
    }
    Ok(Runtime { dir, downloaded })
}

fn tempfile_in(cache: &Path) -> Result<PathBuf, String> {
    let dir = cache.join(format!(".extract-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}
