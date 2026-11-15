//! A service or CLI takes a versioned library (libraries.json) from its registry, so a library edit with no release cannot move it: a
//! `path` dependency on one defeats that. Only a library's own manifests may name a sibling library by path, beside its version and
//! registry (that is how it is packaged and developed). Read from the manifests with no toolchain, like `core_purity`.
use std::path::{Path, PathBuf};
use toml::Value;

const DEP_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
const SKIP: [&str; 5] = ["target", "node_modules", "references", ".git", ".nx"];

pub(crate) fn manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !SKIP.contains(&entry.file_name().to_string_lossy().as_ref()) {
                manifests(&path, out);
            }
        } else if entry.file_name() == "Cargo.toml" {
            out.push(path);
        }
    }
}

pub(crate) fn tables(manifest: &Value) -> Vec<&toml::Table> {
    let mut found: Vec<&toml::Table> = DEP_TABLES.iter().filter_map(|k| manifest.get(*k).and_then(Value::as_table)).collect();
    if let Some(workspace) = manifest.get("workspace").and_then(|w| w.get("dependencies")).and_then(Value::as_table) {
        found.push(workspace);
    }
    for target in manifest.get("target").and_then(Value::as_table).into_iter().flat_map(|t| t.values()) {
        found.extend(DEP_TABLES.iter().filter_map(|k| target.get(*k).and_then(Value::as_table)));
    }
    found
}

/// Whether a version of the library exists for a consumer to pin: on a registry kept in this repository (`git-sparse`) that is a
/// committed `registry/crates/<name>`; anywhere else, or when git cannot say, it is taken to be released.
fn released(root: &Path, catalog: &serde_json::Value, library: &serde_json::Value) -> bool {
    let registry = catalog["registries"].as_array().into_iter().flatten().find(|r| r["alias"] == library["registry"]);
    if registry.and_then(|r| r["publish"].as_str()) != Some("git-sparse") {
        return true;
    }
    let Some(name) = library["name"].as_str() else { return true };
    let run = |args: &[&str]| std::process::Command::new("git").arg("-C").arg(root).args(args).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok();
    match (run(&["rev-parse", "--verify", "-q", "HEAD"]), run(&["cat-file", "-e", &format!("HEAD:registry/crates/{name}")])) {
        (Some(head), Some(found)) if head.status.success() => found.status.success(),
        _ => true,
    }
}

/// `<manifest>: <library>` for each path dependency on a released library, outside the libraries' own manifests.
pub fn violations(root: &Path) -> Vec<String> {
    let Some(catalog) = std::fs::read_to_string(root.join("libraries.json")).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) else { return Vec::new() };
    let libraries: Vec<(String, String)> = catalog["libraries"].as_array().into_iter().flatten().filter(|l| released(root, &catalog, l)).filter_map(|l| Some((l["name"].as_str()?.to_string(), l["path"].as_str()?.to_string()))).collect();
    let mut files = Vec::new();
    for top in ["backend", "clis", "libraries", "infra"] {
        manifests(&root.join(top), &mut files);
    }
    files.sort();
    let mut found = Vec::new();
    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(&file).parent().unwrap_or(Path::new("")).to_path_buf();
        if libraries.iter().any(|(_, path)| relative.starts_with(path)) {
            continue;
        }
        let Some(manifest) = std::fs::read_to_string(&file).ok().and_then(|t| t.parse::<Value>().ok()) else { continue };
        for table in tables(&manifest) {
            for (key, spec) in table {
                let real = spec.get("package").and_then(Value::as_str).unwrap_or(key);
                if spec.get("path").is_some() && libraries.iter().any(|(name, _)| name == real) {
                    found.push(format!("{}: {real}", file.strip_prefix(root).unwrap_or(&file).display()));
                }
            }
        }
    }
    found
}

pub fn count(root: &Path) -> usize {
    violations(root).len()
}

#[cfg(test)]
mod tests {
    use super::violations;
    use std::process::Command;

    fn git(dir: &std::path::Path, args: &[&str]) {
        assert!(Command::new("git").arg("-C").arg(dir).args(["-c", "user.email=t@example.com", "-c", "user.name=t"]).args(args).status().unwrap().success(), "{args:?}");
    }

    #[test]
    fn given_a_library_on_a_git_registry_when_it_is_not_yet_released_then_a_path_to_it_is_no_violation_until_the_release_is_committed() {
        // Given a library on the repository's own registry, a service that names it by path, and nothing released
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let put = |path: &str, text: &str| {
            std::fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            std::fs::write(root.join(path), text).unwrap();
        };
        put("libraries.json", r#"{"registries":[{"alias":"mla","kind":"cargo-sparse","publish":"git-sparse"}],"libraries":[{"name":"lib-a","kind":"rust","path":"libs/lib-a","registry":"mla"}]}"#);
        put("libs/lib-a/Cargo.toml", "[package]\nname = \"lib-a\"\n");
        put("clis/svc/Cargo.toml", "[dependencies]\nlib-a = { path = \"../../libs/lib-a\" }\n");
        git(root, &["init", "-q", "-b", "dev"]);
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "start"]);

        // When the manifests are read, Then there is nothing to pin yet
        assert_eq!(violations(root), Vec::<String>::new());

        // Given the release committed, Then the path is a violation
        put("registry/crates/lib-a/0.1.1.crate", "x");
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "registry: lib-a 0.1.1"]);
        assert_eq!(violations(root), ["clis/svc/Cargo.toml: lib-a"]);
    }
}
