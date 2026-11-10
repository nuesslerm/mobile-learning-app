//! Module specifiers to repository paths: relative imports, and a workspace package's subpath through its `exports` map.
use crate::ports::Tree;
use serde_json::Value;
use std::collections::BTreeMap;

/// `a/b/../c` to `a/c`.
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = vec![];
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// A relative specifier to a file that exists, trying it as written, with each extension, as a directory's index, and a
/// `.js` specifier as the `.ts` source it compiles from.
pub fn relative(tree: &dyn Tree, from: &str, spec: &str, exts: &[&str]) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let base = normalize(&format!("{}/{spec}", parent(from)));
    let stem = base.strip_suffix(".js").map(|s| vec![format!("{s}.ts"), format!("{s}.tsx")]).unwrap_or_default();
    let candidates = stem
        .into_iter()
        .chain([base.clone()])
        .chain(exts.iter().map(|e| format!("{base}{e}")))
        .chain(exts.iter().map(|e| format!("{base}/index{e}")));
    candidates.into_iter().find(|c| exts.iter().any(|e| c.ends_with(e)) && tree.is_file(c))
}

/// The workspace packages under `dir`: name to (package directory, `exports` map).
pub fn packages(tree: &dyn Tree, dir: &str) -> BTreeMap<String, (String, Value)> {
    tree.files(dir)
        .into_iter()
        .filter(|f| f.ends_with("/package.json") && f[dir.len() + 1..].matches('/').count() == 1)
        .filter_map(|f| {
            let json: Value = serde_json::from_str(&tree.read(&f)?).ok()?;
            Some((json["name"].as_str()?.to_string(), (parent(&f).to_string(), json["exports"].clone())))
        })
        .collect()
}

/// `@scope/name/sub` through the package's `exports["./sub"]` (its `types`, else `default`).
pub fn package(packages: &BTreeMap<String, (String, Value)>, spec: &str) -> Option<String> {
    let mut split = spec.splitn(3, '/');
    let (scope, name) = (split.next()?, split.next()?);
    let (dir, exports) = packages.get(&format!("{scope}/{name}"))?;
    let key = split.next().map_or(".".to_string(), |sub| format!("./{sub}"));
    let entry = &exports[key.as_str()];
    let target = entry.as_str().or_else(|| entry["types"].as_str()).or_else(|| entry["default"].as_str())?;
    Some(normalize(&format!("{dir}/{target}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MemoryTree;

    #[test]
    fn a_relative_specifier_resolves_with_or_without_its_extension_and_to_an_index() {
        let tree = MemoryTree::of(&[("a/b.ts", ""), ("a/c/index.tsx", ""), ("a/d.ts", "")]);
        assert_eq!(relative(&tree, "a/x.ts", "./b.ts", &[".ts", ".tsx"]).as_deref(), Some("a/b.ts"));
        assert_eq!(relative(&tree, "a/x.ts", "./c", &[".ts", ".tsx"]).as_deref(), Some("a/c/index.tsx"));
        assert_eq!(relative(&tree, "a/c/y.ts", "../d.js", &[".ts", ".tsx"]).as_deref(), Some("a/d.ts"));
        assert_eq!(relative(&tree, "a/x.ts", "react", &[".ts"]), None);
    }

    #[test]
    fn a_package_subpath_resolves_through_its_exports_map() {
        let tree = MemoryTree::of(&[("packages/rt/package.json", r#"{"name":"@t/rt","exports":{"./state":{"types":"./src/state.ts"},".":"./src/index.ts"}}"#)]);
        let pkgs = packages(&tree, "packages");
        assert_eq!(package(&pkgs, "@t/rt/state").as_deref(), Some("packages/rt/src/state.ts"));
        assert_eq!(package(&pkgs, "@t/rt").as_deref(), Some("packages/rt/src/index.ts"));
        assert_eq!(package(&pkgs, "@t/rt/missing"), None);
    }
}
