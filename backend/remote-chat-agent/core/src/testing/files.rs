//! `MemoryFiles`: the `WorkspaceFiles` port over a map, for tests that must not touch the disk.

use crate::application::ports::WorkspaceFiles;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

#[derive(Default)]
struct Tree {
    files: BTreeMap<PathBuf, Vec<u8>>,
    dirs: BTreeSet<PathBuf>,
    ages: BTreeMap<PathBuf, Duration>,
    temps: u64,
}

impl Tree {
    fn make_dirs(&mut self, path: &Path) {
        for dir in path.ancestors().filter(|d| !d.as_os_str().is_empty()) {
            self.dirs.insert(dir.to_path_buf());
        }
    }
}

/// Files and directories held in memory; a symlink does not exist here, so a path is its own canonical form.
#[derive(Default)]
pub struct MemoryFiles {
    tree: Mutex<Tree>,
}

impl MemoryFiles {
    pub fn new() -> Self {
        Self::default()
    }

    /// How long ago the file was last modified, as `age` answers it (a file without one is brand new).
    pub fn set_age(&self, path: &Path, age: Duration) {
        self.lock().ages.insert(path.to_path_buf(), age);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Tree> {
        self.tree.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl WorkspaceFiles for MemoryFiles {
    fn canonical(&self, path: &Path) -> Option<PathBuf> {
        self.exists(path).then(|| path.to_path_buf())
    }

    fn exists(&self, path: &Path) -> bool {
        let tree = self.lock();
        tree.files.contains_key(path) || tree.dirs.contains(path)
    }

    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        self.lock().files.get(path).cloned()
    }

    fn create_dir(&self, path: &Path) -> Result<(), String> {
        self.lock().make_dirs(path);
        Ok(())
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let mut tree = self.lock();
        if path.parent().is_some_and(|dir| !dir.as_os_str().is_empty() && !tree.dirs.contains(dir)) {
            return Err(format!("No such file or directory: {}", path.display()));
        }
        tree.files.insert(path.to_path_buf(), bytes.to_vec());
        tree.ages.remove(path);
        Ok(())
    }

    fn append(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let mut tree = self.lock();
        if let Some(dir) = path.parent() {
            tree.make_dirs(dir);
        }
        tree.files.entry(path.to_path_buf()).or_default().extend_from_slice(bytes);
        Ok(())
    }

    fn remove(&self, path: &Path) {
        let mut tree = self.lock();
        tree.files.retain(|p, _| !p.starts_with(path));
        tree.dirs.retain(|p| !p.starts_with(path));
        tree.ages.retain(|p, _| !p.starts_with(path));
    }

    fn list(&self, dir: &Path) -> Vec<PathBuf> {
        let tree = self.lock();
        let direct = |p: &&PathBuf| p.parent() == Some(dir);
        let mut entries: Vec<PathBuf> = tree.files.keys().filter(direct).chain(tree.dirs.iter().filter(direct)).cloned().collect();
        entries.sort();
        entries
    }

    fn files_under(&self, dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        self.lock().files.iter().filter(|(p, _)| p.starts_with(dir)).map(|(p, b)| (p.clone(), b.clone())).collect()
    }

    fn age(&self, path: &Path) -> Option<Duration> {
        let tree = self.lock();
        (tree.files.contains_key(path) || tree.dirs.contains(path)).then(|| tree.ages.get(path).copied().unwrap_or_default())
    }

    fn temp_dir(&self, base: &Path, prefix: &str) -> Result<PathBuf, String> {
        let mut tree = self.lock();
        tree.temps += 1;
        let dir = base.join(format!("{prefix}{:06}", tree.temps));
        tree.make_dirs(&dir);
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_a_file_written_under_a_made_directory_when_the_tree_is_removed_then_nothing_of_it_is_listed_or_read() {
        // Given a directory with a file in it and one beside it
        let files = MemoryFiles::new();
        files.create_dir(Path::new("/run/a")).unwrap();
        files.write(Path::new("/run/a/log"), b"one").unwrap();
        files.append(Path::new("/run/a/log"), b"two").unwrap();
        files.write(Path::new("/run/keep"), b"k").unwrap();
        assert_eq!((files.read(Path::new("/run/a/log")), files.list(Path::new("/run"))), (Some(b"onetwo".to_vec()), vec![PathBuf::from("/run/a"), PathBuf::from("/run/keep")]));
        // When the directory is removed
        files.remove(Path::new("/run/a"));
        // Then it and its file are gone and the neighbour stays
        assert_eq!((files.exists(Path::new("/run/a")), files.read(Path::new("/run/a/log")), files.read(Path::new("/run/keep"))), (false, None, Some(b"k".to_vec())));
    }

    #[test]
    fn given_a_write_into_a_missing_directory_when_made_then_it_fails_and_a_temp_dir_is_fresh_each_time() {
        // Given no directories
        let files = MemoryFiles::new();
        // When a file is written into a directory that was never made
        let refused = files.write(Path::new("/nowhere/f"), b"x");
        // Then it is refused, and two temporary directories differ and exist
        assert!(refused.is_err());
        let (a, b) = (files.temp_dir(Path::new("/tmp"), "run-").unwrap(), files.temp_dir(Path::new("/tmp"), "run-").unwrap());
        assert!(a != b && files.exists(&a) && files.exists(&b));
    }
}
