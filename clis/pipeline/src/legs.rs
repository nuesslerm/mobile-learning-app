//! The deploy legs and the planning step CI runs behind `wf ship --changed`: which services a push deploys and whether it publishes
//! an EAS Update, and the commit a service's deployment verification test should find. A service ships when the hash of what its images are
//! built from (`image_hash`) differs between the commit it runs and HEAD; the app's update when its package graph moved.
use wf_vmops::services::{running_commit, VM_OPS_API};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Leg {
    app: &'static str,
    package: &'static str,
}

/// What the desktop's image is built from besides its own package: the directories of its listener's cargo closure (the path dependencies of
/// `clis/vnc-auth-capture/listener/Cargo.toml`, transitively) and the registry the secrets crate embeds. A flow test of `deploy` compares
/// this list with those manifests and with the Dockerfile's COPY lines.
pub(crate) const DESKTOP_IMAGE_PATHS: [&str; 8] = [
    "clis/cdp",
    "clis/http",
    "clis/wf-kit",
    "clis/wf-login",
    "clis/secrets",
    "clis/wf-testkit",
    "clis/wf-vmops",
    "clis/secrets/src/data/registry.json",
];

/// In priority order: the one runner serves the legs top to bottom. The package names the leg's tests (`expected_revisions`).
const DEPLOY_LEGS: &[Leg] = &[
    Leg { app: "remote-chat-agent", package: "@app/remote-chat-agent" },
    Leg { app: "leetcode-backend", package: "@app/leetcode-backend" },
    Leg { app: "leetcode-auth-cookie-capture", package: "@app/leetcode-auth-cookie-capture" },
    Leg { app: "reporting-service", package: "@backend/reporting-service" },
    Leg { app: "t3-serve", package: "crate-t3-serve" },
    Leg { app: "orca-serve", package: "crate-orca-serve" },
    Leg { app: "vnc-auth-session", package: "@cli/vnc-auth-capture" },
];

/// The mobile app's pnpm package: what the EAS planner counts changes against (no service runs it).
/// The agent workspace: no package of its own, planned from the hash of its image inputs like vm-ops-api, and deployed last so a redeploy that restarts it
/// (which ends the exec jobs running in it) comes after every other leg.
const AGENT_SHELL: &str = "agent-shell";

pub const APP_PACKAGE: &str = "mla-tamagui";

/* ---------- plan ---------- */

/// A command run in the repository at `dir` (the working directory without one).
fn in_repo(program: &str, dir: Option<&Path>) -> Command {
    let mut cmd = Command::new(program);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    cmd
}

fn git_ok(dir: Option<&Path>, args: &[&str]) -> bool {
    in_repo("git", dir)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Files no image is built from: a change to only these leaves its package unchanged.
const NOT_SHIPPED: &[&str] = &["**/__tests__/**", "**/*.test.*", "**/*.md"];

/// Packages changed since `sha` and the packages that depend on them (pnpm's `...[sha]`). pnpm runs without its check for another version to
/// switch to (`manage-package-manager-versions`): the one installed is the pinned one, and the check is 0.4 s of every call.
fn affected_since(dir: Option<&Path>, sha: &str) -> Result<Vec<String>, String> {
    let ignore = NOT_SHIPPED.iter().map(|p| format!("--changed-files-ignore-pattern={p}"));
    let out = in_repo("pnpm", dir)
        .env("npm_config_manage_package_manager_versions", "false")
        .args(["--filter".to_string(), format!("...[{sha}]")])
        .args(ignore)
        .args(["list", "--depth", "-1", "--json"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run pnpm: {e}"))?;
    let (text, errors) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    // a pnpm that fails prints an error object (or nothing) and must never read as "nothing changed"
    let clip = |s: &str| s.trim().chars().take(200).collect::<String>();
    if !out.status.success() {
        let said = if text.trim().is_empty() { clip(&errors) } else { clip(&text) };
        return Err(format!("pnpm exited {} listing the packages changed since {sha}: {said}", out.status.code().map_or("on a signal".to_string(), |c| c.to_string())));
    }
    let listed: Value = serde_json::from_str(if text.trim().is_empty() { "[]" } else { &text }).map_err(|e| format!("pnpm list printed unreadable JSON: {e}"))?;
    let Some(packages) = listed.as_array() else { return Err(format!("pnpm list printed no package list: {}", clip(&text))) };
    Ok(packages.iter().filter_map(|p| p.get("name").and_then(Value::as_str).map(str::to_string)).collect())
}

/// Why the app's update is behind: empty when nothing it is built from changed since `sha`, the commit it was published from: its package
/// (or one it depends on), its own `extra` paths, or a lockfile entry its own dependency closure reaches.
struct Reasons {
    affected: Vec<(String, Vec<String>)>,
    /// The repository the commits are compared in (the working directory without one).
    dir: Option<PathBuf>,
}

impl Reasons {
    fn since(&mut self, sha: Option<String>, package: &str, extra: &[&str], importer: &str) -> Result<Vec<String>, String> {
        let Some(sha) = sha else { return Ok(vec!["its running commit is unknown".into()]) };
        let dir = self.dir.as_deref();
        if !git_ok(dir, &["cat-file", "-e", &format!("{sha}^{{commit}}")]) {
            return Ok(vec![format!("its running commit {sha} is not in this checkout")]);
        }
        if !self.affected.iter().any(|(s, _)| *s == sha) {
            self.affected.push((sha.clone(), affected_since(dir, &sha)?));
        }
        let mut why = Vec::new();
        if self.affected.iter().any(|(s, packages)| *s == sha && packages.iter().any(|p| p == package)) {
            why.push(format!("{package} changed since {sha}"));
        }
        let lockfile = "pnpm-lock.yaml";
        let mut args = vec!["diff", "--name-only", sha.as_str(), "HEAD", "--"];
        args.extend(extra);
        args.push(lockfile);
        let out = in_repo("git", dir).args(&args).stdin(Stdio::null()).output().map_err(|e| format!("cannot run git: {e}"))?;
        if !out.status.success() {
            return Err(format!("git diff {sha} HEAD failed: {}", String::from_utf8_lossy(&out.stderr).trim().chars().take(200).collect::<String>()));
        }
        let changed = String::from_utf8_lossy(&out.stdout).into_owned();
        why.extend(changed.lines().filter(|f| *f != lockfile).take(5).map(|f| format!("{f} changed")));
        // a lockfile edit counts only through the packages this app's own closure reaches
        if changed.lines().any(|f| f == lockfile) {
            let moved = crate::lock_closure::pnpm_changed(dir, &sha, importer)?;
            if !moved.is_empty() {
                let shown: Vec<&str> = moved.iter().take(5).map(String::as_str).collect();
                why.push(format!("{} of its locked dependencies moved: {}", moved.len(), shown.join(", ")));
            }
        }
        Ok(why)
    }
}

/// Why a service ships: empty when the hash of what its images are built from is the same at `running`, the commit it runs, as at HEAD.
fn image_changes(dir: Option<&Path>, running: Option<String>, app: &str) -> Result<Vec<String>, String> {
    let Some(sha) = running else { return Ok(vec!["its running commit is unknown".into()]) };
    if !git_ok(dir, &["cat-file", "-e", &format!("{sha}^{{commit}}")]) {
        return Ok(vec![format!("its running commit {sha} is not in this checkout")]);
    }
    let now = crate::image_hash::leg_hash(dir, "HEAD", app)?;
    Ok(match crate::image_hash::leg_hash(dir, &sha, app) {
        Ok(then) if then == now => Vec::new(),
        Ok(then) => vec![format!("its image inputs changed since {sha}: {then} -> {now}")],
        Err(e) => vec![format!("its image inputs at {sha} cannot be hashed: {e}")],
    })
}

/// Why the app's EAS Update is behind: what the app's package graph (and the lockfile entries its closure reaches) changed
/// since `commit`.
fn app_changed(reasons: &mut Reasons, commit: Option<String>) -> Result<Vec<String>, String> {
    reasons.since(commit, APP_PACKAGE, &["pnpm-workspace.yaml"], "frontend/mla-tamagui")
}

/// Whether the update published from `commit` still carries the tree at HEAD of the repository at `dir`: empty when it does.
/// One rule for the planner (publish when it is not empty) and for the Android runner (refuse a pointer that is not empty).
pub fn app_changed_since(commit: &str, dir: Option<&Path>) -> Result<Vec<String>, String> {
    app_changed(&mut Reasons { affected: Vec::new(), dir: dir.map(Path::to_path_buf) }, Some(commit.to_string()))
}

/// The commit a service should run when its deployment verification test asks, and why: `sha` when its image was built from it, else the commit
/// its running image was built from while the hash of what that image is built from is the same at `sha` (the question a leg is planned by);
/// `sha` when its commit is unknown or its inputs moved, which a service still running an older commit fails.
fn expected_commit(dir: &Path, app: &str, running: Option<String>, sha: &str) -> (String, String) {
    let Some(running) = running else { return (sha.to_string(), "its running commit is unknown".into()) };
    if sha.starts_with(&running) {
        return (sha.to_string(), "its image is built from the pushed commit".into());
    }
    match image_changes(Some(dir), Some(running.clone()), app) {
        Ok(why) if why.is_empty() => (running, "nothing its image is built from changed since".into()),
        Ok(why) => (sha.to_string(), why.join("; ")),
        Err(e) => (sha.to_string(), e),
    }
}

/// `{app, commit, why}` for each deploy leg among the packages: what `wf test run --stage deployment-verification-tests` hands the tests as
/// `WF_EXPECTED_REVISIONS`, since a push that leaves a service's image inputs alone ships no leg and the service keeps the commit it was built from.
pub(crate) fn expected_revisions(packages: &[&str], sha: &str, dir: &Path) -> Vec<Value> {
    let legs = packages.iter().filter_map(|p| DEPLOY_LEGS.iter().find(|l| l.package == *p));
    legs.map(|leg| {
        let (commit, why) = expected_commit(dir, leg.app, running_commit(leg.app), sha);
        json!({ "app": leg.app, "commit": commit, "why": why })
    })
    .collect()
}

/// The legs a push deploys, each with why (`also` names services that deploy whatever changed), and whether it publishes an EAS Update; `since` stands in for every leg's running
/// commit and for the commit of the latest update (no vm-ops-api and no bucket asked). `latest_update` reads the commit the latest
/// EAS Update was published from and the backend hosts it dials that have moved: `deploy` (its `ship_eas`) holds the pointer, which this crate cannot reach.
pub fn plan(force: bool, also: &[String], since: Option<&str>, github_output: bool, latest_update: impl FnOnce() -> Result<(String, Vec<String>), String>) -> Result<Option<i32>, String> {
    let (legs, update): (Vec<(&str, Vec<String>)>, Vec<String>) = if force {
        (std::iter::once(VM_OPS_API).chain(DEPLOY_LEGS.iter().map(|l| l.app)).chain(std::iter::once(AGENT_SHELL)).map(|a| (a, vec!["--force".to_string()])).collect(), vec!["--force".to_string()])
    } else {
        let mut reasons = Reasons { affected: Vec::new(), dir: None };
        let running = |app: &str| since.map(str::to_string).or_else(|| running_commit(app));
        let mut legs = Vec::new();
        for leg in DEPLOY_LEGS {
            let why = image_changes(None, running(leg.app), leg.app)?;
            if !why.is_empty() {
                legs.push((leg.app, why));
            }
        }
        let why = image_changes(None, running(AGENT_SHELL), AGENT_SHELL)?;
        if !why.is_empty() {
            legs.push((AGENT_SHELL, why));
        }
        // first: the other legs deploy through it, so a change to how it deploys reaches them in the same run
        let vm_ops = since.map(str::to_string).or_else(|| running_commit(VM_OPS_API));
        // the watchdog and the Rust port are its images; its compose project is part of the leg's hash
        let why = image_changes(None, vm_ops, VM_OPS_API)?;
        if !why.is_empty() {
            legs.insert(0, (VM_OPS_API, why));
        }
        // `--also`: named services deploy whatever changed
        for name in also {
            let Some(app) = std::iter::once(VM_OPS_API).chain(DEPLOY_LEGS.iter().map(|l| l.app)).chain(std::iter::once(AGENT_SHELL)).find(|a| a == name) else { continue };
            if !legs.iter().any(|(a, _)| *a == app) {
                let leg = (app, vec!["forced".to_string()]);
                if app == VM_OPS_API { legs.insert(0, leg) } else { legs.push(leg) }
            }
        }
        // the update's base is its own latest commit, not a service's: a publish that failed leaves that behind, and the next push repairs it
        // the tailnet names it baked in count too: the runner refuses an update whose backends moved
        let latest = match since {
            Some(s) => Ok((s.to_string(), Vec::new())),
            None => latest_update(),
        };
        // an update that cannot be shown current is published: the runner refuses what it cannot show current too
        let update = match latest {
            Ok((commit, moved)) => {
                let mut why = app_changed(&mut reasons, Some(commit.clone())).unwrap_or_else(|e| vec![format!("cannot tell whether the app changed since the latest update ({commit}): {e}")]);
                why.extend(moved);
                why
            }
            Err(e) => vec![format!("the commit of the latest update is unknown: {e}")],
        };
        (legs, update)
    };
    let legs_json: Vec<Value> = legs.iter().map(|(app, why)| json!({ "app": app, "why": why })).collect();
    let plan = json!({ "legs": legs_json, "easUpdate": { "publish": !update.is_empty(), "why": update } });
    println!("{plan}");
    if let (true, Ok(path)) = (github_output, std::env::var("GITHUB_OUTPUT")) {
        if !path.is_empty() {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(&path)
                .map_err(|e| format!("cannot open {path}: {e}"))?;
            writeln!(f, "legs={}\neas_update={}", plan["legs"], plan["easUpdate"]["publish"])
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(Some(0))
}
