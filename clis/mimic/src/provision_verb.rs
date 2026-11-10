//! `wf mimic provision <set>`: install the reference of a set on this machine as its `[provision]` block says (`provision.rs`): the pinned
//! source fetched at its commit, then the steps in order, each bounded. A work directory that already holds the same provisioning is reused.
//! Steps run with the environment of the caller minus every credential-looking variable, and their output goes to stderr.
use super::{fail, set_ref, Done, Fail};
use crate::provision::{expand, Provision, When};
use mimic_core::json::sha256_label;
use serde_json::{json, Value};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use wf_kit::args::Parsed;

const MARKER: &str = ".provisioned.json";
const DRIVERS_MINUTES: u64 = 30;

/// Whether a variable of the caller's environment is left out of what a provisioned program sees.
pub fn credential_like(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSWORD", "CREDENTIAL", "PAR_URL", "API_KEY"].iter().any(|s| n.contains(s)) || n.ends_with("_KEY") || n.starts_with("BWS_")
}

fn clean_env(cmd: &mut Command) {
    for (k, _) in std::env::vars_os() {
        if k.to_str().is_some_and(credential_like) {
            cmd.env_remove(&k);
        }
    }
}

fn to_stderr(cmd: &mut Command) -> std::io::Result<()> {
    let fd = std::io::stderr().as_fd().try_clone_to_owned()?;
    cmd.stdout(Stdio::from(fd)).stdin(Stdio::null());
    Ok(())
}

/// Runs `cmd` for at most `limit`; the child's output is the caller's stderr.
fn run(mut cmd: Command, limit: Duration, what: &str) -> Result<(), String> {
    clean_env(&mut cmd);
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

fn git(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0");
    cmd
}

fn fetch_source(source: &crate::provision::Source, dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let limit = Duration::from_secs(15 * 60);
    run(git(dir, &["init", "-q"]), limit, "git init")?;
    run(git(dir, &["remote", "add", "origin", &source.repo]), limit, "git remote add")?;
    run(git(dir, &["fetch", "-q", "--depth", "1", "origin", &source.commit]), limit, &format!("git fetch of {}", source.commit))?;
    run(git(dir, &["checkout", "-q", "--detach", "FETCH_HEAD"]), limit, "git checkout")?;
    let head = git(dir, &["rev-parse", "HEAD"]).output().map_err(|e| format!("git rev-parse: {e}"))?;
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
    let out = Command::new(bin).args(args).output().map_err(|e| format!("the set pins {tool} {want} and `{bin} {}` failed: {e} (set it up first)", args[0]))?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let have = text.split(|c: char| !(c.is_ascii_digit() || c == '.')).find(|t| t.starts_with(|c: char| c.is_ascii_digit())).unwrap_or("").trim_end_matches('.');
    if have == want || have.starts_with(&format!("{want}.")) {
        Ok(())
    } else {
        Err(format!("the set pins {tool} {want} and the runner has {text:?}: set it up first (the record-references workflow does, from the same pin)"))
    }
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
    prov.env.iter().map(|(k, v)| (k.clone(), expand(v, &vars))).collect()
}

pub fn provision_verb(p: &Parsed) -> Done {
    let set = set_ref(&p.positional[0]);
    let cfg = super::load_config(&set.dir.join("mimic.toml"))?;
    let prov = cfg.provision.as_ref().ok_or_else(|| fail(format!("{}/mimic.toml has no [provision] block or provision_file", set.dir.display()), "add one (schema: docs/runbooks/record-reference-servers-on-the-vm.md)"))?;
    let when = match p.val("when").unwrap_or("record") {
        "record" => When::Record,
        "replay" => When::Replay,
        other => return Err(fail(format!("--when {other} is record or replay"), "wf mimic provision --help")),
    };
    let steps: Vec<_> = prov.steps_for(when).collect();
    let work = work_dir(p, &set.name, prov);
    let source_dir = work.join("source");
    let vars = vars(p, &set.name, &set.dir, prov);
    let env: Value = block_env(p, &set.name, &set.dir, prov).into_iter().map(|(k, v)| (k, json!(v))).collect::<serde_json::Map<_, _>>().into();
    let drivers = prov.drivers && matches!(when, When::Record);
    let key = sha256_label(&serde_json::to_vec(&(prov, p.val("when").unwrap_or("record"))).expect("provision serialises"));
    let mut out = json!({
        "ok": true, "set": set.name, "when": p.val("when").unwrap_or("record"), "work": work.display().to_string(), "source": prov.source.as_ref().map(|s| json!({"repo": s.repo, "commit": s.commit, "dir": source_dir.display().to_string()})),
        "toolchain": prov.toolchain, "env": env, "key": key, "steps": drivers.then_some("drivers").into_iter().chain(steps.iter().map(|s| s.name.as_str())).collect::<Vec<_>>(), "warnings": [],
    });
    if p.flag("check") {
        out["checked"] = json!(true);
        out["next"] = json!(format!("wf mimic provision {}", set.dir.display()));
        return Ok((out, 0));
    }
    for (tool, want) in &prov.toolchain {
        toolchain_ok(tool, want).map_err(|e| fail(e, "wf mimic provision --help"))?;
    }
    let cached = !p.flag("force") && std::fs::read_to_string(work.join(MARKER)).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).is_some_and(|v| v["key"] == json!(key));
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
            run(cmd, Duration::from_secs(DRIVERS_MINUTES * 60), "the install of the mimic drivers").map_err(|e| failed(&out, "drivers", &e))?;
            ran.push(json!({"name": "drivers", "seconds": started.elapsed().as_secs()}));
        }
        for step in steps {
            let started = Instant::now();
            let cwd = PathBuf::from(expand(&step.cwd, &vars));
            let mut cmd = Command::new(expand(&step.run[0], &vars));
            cmd.args(step.run[1..].iter().map(|a| expand(a, &vars))).current_dir(&cwd);
            for (k, v) in prov.env.iter().chain(&step.env) {
                cmd.env(k, expand(v, &vars));
            }
            run(cmd, Duration::from_secs(step.timeout_minutes * 60), &format!("step {:?}", step.name)).map_err(|e| failed(&out, &step.name, &e))?;
            ran.push(json!({"name": step.name, "seconds": started.elapsed().as_secs()}));
        }
        std::fs::write(work.join(MARKER), json!({"key": key}).to_string()).map_err(|e| fail(format!("{}: {e}", work.display()), "free the work directory"))?;
    }
    out["cached"] = json!(cached);
    out["ran"] = json!(ran);
    out["next"] = json!(format!("wf mimic job {} {}", if matches!(when, When::Record) { "record" } else { "replay" }, set.dir.display()));
    Ok((out, 0))
}

fn failed(out: &Value, step: &str, error: &str) -> Fail {
    let mut f = fail(format!("provisioning stopped at {step}: {error}"), "fix the step in the set's [provision] (or its provision_file) and re-run; --force starts the work directory over");
    f.evidence = json!({"work": out["work"], "step": step});
    f
}
