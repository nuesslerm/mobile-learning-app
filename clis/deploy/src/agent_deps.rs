//! `wf vm deps add|remove|list|replay`: the manifest of dependencies replayed at agent-shell
//! boot; `install` installs now through an agent-shell exec job and records the entry in the same job.
use wf_kit::args::parse;
use wf_kit::time::now_ms;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DEFAULT_PATH: &str = "/workspace/.agent-env/packages.json";
const DEFAULT_NPM_PREFIX: &str = "/workspace/tools/npm";
const APT_CACHE_DIR: &str = "/workspace/cache/apt";
const MANAGERS: [&str; 3] = ["apt", "npm", "mise"];

fn manifest_path() -> PathBuf {
    std::env::var_os("AGENT_DEPS_MANIFEST_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PATH))
}

fn valid_entry(e: &Value) -> bool {
    let str_of = |k: &str| e.get(k).and_then(Value::as_str);
    str_of("manager").is_some_and(|m| MANAGERS.contains(&m))
        && str_of("name").is_some_and(|n| !n.is_empty())
        && str_of("addedAt").is_some()
}

/// A missing, unparseable or wrongly shaped file reads as empty, so a corrupt manifest never blocks boot.
fn load(path: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v) => match v.get("entries").and_then(Value::as_array) {
            Some(entries) if entries.iter().all(valid_entry) => entries.clone(),
            _ => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

/// Write then rename, so a concurrent reader never sees a half-written file.
fn save(path: &Path, entries: &[Value]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = PathBuf::from(format!("{}.tmp.{}", path.display(), std::process::id()));
    let body = serde_json::to_string_pretty(&json!({ "entries": entries })).expect("json");
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn iso_now() -> String {
    rca_core::time::iso(now_ms())
}

fn manager(name: &str) -> Result<&'static str, String> {
    MANAGERS.iter().copied().find(|m| *m == name).ok_or_else(|| {
        format!(
            "unknown manager \"{name}\" -- expected one of: {}",
            MANAGERS.join(", ")
        )
    })
}

fn same(e: &Value, manager: &str, name: &str) -> bool {
    e["manager"] == manager && e["name"] == name
}

fn print(v: &Value) {
    println!("{v}");
}

fn add(manager: &str, name: &str) -> Result<i32, String> {
    let path = manifest_path();
    let mut entries = load(&path);
    if entries.iter().any(|e| same(e, manager, name)) {
        print(&json!({ "manifest": { "entries": entries }, "added": false }));
        return Ok(0);
    }
    entries.push(json!({ "manager": manager, "name": name, "addedAt": iso_now() }));
    save(&path, &entries)?;
    print(&json!({ "manifest": { "entries": entries }, "added": true }));
    Ok(0)
}

fn remove(manager: &str, name: &str) -> Result<i32, String> {
    let path = manifest_path();
    let current = load(&path);
    let kept: Vec<Value> = current
        .iter()
        .filter(|e| !same(e, manager, name))
        .cloned()
        .collect();
    let removed = kept.len() != current.len();
    if removed {
        save(&path, &kept)?;
    }
    print(&json!({ "manifest": { "entries": kept }, "removed": removed }));
    Ok(0)
}

fn probe(file: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(file)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if stderr.is_empty() {
        format!("Command failed: {file} {}", args.join(" "))
    } else {
        stderr
    })
}

fn present(entry: &Value, npm_prefix: &str) -> bool {
    let name = entry["name"].as_str().unwrap_or_default();
    match entry["manager"].as_str().unwrap_or_default() {
        "apt" => probe("dpkg-query", &["-W", "-f=${Status}", name])
            .is_ok_and(|out| out.contains("install ok installed")),
        "npm" => Path::new(npm_prefix)
            .join("lib")
            .join("node_modules")
            .join(name)
            .exists(),
        _ => probe("mise", &["which", name]).is_ok(),
    }
}

fn install(entry: &Value) -> Result<(), String> {
    let name = entry["name"].as_str().unwrap_or_default();
    match entry["manager"].as_str().unwrap_or_default() {
        "apt" => {
            // apt-get errors when the archive dir named by Dir::Cache::Archives is missing.
            let _ = probe("mkdir", &["-p", APT_CACHE_DIR]);
            let cache = format!("Dir::Cache::Archives={APT_CACHE_DIR}");
            probe("apt-get", &["install", "-y", "-o", &cache, name]).map(drop)
        }
        "npm" => probe("npm", &["install", "-g", name]).map(drop),
        _ => probe("mise", &["install", name]).map(drop),
    }
}

fn replay(managers: &str) -> Result<i32, String> {
    let wanted = managers
        .split(',')
        .map(|m| manager(m.trim()))
        .collect::<Result<Vec<_>, _>>()?;
    let npm_prefix =
        std::env::var("NPM_CONFIG_PREFIX").unwrap_or_else(|_| DEFAULT_NPM_PREFIX.to_string());
    let mut failed = false;
    let mut results = Vec::new();
    for entry in load(&manifest_path()) {
        if !wanted.contains(&entry["manager"].as_str().unwrap_or_default()) {
            continue;
        }
        if present(&entry, &npm_prefix) {
            results.push(json!({ "entry": entry, "outcome": "already-present" }));
            continue;
        }
        results.push(match install(&entry) {
            Ok(()) => json!({ "entry": entry, "outcome": "installed" }),
            Err(detail) => {
                failed = true;
                json!({ "entry": entry, "outcome": "failed", "detail": detail })
            }
        });
    }
    print(&json!({ "results": results }));
    Ok(i32::from(failed))
}

/// A real package or tool name starts alphanumeric and holds only these characters, so a fumbled multi-word argument
/// never becomes two install arguments.
fn valid_name(n: &str) -> bool {
    let mut chars = n.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "._+@/-".contains(c))
}

/// One exec job (install && record), so a failed install is never recorded. Streams the job output and returns its exit code.
fn install_now(manager: &str, name: &str) -> Result<i32, String> {
    if !valid_name(name) {
        return Err(format!("invalid package/tool name: {}", Value::String(name.to_string())));
    }
    let command = match manager {
        "apt" => format!("mkdir -p {APT_CACHE_DIR} && apt-get install -y -o Dir::Cache::Archives={APT_CACHE_DIR} {name}"),
        "npm" => format!("npm install -g {name}"),
        _ => format!("mise install {name}"),
    };
    let job_id = wf_vmops::vm_exec::start(&format!("{command} && wf vm deps add {manager} {name}"))?;
    eprintln!("jobId: {job_id}");
    let job = wf_vmops::vm_exec::wait_streaming(&job_id)?;
    Ok(match job.exit_code {
        Some(0) => 0,
        Some(code) => wf_vmops::vm_exec::exit_code_of(code),
        None => 1,
    })
}

/// `args` follow `agent-deps`; `None` leaves help and odd shapes to TypeScript.
pub fn run(args: &[String]) -> Result<Option<i32>, String> {
    let Some((sub, rest)) = args.split_first() else {
        return Ok(None);
    };
    match sub.as_str() {
        "add" | "remove" => match parse(rest, &[], &[]) {
            Some(p) if p.positional.len() == 2 => {
                let m = manager(&p.positional[0])?;
                let n = &p.positional[1];
                if sub == "add" { add(m, n) } else { remove(m, n) }.map(Some)
            }
            _ => Ok(None),
        },
        "install" => match parse(rest, &[], &[]) {
            Some(p) if p.positional.len() == 2 => {
                let m = manager(&p.positional[0])?;
                install_now(m, &p.positional[1]).map(Some)
            }
            _ => Ok(None),
        },
        "list" => match parse(rest, &[], &[]) {
            Some(p) if p.positional.is_empty() => {
                print(&json!({ "entries": load(&manifest_path()) }));
                Ok(Some(0))
            }
            _ => Ok(None),
        },
        "replay" => match parse(rest, &["managers"], &[]) {
            Some(p) if p.positional.is_empty() => {
                replay(p.opts.get("managers").map_or("apt,npm,mise", String::as_str)).map(Some)
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}
