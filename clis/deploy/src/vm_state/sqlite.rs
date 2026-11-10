//! SQLite databases inside a state volume: a live file (with its `-wal`) is never read as is. The backup makes a consistent copy with
//! `sqlite3 .backup` and snapshots that in its place; verify runs `PRAGMA integrity_check` on every archived database.
use super::tools::run;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// The same matching tar's `--exclude` does for the plain names the plan uses: `./a/b` is anchored, `name` matches any path component.
pub fn excluded(rel: &str, excludes: &[String]) -> bool {
    excludes.iter().any(|e| match e.strip_prefix("./") {
        Some(anchored) => rel == anchored || rel.starts_with(&format!("{anchored}/")),
        None => rel.split('/').any(|part| part == e),
    })
}

fn has_magic(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 16];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && &head == MAGIC
}

/// Every regular file under `root` that starts with the SQLite magic bytes, whatever its name, as paths relative to `root`: the files a
/// backup copies with `.backup` instead of reading live. The `-wal`, `-shm` and `-journal` siblings do not start with it.
pub fn find(root: &Path, excludes: &[String]) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let (path, Ok(kind)) = (entry.path(), entry.file_type()) else { continue };
            let Ok(rel) = path.strip_prefix(root) else { continue };
            let rel = rel.to_string_lossy().into_owned();
            if excluded(&rel, excludes) {
                continue;
            }
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() && has_magic(&path) {
                found.push(rel);
            }
        }
    }
    found.sort();
    found
}

pub fn available() -> bool {
    run("sqlite3", &["-version"]).is_ok()
}

/// A consistent copy of `source` at `dest` (the online backup API, so a WAL and a writer in flight are no problem).
pub fn snapshot(source: &Path, dest: &Path) -> Result<(), String> {
    let command = format!(".backup '{}'", dest.display().to_string().replace('\'', "''"));
    run("sqlite3", &[&source.display().to_string(), &command]).map(|_| ())
}

pub fn integrity(path: &Path) -> Result<(), String> {
    let out = run("sqlite3", &[&path.display().to_string(), "PRAGMA integrity_check;"])?;
    let said = String::from_utf8_lossy(&out);
    if said.trim() == "ok" { Ok(()) } else { Err(format!("integrity_check said: {}", said.split_whitespace().take(12).collect::<Vec<_>>().join(" "))) }
}

/// What a volume holds, measured from the files' sizes without reading any of them: the check that runs before restic reads anything.
pub fn measure(root: &Path, excludes: &[String]) -> Result<u64, String> {
    let (mut total, mut stack) = (0u64, vec![root.to_path_buf()]);
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = entry.path();
            let rel = path.strip_prefix(root).map(|r| r.to_string_lossy().into_owned()).unwrap_or_default();
            if excluded(&rel, excludes) {
                continue;
            }
            let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            if meta.is_dir() {
                stack.push(path);
            } else {
                total += meta.len();
            }
        }
    }
    Ok(total)
}
