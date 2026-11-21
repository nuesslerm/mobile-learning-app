//! The provision phase of `wf mimic capture <record|replay> <set>`: install the reference of a set on this machine as its `[provision]` block
//! says (`provision.rs`): the pinned source fetched at its commit, then the steps in order, each bounded. A work directory that already holds
//! the same provisioning (its marker key) is reused. Provisioning builds third-party code: every child runs in a `ChildEnv` (an allowlist of the
//! caller's environment plus the block's `[env]`), and its output goes to stderr.
use super::{fail, Fail, SetRef};
use crate::child_env::ChildEnv;
use crate::config::Config;
use crate::provision::{expand, Provision, RealAgent, When};
use mimic_core::json::sha256_label;
use serde_json::{json, Value};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use wf_kit::args::Parsed;

const MARKER: &str = ".provisioned.json";
const REAL_DIR: &str = "real-clis";
const DRIVERS_MINUTES: u64 = 30;

fn to_stderr(cmd: &mut Command) -> std::io::Result<()> {
    let fd = std::io::stderr().as_fd().try_clone_to_owned()?;
    cmd.stdout(Stdio::from(fd)).stdin(Stdio::null());
    Ok(())
}

/// Runs `cmd` for at most `limit`; the child's output is the caller's stderr.
fn run(mut cmd: Command, env: &ChildEnv, limit: Duration, what: &str) -> Result<(), String> {
    env.apply(&mut cmd);
    to_stderr(&mut cmd).map_err(|e| format!("{what}: {e}"))?;
    let mut child = cmd.spawn().map_err(|e| format!("{what}: {e}"))?;
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait().map_err(|e| format!("{what}: {e}"))? {
            Some(status) if status.success() => return Ok(()),
            Some(status) => return Err(format!("{what} exited {}", status.code().map_or_else(|| "by signal".to_string(), |c| c.to_string()))),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{what} ran over its {} minutes and was stopped", limit.as_secs() / 60));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn git_env() -> ChildEnv {
    ChildEnv::base().with([("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())])
}

fn git(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    cmd
}

fn fetch_source(source: &crate::provision::Source, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let limit = Duration::from_secs(15 * 60);
    let env = git_env();
    run(git(dir, &["init", "-q"]), &env, limit, "git init")?;
    run(git(dir, &["remote", "add", "origin", &source.repo]), &env, limit, "git remote add")?;
    run(git(dir, &["fetch", "-q", "--depth", "1", "origin", &source.commit]), &env, limit, &format!("git fetch of {}", source.commit))?;
    run(git(dir, &["checkout", "-q", "--detach", "FETCH_HEAD"]), &env, limit, "git checkout")?;
    let mut rev = git(dir, &["rev-parse", "HEAD"]);
    env.apply(&mut rev);
    let head = rev.output().map_err(|e| format!("git rev-parse: {e}"))?;
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    if head != source.commit {
        return Err(format!("the checkout is at {head}, not the pinned {}", source.commit));
    }
    Ok(())
}

fn toolchain_ok(tool: &str, want: &str) -> Result<(), String> {
    let (bin, args): (&str, &[&str]) = match tool {
        "node" => ("node", &["--version"]),
        "python" => ("python3", &["--version"]),
        "go" => ("go", &["version"]),
        _ => ("rustc", &["--version"]),
    };
    if want == "stable" {
        return Ok(());
    }
    let out = crate::child_env::command(bin).args(args).output().map_err(|e| format!("the set pins {tool} {want} and `{bin} {}` failed: {e} (set it up first)", args[0]))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let have = text.split(|c: char| !(c.is_ascii_digit() || c == '.')).find(|t| t.starts_with(|c: char| c.is_ascii_digit())).unwrap_or("").trim_end_matches('.');
    if have == want || have.starts_with(&format!("{want}.")) {
        Ok(())
    } else {
        Err(format!("the set pins {tool} {want} and the runner has {text:?}: set it up first (the record-references workflow does, from the same pin)"))
    }
}

fn platform() -> &'static str {
    if std::env::consts::ARCH == "aarch64" {
        "linux-arm64"
    } else {
        "linux-x64"
    }
}

/// Installs each pinned real agent CLI into `{work}/real-clis`: the release asset (a file of `cache` of the same name, else downloaded) must hash to the
/// pin before anything is unpacked, and the program must then report the pinned version.
fn install_real_agents(agents: &[RealAgent], work: &Path, cache: Option<&Path>) -> Result<(), String> {
    let dir = work.join(REAL_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let platform = platform();
    let env = ChildEnv::base();
    for a in agents {
        let asset = format!("{}-{platform}.tar.gz", a.name);
        let archive = dir.join(format!(".{asset}"));
        let package = a.packages.get(platform);
        match cache.map(|c| c.join(&asset)).filter(|p| p.is_file()) {
            Some(cached) => drop(std::fs::copy(&cached, &archive).map_err(|e| format!("{}: {e}", cached.display()))?),
            None => {
                let mut curl = Command::new("curl");
                let from = package.map_or_else(|| format!("{}/{asset}", a.release), |p| p.url.clone());
                curl.args(["-fsSL", "--retry", "3", "--connect-timeout", "30", "--max-time", "900", "-o"]).arg(&archive).arg(from);
                run(curl, &env, Duration::from_secs(16 * 60), &format!("the download of {asset}"))?;
            }
        }
        let bytes = std::fs::read(&archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        let got = sha256_label(&bytes);
        let want = format!("sha256:{}", a.sha256[platform]);
        if got != want {
            let _ = std::fs::remove_file(&archive);
            return Err(format!("{asset} of {} {} hashes to {got} and the pin says {want}: nothing was unpacked", a.name, a.version));
        }
        let unpack = match package {
            Some(_) => dir.join(format!(".{}-package", a.name)),
            None => dir.clone(),
        };
        std::fs::create_dir_all(&unpack).map_err(|e| format!("{}: {e}", unpack.display()))?;
        let mut tar = Command::new("tar");
        tar.arg("xzf").arg(&archive).arg("-C").arg(&unpack);
        run(tar, &env, Duration::from_secs(5 * 60), &format!("the unpacking of {asset}"))?;
        let _ = std::fs::remove_file(&archive);
        let program = dir.join(&a.name);
        if let Some(p) = package {
            let inside = unpack.join(&p.bin);
            if !inside.is_file() {
                return Err(format!("{asset} of {} {} holds no {} (the pin's bin)", a.name, a.version, p.bin));
            }
            std::os::unix::fs::symlink(&inside, &program).map_err(|e| format!("{}: {e}", program.display()))?;
        }
        let out = env.command(&program).arg("--version").output().map_err(|e| format!("{} --version: {e}", program.display()))?;
        let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !said.split_whitespace().any(|w| w == a.version) {
            return Err(format!("{} reports {said:?}, not the pinned version {}", program.display(), a.version));
        }
    }
    Ok(())
}

pub fn work_dir(p: &Parsed, set_name: &str, prov: &Provision) -> PathBuf {
    let root = p.val("work-root").map(PathBuf::from).or_else(|| std::env::var_os("MIMIC_WORK_ROOT").filter(|v| !v.is_empty()).map(PathBuf::from)).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/mimic-work"))).unwrap_or_else(|| std::env::temp_dir().join("mimic-work"));
    root.join(prov.workdir.as_deref().unwrap_or(set_name))
}

/// The values `{repo}`, `{work}`, `{source}` and `{set}` stand for.
pub fn vars(p: &Parsed, set_name: &str, set_dir: &Path, prov: &Provision) -> [(&'static str, String); 4] {
    let work = work_dir(p, set_name, prov);
    let repo = p.val("root").map_or_else(wf_kit::root::repo_root, PathBuf::from);
    [("repo", repo.display().to_string()), ("work", work.display().to_string()), ("source", work.join("source").display().to_string()), ("set", set_dir.display().to_string())]
}

/// The `[env]` of the block with its placeholders filled in: what the record of the set runs with. It goes to that one program through the job,
/// never to $GITHUB_ENV, so no later step of the workflow (one that holds a secret) inherits it.
pub fn block_env(p: &Parsed, set_name: &str, set_dir: &Path, prov: &Provision) -> Vec<(String, String)> {
    let vars = vars(p, set_name, set_dir, prov);
    let real = (!prov.real_agents.is_empty()).then(|| ("MIMIC_REAL_CLI_DIR".to_string(), work_dir(p, set_name, prov).join(REAL_DIR).display().to_string()));
    prov.env.iter().map(|(k, v)| (k.clone(), expand(v, &vars))).chain(real).collect()
}

/// Whether the capture runs the phase, and what it does with an existing work directory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Only,
    Skip,
}

impl Mode {
    pub fn parse(p: &Parsed) -> Result<Mode, Fail> {
        match p.val("provision").unwrap_or("auto") {
            "auto" => Ok(Mode::Auto),
            "only" => Ok(Mode::Only),
            "skip" => Ok(Mode::Skip),
            other => Err(fail(format!("--provision {other} is auto, only or skip"), "wf mimic capture --help")),
        }
    }
}

pub fn when_of(action: &str) -> When {
    if action == "record" {
        When::Record
    } else {
        When::Replay
    }
}

/// The phase for one set: `{state: ran|cached|skipped|none|checked, ...}`. `none` is a set without a `[provision]` block.
pub fn phase(p: &Parsed, set: &SetRef, cfg: &Config, action: &str, mode: Mode) -> Result<Value, Fail> {
    let command = format!("wf mimic capture {action} {}", set.dir.display());
    let Some(prov) = cfg.provision.as_ref() else {
        if mode == Mode::Only {
            return Err(fail(format!("{}/mimic.toml has no [provision] block or provision_file: --provision only has nothing to do", set.dir.display()), "add one (schema: docs/runbooks/record-reference-servers-on-the-vm.md) or drop --provision only"));
        }
        return Ok(json!({"state": "none"}));
    };
    let when = when_of(action);
    let steps: Vec<_> = prov.steps_for(when).collect();
    let work = work_dir(p, &set.name, prov);
    let source_dir = work.join("source");
    let vars = vars(p, &set.name, &set.dir, prov);
    let env: Value = block_env(p, &set.name, &set.dir, prov).into_iter().map(|(k, v)| (k, json!(v))).collect::<serde_json::Map<_, _>>().into();
    let drivers = prov.drivers && matches!(when, When::Record);
    let real_agents: &[RealAgent] = if matches!(when, When::Record) { &prov.real_agents } else { &[] };
    let real = !real_agents.is_empty();
    let key = sha256_label(&serde_json::to_vec(&(prov, action)).expect("provision serialises"));
    let marked = std::fs::read_to_string(work.join(MARKER)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["key"] == json!(key));
    let mut out = json!({
        "state": "ran", "work": work.display().to_string(), "source": prov.source.as_ref().map(|s| json!({"repo": s.repo, "commit": s.commit, "dir": source_dir.display().to_string()})),
        "toolchain": prov.toolchain, "env": env, "key": key, "steps": drivers.then_some("drivers").into_iter().chain(real.then_some("real-agents")).chain(steps.iter().map(|s| s.name.as_str())).collect::<Vec<_>>(),
        "real_agents": real_agents.iter().map(|a| json!({"name": a.name, "version": a.version, "sha256": a.sha256[platform()], "dir": work.join(REAL_DIR).display().to_string()})).collect::<Vec<_>>(),
    });
    if p.flag("check") {
        out["state"] = json!("checked");
        return Ok(out);
    }
    if mode == Mode::Skip {
        if !marked || p.flag("force") {
            let why = if p.flag("force") { "--force starts the work directory over, which --provision skip does not do".to_string() } else { format!("{} holds no provisioning of this block (its marker is missing or names another block or commit)", work.display()) };
            return Err(fail(format!("--provision skip: {why}"), format!("{command} --provision only")));
        }
        out["state"] = json!("skipped");
        return Ok(out);
    }
    for (tool, want) in &prov.toolchain {
        toolchain_ok(tool, want).map_err(|e| fail(e, "wf mimic capture --help"))?;
    }
    let cached = !p.flag("force") && marked;
    let mut ran: Vec<Value> = Vec::new();
    if !cached {
        if work.exists() {
            std::fs::remove_dir_all(&work).map_err(|e| fail(format!("{}: {e}", work.display()), "free the work directory"))?;
        }
        std::fs::create_dir_all(&work).map_err(|e| fail(format!("{}: {e}", work.display()), "set --work-root or MIMIC_WORK_ROOT to a writable directory"))?;
        if let Some(source) = &prov.source {
            let started = Instant::now();
            fetch_source(source, &source_dir).map_err(|e| failed(&out, "source", &e))?;
            ran.push(json!({"name": "source", "seconds": started.elapsed().as_secs()}));
        }
        if drivers {
            let started = Instant::now();
            let mut cmd = Command::new("pnpm");
            cmd.args(["install", "--frozen-lockfile", "--filter", "mimic-drivers..."]).current_dir(&vars[0].1);
            run(cmd, &ChildEnv::base(), Duration::from_secs(DRIVERS_MINUTES * 60), "the install of the mimic drivers").map_err(|e| failed(&out, "drivers", &e))?;
            ran.push(json!({"name": "drivers", "seconds": started.elapsed().as_secs()}));
        }
        if real {
            let started = Instant::now();
            let cache = p.val("agents-cache").map(PathBuf::from);
            install_real_agents(real_agents, &work, cache.as_deref()).map_err(|e| failed(&out, "real-agents", &e))?;
            ran.push(json!({"name": "real-agents", "seconds": started.elapsed().as_secs()}));
        }
        for step in steps {
            let started = Instant::now();
            let cwd = PathBuf::from(expand(&step.cwd, &vars));
            let mut cmd = Command::new(expand(&step.run[0], &vars));
            cmd.args(step.run[1..].iter().map(|a| expand(a, &vars))).current_dir(&cwd);
            let env = ChildEnv::base().with(prov.env.iter().chain(&step.env).map(|(k, v)| (k.clone(), expand(v, &vars))));
            run(cmd, &env, Duration::from_secs(step.timeout_minutes * 60), &format!("step {:?}", step.name)).map_err(|e| failed(&out, &step.name, &e))?;
            ran.push(json!({"name": step.name, "seconds": started.elapsed().as_secs()}));
        }
        std::fs::write(work.join(MARKER), json!({"key": key}).to_string()).map_err(|e| fail(format!("{}: {e}", work.display()), "free the work directory"))?;
    }
    out["state"] = json!(if cached { "cached" } else { "ran" });
    out["ran"] = json!(ran);
    Ok(out)
}

fn failed(out: &Value, step: &str, error: &str) -> Fail {
    let mut f = fail(format!("provisioning stopped at {step}: {error}"), "fix the step in the set's [provision] (or its provision_file) and re-run; --force starts the work directory over");
    f.evidence = json!({"work": out["work"], "step": step});
    f
}
