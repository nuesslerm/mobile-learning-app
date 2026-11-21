//! Files a terminal printed, read and written beyond the workspace root: `artifact_stat`, `artifact_read` and `artifact_write`. A path is served only
//! below the root, the worktree base or a directory the executor was told to allow for artifacts (none by default), after its links are resolved;
//! a file is named by an identity (device, inode, size and both times) and a fingerprint of its first 16 MiB, and a read or write that carries them is
//! refused with `CHANGED` when the file is no longer the one they name.
use exec_fs::{ArtifactRead, ArtifactStat, ArtifactStatResult, ArtifactWrite, ArtifactWriteResult, ChunkResult, EntryKind, Granted, ARTIFACT_HASH_MAX};
use exec_proto::{Code, ExecError, ExecResult};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMPS: AtomicU64 = AtomicU64::new(0);

fn io_at(path: &str, e: std::io::Error) -> ExecError {
    match e.kind() {
        std::io::ErrorKind::NotFound => ExecError::new(Code::NotFound, format!("{path} does not exist")),
        _ => ExecError::new(Code::Io, format!("{path}: {e}")),
    }
}

fn outside(path: &str) -> ExecError {
    ExecError::new(Code::PathOutsideRoot, format!("{path} is outside the root"))
}

fn changed(path: &str) -> ExecError {
    ExecError::new(Code::Changed, format!("{path} changed since it was granted"))
}

/// The canonical path of `path`, when it lies below one of `places`; a relative path is the root's (`resolved` is the root-confined path). A path
/// that is not there and not below a place is answered as outside, so the answer does not tell whether a file elsewhere exists.
pub fn locate(path: &str, resolved: impl FnOnce() -> ExecResult<PathBuf>, places: &[&Path]) -> ExecResult<PathBuf> {
    let given = Path::new(path);
    let target = if given.is_absolute() { given.to_path_buf() } else { resolved()? };
    let canonical = match fs::canonicalize(&target) {
        Ok(canonical) => canonical,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !places.iter().any(|p| lexical(&target).starts_with(p)) => return Err(outside(path)),
        Err(e) => return Err(io_at(path, e)),
    };
    if places.iter().any(|p| canonical.starts_with(p)) {
        Ok(canonical)
    } else {
        Err(outside(path))
    }
}

/// The path with its `.` and `..` folded away, without asking the disk.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// FNV-1a over the first 16 MiB: a change detector, not a defence against someone who can write the file.
fn fingerprint_of(file: &mut fs::File, size: u64) -> ExecResult<Option<String>> {
    if size > ARTIFACT_HASH_MAX {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(0)).map_err(|e| ExecError::new(Code::Io, e.to_string()))?;
    let (mut hash, mut buf) = (0xcbf2_9ce4_8422_2325u64, [0u8; 64 * 1024]);
    loop {
        let n = file.read(&mut buf).map_err(|e| ExecError::new(Code::Io, e.to_string()))?;
        if n == 0 {
            return Ok(Some(format!("{hash:016x}")));
        }
        for byte in &buf[..n] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn identity_of(m: &fs::Metadata) -> String {
    format!("{}:{}:{}:{}.{}:{}.{}", m.dev(), m.ino(), m.size(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec())
}

fn millis(secs: i64, nanos: i64) -> u64 {
    (secs.max(0) as u64) * 1000 + (nanos.max(0) as u64) / 1_000_000
}

fn is_binary(file: &mut fs::File) -> bool {
    let mut head = [0u8; 8192];
    let n = file.seek(SeekFrom::Start(0)).and_then(|_| file.read(&mut head)).unwrap_or(0);
    head[..n].contains(&0)
}

fn open(path: &Path, shown: &str) -> ExecResult<(fs::File, fs::Metadata)> {
    let file = fs::File::open(path).map_err(|e| io_at(shown, e))?;
    let meta = file.metadata().map_err(|e| io_at(shown, e))?;
    Ok((file, meta))
}

fn described(canonical: &Path, file: &mut fs::File, meta: &fs::Metadata) -> ExecResult<ArtifactStatResult> {
    let dir = meta.is_dir();
    Ok(ArtifactStatResult {
        path: canonical.to_string_lossy().into_owned(),
        kind: if dir { EntryKind::Dir } else { EntryKind::File },
        size: meta.size(),
        modified_ms: millis(meta.mtime(), meta.mtime_nsec()),
        changed_ms: millis(meta.ctime(), meta.ctime_nsec()),
        identity: identity_of(meta),
        fingerprint: if dir { None } else { fingerprint_of(file, meta.size())? },
        binary: !dir && is_binary(file),
    })
}

pub fn stat(args: &ArtifactStat, canonical: &Path) -> ExecResult<ArtifactStatResult> {
    let (mut file, meta) = open(canonical, &args.path)?;
    described(canonical, &mut file, &meta)
}

/// The file `granted` names, still: its identity and, when given, its fingerprint match what is there now.
fn verify(file: &mut fs::File, meta: &fs::Metadata, granted: &Granted, shown: &str) -> ExecResult<()> {
    if granted.identity.as_deref().is_some_and(|id| id != identity_of(meta)) {
        return Err(changed(shown));
    }
    if let Some(expected) = &granted.fingerprint {
        if fingerprint_of(file, meta.size())?.as_deref() != Some(expected.as_str()) {
            return Err(changed(shown));
        }
    }
    Ok(())
}

pub fn read(args: &ArtifactRead, canonical: &Path) -> ExecResult<ChunkResult> {
    let (mut file, meta) = open(canonical, &args.path)?;
    if !meta.is_file() {
        return Err(ExecError::new(Code::NotAFile, format!("{} is not a regular file", args.path)));
    }
    verify(&mut file, &meta, &args.granted, &args.path)?;
    let total = meta.len();
    let mut bytes = Vec::new();
    if args.offset < total {
        file.seek(SeekFrom::Start(args.offset)).map_err(|e| io_at(&args.path, e))?;
        file.take(args.length).read_to_end(&mut bytes).map_err(|e| io_at(&args.path, e))?;
    }
    let read = bytes.len() as u64;
    use base64::Engine;
    Ok(ChunkResult { data: base64::engine::general_purpose::STANDARD.encode(&bytes), offset: args.offset, bytes: read, total_bytes: total, eof: args.offset.saturating_add(read) >= total })
}

/// Replaces the file's content through a temporary file beside it and a rename, so a reader sees the old file or the new one; the target is
/// looked at again just before the rename, so a swap in between is refused. The file must exist and be writable, and keeps its mode.
pub fn write(args: &ArtifactWrite, canonical: &Path) -> ExecResult<ArtifactWriteResult> {
    let (mut file, meta) = open(canonical, &args.path)?;
    if !meta.is_file() {
        return Err(ExecError::new(Code::NotAFile, format!("{} is not a regular file", args.path)));
    }
    if meta.permissions().readonly() {
        return Err(ExecError::new(Code::Io, format!("{} is read-only", args.path)));
    }
    if meta.size() > ARTIFACT_HASH_MAX {
        return Err(ExecError::invalid_args(format!("{} is longer than {ARTIFACT_HASH_MAX} bytes, which no artifact grant writes", args.path)));
    }
    verify(&mut file, &meta, &args.granted, &args.path)?;
    let dir = canonical.parent().ok_or_else(|| outside(&args.path))?;
    let temp = dir.join(format!(".rca-artifact-{}-{}", std::process::id(), TEMPS.fetch_add(1, Ordering::SeqCst)));
    let staged = (|| -> std::io::Result<()> {
        let mut out = fs::OpenOptions::new().write(true).create_new(true).mode(meta.mode() & 0o7777).open(&temp)?;
        out.write_all(args.content.as_bytes())?;
        out.sync_all()
    })();
    if let Err(e) = staged {
        let _ = fs::remove_file(&temp);
        return Err(io_at(&args.path, e));
    }
    let now = fs::metadata(canonical).map_err(|e| {
        let _ = fs::remove_file(&temp);
        io_at(&args.path, e)
    })?;
    if identity_of(&now) != identity_of(&meta) {
        let _ = fs::remove_file(&temp);
        return Err(changed(&args.path));
    }
    if let Err(e) = fs::rename(&temp, canonical) {
        let _ = fs::remove_file(&temp);
        return Err(io_at(&args.path, e));
    }
    let (mut written, after) = open(canonical, &args.path)?;
    Ok(ArtifactWriteResult { path: canonical.to_string_lossy().into_owned(), bytes: args.content.len() as u64, identity: identity_of(&after), fingerprint: fingerprint_of(&mut written, after.size())? })
}
