//! How a set's reference server is provisioned on a runner, as data: the `[provision]` table of `mimic.toml`, or a sibling file it names with
//! `provision_file` (so several sets of one app share it). Adding a clone of another CLI or server is adding this block, not workflow YAML.
//! Paths are relative to the file that declares them. `{repo}`, `{work}`, `{source}` and `{set}` are filled in when the steps run.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct Raw {
    /// The toolchains the runner sets up before the steps run (`TOOLCHAINS`): `node = "24"`, `python = "3.12"`, `go = "1.23"`, `rust = "stable"`.
    #[serde(default)]
    pub toolchain: BTreeMap<String, String>,
    /// Install the repository's own mimic drivers (`pnpm install --filter mimic-drivers...`) before the steps; default: when a node toolchain is set.
    pub drivers: Option<bool>,
    /// The directory under the work root; defaults to the set's name.
    pub workdir: Option<String>,
    pub source: Option<RawSource>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub step: Vec<RawStep>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct RawSource {
    /// An https URL without credentials: the reference's own repository.
    pub repo: String,
    /// A full commit id, or the pin file and a JSON pointer to it (`/commit` by default).
    pub commit: Option<String>,
    pub pin: Option<String>,
    pub commit_pointer: Option<String>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct RawStep {
    pub name: String,
    /// The program and its arguments: no shell.
    pub run: Vec<String>,
    /// Defaults to `{source}` when the block has a source, else `{work}`.
    pub cwd: Option<String>,
    /// `record` (default), `replay` (what builds the candidate) or `always`.
    pub when: Option<String>,
    pub timeout_minutes: Option<u64>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Record,
    Replay,
}

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub repo: String,
    pub commit: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub name: String,
    pub run: Vec<String>,
    pub cwd: String,
    pub when: String,
    pub timeout_minutes: u64,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Provision {
    pub toolchain: BTreeMap<String, String>,
    pub drivers: bool,
    pub workdir: Option<String>,
    pub source: Option<Source>,
    pub env: BTreeMap<String, String>,
    pub steps: Vec<Step>,
}

pub const DEFAULT_TIMEOUT_MINUTES: u64 = 30;
const MAX_TIMEOUT_MINUTES: u64 = 180;
const PLACEHOLDERS: [&str; 4] = ["repo", "work", "source", "set"];
/// A key naming one of these holds a credential: a `mimic.toml` is committed, so it never carries one.
/// The toolchains a runner can set up from a version: the record-references workflow has one setup step for each, keyed by the matrix entry of the same name.
pub const TOOLCHAINS: [&str; 4] = ["node", "python", "go", "rust"];
/// A variable that changes how the next program is found, loaded or networked: a set's env never carries one, so it cannot redirect what runs
/// with it (the steps, the record, and everything the job starts).
const HIJACK_EXACT: [&str; 14] = ["PATH", "HOME", "SHELL", "IFS", "ENV", "BASH_ENV", "NODE_OPTIONS", "NODE_PATH", "PYTHONPATH", "PYTHONSTARTUP", "PERL5OPT", "RUBYOPT", "RUSTC_WRAPPER", "GOFLAGS"];
const SECRETISH: [&str; 6] = ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "PAR_URL", "API_KEY"];

fn is_hex_commit(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    (1..=3).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The placeholders a value uses; an unknown or unclosed one is an error.
fn placeholders(value: &str) -> Result<Vec<&str>, String> {
    let mut found = Vec::new();
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let close = after.find('}').ok_or_else(|| format!("{value:?} has an unclosed {{"))?;
        let name = &after[..close];
        if !PLACEHOLDERS.contains(&name) {
            return Err(format!("{value:?} uses {{{name}}}: the placeholders are {{repo}}, {{work}}, {{source}} and {{set}}"));
        }
        found.push(name);
        rest = &after[close + 1..];
    }
    Ok(found)
}

fn env_ok(what: &str, env: &BTreeMap<String, String>, has_source: bool) -> Result<(), String> {
    for (k, v) in env {
        let valid = !k.is_empty() && k.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') && !k.starts_with(|c: char| c.is_ascii_digit());
        if !valid {
            return Err(format!("{what}: env key {k:?} is not an UPPER_SNAKE name"));
        }
        if HIJACK_EXACT.contains(&k.as_str()) || k.starts_with("LD_") || k.starts_with("DYLD_") || k.ends_with("_PROXY") {
            return Err(format!("{what}: env {k} changes how programs are found, loaded or networked: a set cannot set it"));
        }
        if SECRETISH.iter().any(|s| k.contains(s)) || k.starts_with("BWS_") || k.starts_with("GITHUB_") {
            return Err(format!("{what}: env {k} looks like a credential, and a mimic.toml is committed: the reference runs on a throwaway home with no secrets"));
        }
        if v.contains('\n') {
            return Err(format!("{what}: env {k} has a newline in its value"));
        }
        check_placeholders(what, v, has_source)?;
    }
    Ok(())
}

fn check_placeholders(what: &str, value: &str, has_source: bool) -> Result<(), String> {
    let used = placeholders(value).map_err(|e| format!("{what}: {e}"))?;
    if !has_source && used.contains(&"source") {
        return Err(format!("{what}: {value:?} uses {{source}} but the block has no [source]"));
    }
    Ok(())
}

impl Raw {
    /// The checked block: `dir` is where the declaring file is (the pin file path is read from there), `origin` names it in errors.
    pub fn resolve(self, dir: &Path, origin: &str) -> Result<Provision, String> {
        let err = |m: String| format!("{origin}: {m}");
        for (tool, v) in &self.toolchain {
            if !TOOLCHAINS.contains(&tool.as_str()) {
                return Err(err(format!("toolchain {tool:?} is not one the runner sets up ({})", TOOLCHAINS.join(", "))));
            }
            if !(is_version(v) || (tool == "rust" && v == "stable")) {
                return Err(err(format!("{tool} = {v:?} is not a version (24, 22.14 or 22.14.0{})", if tool == "rust" { ", or stable" } else { "" })));
            }
        }
        if let Some(w) = &self.workdir {
            if w.is_empty() || w == "." || w == ".." || !w.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-') {
                return Err(err(format!("workdir = {w:?} must be one plain directory name")));
            }
        }
        let source = self.source.map(|s| source(dir, s).map_err(&err)).transpose()?;
        let has_source = source.is_some();
        env_ok(&format!("{origin} [env]"), &self.env, has_source)?;
        let drivers = self.drivers.unwrap_or(self.toolchain.contains_key("node"));
        if self.step.is_empty() && !drivers {
            return Err(err("no [[step]] and no drivers: a reference with nothing to install or build needs no [provision]".into()));
        }
        let mut steps: Vec<Step> = Vec::new();
        for s in self.step {
            let what = format!("{origin} step {:?}", s.name);
            if s.name.trim().is_empty() || steps.iter().any(|t| t.name == s.name) {
                return Err(err(format!("step name {:?} is empty or used twice", s.name)));
            }
            if s.run.is_empty() || s.run[0].is_empty() {
                return Err(format!("{what}: run is empty"));
            }
            let when = s.when.unwrap_or_else(|| "record".into());
            if !["record", "replay", "always"].contains(&when.as_str()) {
                return Err(format!("{what}: when = {when:?} is record, replay or always"));
            }
            let timeout = s.timeout_minutes.unwrap_or(DEFAULT_TIMEOUT_MINUTES);
            if !(1..=MAX_TIMEOUT_MINUTES).contains(&timeout) {
                return Err(format!("{what}: timeout_minutes = {timeout} is outside 1..={MAX_TIMEOUT_MINUTES}"));
            }
            let cwd = s.cwd.unwrap_or_else(|| if has_source { "{source}".into() } else { "{work}".into() });
            for a in s.run.iter().chain(std::iter::once(&cwd)) {
                check_placeholders(&what, a, has_source)?;
            }
            env_ok(&what, &s.env, has_source)?;
            steps.push(Step { name: s.name, run: s.run, cwd, when, timeout_minutes: timeout, env: s.env });
        }
        Ok(Provision { toolchain: self.toolchain, drivers, workdir: self.workdir, source, env: self.env, steps })
    }
}

fn source(dir: &Path, s: RawSource) -> Result<Source, String> {
    let host = s.repo.strip_prefix("https://").ok_or_else(|| format!("[source] repo {:?} is not an https URL", s.repo))?;
    if host.is_empty() || host.contains(['@', ' ']) || host.split('/').next().is_some_and(str::is_empty) {
        return Err(format!("[source] repo {:?} must be a plain https URL, with no credentials", s.repo));
    }
    let commit = match (s.commit, s.pin) {
        (Some(c), None) => c,
        (None, Some(pin)) => {
            let file: PathBuf = dir.join(&pin);
            let text = std::fs::read_to_string(&file).map_err(|e| format!("[source] pin {}: {e}", file.display()))?;
            let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("[source] pin {} is not JSON: {e}", file.display()))?;
            let pointer = s.commit_pointer.as_deref().unwrap_or("/commit");
            json.pointer(pointer).and_then(|v| v.as_str()).map(String::from).ok_or_else(|| format!("[source] pin {} has no string at {pointer}", file.display()))?
        }
        (Some(_), Some(_)) => return Err("[source] names both commit and pin: a source has one pin".into()),
        (None, None) => return Err("[source] needs a commit or a pin file: a floating ref is no pin".into()),
    };
    if !is_hex_commit(&commit) {
        return Err(format!("[source] commit {commit:?} is not a full lowercase 40-hex commit id"));
    }
    Ok(Source { repo: s.repo, commit })
}

/// `{repo}`, `{work}`, `{source}` and `{set}` replaced.
pub fn expand(value: &str, vars: &[(&str, String)]) -> String {
    vars.iter().fold(value.to_string(), |acc, (k, v)| acc.replace(&format!("{{{k}}}"), v))
}

impl Provision {
    pub fn steps_for(&self, when: When) -> impl Iterator<Item = &Step> {
        let want = match when {
            When::Record => "record",
            When::Replay => "replay",
        };
        self.steps.iter().filter(move |s| s.when == "always" || s.when == want)
    }
}
