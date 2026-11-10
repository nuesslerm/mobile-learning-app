//! The environment `wf` builds before any command runs (the TypeScript `loadWfEnv`): the shell always wins,
//! then `<repo>/.env.local`, `<repo>/.env`, and `~/.config/wf/.env`. Repo files are read only when there
//! really is a repo root, because a stray `.env` beside a VM app directory once shadowed the vault.
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn home() -> String {
    env::var("HOME").unwrap_or_else(|_| "/".into())
}

fn expand_vars(value: &str) -> String {
    let re = Regex::new(r"\$\{([^}]+)\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    re.replace_all(value, |caps: &regex::Captures| {
        let name = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map_or("", |m| m.as_str());
        if name == "HOME" {
            home()
        } else {
            env::var(name).unwrap_or_default()
        }
    })
    .into_owned()
}

/// One layer of surrounding quotes; only double quotes carry escapes, as in dotenv.
fn unquote(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 2 {
        return value.to_string();
    }
    let (first, last) = (chars[0], chars[chars.len() - 1]);
    if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
        let inner: String = chars[1..chars.len() - 1].iter().collect();
        if first == '"' {
            let re = Regex::new(r#"\\(["\\n])"#).unwrap();
            return re
                .replace_all(&inner, |c: &regex::Captures| {
                    if &c[1] == "n" {
                        "\n".to_string()
                    } else {
                        c[1].to_string()
                    }
                })
                .into_owned();
        }
        return inner;
    }
    value.to_string()
}

fn parse_env_file(contents: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for raw in contents.split('\n') {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(eq) = line.find('=') else { continue };
        let key = line[..eq].trim();
        if key.is_empty() {
            continue;
        }
        let value = expand_vars(&unquote(line[eq + 1..].trim()));
        match out.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => out.push((key.to_string(), value)),
        }
    }
    out
}

fn show_toplevel() -> String {
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Loads the env files into this process and returns the names it set, so a caller that ends up
/// handing the command to the TypeScript `wf` can undo them and let that load its own.
pub fn load() -> Vec<String> {
    let shell_keys: HashSet<String> = env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    let mut merged: HashMap<String, (String, u8)> = HashMap::new();
    let mut set: Vec<String> = Vec::new();

    let absorb = |path: PathBuf, precedence: u8, merged: &mut HashMap<String, (String, u8)>| {
        let Ok(contents) = fs::read(&path) else {
            return;
        };
        for (key, val) in parse_env_file(&String::from_utf8_lossy(&contents)) {
            if merged.get(&key).map_or(0, |(_, p)| *p) < precedence {
                merged.insert(key, (val, precedence));
            }
        }
    };

    absorb(
        PathBuf::from(home()).join(".config/wf/.env"),
        1,
        &mut merged,
    );
    for (key, (val, _)) in &merged {
        if !shell_keys.contains(key) && env::var(key).map_or(true, |v| v.is_empty()) {
            env::set_var(key, val);
            set.push(key.clone());
        }
    }

    let repo_root = match env::var("REPO_ROOT") {
        Ok(v) => v,
        Err(_) => {
            let root = show_toplevel();
            if !root.is_empty() {
                env::set_var("REPO_ROOT", &root);
                set.push("REPO_ROOT".into());
            }
            root
        }
    };
    if !repo_root.is_empty() {
        absorb(PathBuf::from(&repo_root).join(".env"), 2, &mut merged);
        absorb(PathBuf::from(&repo_root).join(".env.local"), 3, &mut merged);
    }

    for (key, (val, _)) in &merged {
        if !shell_keys.contains(key) {
            env::set_var(key, val);
            set.push(key.clone());
        }
    }
    set
}

/// Removes what `load` set, restoring the environment the process started with.
pub(crate) fn unload(set: &[String]) {
    for key in set {
        env::remove_var(key);
    }
}
