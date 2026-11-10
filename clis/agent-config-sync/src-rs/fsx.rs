use crate::Res;
use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub struct Meta {
    pub symlink: bool,
    pub dir: bool,
}

fn describe(e: &io::Error) -> (&'static str, &'static str) {
    match e.raw_os_error() {
        Some(2) => ("ENOENT", "no such file or directory"),
        Some(13) => ("EACCES", "permission denied"),
        Some(1) => ("EPERM", "operation not permitted"),
        Some(30) => ("EROFS", "read-only file system"),
        Some(20) => ("ENOTDIR", "not a directory"),
        Some(21) => ("EISDIR", "illegal operation on a directory"),
        _ => ("EIO", "i/o error"),
    }
}

/// Node-style text, so `is_not_ours_to_write` can match errno names in it as the TypeScript does.
pub fn io_err(e: &io::Error, syscall: &str, path: &Path) -> String {
    let (code, text) = describe(e);
    format!("{code}: {text}, {syscall} '{}'", path.display())
}

pub fn lstat(p: &Path) -> Res<Option<Meta>> {
    match fs::symlink_metadata(p) {
        Ok(m) => Ok(Some(Meta {
            symlink: m.file_type().is_symlink(),
            dir: m.is_dir(),
        })),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(&e, "lstat", p)),
    }
}

pub fn read_text_or_null(p: &Path) -> Res<Option<String>> {
    match fs::read(p) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(&e, "open", p)),
    }
}

pub fn read_text(p: &Path) -> Res<String> {
    read_text_or_null(p)?
        .ok_or_else(|| format!("ENOENT: no such file or directory, open '{}'", p.display()))
}

pub fn list_dir(p: &Path) -> Res<Vec<String>> {
    let mut names = match fs::read_dir(p) {
        Ok(rd) => rd
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| io_err(&e, "scandir", p))?,
        Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(io_err(&e, "scandir", p)),
    };
    names.sort();
    Ok(names)
}

/// Every file under `p`, recursively, as paths relative to it (symlinks count as files).
pub fn list_files(p: &Path) -> Res<Vec<String>> {
    let mut out = Vec::new();
    walk(p, "", &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(root: &Path, prefix: &str, out: &mut Vec<String>) -> Res<()> {
    let dir = if prefix.is_empty() {
        root.to_path_buf()
    } else {
        root.join(prefix)
    };
    let entries = match fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(io_err(&e, "scandir", &dir)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| io_err(&e, "scandir", &dir))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let is_dir = entry
            .file_type()
            .map_err(|e| io_err(&e, "scandir", &dir))?
            .is_dir();
        if is_dir {
            walk(root, &rel, out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

pub fn write_text(p: &Path, content: &str, executable: bool) -> Res<()> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| io_err(&e, "mkdir", parent))?;
    }
    fs::write(p, content).map_err(|e| io_err(&e, "open", p))?;
    if executable {
        fs::set_permissions(p, fs::Permissions::from_mode(0o755))
            .map_err(|e| io_err(&e, "chmod", p))?;
    }
    Ok(())
}

pub fn remove(p: &Path, recursive: bool) -> Res<()> {
    match lstat(p)? {
        None => Ok(()),
        Some(m) if m.symlink => fs::remove_file(p).map_err(|e| io_err(&e, "unlink", p)),
        Some(m) if m.dir && recursive => fs::remove_dir_all(p).map_err(|e| io_err(&e, "rm", p)),
        Some(m) if m.dir => Err(format!(
            "EISDIR: illegal operation on a directory, rm '{}'",
            p.display()
        )),
        Some(_) => fs::remove_file(p).map_err(|e| io_err(&e, "unlink", p)),
    }
}

pub fn is_not_ours_to_write(message: &str) -> bool {
    ["EACCES", "EPERM", "EROFS"]
        .iter()
        .any(|c| message.contains(c))
}
