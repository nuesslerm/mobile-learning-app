use crate::contract::{load_contract, parse_json};
use crate::fsx::{list_files, lstat, read_text_or_null, remove, write_text};
use crate::paths::join;
use crate::render::{owned_dirs, render_outputs, Output, LEGACY_PATHS};
use crate::Res;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::path::Path;

pub struct Plan {
    outputs: Vec<Output>,
    owned: Vec<String>,
}

pub struct SyncResult {
    pub path: String,
    pub ok: bool,
}

enum Fix {
    RemoveTree(String),
    RemoveFile(String),
    Write(usize),
    UnlinkThenWrite(String, usize),
}

struct Drift {
    path: String,
    fix: Fix,
}

fn abs(root: &str, rel: &str) -> String {
    join(&[root, rel])
}

pub fn plan(root: &str) -> Res<Plan> {
    let contract = load_contract(root)?;
    let settings_path = abs(root, ".claude/settings.json");
    let raw = match lstat(Path::new(&settings_path))? {
        Some(m) if !m.symlink => read_text_or_null(Path::new(&settings_path))?,
        _ => None,
    };
    let existing: Map<String, Value> = match raw.filter(|r| !r.is_empty()) {
        Some(raw) => match parse_json(&raw, ".claude/settings.json")? {
            Value::Object(m) => m,
            _ => return Err(".claude/settings.json: expected an object".into()),
        },
        None => Map::new(),
    };
    Ok(Plan {
        outputs: render_outputs(&contract, root, &existing)?,
        owned: owned_dirs(&contract),
    })
}

/// The first path from `rel` up to the root that is a symlink, or none.
fn symlink_on_path(root: &str, rel: &str) -> Res<Option<String>> {
    let parts: Vec<&str> = rel.split('/').collect();
    for i in 1..=parts.len() {
        let p = parts[..i].join("/");
        if lstat(Path::new(&abs(root, &p)))?.is_some_and(|m| m.symlink) {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

fn find_drift(root: &str, plan: &Plan) -> Res<Vec<Drift>> {
    let mut drift = Vec::new();
    for legacy in LEGACY_PATHS {
        if lstat(Path::new(&abs(root, legacy)))?.is_some() {
            drift.push(Drift {
                path: (*legacy).into(),
                fix: Fix::RemoveTree(abs(root, legacy)),
            });
        }
    }
    for (i, o) in plan.outputs.iter().enumerate() {
        let link = symlink_on_path(root, &o.path)?;
        if let Some(link) = link {
            drift.push(Drift {
                path: o.path.clone(),
                fix: Fix::UnlinkThenWrite(abs(root, &link), i),
            });
        } else if read_text_or_null(Path::new(&abs(root, &o.path)))?.as_deref()
            != Some(o.content.as_str())
        {
            drift.push(Drift {
                path: o.path.clone(),
                fix: Fix::Write(i),
            });
        }
    }
    let expected: HashSet<&str> = plan.outputs.iter().map(|o| o.path.as_str()).collect();
    for dir in &plan.owned {
        if symlink_on_path(root, dir)?.is_some() {
            continue;
        }
        for rel in list_files(Path::new(&abs(root, dir)))? {
            let path = join(&[dir, &rel]);
            if !expected.contains(path.as_str()) {
                drift.push(Drift {
                    fix: Fix::RemoveFile(abs(root, &path)),
                    path,
                });
            }
        }
    }
    Ok(drift)
}

fn apply(plan: &Plan, root: &str, d: &Drift) -> Res<()> {
    let write = |i: usize| {
        let o = &plan.outputs[i];
        write_text(Path::new(&abs(root, &o.path)), &o.content, o.executable)
    };
    match &d.fix {
        Fix::RemoveTree(p) => remove(Path::new(p), true),
        Fix::RemoveFile(p) => remove(Path::new(p), false),
        Fix::Write(i) => write(*i),
        Fix::UnlinkThenWrite(link, i) => {
            if lstat(Path::new(link))?.is_some_and(|m| m.symlink) {
                remove(Path::new(link), false)?;
            }
            write(*i)
        }
    }
}

/// Writes every generated file whose content differs; returns the paths it changed.
pub fn sync(root: &str) -> Res<Vec<SyncResult>> {
    let p = plan(root)?;
    let first = find_drift(root, &p)?;
    for d in &first {
        apply(&p, root, d)?;
    }
    // Stale files under a just-replaced symlink only become visible after the first pass.
    let rest = find_drift(root, &p)?;
    for d in &rest {
        apply(&p, root, d)?;
    }
    Ok(first
        .into_iter()
        .chain(rest)
        .map(|d| SyncResult {
            path: d.path,
            ok: true,
        })
        .collect())
}

/// Every generated path, `ok: false` where disk differs from what `sync` would write.
pub fn check_sync(root: &str) -> Res<Vec<SyncResult>> {
    let p = plan(root)?;
    let drifted: Vec<String> = find_drift(root, &p)?.into_iter().map(|d| d.path).collect();
    let mut results: Vec<SyncResult> = p
        .outputs
        .iter()
        .map(|o| SyncResult {
            path: o.path.clone(),
            ok: !drifted.contains(&o.path),
        })
        .collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let extra: Vec<String> = drifted
        .iter()
        .filter(|d| seen.insert(d.as_str()) && !results.iter().any(|r| &r.path == *d))
        .cloned()
        .collect();
    results.extend(extra.into_iter().map(|path| SyncResult { path, ok: false }));
    Ok(results)
}
