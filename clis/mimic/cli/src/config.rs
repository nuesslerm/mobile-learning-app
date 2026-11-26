//! `mimic.toml`, checked in next to a cassette set: where its rules, corpus and allowlist are, how to run the reference and the
//! candidates, and the pin the reference is recorded against. Data, not a wrapper script; paths are relative to the file.
use crate::adapters::StdioSpec;
use mimic_core::cassette::Target as Provenance;
use mimic_core::differ::DiffOptions;
use mimic_core::json::sha256_label;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

const DEFAULT_TIMEOUT_MS: u64 = 120_000;

fn rules_default() -> String {
    "rules.json".into()
}

fn stdio() -> String {
    "stdio".into()
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default = "rules_default")]
    rules: String,
    corpus: Option<String>,
    allowlist: Option<String>,
    reference: Option<TargetFile>,
    #[serde(default)]
    candidate: BTreeMap<String, TargetFile>,
    #[serde(default)]
    diff: DiffFile,
    /// How a runner provisions the reference (`provision.rs`): inline, or the sibling file `provision_file` names.
    provision: Option<crate::provision::Raw>,
    provision_file: Option<String>,
    record: Option<RecordFile>,
    #[serde(default)]
    proxy: ProxyFile,
}

/// `[proxy.replay]`: what `wf mimic proxy replay` lets differ between the recording and a live client.
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct ProxyFile {
    #[serde(default)]
    replay: ReplayFile,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct ReplayFile {
    /// Keys a client sends on some runs and not on others: they are not compared.
    #[serde(default)]
    ignore_keys: Vec<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct RecordFile {
    #[serde(default)]
    allow_nondeterministic: bool,
    #[serde(default)]
    args: Vec<String>,
    /// State-free cases sent to the reference at once (it answers them concurrently).
    batch: Option<usize>,
    /// State-changing cases (tag `state:write`) per state of the reference: it is reset between chunks.
    write_chunk: Option<usize>,
    /// Percent of the cases run twice as the stability study (default 100: every case).
    stability_pct: Option<u8>,
}

/// What `wf mimic capture record` adds to the `wf mimic record` call of this set.
#[derive(Debug, Clone, Default)]
pub struct RecordOptions {
    pub allow_nondeterministic: bool,
    pub args: Vec<String>,
    pub batch: Option<usize>,
    pub write_chunk: Option<usize>,
    pub stability_pct: Option<u8>,
}

/// Flags the capture sets itself: a `[record] args` entry cannot override them.
const CAPTURE_FLAGS: [&str; 5] = ["--resume", "--max-minutes", "--recorded-on", "--source", "--allow-nondeterministic"];

fn record_options(file: &Path, r: Option<RecordFile>) -> Result<RecordOptions, String> {
    let r = r.unwrap_or_default();
    if let Some(bad) = r.args.iter().find(|a| CAPTURE_FLAGS.contains(&a.split('=').next().unwrap_or("")) || a.is_empty()) {
        return Err(format!("{}: [record] args has {bad:?}, which the capture sets itself ({}); use allow_nondeterministic for that one", file.display(), CAPTURE_FLAGS.join(", ")));
    }
    Ok(RecordOptions { allow_nondeterministic: r.allow_nondeterministic, args: r.args, batch: r.batch, write_chunk: r.write_chunk, stability_pct: r.stability_pct })
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct DiffFile {
    #[serde(default)]
    ignore_dirs: Vec<mimic_core::cassette::Dir>,
    fold: Option<mimic_core::differ::Fold>,
    #[serde(default)]
    breaking_paths: Vec<String>,
    #[serde(default)]
    advertised_prefixes: Vec<String>,
}

/// `[reference.script]` / `[candidate.<n>.script]`: the step interpreter's server and waits.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ScriptFile {
    /// `spawn`: start the release the pin names; `url`: a server that is already running; `chain`: a server the `launch` command starts and stops (then a `url` target); `orca`: the phone's own client in the sidecar
    /// below, paired with the host the environment names.
    target: String,
    /// `orca`: the sidecar command, relative paths as the file's directory sees them.
    #[serde(default)]
    sidecar: Vec<String>,
    /// `chain` (or `orca`): the command that starts the candidate (`{work}` and `{repo}` of the provision block are filled in). It prints one JSON line
    /// `{"ok":true,"event":"listening","addr","workspace","mint","cli_dir"}` (an `orca` launcher prints `"pairing"` instead of `mint`), serves until its stdin closes, and is started by the replay verb itself.
    #[serde(default)]
    launch: Vec<String>,
    /// `orca`: names of the environment variables the sidecar gets besides `PATH`, `HOME`, `TZ` and `LANG` (the host to pair with, e.g.
    /// `ORCA_ORCAD_CHECKOUT` or `ORCA_PAIRING_URI`): names only, a value never appears in a set.
    #[serde(default)]
    env_pass: Vec<String>,
    /// `spawn`: the directory the verified release is unpacked under; default `~/.cache/mimic-t3`.
    cache: Option<String>,
    /// `url`: the server's `host:port`, its workspace folder (masked in steps) and a command that prints one credential.
    addr: Option<String>,
    workspace: Option<String>,
    #[serde(default)]
    mint: Vec<String>,
    /// A JSON file with the entries every case that names `prelude: "session"` starts with.
    prelude: Option<String>,
    /// `spawn`: a directory of scripted agent CLIs placed ahead of the stubs on the server's PATH (default `$MIMIC_CLI_DIR`, else `$T3_CLI_DIR`).
    cli_dir: Option<String>,
    /// One server and one prelude for many cases (default for `spawn`).
    session: Option<bool>,
    ready_ms: Option<u64>,
    settle_ms: Option<u64>,
    quiet_cap_ms: Option<u64>,
    first_answer_ms: Option<u64>,
}

/// A script target as the driver factory needs it, with the paths made absolute.
#[derive(Clone, Debug)]
pub struct ScriptSpec {
    pub spawn: bool,
    /// `target = "orca"`: the sidecar and what it is given; `dir` is the working directory it runs in.
    pub orca: bool,
    pub sidecar: Vec<String>,
    /// `target = "chain"`: the command the replay starts the candidate with; empty for any other target.
    pub launch: Vec<String>,
    /// Variables the sidecar gets besides `env_pass`, set by the replay from what a started chain printed (the pairing link of an `orca` candidate with a `launch`).
    pub env_set: Vec<(String, String)>,
    pub env_pass: Vec<String>,
    /// The directory of real agent CLIs the provision installed (`MIMIC_REAL_CLI_DIR`, set for a block with `real_agents`), when there is one.
    pub real_clis: Option<PathBuf>,
    pub dir: PathBuf,
    pub cache: Option<PathBuf>,
    pub addr: Option<String>,
    pub workspace: Option<String>,
    pub mint: Vec<String>,
    pub prelude: Option<PathBuf>,
    pub cli_dir: Option<PathBuf>,
    pub session: bool,
    pub ready_ms: u64,
    pub settle_ms: Option<u64>,
    pub quiet_cap_ms: Option<u64>,
    pub first_answer_ms: Option<u64>,
}

fn script_spec(dir: &Path, name: &str, s: ScriptFile) -> Result<ScriptSpec, String> {
    let orca = s.target == "orca";
    let spawn = match s.target.as_str() {
        "spawn" => true,
        "url" | "orca" | "chain" => false,
        other => return Err(format!("[{name}.script] target {other:?}: spawn, url, chain or orca")),
    };
    if s.target == "chain" && s.launch.is_empty() {
        return Err(format!("[{name}.script] target \"chain\" needs a launch command"));
    }
    if orca && s.sidecar.is_empty() {
        return Err(format!("[{name}.script] target \"orca\" needs a sidecar command"));
    }
    Ok(ScriptSpec {
        spawn,
        orca,
        sidecar: s.sidecar,
        launch: s.launch,
        env_set: Vec::new(),
        env_pass: s.env_pass,
        real_clis: std::env::var_os("MIMIC_REAL_CLI_DIR").filter(|v| !v.is_empty()).map(PathBuf::from),
        dir: dir.to_path_buf(),
        cache: s.cache.map(|c| dir.join(c)),
        addr: s.addr,
        workspace: s.workspace,
        mint: s.mint,
        prelude: s.prelude.map(|p| dir.join(p)),
        cli_dir: s.cli_dir.map(|p| dir.join(p)).or_else(|| std::env::var_os("MIMIC_CLI_DIR").or_else(|| std::env::var_os("T3_CLI_DIR")).map(PathBuf::from)),
        session: s.session.unwrap_or(spawn),
        ready_ms: s.ready_ms.unwrap_or(60_000),
        settle_ms: s.settle_ms,
        quiet_cap_ms: s.quiet_cap_ms,
        first_answer_ms: s.first_answer_ms,
    })
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct TargetFile {
    #[serde(default = "stdio")]
    driver: String,
    #[serde(default)]
    command: Vec<String>,
    /// Beside `driver = "script"`: how the step interpreter reaches its server.
    script: Option<ScriptFile>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// `[reference]` only: `{ ENV_NAME = "REGISTRY_KEY" }`, the variable this program needs (a model API key) and the registry key that holds its value. The
    /// verb reads the keys in memory for a record and puts them in the environment of this program alone; names only, a value never appears in a set.
    #[serde(default)]
    env_from_vault: BTreeMap<String, String>,
    timeout_ms: Option<u64>,
    name: Option<String>,
    version: Option<String>,
    commit: Option<String>,
    /// The pin file the reference is recorded against; its hash goes into the cassette.
    pin: Option<String>,
    /// A directory of the repository the reference is the code of: its git tree hash (working tree included) is the pin.
    pin_tree: Option<String>,
    /// JSON pointers into the pin file for the version and commit the cassette names.
    version_pointer: Option<String>,
    commit_pointer: Option<String>,
    /// Why this reference is re-recorded only when a set is named (`verify <set> --live`), not by `--all --live`: it is a download or a service.
    live_when_named: Option<String>,
}

#[derive(Clone)]
pub struct Target {
    pub name: String,
    pub spec: StdioSpec,
    /// Set for `driver = "script"`: the step interpreter instead of a child process speaking `mimic-driver/1`.
    pub script: Option<ScriptSpec>,
    /// The variable and registry key pairs the set declares for this program (`[reference]` only).
    pub env_from_vault: BTreeMap<String, String>,
    pub pin: Option<PathBuf>,
    pin_as_written: Option<String>,
    pin_tree: Option<PathBuf>,
    pin_tree_as_written: Option<String>,
    version: Option<String>,
    commit: Option<String>,
    version_pointer: Option<String>,
    commit_pointer: Option<String>,
    pub live_when_named: Option<String>,
}

impl Config {
    /// The variables of the caller's environment the set names for its programs (`env_pass`), whose values a capture child may hold. A vault key (`env_from_vault`) is not one of them: only a record's reference gets it.
    pub fn declared_pass(&self) -> Vec<String> {
        let vault: Vec<&String> = self.reference.iter().flat_map(|r| r.env_from_vault.keys()).collect();
        let targets = self.reference.iter().chain(self.candidates.values());
        targets.flat_map(|t| t.script.iter().flat_map(|s| s.env_pass.iter())).filter(|n| !vault.contains(n)).cloned().collect()
    }
}

pub struct Config {
    pub dir: PathBuf,
    pub rules: PathBuf,
    pub corpus: Option<PathBuf>,
    pub allowlist: Option<PathBuf>,
    pub reference: Option<Target>,
    pub candidates: BTreeMap<String, Target>,
    pub diff: DiffOptions,
    pub provision: Option<crate::provision::Provision>,
    pub record: RecordOptions,
    pub replay_ignore_keys: Vec<String>,
}

/// The nearest ancestor of `dir` that is a checkout (holds `.git`, a directory or a worktree's file).
fn checkout_root(dir: &Path) -> Option<PathBuf> {
    let abs = std::path::absolute(dir).ok()?;
    abs.ancestors().find(|a| a.join(".git").exists()).map(Path::to_path_buf)
}

fn load_provision(dir: &Path, file: &Path, inline: Option<crate::provision::Raw>, named: Option<String>) -> Result<Option<crate::provision::Provision>, String> {
    match (inline, named) {
        (None, None) => Ok(None),
        (Some(raw), None) => raw.resolve(dir, &format!("{} [provision]", file.display())).map(Some),
        (Some(_), Some(_)) => Err(format!("{}: both [provision] and provision_file: a set has one", file.display())),
        (None, Some(name)) => {
            let path = match name.strip_prefix("//") {
                Some(from_root) => checkout_root(dir).ok_or_else(|| format!("{}: provision_file {name} is relative to the repository root and {} is in no checkout", file.display(), dir.display()))?.join(from_root),
                None => dir.join(&name),
            };
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: provision_file {}: {e}", file.display(), path.display()))?;
            let raw: crate::provision::Raw = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let base = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new(".")).to_path_buf();
            raw.resolve(&base, &path.display().to_string()).map(Some)
        }
    }
}

fn target(dir: &Path, name: &str, t: TargetFile) -> Result<Target, String> {
    if !matches!(t.driver.as_str(), "stdio" | "script" | "wire") {
        return Err(format!("[{name}] driver {:?}: this build runs drivers of kind \"stdio\", \"script\" and \"wire\" (a Rust closure is the SDK's InProcessDriver)", t.driver));
    }
    // a server observed through `wf mimic proxy record`: nothing is started for it, so it names no command
    let observed = t.driver == "wire";
    for declared in t.env_from_vault.iter().flat_map(|(env, key)| [env, key]).chain(t.script.iter().flat_map(|s| &s.env_pass)) {
        if crate::child_env::never(declared) || !declared.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
            return Err(format!("[{name}] declares {declared:?}: an UPPER_SNAKE name, and never a BWS_* or GITHUB_* one (no child gets those)"));
        }
    }
    if !t.env_from_vault.is_empty() && name != "reference" {
        return Err(format!("[{name}] env_from_vault: only [reference] may declare a vault key, so a replay or a candidate never holds one"));
    }
    let script = match (t.driver.as_str(), t.script) {
        ("script", Some(s)) => Some(script_spec(dir, name, s)?),
        ("script", None) => return Err(format!("[{name}] driver \"script\" needs a [{name}.script] table")),
        (_, Some(_)) => return Err(format!("[{name}] has a script table but driver {:?}", t.driver)),
        _ => None,
    };
    if t.pin.is_some() && t.pin_tree.is_some() {
        return Err(format!("[{name}] names both pin and pin_tree: a reference has one pin"));
    }
    if t.command.is_empty() && script.is_none() && !observed {
        return Err(format!("[{name}] command is empty"));
    }
    let mut command = t.command;
    // a relative program with a directory part is the file's, not the caller's working directory's
    if command.first().is_some_and(|c| c.contains('/') && Path::new(c).is_relative()) {
        command[0] = dir.join(&command[0]).display().to_string();
    }
    let mut script = script;
    if let Some(s) = script.as_mut() {
        s.env_pass.extend(t.env_from_vault.keys().cloned());
    }
    let spec = StdioSpec {
        command,
        cwd: Some(t.cwd.map_or_else(|| dir.to_path_buf(), |c| dir.join(c))),
        env: t.env.into_iter().collect(),
        pass: t.env_from_vault.keys().cloned().collect(),
        timeout: Duration::from_millis(t.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS)),
    };
    Ok(Target {
        name: t.name.unwrap_or_else(|| name.to_string()),
        spec,
        script,
        env_from_vault: t.env_from_vault,
        pin: t.pin.as_ref().map(|p| dir.join(p)),
        pin_as_written: t.pin,
        pin_tree: t.pin_tree.as_ref().map(|p| dir.join(p)),
        pin_tree_as_written: t.pin_tree,
        version: t.version,
        commit: t.commit,
        version_pointer: t.version_pointer,
        commit_pointer: t.commit_pointer,
        live_when_named: t.live_when_named,
    })
}

impl Config {
    pub fn load(file: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let parsed: File = toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        let dir = file.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new(".")).to_path_buf();
        // like rules.json, a corpus.json beside the file is the corpus without being named
        let corpus = parsed.corpus.or_else(|| dir.join("corpus.json").is_file().then(|| "corpus.json".to_string()));
        let mut diff = DiffOptions::default();
        diff.breaking_paths.extend(parsed.diff.breaking_paths);
        diff.advertised_prefixes = parsed.diff.advertised_prefixes;
        diff.ignore_dirs = parsed.diff.ignore_dirs;
        diff.fold = parsed.diff.fold;
        Ok(Self {
            rules: dir.join(&parsed.rules),
            corpus: corpus.as_ref().map(|c| dir.join(c)),
            allowlist: parsed.allowlist.map(|a| dir.join(a)),
            reference: parsed.reference.map(|t| target(&dir, "reference", t)).transpose()?,
            candidates: parsed.candidate.into_iter().map(|(n, t)| target(&dir, &n, t).map(|t| (n, t))).collect::<Result<_, _>>()?,
            diff,
            provision: load_provision(&dir, file, parsed.provision, parsed.provision_file)?,
            record: record_options(file, parsed.record)?,
            replay_ignore_keys: parsed.proxy.replay.ignore_keys,
            dir,
        })
    }
}

impl Target {
    fn pin_json(&self) -> Result<Option<serde_json::Value>, String> {
        let Some(pin) = &self.pin else { return Ok(None) };
        let text = std::fs::read_to_string(pin).map_err(|e| format!("pin file {}: {e}", pin.display()))?;
        serde_json::from_str(&text).map(Some).map_err(|e| format!("pin file {} is not JSON: {e}", pin.display()))
    }

    /// The sha256 of the pin file's bytes: what a cassette is recorded against and `verify` compares.
    pub fn pin_hash(&self) -> Result<Option<String>, String> {
        if let Some(tree) = &self.pin_tree {
            return git_tree(tree).map(Some);
        }
        let Some(pin) = &self.pin else { return Ok(None) };
        std::fs::read(pin).map(|b| Some(sha256_label(&b))).map_err(|e| format!("pin file {}: {e}", pin.display()))
    }

    /// What the cassette says about the reference: name, version and commit (explicit or read from the pin), the pin file and its hash.
    pub fn provenance(&self) -> Result<Provenance, String> {
        let pin = self.pin_json()?;
        let read = |explicit: &Option<String>, pointer: &Option<String>| -> Result<Option<String>, String> {
            match (explicit, pointer) {
                (Some(v), _) => Ok(Some(v.clone())),
                (None, Some(p)) => {
                    let value = pin.as_ref().and_then(|j| j.pointer(p)).ok_or_else(|| format!("the pin file has nothing at {p}"))?;
                    Ok(Some(value.as_str().map_or_else(|| value.to_string(), String::from)))
                }
                (None, None) => Ok(None),
            }
        };
        Ok(Provenance {
            name: self.name.clone(),
            version: read(&self.version, &self.version_pointer)?,
            commit: read(&self.commit, &self.commit_pointer)?,
            pin_file: self.pin_as_written.clone().or_else(|| self.pin_tree_as_written.clone()),
            pin_hash: self.pin_hash()?,
        })
    }
}

/// `git-tree:<hash>` of a directory as it is on disk now (tracked, staged or new files; ignored ones left out): the hash git gives that
/// tree once committed, so an edit to the reference's code moves the pin before it is committed.
pub fn git_tree(dir: &Path) -> Result<String, String> {
    let git = |args: &[&str], index: Option<&Path>| -> Result<String, String> {
        let mut cmd = crate::child_env::command("git");
        cmd.arg("-C").arg(dir).args(args);
        if let Some(index) = index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        let out = cmd.output().map_err(|e| format!("git {}: {e}", args.join(" ")))?;
        if !out.status.success() {
            return Err(format!("git {} in {}: {}", args.join(" "), dir.display(), String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let prefix = git(&["rev-parse", "--show-prefix"], None)?;
    let scratch = std::env::temp_dir().join(format!("mimic-tree-{}-{}", std::process::id(), prefix.replace('/', "_")));
    let _ = std::fs::remove_file(&scratch);
    let tree = git(&["add", "-A", "--", "."], Some(&scratch))
        .and_then(|_| git(&["write-tree", &format!("--prefix={prefix}")], Some(&scratch)));
    let _ = std::fs::remove_file(&scratch);
    tree.map(|t| format!("git-tree:{t}"))
}

/// `to` as seen from the directory `from`, for the paths a header stores.
pub fn relative(from: &Path, to: &Path) -> String {
    let abs = |p: &Path| std::fs::canonicalize(p).or_else(|_| std::path::absolute(p)).unwrap_or_else(|_| p.to_path_buf());
    let (from, to) = (abs(from), abs(to));
    let (a, b): (Vec<Component>, Vec<Component>) = (from.components().collect(), to.components().collect());
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out = PathBuf::new();
    for _ in common..a.len() {
        out.push("..");
    }
    for c in &b[common..] {
        out.push(c);
    }
    out.display().to_string()
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct CrawlFile {
    app: String,
    command: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    /// The committed listing of the pinned source (`wf mimic coverage` writes it): every row id there must be in the crawl.
    catalog: Option<String>,
    /// The pin file the crawl is made against, and the pointer to the commit in it; a crawl at another commit is stale.
    pin: Option<String>,
    commit_pointer: Option<String>,
}

/// `crawl.toml`, checked in next to the three files a crawl writes.
pub struct CrawlConfig {
    pub dir: PathBuf,
    pub app: String,
    pub spec: StdioSpec,
    pub catalog: Option<PathBuf>,
    pub pin: Option<(PathBuf, String)>,
}

impl CrawlConfig {
    pub fn load(file: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
        let parsed: CrawlFile = toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
        if parsed.command.is_empty() {
            return Err(format!("{}: command is empty", file.display()));
        }
        let dir = file.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new(".")).to_path_buf();
        let spec = StdioSpec { command: parsed.command, cwd: Some(parsed.cwd.map_or_else(|| dir.clone(), |c| dir.join(c))), env: parsed.env.into_iter().collect(), pass: Vec::new(), timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS) };
        let pin = match (parsed.pin, parsed.commit_pointer) {
            (Some(p), pointer) => Some((dir.join(p), pointer.unwrap_or_else(|| "/commit".into()))),
            (None, Some(_)) => return Err(format!("{}: commit_pointer needs a pin", file.display())),
            (None, None) => None,
        };
        Ok(Self { app: parsed.app, spec, catalog: parsed.catalog.map(|c| dir.join(c)), pin, dir })
    }

    /// The commit the pin file names now.
    pub fn pin_commit(&self) -> Result<Option<String>, String> {
        let Some((file, pointer)) = &self.pin else { return Ok(None) };
        let text = std::fs::read_to_string(file).map_err(|e| format!("pin file {}: {e}", file.display()))?;
        let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("pin file {} is not JSON: {e}", file.display()))?;
        json.pointer(pointer).and_then(|v| v.as_str()).map(|s| Some(s.to_string())).ok_or_else(|| format!("pin file {} has no string at {pointer}", file.display()))
    }
}
