//! What an image that copies part of `clis/` must copy: the directories of the path dependencies, transitively, of the crates it
//! builds. Cargo needs every path a manifest names (optional and dev ones too) to exist, so a new one breaks an image whose hand-kept
//! COPY list lacks it; `copy_problems` compares a Dockerfile with this closure.
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

/// Reads a file by its path from the repository root: from the working tree, or from a commit.
pub type Reader<'a> = &'a dyn Fn(&str) -> Result<String, String>;

fn manifest(read: Reader, dir: &str) -> Result<toml::Table, String> {
    let path = format!("{dir}/Cargo.toml");
    read(&path)?.parse().map_err(|e| format!("{path}: {e}"))
}

fn join(dir: &str, rel: &str) -> String {
    let mut out = PathBuf::new();
    for c in Path::new(dir).join(rel).components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out.to_string_lossy().into_owned()
}

/// The crate directories of `roots`: a workspace root stands for its members, a package for itself.
fn crate_dirs(read: Reader, roots: &[&str]) -> Result<Vec<String>, String> {
    let mut dirs = Vec::new();
    for root in roots {
        let m = manifest(read, root)?;
        let members = m.get("workspace").and_then(|w| w.get("members")).and_then(toml::Value::as_array);
        match members {
            Some(members) => dirs.extend(members.iter().filter_map(toml::Value::as_str).map(|member| join(root, member))),
            None => dirs.push(root.to_string()),
        }
    }
    Ok(dirs)
}

/// The dependencies of a manifest cargo needs on disk: dev dependencies only for a workspace member (a crate the image builds, or any path
/// dependency under the clis workspace root, which cargo makes a member), not for a dependency outside it.
fn path_deps(m: &toml::Table, dir: &str, member: bool) -> Vec<String> {
    let sections: &[&str] = if member { &["dependencies", "build-dependencies", "dev-dependencies"] } else { &["dependencies", "build-dependencies"] };
    let mut tables: Vec<&toml::Table> = sections.iter().filter_map(|s| m.get(*s)?.as_table()).collect();
    for target in m.get("target").and_then(toml::Value::as_table).into_iter().flat_map(|t| t.values()) {
        tables.extend(sections.iter().filter_map(|s| target.get(*s)?.as_table()));
    }
    tables.into_iter().flat_map(|t| t.values()).filter_map(|spec| spec.get("path")?.as_str()).map(|path| join(dir, path)).collect()
}

/// The directories of the crates `roots` build with, the crates themselves included (a root that is a workspace stands for its members).
pub fn path_closure_by(read: Reader, roots: &[&str]) -> Result<BTreeSet<String>, String> {
    let built = crate_dirs(read, roots)?;
    let (mut seen, mut todo) = (BTreeSet::new(), built.clone());
    while let Some(dir) = todo.pop() {
        if seen.insert(dir.clone()) {
            todo.extend(path_deps(&manifest(read, &dir)?, &dir, built.contains(&dir) || dir.starts_with("clis/")));
        }
    }
    Ok(seen)
}

/// `path_closure_by` over the working tree at `repo`.
pub fn path_closure(repo: &Path, roots: &[&str]) -> Result<BTreeSet<String>, String> {
    path_closure_by(&|path| std::fs::read_to_string(repo.join(path)).map_err(|e| format!("{path}: {e}")), roots)
}

/// The sources of the `COPY` lines of a Dockerfile (not `--from` copies), without a trailing slash.
pub fn copy_sources(dockerfile: &str) -> BTreeSet<String> {
    dockerfile
        .lines()
        .filter_map(|l| l.strip_prefix("COPY "))
        .filter(|l| !l.starts_with("--from"))
        .flat_map(|l| {
            let words: Vec<&str> = l.split_whitespace().filter(|w| !w.starts_with("--")).collect();
            words[..words.len().saturating_sub(1)].iter().map(|w| w.trim_end_matches('/').to_string()).collect::<Vec<_>>()
        })
        .collect()
}

/// What is wrong with the `clis/` COPY lines of `dockerfile` (a path from the repository root) for an image that builds the crates
/// `roots`: every crate of their path closure under `clis/` must be copied, plus the `extra` files, and nothing else may be. Sources
/// inside the Dockerfile's own directory are the image's own.
pub fn copy_problems(repo: &Path, dockerfile: &str, roots: &[&str], extra: &[&str]) -> Result<Vec<String>, String> {
    let text = std::fs::read_to_string(repo.join(dockerfile)).map_err(|e| format!("{dockerfile}: {e}"))?;
    let own = format!("{}/", dockerfile.rsplit_once('/').map_or("", |(dir, _)| dir));
    let is_clis = |p: &String| p.starts_with("clis/");
    let wanted: BTreeSet<String> = path_closure(repo, roots)?.into_iter().chain(extra.iter().map(|e| e.to_string())).filter(is_clis).filter(|p| !p.starts_with(&own)).collect();
    // A crate only a dev-dependency reaches may be copied as its manifest alone (the image stubs its lib.rs): that stands for the directory.
    let stands_for = |p: String| match p.strip_suffix("/Cargo.toml") {
        Some(dir) if wanted.contains(dir) && !wanted.contains(&p) => dir.to_string(),
        _ => p,
    };
    let copied: BTreeSet<String> = copy_sources(&text).into_iter().filter(is_clis).filter(|p| !p.starts_with(&own)).map(stands_for).collect();
    let mut problems: Vec<String> = wanted.difference(&copied).map(|p| format!("{dockerfile} does not COPY {p}, which the cargo closure of {roots:?} reaches")).collect();
    problems.extend(copied.difference(&wanted).map(|p| format!("{dockerfile} copies {p}, which the cargo closure of {roots:?} does not reach")));
    Ok(problems)
}
