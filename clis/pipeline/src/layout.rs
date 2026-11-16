//! `wf check --layout`: every package declares a type from `data/package-types.json`, sits the way the type says, depends only on what the
//! type allows, and the generated regions of config files match the manifest. Report-only: it prints the findings and exits 0 unless
//! `--strict` is given. Read from the tree with no toolchain, like `ts_packages`.
use serde_json::{json, Value as Json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const MANIFEST: &str = "clis/pipeline/data/package-types.json";
const SKIP: [&str; 5] = ["node_modules", "target", "references", "dist", "fixtures"];

pub const LAYOUT: wf_kit::args::Spec = wf_kit::args::Spec { flags: &["layout", "strict"], positional: wf_kit::args::Positional::None, ..wf_kit::args::Spec::NONE };

pub struct Report {
    pub json: Json,
    pub findings: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Normal,
    /// A normal dependency behind a feature (`optional = true`): how a crate ships its doubles behind `testing`.
    Optional,
    Dev,
}

struct Package {
    dir: String,
    name: String,
    ty: Option<String>,
    options: Json,
    deps: Vec<(String, Kind)>,
}

struct Finding {
    pass: &'static str,
    dir: String,
    message: String,
}

fn read_json(path: &Path) -> Option<Json> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn strings(value: &Json) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(|v| v.as_str().map(str::to_string)).collect()
}

fn rel(root: &Path, dir: &Path) -> String {
    dir.strip_prefix(root).map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
}

fn cargo_package(dir: String, manifest: &toml::Value) -> Option<Package> {
    let package = manifest.get("package")?;
    let name = package.get("name")?.as_str()?.to_string();
    let arch = package.get("metadata").and_then(|m| m.get("arch"));
    let ty = arch.and_then(|a| a.get("type")).and_then(|t| t.as_str()).map(str::to_string);
    let options = arch.and_then(|a| serde_json::to_value(a).ok()).unwrap_or(Json::Null);
    let mut deps = Vec::new();
    for (table, kind) in [("dependencies", Kind::Normal), ("build-dependencies", Kind::Normal), ("dev-dependencies", Kind::Dev)] {
        for (key, entry) in manifest.get(table).and_then(|t| t.as_table()).into_iter().flatten() {
            let real = entry.get("package").and_then(|p| p.as_str()).unwrap_or(key);
            let optional = kind == Kind::Normal && entry.get("optional").and_then(|o| o.as_bool()) == Some(true);
            deps.push((real.to_string(), if optional { Kind::Optional } else { kind }));
        }
    }
    Some(Package { dir, name, ty, options, deps })
}

fn node_package(dir: String, manifest: &Json) -> Package {
    let arch = manifest.get("arch");
    let ty = arch.and_then(|a| a["type"].as_str()).map(str::to_string);
    let mut deps = Vec::new();
    for (table, kind) in [("dependencies", Kind::Normal), ("peerDependencies", Kind::Normal), ("devDependencies", Kind::Dev)] {
        for key in manifest.get(table).and_then(Json::as_object).into_iter().flat_map(|o| o.keys()) {
            deps.push((key.clone(), kind));
        }
    }
    let name = manifest["name"].as_str().map_or_else(|| dir.clone(), str::to_string);
    Package { dir, name, ty, options: arch.cloned().unwrap_or(Json::Null), deps }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<Package>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut subdirs = Vec::new();
    let mut here = BTreeSet::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() {
            if !SKIP.contains(&name.as_str()) && !name.starts_with('.') {
                subdirs.push(entry.path());
            }
        } else {
            here.insert(name);
        }
    }
    let at = rel(root, dir);
    let has_cargo = here.contains("Cargo.toml");
    if has_cargo {
        let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).ok().and_then(|t| t.parse::<toml::Value>().ok());
        out.extend(manifest.and_then(|m| cargo_package(at.clone(), &m)));
    }
    // A package.json next to a Cargo.toml is the Nx wrapper of that crate, not a package of its own.
    if !at.is_empty() && !has_cargo {
        if here.contains("package.json") {
            out.extend(read_json(&dir.join("package.json")).map(|m| node_package(at.clone(), &m)));
        } else if let Some(project) = read_json(&dir.join("project.json")).filter(|p| p.get("arch").is_some()) {
            let name = project["name"].as_str().unwrap_or(&at).to_string();
            let ty = project["arch"]["type"].as_str().map(str::to_string);
            out.push(Package { dir: at.clone(), name, ty, options: project["arch"].clone(), deps: Vec::new() });
        }
    }
    subdirs.sort();
    for sub in subdirs {
        walk(root, &sub, out);
    }
}

fn matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail),
    }
}

fn exists(base: &Path, alternatives: &str) -> bool {
    alternatives.split('|').any(|alt| base.join(alt).exists())
}

fn ring_root(base: &Path, spec: &Json) -> Option<String> {
    strings(&spec["ringRoots"]).into_iter().find(|r| base.join(r).is_dir())
}

fn placement(root: &Path, p: &Package, ty: &str, spec: &Json, out: &mut Vec<Finding>) {
    let mut add = |message: String| out.push(Finding { pass: "placement", dir: p.dir.clone(), message });
    let base = root.join(&p.dir);
    let Some(ring) = ring_root(&base, spec) else { return add(format!("type {ty} has no {} directory", strings(&spec["ringRoots"]).join(" or "))) };
    let at = base.join(&ring);
    let shown = |name: &str| if ring == "." { name.to_string() } else { format!("{ring}/{name}") };
    for need in strings(&spec["requiredFiles"]) {
        if !exists(&at, &need) {
            add(format!("type {ty} requires {}", shown(&need)));
        }
    }
    for group in spec["requiredAnyOf"].as_array().into_iter().flatten() {
        let names = strings(group);
        if !names.iter().any(|n| exists(&at, n)) {
            add(format!("type {ty} requires one of {}", names.iter().map(|n| shown(n)).collect::<Vec<_>>().join(", ")));
        }
    }
    for name in strings(&spec["forbiddenFiles"]).into_iter().chain(strings(&spec["forbiddenDirs"])) {
        if at.join(&name).exists() {
            add(format!("type {ty} forbids {}", shown(&name)));
        }
    }
    let mut allowed = strings(&spec["allowedRoot"]);
    allowed.extend(spec["rings"].as_object().into_iter().flat_map(|r| r.keys().flat_map(|k| [k.clone(), format!("{k}.rs")])));
    allowed.extend(strings(&p.options["widgets"]));
    allowed.extend(strings(&p.options["subFeatures"]));
    let forbidden: Vec<String> = strings(&spec["forbiddenFiles"]).into_iter().chain(strings(&spec["forbiddenDirs"])).collect();
    let mut names: Vec<String> = std::fs::read_dir(&at).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    names.sort();
    for name in names {
        let ignored = name.starts_with('.') || SKIP.contains(&name.as_str());
        if !ignored && !forbidden.contains(&name) && !allowed.iter().any(|a| matches(a, &name)) {
            add(format!("{} is not part of type {ty}; allowed: {}", shown(&name), allowed.join(", ")));
        }
    }
}

fn edges(packages: &[Package], types: &Json, out: &mut Vec<Finding>) {
    let by_name: BTreeMap<&str, &Package> = packages.iter().map(|p| (p.name.as_str(), p)).collect();
    for from in packages {
        for (dep, kind) in &from.deps {
            let Some(to) = by_name.get(dep.as_str()).filter(|t| t.dir != from.dir) else { continue };
            let Some(to_ty) = to.ty.as_deref().filter(|t| types.get(*t).is_some()) else { continue };
            let dev_only = types[to_ty]["devOnly"].as_bool() == Some(true);
            let how = if *kind == Kind::Dev { "dev-depends on" } else { "depends on" };
            let mut add = |message: String| out.push(Finding { pass: "edges", dir: from.dir.clone(), message });
            if dev_only && *kind == Kind::Normal {
                add(format!("depends on {} ({to_ty}), which is reachable only through dev-dependencies", to.name));
                continue;
            }
            if dev_only && *kind == Kind::Optional {
                continue;
            }
            let Some(from_ty) = from.ty.as_deref().and_then(|t| types.get(t).map(|s| (t, s))) else { continue };
            let mut allowed = strings(&from_ty.1["mayDependOn"]);
            if *kind == Kind::Dev {
                allowed.extend(strings(&from_ty.1["mayDevDependOn"]));
            }
            if !allowed.iter().any(|a| a == to_ty) {
                add(format!("{} {} ({to_ty}); type {} may depend on [{}]", how, to.name, from_ty.0, allowed.join(", ")));
            }
        }
    }
}

fn purity(packages: &[Package], types: &Json, out: &mut Vec<Finding>) {
    for p in packages {
        let pure = p.ty.as_deref().and_then(|t| types.get(t)).is_some_and(|s| s["pure"].as_bool() == Some(true));
        for (dep, _) in p.deps.iter().filter(|(d, k)| pure && *k != Kind::Dev && crate::core_purity::IO_CRATES.contains(&d.as_str())) {
            out.push(Finding { pass: "edges", dir: p.dir.clone(), message: format!("a pure type depends on the I/O crate {dep}") });
        }
    }
}

/// What a generated region of the given name holds, one line per entry.
fn render(region: &str, types: &Json) -> Option<String> {
    match region {
        "types" => Some(types.as_object()?.keys().map(|k| format!("- {k}\n")).collect()),
        _ => None,
    }
}

fn drift(root: &Path, manifest: &Json, out: &mut Vec<Finding>) -> usize {
    let regions = manifest["generated"].as_array().cloned().unwrap_or_default();
    for entry in &regions {
        let (file, region) = (entry["file"].as_str().unwrap_or(""), entry["region"].as_str().unwrap_or(""));
        let mut add = |message: String| out.push(Finding { pass: "drift", dir: file.to_string(), message });
        let Some(want) = render(region, &manifest["types"]) else { add(format!("region {region} has no generator")); continue };
        let Ok(text) = std::fs::read_to_string(root.join(file)) else { add("the file cannot be read".into()); continue };
        let lines: Vec<&str> = text.lines().collect();
        let begin = lines.iter().position(|l| l.contains(&format!("BEGIN generated:{region}")));
        let end = lines.iter().position(|l| l.contains(&format!("END generated:{region}")));
        match (begin, end) {
            (Some(b), Some(e)) if b < e => {
                let have: String = lines[b + 1..e].iter().map(|l| format!("{l}\n")).collect();
                if have != want {
                    add(format!("region {region} is stale; regenerate it from {MANIFEST}"));
                }
            }
            _ => add(format!("region {region} has no BEGIN/END generated:{region} marker pair")),
        }
    }
    regions.len()
}

pub fn check(root: &Path) -> Report {
    let Some(manifest) = read_json(&root.join(MANIFEST)) else {
        let e = format!("{MANIFEST} is missing or not JSON");
        return Report { json: json!({ "error": e }), findings: vec![e] };
    };
    let types = &manifest["types"];
    let mut packages = Vec::new();
    walk(root, root, &mut packages);
    let mut found = Vec::new();
    for p in &packages {
        match p.ty.as_deref() {
            None => found.push(Finding { pass: "membership", dir: p.dir.clone(), message: "declares no type (add [package.metadata.arch] type, or an arch key)".into() }),
            Some(t) if types.get(t).is_none() => found.push(Finding { pass: "membership", dir: p.dir.clone(), message: format!("type {t} is not in {MANIFEST}") }),
            Some(t) => placement(root, p, t, &types[t], &mut found),
        }
    }
    edges(&packages, types, &mut found);
    purity(&packages, types, &mut found);
    let regions = drift(root, &manifest, &mut found);
    let typed: Vec<&Package> = packages.iter().filter(|p| p.ty.as_deref().is_some_and(|t| types.get(t).is_some())).collect();
    let broken: BTreeSet<&str> = found.iter().filter(|f| f.pass != "membership" && f.pass != "drift").map(|f| f.dir.as_str()).collect();
    let mut by_type: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for p in &typed {
        let slot = by_type.entry(p.ty.clone().unwrap_or_default()).or_default();
        slot.0 += 1;
        slot.1 += usize::from(!broken.contains(p.dir.as_str()));
    }
    let passing = typed.iter().filter(|p| !broken.contains(p.dir.as_str())).count();
    let mut by_pass: BTreeMap<&str, usize> = ["membership", "placement", "edges", "drift"].into_iter().map(|p| (p, 0)).collect();
    for f in &found {
        *by_pass.entry(f.pass).or_default() += 1;
    }
    let list: Vec<Json> = found.iter().map(|f| json!({ "pass": f.pass, "package": f.dir, "message": f.message })).collect();
    let findings = found.iter().map(|f| format!("{}: {} ({})", f.dir, f.message, f.pass)).collect();
    let types_json: Json = by_type.iter().map(|(k, (n, ok))| (k.clone(), json!({ "packages": n, "passing": ok }))).collect::<serde_json::Map<_, _>>().into();
    let json = json!({ "mode": "report-only", "packages": packages.len(), "typed": typed.len(), "passing": passing, "unlisted": packages.len() - typed.len(), "generatedRegions": regions, "byPass": by_pass, "byType": types_json, "findings": list });
    Report { json, findings }
}

/// `check --layout [--strict]`.
pub fn run(args: &[String]) -> Option<i32> {
    let p = wf_kit::args::parse_spec(args, &LAYOUT)?;
    let report = check(&wf_kit::root::repo_root());
    println!("{}", report.json);
    for found in &report.findings {
        wf_kit::wf_err!("wf: layout: {found}");
    }
    Some(i32::from(p.flag("strict") && !report.findings.is_empty() || report.json.get("error").is_some()))
}
