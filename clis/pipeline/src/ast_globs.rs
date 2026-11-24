//! The `files:` globs of the ast-grep rules, generated from the package types (`wf check --layout`, region `ast-grep-files`): a typed
//! Rust package gets the rules of its type the day it is typed, and no rule file lists a crate by hand.
use serde_json::Value as Json;
use std::collections::BTreeSet;
use std::path::Path;

/// A package as the generator sees it: its directory, its declared type and its `[package.metadata.arch]` table.
pub(crate) struct Typed<'a> {
    pub dir: &'a str,
    pub ty: Option<&'a str>,
    pub options: &'a Json,
}

fn strings(value: &Json) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

/// A package is pure when its type says so or the package opts in with `pure = true` (a flat adapter crate that holds only rules and ports).
pub(crate) fn is_pure(ty: Option<&str>, options: &Json, types: &Json) -> bool {
    ty.and_then(|t| types.get(t)).is_some_and(|s| s["pure"].as_bool() == Some(true)) || options["pure"].as_bool() == Some(true)
}

/// The glob lines of one rule file. An entry names the packages it covers by `types` (and `pureOverride` adds every package that opts in
/// as pure) and what it covers in them by `rings`: `["domain"]` is `src/domain/**/*.rs`, `"*"` is every ring of the type, and the
/// entry's `ringFiles` adds `src/<ring>.rs` where that file exists. Without `rings` it covers the whole `src`.
pub(crate) fn render(root: &Path, entry: &Json, types: &Json, packages: &[Typed]) -> String {
    let wanted = strings(&entry["types"]);
    let pure_override = entry["pureOverride"].as_bool() == Some(true);
    let ring_files = entry["ringFiles"].as_bool() == Some(true);
    let all_rings = entry["rings"].as_str() == Some("*");
    let rings = strings(&entry["rings"]);
    // ast-grep matches a glob against the path from the directory of its sgconfig.yml (`backend/` for `backend/arch-rules`), so a package
    // below that directory is spelled without it and one outside it keeps its repository path (`../libraries/x` is matched by `**/libraries/x`).
    let base = Path::new(entry["file"].as_str().unwrap_or("")).parent().and_then(Path::parent).and_then(Path::to_str).unwrap_or("");
    let mut globs = BTreeSet::new();
    for p in packages {
        let shown = p.dir.strip_prefix(base).map(|r| r.trim_start_matches('/')).filter(|_| !base.is_empty()).unwrap_or(p.dir);
        let Some(ty) = p.ty.filter(|t| types.get(*t).is_some_and(|s| s["ecosystem"] == "rust")) else { continue };
        let by_type = wanted.iter().any(|t| t == ty);
        let selected = if rings.is_empty() && !all_rings { by_type || (pure_override && p.options["pure"].as_bool() == Some(true)) } else { wanted.is_empty() || by_type };
        if !selected {
            continue;
        }
        let src = root.join(p.dir).join("src");
        if !src.is_dir() {
            continue;
        }
        if rings.is_empty() && !all_rings {
            globs.insert(format!("**/{shown}/src/**/*.rs"));
            continue;
        }
        let own: Vec<String> = types[ty]["rings"].as_object().map(|r| r.keys().cloned().collect()).unwrap_or_default();
        let names = if all_rings { own.clone() } else { rings.iter().filter(|r| own.contains(r)).cloned().collect() };
        for ring in names {
            if src.join(&ring).is_dir() {
                globs.insert(format!("**/{shown}/src/{ring}/**/*.rs"));
            }
            if ring_files && src.join(format!("{ring}.rs")).is_file() {
                globs.insert(format!("**/{shown}/src/{ring}.rs"));
            }
        }
    }
    globs.iter().map(|g| format!("  - \"{g}\"\n")).collect()
}
