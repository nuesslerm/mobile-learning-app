use sha2::{Digest, Sha256};
use std::fs::File;
use std::io;
use std::path::Path;

const SHA_CHARS: usize = 12;

/// The first 12 hex characters of the SHA-256 of `path`, or `unknown` when it cannot be read.
pub fn sha_of(path: &Path) -> String {
    let hashed = File::open(path).and_then(|mut file| {
        let mut hasher = Sha256::new();
        io::copy(&mut file, &mut hasher)?;
        Ok(hasher.finalize())
    });
    match hashed {
        Ok(digest) => hex::encode(digest)[..SHA_CHARS].to_string(),
        Err(error) => {
            tracing::warn!(path = %path.display(), error = %error, "binary_sha.unreadable");
            "unknown".to_string()
        }
    }
}

/// What `/health` reports as `serverScriptSha`: a hash of the running executable. `/proc/self/exe` still names the running file
/// after a deploy replaced it; where there is no procfs the executable's path is all there is.
pub fn running_binary_sha() -> String {
    let procfs = Path::new("/proc/self/exe");
    if procfs.exists() {
        return sha_of(procfs);
    }
    match std::env::current_exe() {
        Ok(path) => sha_of(&path),
        Err(error) => {
            tracing::warn!(error = %error, "binary_sha.no_executable");
            "unknown".to_string()
        }
    }
}
