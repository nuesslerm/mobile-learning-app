//! The Rust cores' purity (docs/solutions/architecture/hexagonal-architecture-by-package-type.md, "The dependency rule"): a `core` crate
//! does no I/O, so it names no I/O crate and its tokio is built without `fs`, `net`, `process`, `signal` or `io-util`. The workspace's own
//! tokio entry carries none of them either, or every core would inherit them: a crate's `features` only add to it. Read from the
//! manifests, the way `tooling/nx/cargo-crates.js` does, because the budget check runs on every commit with no toolchain.
use std::collections::BTreeSet;
use std::path::Path;
use toml::Value;

/// Crates a core must not depend on: an HTTP server or client, a socket protocol, an OS directory lookup.
const IO_CRATES: &[&str] = &["reqwest", "axum", "hyper", "hyper-util", "tokio-tungstenite", "tower", "tower-http", "clap", "dirs"];

/// tokio features that reach a file, a socket, a child process or a signal.
const IO_FEATURES: &[&str] = &["fs", "io-util", "net", "process", "signal", "full"];

/// The tokio features a manifest entry names, with the workspace's own added when the entry inherits them.
fn tokio_features(entry: &Value, workspace: &Value) -> Vec<String> {
    let own = entry.get("features").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from);
    let inherited = if entry.get("workspace").and_then(Value::as_bool) == Some(true) { tokio_features(&workspace["workspace"]["dependencies"]["tokio"], workspace) } else { Vec::new() };
    own.chain(inherited).collect()
}

fn io_only(features: Vec<String>) -> Vec<String> {
    features.into_iter().filter(|f| IO_FEATURES.contains(&f.as_str())).collect()
}

/// What breaks the rule in the workspace manifest and in each `(name, manifest)` core: one line per finding.
pub fn violations(workspace: &Value, cores: &[(String, Value)]) -> Vec<String> {
    let mut found = Vec::new();
    let shared = io_only(tokio_features(&workspace["workspace"]["dependencies"]["tokio"], workspace));
    if !shared.is_empty() {
        found.push(format!("the workspace tokio enables {shared:?}, which every core inherits"));
    }
    for (name, manifest) in cores {
        let deps = manifest.get("dependencies").and_then(Value::as_table).cloned().unwrap_or_default();
        for crate_name in IO_CRATES.iter().filter(|c| deps.contains_key(**c)) {
            found.push(format!("{name} depends on the I/O crate {crate_name}"));
        }
        if let Some(entry) = deps.get("tokio") {
            let features = io_only(tokio_features(entry, workspace));
            if !features.is_empty() {
                found.push(format!("{name} builds tokio with {features:?}"));
            }
        }
    }
    found
}

fn read(path: &Path) -> Option<Value> {
    std::fs::read_to_string(path).ok()?.parse().ok()
}

/// A crate directory as the one name it is known by, so a crate that a workspace names and a rule's glob names too is checked once.
pub(crate) fn identity(dir: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())
}

/// How many findings the workspace whose manifest is `manifest` has: 1 for a manifest that cannot be read, so a broken one fails the
/// budget instead of passing it. Its `core` members are checked by name; `checked` collects them so the rules' registry does not count them again.
fn count_workspace(manifest: &Path, checked: &mut BTreeSet<std::path::PathBuf>) -> usize {
    let Some(workspace) = read(manifest) else { return 1 };
    let base = manifest.parent().unwrap_or(Path::new("."));
    let mut cores = Vec::new();
    for member in workspace["workspace"]["members"].as_array().into_iter().flatten().filter_map(Value::as_str).filter(|m| *m == "core" || m.ends_with("/core")) {
        match read(&base.join(member).join("Cargo.toml")) {
            Some(core) => {
                checked.insert(identity(&base.join(member)));
                cores.push((format!("{}/{member}", base.display()), core));
            }
            None => return 1,
        }
    }
    violations(&workspace, &cores).len()
}

/// Where the ast-grep rules live. A rule that matches `does-io` keeps the code its `files` globs name from doing I/O (the wire-contract crates
/// of `libraries/agent-api`, `rca-exec`'s domain module), and the manifest of each crate those globs name is held to the same dependency rule
/// as a core's.
const RULES_DIR: &str = "backend/arch-rules";

/// The text of every rule file under `root` that matches `does-io`, in file-name order.
fn purity_rules(root: &Path) -> Vec<String> {
    let mut files: Vec<_> = std::fs::read_dir(root.join(RULES_DIR)).map(|dir| dir.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "yml")).collect()).unwrap_or_default();
    files.sort();
    files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).filter(|yml| yml.lines().any(|l| l.trim() == "matches: does-io")).collect()
}

/// The globs of one list of a rule file (`files:` or `ignores:`), one `- "glob"` per line; a comment line inside the list is skipped.
fn list_globs(yml: &str, list: &str) -> Vec<String> {
    let mut in_list = false;
    let mut globs = Vec::new();
    for line in yml.lines() {
        if let Some(item) = line.trim_start().strip_prefix("- ").filter(|_| in_list && line.starts_with(' ')) {
            globs.push(item.trim().trim_matches(['"', '\'']).to_string());
        } else if !(in_list && line.trim_start().starts_with('#')) {
            in_list = line.trim_end() == format!("{list}:");
        }
    }
    globs
}

/// The globs of a rule file's `files:` list.
fn rule_globs(yml: &str) -> Vec<String> {
    list_globs(yml, "files")
}

/// The crates whose whole `src` the rule's `ignores:` names (`<crate>/src/**`): the rule leaves them alone, so they are not pure crates.
fn ignored_crates(yml: &str) -> Vec<Vec<String>> {
    list_globs(yml, "ignores").iter().filter(|glob| glob.ends_with("/src/**") || glob.ends_with("/src/**/*.rs")).filter_map(|glob| crate_dirs_of(glob)).collect()
}

/// The crate directories a rule glob stands for, with `*` standing for any one directory: what precedes `/src/` without its leading `**/`.
/// None for a glob that is no crate's sources. ast-grep reads a glob relative to `backend/` (where `sgconfig.yml` is), so a crate of a service
/// is spelled from there (`**/remote-chat-agent/agent-core/src/**/*.rs`) and one outside it from the repository root (`**/libraries/...`).
fn crate_dirs_of(glob: &str) -> Option<Vec<String>> {
    let (before, _) = glob.split_once("/src/")?;
    Some(before.trim_start_matches("**/").split('/').map(String::from).collect())
}

/// The directories under `root` that `segments` name, each a crate (it has a `Cargo.toml`).
fn expand(root: &Path, segments: &[String]) -> Vec<std::path::PathBuf> {
    let mut found = vec![root.to_path_buf()];
    for segment in segments {
        found = found
            .into_iter()
            .flat_map(|dir| {
                if segment == "*" {
                    std::fs::read_dir(&dir).map(|entries| entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default()
                } else {
                    vec![dir.join(segment)]
                }
            })
            .collect();
    }
    found.retain(|dir| dir.join("Cargo.toml").is_file());
    found.sort();
    found
}

/// The registry of pure crates: the directories the purity rules' globs name, sorted and without repeats. The one list every check of a pure crate
/// reads (the ast-grep rules, this module's manifest rule, the crate-against-crate rules of `cargo_deps`), so a crate is added in one place.
pub(crate) fn registry_dirs(root: &Path) -> Vec<std::path::PathBuf> {
    let bases = [root.to_path_buf(), root.join("backend")];
    let named = |segments: &Vec<String>| bases.iter().flat_map(|base| expand(base, segments)).collect::<Vec<_>>();
    let mut crates: Vec<std::path::PathBuf> = Vec::new();
    for yml in purity_rules(root) {
        let ignored: BTreeSet<std::path::PathBuf> = ignored_crates(&yml).iter().flat_map(named).map(|dir| identity(&dir)).collect();
        crates.extend(rule_globs(&yml).iter().filter_map(|g| crate_dirs_of(g)).flat_map(|segments| named(&segments)).filter(|dir| !ignored.contains(&identity(dir))));
    }
    crates.sort();
    crates.dedup();
    crates
}

/// How many findings the crates that the purity rules' globs name have: each manifest held to the rule of a core (no I/O crate, no I/O tokio
/// feature), and a manifest that cannot be read is one finding. A crate already in `checked` is skipped. 0 when the repository has no such rule.
fn count_purity_crates(root: &Path, checked: &BTreeSet<std::path::PathBuf>) -> usize {
    let bare: Value = "[workspace]\n[workspace.dependencies]\ntokio = { version = \"1\" }\n".parse().expect("a literal manifest");
    let mut found = 0;
    for dir in registry_dirs(root).into_iter().filter(|dir| !checked.contains(&identity(dir))) {
        match read(&dir.join("Cargo.toml")) {
            Some(manifest) => found += violations(&bare, &[(dir.display().to_string(), manifest)]).len(),
            None => found += 1,
        }
    }
    found
}

/// How many findings the backend workspaces under `root` have: the shared `backend/Cargo.toml` and the workspace of every service that
/// has one of its own (`backend/<service>/Cargo.toml` with a `[workspace]` table), plus the crates the purity rules name. 0 when there is none.
pub fn count(root: &Path) -> usize {
    let backend = root.join("backend");
    let mut manifests = vec![backend.join("Cargo.toml")];
    if let Ok(entries) = std::fs::read_dir(&backend) {
        let mut own: Vec<_> = entries.flatten().map(|e| e.path().join("Cargo.toml")).filter(|m| read(m).is_some_and(|t| t.get("workspace").is_some())).collect();
        own.sort();
        manifests.extend(own);
    }
    let mut checked = BTreeSet::new();
    let workspaces: usize = manifests.iter().filter(|m| m.exists()).map(|m| count_workspace(m, &mut checked)).sum();
    workspaces + count_purity_crates(root, &checked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml(text: &str) -> Value {
        text.parse().unwrap()
    }

    #[test]
    fn given_cores_that_name_io_crates_or_inherit_io_features_when_checked_then_each_is_reported_and_a_clean_core_is_not() {
        // Given a workspace whose tokio enables fs, a core on reqwest, a core that asks tokio for net, and a clean core
        let workspace = toml("[workspace]\nmembers = []\n[workspace.dependencies]\ntokio = { version = \"1\", features = [\"macros\", \"fs\"] }\n");
        let cores = vec![
            ("a/core".to_string(), toml("[dependencies]\nreqwest = { workspace = true }\n")),
            ("b/core".to_string(), toml("[dependencies]\ntokio = { workspace = true, features = [\"net\"] }\n")),
            ("c/core".to_string(), toml("[dependencies]\nserde = \"1\"\n")),
        ];

        // When the rule runs
        let found = violations(&workspace, &cores).join("\n");

        // Then the workspace, a/core and b/core are named, and c/core is not
        assert!(found.contains("workspace tokio enables"), "{found}");
        assert!(found.contains("a/core depends on the I/O crate reqwest"), "{found}");
        assert!(found.contains("b/core builds tokio with") && found.contains("net"), "{found}");
        assert!(!found.contains("c/core"), "{found}");
    }

    #[test]
    fn given_a_rule_file_when_its_globs_are_read_then_each_names_the_crates_it_covers_and_a_glob_that_is_no_crate_names_none() {
        // Given a rule file with two globs under files, an ignore and a message that contains a dash
        let yml = "id: x\nmessage: - not a glob\nfiles:\n  - \"**/libraries/agent-api/*/src/**/*.rs\"\n  - '**/clis/rca-exec/src/domain.rs'\nignores:\n  - \"**/src/testing.rs\"\nrule:\n  matches: does-io\n";
        // When the globs are read
        let globs = rule_globs(yml);
        // Then only the files list is read, and a glob names the directories before its /src/ (a file glob inside an adapter crate names its crate)
        assert_eq!(globs, ["**/libraries/agent-api/*/src/**/*.rs", "**/clis/rca-exec/src/domain.rs"]);
        assert_eq!(crate_dirs_of(&globs[0]), Some(vec!["libraries".to_string(), "agent-api".into(), "*".into()]));
        assert_eq!(crate_dirs_of("**/Cargo.toml"), None);
    }

    #[test]
    fn given_a_rule_that_ignores_the_whole_src_of_a_crate_when_the_registry_is_read_then_that_crate_is_not_a_pure_crate() {
        // Given a repository whose purity rule names every crate of a directory and ignores the src of one of them (and the testing module of all)
        let root = tempfile::tempdir().unwrap();
        for name in ["wire", "client", "kit"] {
            let dir = root.path().join("libraries/agent-api").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        }
        let rules = root.path().join(RULES_DIR);
        std::fs::create_dir_all(&rules).unwrap();
        let yml = "id: x\nfiles:\n  - \"**/libraries/agent-api/*/src/**/*.rs\"\nignores:\n  - \"**/libraries/agent-api/*/src/testing.rs\"\n  # a comment between two ignores\n  - \"**/libraries/agent-api/client/src/**\"\n  - \"**/libraries/agent-api/kit/src/**/*.rs\"\nrule:\n  matches: does-io\n";
        std::fs::write(rules.join("x.yml"), yml).unwrap();
        // When the registry is read
        let names: Vec<String> = registry_dirs(root.path()).iter().map(|d| d.file_name().unwrap().to_string_lossy().into_owned()).collect();
        // Then only the crate no ignore names whole is in it, and the ignores of the list are all read past the comment
        assert_eq!(names, ["wire"]);
        assert_eq!(list_globs(yml, "ignores").len(), 3);
        assert_eq!(list_globs(yml, "files"), ["**/libraries/agent-api/*/src/**/*.rs"]);
    }

    #[test]
    fn given_a_workspace_tokio_without_io_features_and_a_pure_core_when_checked_then_nothing_is_reported() {
        // Given a workspace tokio with only macros, rt, sync and time, and a core that inherits it and adds rt
        let workspace = toml("[workspace]\nmembers = []\n[workspace.dependencies]\ntokio = { version = \"1\", features = [\"macros\", \"rt\", \"sync\", \"time\"] }\n");
        let cores = vec![("a/core".to_string(), toml("[dependencies]\ntokio = { workspace = true, features = [\"rt\"] }\nserde = \"1\"\n"))];

        // When the rule runs, Then nothing is reported
        assert!(violations(&workspace, &cores).is_empty());
    }
}

/// What the purity rule cannot see: a use case that asks the system for the time, a random id or a file's existence instead of a port.
const AMBIENT_CALLS: &[&str] = &["SystemTime::now", "Instant::now", "Uuid::new_v4", ".exists()"];

const AMBIENT_CORE: &str = "backend/remote-chat-agent/service-core/src";

fn ambient_in(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                ambient_in(&path)
            } else if path.extension().is_some_and(|x| x == "rs") && path.file_name().is_some_and(|n| n != "testing.rs") {
                std::fs::read_to_string(&path).map(|text| text.lines().map(|l| AMBIENT_CALLS.iter().filter(|c| l.contains(**c)).count()).sum()).unwrap_or(0)
            } else {
                0
            }
        })
        .sum()
}

/// Direct clock, randomness and file-probe calls in `service-core` (the budget `core-ambient-calls` only shrinks, as each moves behind a port).
pub fn ambient_calls(root: &Path) -> usize {
    ambient_in(&root.join(AMBIENT_CORE))
}

#[cfg(test)]
mod ambient_tests {
    use super::*;

    #[test]
    fn given_a_core_with_clock_and_random_calls_when_counted_then_each_call_counts_and_testing_rs_is_skipped() {
        let root = std::env::temp_dir().join(format!("ambient-{}", std::process::id()));
        let src = root.join(AMBIENT_CORE);
        std::fs::create_dir_all(src.join("application")).expect("dirs");
        std::fs::write(src.join("application/a.rs"), "let a = Instant::now();\nlet b = Uuid::new_v4(); let c = SystemTime::now();\n").expect("write");
        std::fs::write(src.join("testing.rs"), "let a = Instant::now();\n").expect("write");
        assert_eq!(ambient_calls(&root), 3);
        std::fs::remove_dir_all(&root).ok();
    }
}
