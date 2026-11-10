//! Dependency rules between crates that Cargo itself does not hold (docs/plans/rca-parity/plan.md §3, "Rules"). Read from the manifests with
//! no toolchain, like `core_purity`:
//!
//! - **`exec-testkit` is a dev-dependency only**, and only of `rca-service`, `rca-exec`, `rca-cli-driver`, `t3-serve` and `orca-serve` (its integration chain):
//!   it holds the scripted CLI and the temp git repo of their tests, so a production build that reached it would ship test doubles.
//! - **An adapter library depends on nothing that consumes it** (docs/solutions/architecture/hexagonal-architecture-by-package-type.md, type 6):
//!   Cargo refuses a normal dependency cycle but allows one through a `dev-dependency`, which would make a consumer's types part of the library's
//!   own tests and pull its crate graph into the library's Nx inputs.
//!
//! - **A pure crate depends on no adapter library**: the adapter libraries are all I/O. Which crates are pure is not decided here: the globs of the
//!   ast-grep rules that match `does-io` are that registry (`core_purity::registry_dirs`).
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use toml::Value;

/// The crate of the scripted CLI and the temp git repository.
const TESTKIT: &str = "exec-testkit";
/// The only crates that may name it, and only under `[dev-dependencies]`: the service and the executor run against the scripted CLI, the CLI
/// driver reads its recordings, and the T3 mapper's integration chain runs all of them as built binaries.
const TESTKIT_USERS: &[&str] = &["rca-service", "rca-exec", "rca-cli-driver", "t3-serve", "orca-serve"];
/// The adapter libraries: all the I/O against one outside system and no product rules (the agent API client over WebSocket, the CDP client, the
/// HTTP client, telemetry, the service kit).
const ADAPTER_LIBRARIES: &[&str] = &["agent-api-client", "cdp", "cli-http", "telemetry", "service-kit"];

const TOPS: [&str; 3] = ["clis", "backend", "libraries"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Normal,
    Dev,
    Build,
}

/// A crate of the repository: its package name and what it names under each dependency table, by the package a dependency really is.
#[derive(Clone, Debug)]
pub struct Crate {
    name: String,
    deps: Vec<(String, Kind)>,
}

impl Crate {
    /// None for a manifest with no `[package]` (a workspace root).
    pub fn parse(manifest: &Value) -> Option<Crate> {
        let name = manifest.get("package")?.get("name")?.as_str()?.to_string();
        let mut deps = Vec::new();
        let mut collect = |holder: &Value| {
            for (table, kind) in [("dependencies", Kind::Normal), ("dev-dependencies", Kind::Dev), ("build-dependencies", Kind::Build)] {
                for (key, spec) in holder.get(table).and_then(Value::as_table).into_iter().flatten() {
                    deps.push((spec.get("package").and_then(Value::as_str).unwrap_or(key).to_string(), kind));
                }
            }
        };
        collect(manifest);
        for target in manifest.get("target").and_then(Value::as_table).into_iter().flat_map(|t| t.values()) {
            collect(target);
        }
        Some(Crate { name, deps })
    }
}

/// The crates each crate reaches through normal and build dependencies: what a production build of it contains.
fn reach(crates: &[Crate]) -> BTreeMap<&str, BTreeSet<&str>> {
    let local: BTreeSet<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    let direct: BTreeMap<&str, Vec<&str>> = crates.iter().map(|c| (c.name.as_str(), c.deps.iter().filter(|(_, k)| *k != Kind::Dev).map(|(d, _)| d.as_str()).filter(|d| local.contains(d)).collect())).collect();
    let mut all = BTreeMap::new();
    for start in &local {
        let mut seen = BTreeSet::new();
        let mut stack = direct[start].clone();
        while let Some(next) = stack.pop() {
            if seen.insert(next) {
                stack.extend(direct[next].iter().copied());
            }
        }
        all.insert(*start, seen);
    }
    all
}

/// One line per broken rule among `crates`; `pure` names the crates of the purity registry.
pub fn violations_of(crates: &[Crate], pure: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    for krate in crates.iter().filter(|c| pure.contains(&c.name)) {
        let used: BTreeSet<&str> = krate.deps.iter().filter(|(d, k)| *k != Kind::Dev && ADAPTER_LIBRARIES.contains(&d.as_str())).map(|(d, _)| d.as_str()).collect();
        found.extend(used.into_iter().map(|library| format!("the pure crate {} depends on the adapter library {library}", krate.name)));
    }
    for krate in crates {
        for (dep, kind) in krate.deps.iter().filter(|(d, _)| d == TESTKIT) {
            if *kind != Kind::Dev {
                found.push(format!("{} depends on {dep} outside dev-dependencies", krate.name));
            } else if !TESTKIT_USERS.contains(&krate.name.as_str()) {
                found.push(format!("{} is not one of {}, which alone may dev-depend on {dep}", krate.name, TESTKIT_USERS.join(" and ")));
            }
        }
    }
    let reached = reach(crates);
    for library in crates.iter().filter(|c| ADAPTER_LIBRARIES.contains(&c.name.as_str())) {
        let consumers: BTreeSet<&str> = reached.iter().filter(|(name, through)| **name != library.name && through.contains(library.name.as_str())).map(|(name, _)| *name).collect();
        let own: BTreeSet<&str> = library.deps.iter().map(|(d, _)| d.as_str()).filter(|d| consumers.contains(d)).collect();
        found.extend(own.into_iter().map(|consumer| format!("the adapter library {} depends on {consumer}, which consumes it", library.name)));
    }
    found
}

/// `<finding>` for each broken rule among the crates under `clis`, `backend` and `libraries`; a manifest that cannot be read is a finding.
pub fn violations(root: &Path) -> Vec<String> {
    let mut files: Vec<PathBuf> = Vec::new();
    for top in TOPS {
        crate::library_paths::manifests(&root.join(top), &mut files);
    }
    files.sort();
    let registry: BTreeSet<PathBuf> = crate::core_purity::registry_dirs(root).iter().map(|dir| crate::core_purity::identity(dir)).collect();
    let (mut crates, mut pure, mut found) = (Vec::new(), BTreeSet::new(), Vec::new());
    for file in files {
        match std::fs::read_to_string(&file).ok().and_then(|t| t.parse::<Value>().ok()) {
            Some(manifest) => {
                let parsed = Crate::parse(&manifest);
                if let (Some(krate), Some(dir)) = (&parsed, file.parent()) {
                    if registry.contains(&crate::core_purity::identity(dir)) {
                        pure.insert(krate.name.clone());
                    }
                }
                crates.extend(parsed);
            }
            None => found.push(format!("{} cannot be read", file.strip_prefix(root).unwrap_or(&file).display())),
        }
    }
    found.extend(violations_of(&crates, &pure));
    found
}

pub fn count(root: &Path) -> usize {
    violations(root).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    fn krate(text: &str) -> Crate {
        Crate::parse(&text.parse::<Value>().unwrap()).unwrap()
    }

    fn manifest(name: &str, tables: &[(&str, &[&str])]) -> String {
        let mut out = format!("[package]\nname = \"{name}\"\n");
        for (table, deps) in tables {
            out.push_str(&format!("[{table}]\n"));
            for dep in *deps {
                out.push_str(&format!("{dep} = {{ path = \"../{dep}\" }}\n"));
            }
        }
        out
    }

    #[test]
    fn given_the_testkit_under_each_table_when_checked_then_only_a_dev_dependency_of_its_users_passes() {
        // Given the testkit named as a normal dependency, as a build dependency, as a dev-dependency of a crate that is not a user, and as a
        // dev-dependency of each user
        let users: Vec<Crate> = TESTKIT_USERS.iter().map(|user| krate(&manifest(user, &[("dev-dependencies", &[TESTKIT])]))).collect();
        let crates = [
            vec![
                krate(&manifest("shipper", &[("dependencies", &[TESTKIT])])),
                krate(&manifest("builder", &[("build-dependencies", &[TESTKIT])])),
                krate(&manifest("t3-core", &[("dev-dependencies", &[TESTKIT])])),
                krate(&manifest(TESTKIT, &[])),
            ],
            users,
        ]
        .concat();
        // When the rules run
        let found = violations_of(&crates, &none()).join("\n");
        // Then the three misuses are named and the users are not
        assert!(found.contains(&format!("shipper depends on {TESTKIT} outside dev-dependencies")), "{found}");
        assert!(found.contains(&format!("builder depends on {TESTKIT} outside dev-dependencies")), "{found}");
        assert!(found.contains(&format!("t3-core is not one of {}", TESTKIT_USERS.join(" and "))), "{found}");
        assert_eq!(found.lines().count(), 3, "{found}");
    }

    #[test]
    fn given_a_renamed_dependency_and_a_target_table_when_read_then_the_real_package_is_what_counts() {
        // Given the testkit renamed under a normal table and named under a platform's table
        let renamed = krate(&format!("[package]\nname = \"a\"\n[dependencies]\nkit = {{ package = \"{TESTKIT}\", path = \"../x\" }}\n"));
        let targeted = krate(&format!("[package]\nname = \"b\"\n[target.'cfg(unix)'.dependencies]\n{TESTKIT} = \"0.1\"\n"));
        // When checked, Then both are normal dependencies on it
        assert_eq!(violations_of(&[renamed, targeted], &none()).len(), 2);
    }

    #[test]
    fn given_an_adapter_library_when_it_dev_depends_on_a_consumer_then_it_is_named_and_a_dependency_below_it_is_not() {
        // Given the client library, a mapper core that depends on it, an area crate on that core, a contract crate below the library, and
        // the library's dev-dependencies on the area crate (a cycle Cargo allows), on the contract crate and on a crate that is nobody's consumer
        let library = ADAPTER_LIBRARIES[0];
        let crates = vec![
            krate(&manifest(library, &[("dependencies", &["agent-api-proto"]), ("dev-dependencies", &["t3-vcs", "agent-api-proto", "fake-server"])])),
            krate(&manifest("agent-api-proto", &[])),
            krate(&manifest("fake-server", &[])),
            krate(&manifest("t3-core", &[("dependencies", &[library])])),
            krate(&manifest("t3-vcs", &[("dependencies", &["t3-core"])])),
        ];
        // When the rules run
        let found = violations_of(&crates, &none());
        // Then the area crate, which consumes the library through the core, is the one finding
        assert_eq!(found, [format!("the adapter library {library} depends on t3-vcs, which consumes it")]);
    }

    #[test]
    fn given_a_crate_that_is_no_adapter_library_when_it_dev_depends_on_its_consumer_then_the_rule_does_not_apply() {
        // Given a crate outside the registry that dev-depends on a crate that depends on it
        let crates = vec![krate(&manifest("t3-core", &[("dev-dependencies", &["t3-vcs"])])), krate(&manifest("t3-vcs", &[("dependencies", &["t3-core"])]))];
        // When checked, Then nothing is reported
        assert!(violations_of(&crates, &none()).is_empty());
    }

    #[test]
    fn given_pure_crates_when_one_depends_on_an_adapter_library_then_it_is_named_unless_the_dependency_is_a_dev_one_or_the_crate_is_not_pure() {
        // Given a pure crate on the service kit, a pure crate that only dev-depends on the HTTP client, a pure crate on a contract crate, and an
        // impure crate on telemetry
        let crates = vec![
            krate(&manifest("a-core", &[("dependencies", &["service-kit"])])),
            krate(&manifest("b-core", &[("dev-dependencies", &["cli-http"])])),
            krate(&manifest("c-core", &[("dependencies", &["agent-api-proto"])])),
            krate(&manifest("d-shell", &[("dependencies", &["telemetry"])])),
        ];
        let pure: BTreeSet<String> = ["a-core", "b-core", "c-core"].iter().map(|n| n.to_string()).collect();
        // When the rules run
        let found = violations_of(&crates, &pure);
        // Then only the first is named
        assert_eq!(found, ["the pure crate a-core depends on the adapter library service-kit"]);
    }

    #[test]
    fn given_a_workspace_root_when_parsed_then_it_is_no_crate() {
        assert!(Crate::parse(&"[workspace]\nmembers = []\n".parse::<Value>().unwrap()).is_none());
    }
}
