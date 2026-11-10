//! What a lockfile change means for one deployable: the resolved packages its own dependency closure reaches, compared
//! between two commits. A lockfile edit for another package (a CLI's dev tool, another service's crate) ships nothing.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use yaml_rust2::{Yaml, YamlLoader};

/// The file as it was at `rev` (`git show <rev>:<path>`) in the repository at `dir` (the working directory without one), or
/// none when it did not exist there.
fn at(dir: Option<&Path>, rev: &str, path: &str) -> Result<Option<String>, String> {
    let mut git = std::process::Command::new("git");
    git.args(["show", &format!("{rev}:{path}")]).stderr(std::process::Stdio::null());
    if let Some(dir) = dir {
        git.current_dir(dir);
    }
    let out = git.output().map_err(|e| format!("cannot run git: {e}"))?;
    Ok(out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// The names whose resolved entry differs between the two closures (added, removed or changed), first few only.
fn differ(before: &BTreeSet<String>, after: &BTreeSet<String>, name: impl Fn(&str) -> String) -> Vec<String> {
    let names: BTreeSet<String> = before.symmetric_difference(after).map(|k| name(k)).collect();
    names.into_iter().collect()
}

/// npm packages an importer of `pnpm-lock.yaml` reaches whose resolved version (peers and patch hash included) changed
/// between `sha` and HEAD.
pub(crate) fn pnpm_changed(dir: Option<&Path>, sha: &str, importer: &str) -> Result<Vec<String>, String> {
    let closure = |rev: &str| -> Result<BTreeSet<String>, String> {
        let Some(text) = at(dir, rev, "pnpm-lock.yaml")? else { return Ok(BTreeSet::new()) };
        let doc = YamlLoader::load_from_str(&text).map_err(|e| format!("pnpm-lock.yaml at {rev}: {e}"))?;
        Ok(pnpm_closure(doc.first().unwrap_or(&Yaml::Null), importer))
    };
    Ok(differ(&closure(sha)?, &closure("HEAD")?, |k| snapshot_name(k).to_string()))
}

/// The workspace directories the importer `top` of a `pnpm-lock.yaml` links to, itself included and links of links too,
/// dev dependencies as well (a source the bundle might read is better counted than missed).
pub fn linked_dirs(lock: &str, top: &str) -> Result<BTreeSet<String>, String> {
    let doc = YamlLoader::load_from_str(lock).map_err(|e| format!("pnpm-lock.yaml: {e}"))?;
    let importers = &doc.first().unwrap_or(&Yaml::Null)["importers"];
    let (mut seen, mut todo) = (BTreeSet::new(), vec![top.to_string()]);
    while let Some(dir) = todo.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        for section in ["dependencies", "devDependencies", "optionalDependencies"] {
            for (_, entry) in importers[dir.as_str()][section].as_hash().into_iter().flatten() {
                if let Some(link) = entry["version"].as_str().and_then(|v| v.strip_prefix("link:")) {
                    todo.push(normalize(&Path::new(&dir).join(link)));
                }
            }
        }
    }
    Ok(seen)
}

/// `@scope/name@1.2.3(peer@1)` -> `@scope/name`.
fn snapshot_name(key: &str) -> &str {
    let from = usize::from(key.starts_with('@'));
    key[from..].find('@').map_or(key, |i| &key[..from + i])
}

fn pnpm_closure(doc: &Yaml, top: &str) -> BTreeSet<String> {
    let importers = &doc["importers"];
    let snapshots = &doc["snapshots"];
    let (mut seen, mut dirs, mut keys) = (BTreeSet::new(), vec![(top.to_string(), true)], Vec::new());
    let mut visited_dirs = BTreeSet::new();
    while let Some((dir, top_level)) = dirs.pop() {
        if !visited_dirs.insert(dir.clone()) {
            continue;
        }
        // a library's dev tools are not in the app: only the deployable's own devDependencies count
        let sections: &[&str] = if top_level { &["dependencies", "devDependencies", "optionalDependencies"] } else { &["dependencies", "optionalDependencies"] };
        for section in sections {
            for (name, entry) in importers[dir.as_str()][*section].as_hash().into_iter().flatten() {
                let (Some(name), Some(version)) = (name.as_str(), entry["version"].as_str()) else { continue };
                match version.strip_prefix("link:") {
                    Some(link) => dirs.push((normalize(&Path::new(&dir).join(link)), false)),
                    None => keys.push(key(name, version)),
                }
            }
        }
    }
    while let Some(k) = keys.pop() {
        if !seen.insert(k.clone()) {
            continue;
        }
        for section in ["dependencies", "optionalDependencies"] {
            for (name, version) in snapshots[k.as_str()][section].as_hash().into_iter().flatten() {
                if let (Some(name), Some(version)) = (name.as_str(), version.as_str()) {
                    keys.push(key(name, version));
                }
            }
        }
    }
    seen
}

/// A snapshot key; an alias (`string-width-cjs: string-width@4.2.3`) names its target itself.
fn key(name: &str, version: &str) -> String {
    if version.chars().next().is_some_and(|c| c.is_ascii_digit()) { format!("{name}@{version}") } else { version.to_string() }
}

fn normalize(path: &Path) -> String {
    let mut out = PathBuf::new();
    for c in path.components() {
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

/// The crates of the roots (package names) that a `Cargo.lock` resolves, each with its version, source and checksum.
pub(crate) fn cargo_closure(lock: &toml::Table, roots: &[String]) -> BTreeSet<String> {
    let packages: Vec<&toml::Table> = lock.get("package").and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_table).collect();
    let field = |p: &toml::Table, k: &str| p.get(k).and_then(toml::Value::as_str).unwrap_or_default().to_string();
    let mut by_name: BTreeMap<String, Vec<&toml::Table>> = BTreeMap::new();
    for p in &packages {
        by_name.entry(field(p, "name")).or_default().push(p);
    }
    // a dependency is written `name`, `name version` or `name version (source)`
    let find = |spec: &str| -> Option<&toml::Table> {
        let mut words = spec.split(' ');
        let (name, version) = (words.next()?, words.next());
        by_name.get(name)?.iter().find(|p| version.is_none_or(|v| field(p, "version") == v)).copied()
    };
    let (mut seen, mut stack): (BTreeSet<String>, Vec<&toml::Table>) = (BTreeSet::new(), roots.iter().filter_map(|r| find(r)).collect());
    while let Some(p) = stack.pop() {
        if !seen.insert(format!("{} {} {} {}", field(p, "name"), field(p, "version"), field(p, "source"), field(p, "checksum"))) {
            continue;
        }
        stack.extend(p.get("dependencies").and_then(toml::Value::as_array).into_iter().flatten().filter_map(toml::Value::as_str).filter_map(&find));
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOCK: &str = "
importers:
  app:
    dependencies:
      '@lib/ui':
        specifier: workspace:*
        version: link:../libs/ui
      react:
        specifier: ^19
        version: 19.2.3
    devDependencies:
      babel-preset:
        specifier: ^1
        version: 1.0.0
  libs/ui:
    dependencies:
      sonner:
        specifier: ^0.26
        version: 0.26.4(patch_hash=aaa)(react@19.2.3)
    devDependencies:
      eslint:
        specifier: ^9
        version: 9.0.0
  cli:
    devDependencies:
      vitest:
        specifier: ^3
        version: 3.0.0
snapshots:
  react@19.2.3: {}
  babel-preset@1.0.0:
    dependencies:
      helper: 2.0.0
  helper@2.0.0: {}
  sonner@0.26.4(patch_hash=aaa)(react@19.2.3):
    dependencies:
      react: 19.2.3
  eslint@9.0.0: {}
  vitest@3.0.0: {}
";

    fn closure(text: &str) -> BTreeSet<String> {
        pnpm_closure(&YamlLoader::load_from_str(text).unwrap()[0], "app")
    }

    #[test]
    fn an_app_reaches_its_own_dev_tools_and_its_libraries_runtime_packages_only() {
        // Given an app linking a library, When its closure is taken, Then it has its dev preset, the library's patched
        // package and their dependencies, and neither the library's nor another importer's dev tools
        assert_eq!(
            closure(LOCK),
            ["babel-preset@1.0.0", "helper@2.0.0", "react@19.2.3", "sonner@0.26.4(patch_hash=aaa)(react@19.2.3)"].map(String::from).into()
        );
    }

    #[test]
    fn a_new_patch_or_version_in_the_closure_differs_and_an_unrelated_bump_does_not() {
        let before = closure(LOCK);
        // When the library's package is patched anew, Then that package differs
        let repatched = closure(&LOCK.replace("patch_hash=aaa", "patch_hash=bbb"));
        assert_eq!(differ(&before, &repatched, |k| snapshot_name(k).to_string()), ["sonner"]);
        // When another importer's dev tool moves, Then nothing differs
        let bumped = closure(&LOCK.replace("version: 3.0.0", "version: 3.1.0").replace("vitest@3.0.0", "vitest@3.1.0"));
        assert!(differ(&before, &bumped, |k| snapshot_name(k).to_string()).is_empty());
        assert_eq!(snapshot_name("@scope/name@1.0.0(peer@2)"), "@scope/name");
    }

    #[test]
    fn a_service_reaches_its_crates_dependencies_and_not_another_service_s() {
        let lock = |serde: &str| -> toml::Table {
            format!(
                "[[package]]\nname = \"svc\"\nversion = \"0.1.0\"\ndependencies = [\"kit\", \"serde 1.0.{serde}\"]\n\n\
                 [[package]]\nname = \"kit\"\nversion = \"0.1.0\"\n\n\
                 [[package]]\nname = \"serde\"\nversion = \"1.0.{serde}\"\nsource = \"registry\"\nchecksum = \"c{serde}\"\n\n\
                 [[package]]\nname = \"other\"\nversion = \"0.1.0\"\ndependencies = [\"tokio\"]\n\n\
                 [[package]]\nname = \"tokio\"\nversion = \"1.0.0\"\nsource = \"registry\"\nchecksum = \"t\"\n"
            )
            .parse()
            .unwrap()
        };
        // Given a service crate using kit and serde, When its closure is taken, Then another service's tokio is not in it
        let before = cargo_closure(&lock("1"), &["svc".into()]);
        assert_eq!(before.len(), 3, "{before:?}");
        // When serde moves, Then serde differs
        assert_eq!(differ(&before, &cargo_closure(&lock("2"), &["svc".into()]), |k| k.split(' ').next().unwrap().to_string()), ["serde"]);
    }
}
