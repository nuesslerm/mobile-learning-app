//! Git holds an authenticated archive. The previous ciphertext is the merge base for this checkout.
use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Component, Path},
};
use tempfile::NamedTempFile;

type Notes = BTreeMap<String, String>;
const ARCHIVE: &str = ".agents/basic-memory.enc";
const LOCAL: &str = ".agents/tmp/basic-memory";
const BASE: &str = ".agents/tmp/basic-memory.base";
const MAGIC: &[u8] = b"WFBM\x01";
const KEY: &str = "BASIC_MEMORY_ENCRYPTION_KEY";
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn checked(root: &Path, name: &str) -> Result<std::path::PathBuf> {
    let mut path = root.to_owned();
    for part in Path::new(name).components() {
        let Component::Normal(part) = part else {
            return Err("invalid Basic Memory path".into());
        };
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_symlink() => {
                return Err("refusing a symlink in Basic Memory storage".into())
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    Ok(path)
}

fn read(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(data)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or("missing parent directory")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.persist(path)?;
    Ok(())
}

fn note_path(name: &str) -> bool {
    name.split('/')
        .all(|p| !matches!(p, "" | "." | ".." | ".git"))
        && Path::new(name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

fn local_notes(dir: &Path) -> Result<Notes> {
    let mut notes = Notes::new();
    for path in crate::fsx::list_files(dir)? {
        if note_path(&path) {
            let file = checked(dir, &path)?;
            if !file.is_file() {
                return Err("expected a regular Markdown file".into());
            }
            notes.insert(path, fs::read_to_string(file)?);
        }
    }
    Ok(notes)
}

fn open(cipher: &XChaCha20Poly1305, data: Option<&[u8]>) -> Result<Notes> {
    let Some(data) = data else {
        return Ok(Notes::new());
    };
    let body = data
        .strip_prefix(MAGIC)
        .filter(|b| b.len() >= 24)
        .ok_or("invalid Basic Memory archive")?;
    let (nonce, sealed) = body.split_at(24);
    let plain = cipher
        .decrypt(XNonce::from_slice(nonce), sealed)
        .map_err(|_| "Basic Memory archive authentication failed")?;
    let notes: Notes = serde_json::from_slice(&plain)?;
    if !notes.keys().all(|p| note_path(p)) {
        return Err("invalid note path in archive".into());
    }
    Ok(notes)
}

fn seal(cipher: &XChaCha20Poly1305, notes: &Notes) -> Result<Vec<u8>> {
    let mut nonce = [0; 24];
    getrandom::fill(&mut nonce).map_err(|_| "could not generate a Basic Memory nonce")?;
    let plain = serde_json::to_vec(notes)?;
    let sealed = cipher
        .encrypt((&nonce).into(), plain.as_slice())
        .map_err(|_| "could not encrypt Basic Memory")?;
    Ok([MAGIC, &nonce, &sealed].concat())
}

fn sync(root: &Path, save: bool, warning: &mut String) -> Result<usize> {
    sync_with_key(root, save, warning, || {
        secrets::secret_from(KEY, "bitwarden")
    })
}

fn sync_with_key(
    root: &Path,
    save: bool,
    warning: &mut String,
    key: impl FnOnce() -> crate::Res<Option<String>>,
) -> Result<usize> {
    let archive = checked(root, ARCHIVE)?;
    if !save && !archive.try_exists()? {
        return Ok(0);
    }
    let lock_path = checked(root, ".agents/tmp/basic-memory.lock")?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(lock_path.parent().unwrap())?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(lock_path)?;
    lock.lock()?;
    let base_path = checked(root, BASE)?;
    let previous = read(&base_path)?;
    let current = read(&archive)?;
    if current.is_none() && previous.is_some() {
        return Err("Basic Memory archive disappeared; recover it from Git before saving".into());
    }
    let dir = checked(root, LOCAL)?;
    if !save && previous == current && dir.is_dir() {
        return Ok(0);
    }
    let key = match key() {
        Ok(Some(key)) => key,
        _ if !save => {
            warning.push_str("Basic Memory restore skipped: BASIC_MEMORY_ENCRYPTION_KEY unavailable from Bitwarden\n");
            return Ok(0);
        }
        _ => return Err("Basic Memory requires BASIC_MEMORY_ENCRYPTION_KEY from Bitwarden".into()),
    };
    let cipher = XChaCha20Poly1305::new_from_slice(&hex::decode(key.trim())?)
        .map_err(|_| "expected a 32-byte Basic Memory key")?;
    let base = if dir.is_dir() {
        open(&cipher, previous.as_deref())?
    } else {
        Notes::new()
    };
    let remote = open(&cipher, current.as_deref())?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let local = local_notes(&dir)?;
    let mut merged = local.clone();
    for path in base.keys().chain(remote.keys()).collect::<BTreeSet<_>>() {
        let (old, disk, incoming) = (base.get(path), local.get(path), remote.get(path));
        if old == incoming || disk == incoming {
            continue;
        }
        if disk != old {
            return Err(format!("Basic Memory conflict in {path}: preserve the local edit and reconcile it with Git").into());
        }
        match incoming {
            Some(text) => {
                merged.insert(path.clone(), text.clone());
            }
            None => {
                merged.remove(path);
            }
        }
    }
    for path in local.keys().chain(merged.keys()).collect::<BTreeSet<_>>() {
        if local.get(path) == merged.get(path) {
            continue;
        }
        let file = checked(&dir, path)?;
        if read(&file)?.as_deref() != local.get(path).map(String::as_bytes) {
            return Err("note changed during Basic Memory sync; retry".into());
        }
        match merged.get(path) {
            Some(text) => write(&file, text.as_bytes())?,
            None => fs::remove_file(file)?,
        }
    }
    let mut snapshot = current;
    if save && (snapshot.is_none() || merged != remote) {
        let data = seal(&cipher, &merged)?;
        write(&archive, &data)?;
        snapshot = Some(data);
    }
    if previous != snapshot {
        if let Some(data) = snapshot {
            write(&base_path, &data)?;
        }
    }
    Ok(merged.len())
}

pub fn restore(root: &Path, warning: &mut String) -> crate::Res<()> {
    sync(root, false, warning)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn save(root: &Path) -> crate::Res<usize> {
    sync(root, true, &mut String::new()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypted_notes_restore_and_reconcile_in_another_checkout() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let run = |root: &Path, save| {
            sync_with_key(root, save, &mut String::new(), || Ok(Some("42".repeat(32))))
        };
        let a = first.path();
        let b = second.path();
        let source = a.join(LOCAL).join("nested/private.md");
        let restored = b.join(LOCAL).join("nested/private.md");
        // Given a local note and a second checkout containing only its encrypted archive.
        write(&source, b"confidential markdown").unwrap();
        run(a, true).unwrap();
        let snapshot = fs::read(a.join(ARCHIVE)).unwrap();
        assert!(!snapshot
            .windows(7)
            .any(|w| w == b"private" || w == b"markdow"));
        run(a, true).unwrap();
        assert_eq!(fs::read(a.join(ARCHIVE)).unwrap(), snapshot);
        write(&b.join(ARCHIVE), &snapshot).unwrap();
        // When config sync runs, then the note is restored and concurrent edits are preserved.
        run(b, false).unwrap();
        assert_eq!(fs::read(&restored).unwrap(), b"confidential markdown");
        write(&restored, b"local edit").unwrap();
        write(&source, b"remote edit").unwrap();
        run(a, true).unwrap();
        fs::copy(a.join(ARCHIVE), b.join(ARCHIVE)).unwrap();
        assert!(run(b, false).is_err());
        assert_eq!(fs::read(&restored).unwrap(), b"local edit");
        write(&restored, b"remote edit").unwrap();
        run(b, false).unwrap();
        fs::remove_file(source).unwrap();
        run(a, true).unwrap();
        fs::copy(a.join(ARCHIVE), b.join(ARCHIVE)).unwrap();
        run(b, false).unwrap();
        assert!(!restored.exists());
    }

    #[test]
    fn damaged_ciphertext_and_links_cannot_replace_plaintext() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let note = root.join(LOCAL).join("note.md");
        let run =
            |save| sync_with_key(root, save, &mut String::new(), || Ok(Some("12".repeat(32))));
        // Given an encrypted copy, when the ciphertext is altered, then sync leaves the note intact.
        write(&note, b"unchanged").unwrap();
        run(true).unwrap();
        let mut archive = fs::read(root.join(ARCHIVE)).unwrap();
        *archive.last_mut().unwrap() ^= 1;
        write(&root.join(ARCHIVE), &archive).unwrap();
        assert!(run(false).is_err());
        assert_eq!(fs::read(&note).unwrap(), b"unchanged");
        fs::copy(root.join(BASE), root.join(ARCHIVE)).unwrap();
        std::os::unix::fs::symlink(&note, root.join(LOCAL).join("link.md")).unwrap();
        assert!(run(true).is_err());
    }
}
