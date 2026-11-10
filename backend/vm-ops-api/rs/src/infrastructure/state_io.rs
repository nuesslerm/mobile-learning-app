//! What the state adapters share: a file replaced through a temp file and a rename, a JSON object read leniently, and Node's words
//! for a failed file call (`EISDIR: illegal operation on a directory, open '/state/x.json'`), which replies and events carry.
use serde_json::Value;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::AsyncWriteExt;
use vm_ops_api_core::application::ports::Object;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The text of a file as Node's `readFileSync(file, 'utf8')` decodes it; none when it cannot be read.
pub(super) async fn read_text(file: &Path) -> Option<String> {
    let bytes = tokio::fs::read(file).await.ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The JSON object in `text`; none for anything else, which Node reads as an empty store.
pub(super) fn object_of(text: &str) -> Option<Object> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(object)) => Some(object),
        _ => None,
    }
}

/// `mkdir -p`, failing with Node's message for `fs.mkdirSync(dir, { recursive: true })`.
pub(super) async fn make_dir(dir: &Path) -> Result<(), String> {
    tokio::fs::create_dir_all(dir).await.map_err(|error| node_message(&error, "mkdir", Some(dir)))
}

/// Puts `contents` in `file` whole: written beside it, then renamed over it, so a reader sees the old file or the new one.
/// An existing file's permissions carry over; the error is Node's message for a failed write.
pub(super) async fn replace_file(file: &Path, contents: &[u8]) -> Result<(), String> {
    let temp = temp_beside(file);
    let written = write_then_rename(&temp, file, contents).await;
    if written.is_err() {
        let _ = tokio::fs::remove_file(&temp).await;
    }
    written.map_err(|error| node_message(&error, "open", Some(file)))
}

async fn write_then_rename(temp: &Path, file: &Path, contents: &[u8]) -> io::Result<()> {
    let mut handle = tokio::fs::OpenOptions::new().write(true).create_new(true).open(temp).await?;
    handle.write_all(contents).await?;
    handle.sync_all().await?;
    drop(handle);
    if let Ok(existing) = tokio::fs::metadata(file).await {
        if existing.is_file() {
            tokio::fs::set_permissions(temp, existing.permissions()).await?;
        }
    }
    tokio::fs::rename(temp, file).await
}

fn temp_beside(file: &Path) -> PathBuf {
    let name = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    file.with_file_name(format!(".{name}.{}.{unique}.tmp", std::process::id()))
}

/// `<CODE>: <what libuv says>, <syscall> '<path>'`, as the message of the `Error` Node throws.
pub(super) fn node_message(error: &io::Error, syscall: &str, path: Option<&Path>) -> String {
    let named = match (error.raw_os_error(), error.kind()) {
        (Some(1), _) => Some(("EPERM", "operation not permitted")),
        (Some(5), _) => Some(("EIO", "i/o error")),
        (Some(24), _) => Some(("EMFILE", "too many open files")),
        (_, ErrorKind::NotFound) => Some(("ENOENT", "no such file or directory")),
        (_, ErrorKind::PermissionDenied) => Some(("EACCES", "permission denied")),
        (_, ErrorKind::AlreadyExists) => Some(("EEXIST", "file already exists")),
        (_, ErrorKind::NotADirectory) => Some(("ENOTDIR", "not a directory")),
        (_, ErrorKind::IsADirectory) => Some(("EISDIR", "illegal operation on a directory")),
        (_, ErrorKind::ReadOnlyFilesystem) => Some(("EROFS", "read-only file system")),
        (_, ErrorKind::StorageFull) => Some(("ENOSPC", "no space left on device")),
        (_, ErrorKind::DirectoryNotEmpty) => Some(("ENOTEMPTY", "directory not empty")),
        _ => None,
    };
    let what = match named {
        Some((code, text)) => format!("{code}: {text}, {syscall}"),
        None => format!("{error}, {syscall}"),
    };
    match path {
        Some(path) => format!("{what} '{}'", path.display()),
        None => what,
    }
}
