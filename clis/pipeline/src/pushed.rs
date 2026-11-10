//! `wf check --pushed`: the lefthook pre-push gate. What CI would run for the commits being pushed, in the stages the sandbox can run
//! (typecheck, lint, unit, flow and integration tests, and the browser e2e tests), run here through the same Nx cache, so CI replays them instead of running them. A push
//! to `dev` also lets the run write its passes to the shared cache (`Scope::Pushed`): a pre-push run is the one that proves them.
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use wf_kit::args::{parse_spec, Positional, Spec};
use wf_kit::root::repo_root;

pub const PUSHED: Spec = Spec { values: &["base"], flags: &["pushed"], positional: Positional::None, ..Spec::NONE };

/// The CI stages the sandbox runs before a push, in CI's order. The rest (deploys, the services' e2e, the browser and Android tests)
/// need the VM, the deployed services or the device.
const STAGES: &[&str] = &["typecheck-lint-unit-tests", "flow-tests", "integration-tests", "browser-e2e-tests"];

/// The crates' live-model e2e suite: a push runs it here, so its passes reach the shared Nx cache and CI replays them instead of
/// spending about 40 minutes on the VM. `WF_PUSH_CLI_E2E=0` is the only way to leave it out; without the vault's model key the push fails before any stage runs.
const CLI_E2E: &str = "cli-e2e-tests";

/// The stages this push runs: the fixed ones, then the live-model e2e unless it is switched off; no key to run it with is an error.
fn stages(root: &Path) -> Result<Vec<&'static str>, String> {
    let mut stages = STAGES.to_vec();
    if std::env::var("WF_PUSH_CLI_E2E").is_ok_and(|v| v == "0") {
        return Ok(stages);
    }
    let key = me(root, &["secret", "get", "OPENCODE_GO_API_KEY", "--backend", "bitwarden"]).stderr(Stdio::null()).output().is_ok_and(|o| o.status.success() && !o.stdout.is_empty());
    if !key {
        return Err(format!("pushed: the vault has no OPENCODE_GO_API_KEY for this machine, so {CLI_E2E} cannot run here and CI would spend its 40 minutes on it. Fix the vault, or set WF_PUSH_CLI_E2E=0 to push without it."));
    }
    stages.push(CLI_E2E);
    Ok(stages)
}

/// The stage whose tests need a web export of the commit, baked with the backends' tailnet addresses and the gateway token.
const BROWSER: &str = "browser-e2e-tests";

/// Whether the ref lines git hands a pre-push hook (`<local ref> <local sha> <remote ref> <remote sha>`) include a push to `dev`.
pub fn pushes_to_dev(lines: &str) -> bool {
    lines.lines().any(|l| l.split_whitespace().nth(2) == Some("refs/heads/dev"))
}

/// The local sha a push to `dev` sends; none for a deletion or when `dev` is not pushed.
pub fn dev_push_sha(lines: &str) -> Option<&str> {
    lines.lines().filter_map(|l| {
        let mut f = l.split_whitespace();
        let sha = f.nth(1)?;
        (f.next() == Some("refs/heads/dev") && sha.chars().any(|c| c != '0')).then_some(sha)
    }).next()
}

/// Whether `rev` holds every commit of `origin/main`; a checkout with no `origin/main` has nothing to be behind.
pub fn contains_main(root: &Path, rev: &str) -> bool {
    let git = |args: &[&str]| Command::new("git").args(args).current_dir(root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    !git(&["rev-parse", "--verify", "-q", "refs/remotes/origin/main"]) || git(&["merge-base", "--is-ancestor", "origin/main", rev])
}

/// This binary again, as `wf` or as the thin `pipeline` one: both mount `test`. `WF_PUSHED_EXE` stands in for it in tests.
fn me(root: &Path, args: &[&str]) -> Command {
    let exe = std::env::var_os("WF_PUSHED_EXE").filter(|v| !v.is_empty()).map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok()).unwrap_or_else(|| "wf".into());
    let mut cmd = Command::new(exe);
    cmd.args(args).current_dir(root).stdin(Stdio::null());
    cmd
}

/// Nx names the hoisted version of every npm package from `node_modules` but caches its project graph by the lockfile alone, so a
/// graph built before a reinstall keeps the old names and every package task hashes differently from the runner's. The cache is
/// dropped when the installed tree's `.modules.yaml` is not the one it was built under.
pub fn drop_stale_graph(root: &Path) {
    use std::hash::{Hash, Hasher};
    let Ok(modules) = std::fs::read(root.join("node_modules/.modules.yaml")) else { return };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    modules.hash(&mut hasher);
    let want = format!("{:016x}", hasher.finish());
    let data = root.join(".nx/workspace-data");
    let marker = root.join(".nx/modules-yaml.hash");
    if std::fs::read_to_string(&marker).is_ok_and(|have| have == want) {
        return;
    }
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::create_dir_all(root.join(".nx"));
    let _ = std::fs::write(&marker, want);
}

/// The first line `cmd` prints on stdout when it succeeds; its text never goes anywhere but the caller.
fn capture(cmd: &mut Command) -> Option<String> {
    let out = cmd.stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|t| !t.is_empty())
}

/// The environment CI's browser job gives the export: the backends' tailnet addresses, the gateway token as the backend token, and the
/// app's BetterStack pair when the vault holds both. `Err` names what is missing, so the push stops with the next step.
fn browser_env(root: &Path) -> Result<Vec<(String, String)>, String> {
    let step = |args: &[&str], what: &str, next: &str| me(root, args).status().ok().filter(|s| s.success()).map(|_| ()).ok_or_else(|| format!("{what} failed; {next}"));
    step(&["vm", "net", "up"], "joining the tailnet", "run `wf vm net up` and read its error")?;
    // The sandbox reaches the tailnet in userspace (a SOCKS5 listener, no DNS), where the probe and the name lookup see nothing: the
    // browser harness tunnels by name, so the bare names work there, and a service that is down fails its tests with a clear error.
    if step(&["vm", "ops", "probe", "--wait", "45", "remote-chat-agent", "leetcode-backend"], "the services probe", "").is_err() {
        eprintln!("pushed: the services were not probed from here (no tailnet DNS); the tests report one that is down");
    }
    let resolved: Option<serde_json::Value> = capture(&mut me(root, &["vm", "net", "resolve", "leetcode-backend-prod", "remote-chat-agent-prod", "vm-ops-api-main"])).and_then(|t| serde_json::from_str(&t).ok());
    let host = |name: &str| resolved.as_ref().and_then(|h| h[name].as_str()).map_or_else(|| name.to_string(), str::to_string);
    let seed = capture(me(root, &["secret", "get", "VM_GATEWAY_AUTH_SEED", "--backend", "bitwarden"]).stdout(Stdio::piped()))
        .ok_or("the vault has no VM_GATEWAY_AUTH_SEED (`wf secret doctor --backend bitwarden`)")?;
    let mut env = vec![
        ("EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL".to_string(), format!("http://{}:8789", host("leetcode-backend-prod"))),
        ("EXPO_PUBLIC_AGENT_BASE_URL".to_string(), format!("http://{}:8787", host("remote-chat-agent-prod"))),
        ("EXPO_PUBLIC_VM_OPS_API_BASE_URL".to_string(), format!("http://{}:8080", host("vm-ops-api-main"))),
        ("EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN".to_string(), seed),
    ];
    let pair = capture(me(root, &["secret", "get", "EXPO_PUBLIC_BETTERSTACK_TOKEN", "--backend", "bitwarden"]).stdout(Stdio::piped()))
        .zip(capture(me(root, &["secret", "get", "BETTERSTACK_INGEST_HOST", "--backend", "bitwarden"]).stdout(Stdio::piped())));
    if let Some((token, host)) = pair {
        env.push(("EXPO_PUBLIC_BETTERSTACK_TOKEN".to_string(), token));
        env.push(("EXPO_PUBLIC_BETTERSTACK_HOST".to_string(), host));
    }
    Ok(env)
}

/// What the browser stage would run: the tests its `--probe` says are not replays.
fn browser_would_run(root: &Path, plan: &str, to_dev: bool, local_cache: &Path) -> bool {
    let mut cmd = me(root, &["test", "run", "--stage", BROWSER, "--plan", plan, "--probe"]);
    if to_dev {
        cmd.env("WF_PUSH_TO_DEV", "1").env("NX_CACHE_DIRECTORY", local_cache);
    }
    capture(&mut cmd).and_then(|t| serde_json::from_str::<serde_json::Value>(t.lines().last().unwrap_or("")).ok()).is_none_or(|v| v["ran"].as_array().is_none_or(|a| !a.is_empty()))
}

/// What a stage's stderr says about the shared cache: the whole point of a push to dev is that its passes reach CI, so a run that
/// only read the cache (a dirty tree, a commit not on origin/dev, unpinned tools) or went cold (the store was unreachable) fails the push.
pub fn seeding(stderr: &str) -> Result<(), String> {
    if let Some(line) = stderr.lines().find(|l| l.contains("build cache reads only for this run")) {
        return Err(line.trim().to_string());
    }
    for line in stderr.lines().filter(|l| l.starts_with("wf: build cache {")) {
        let stats: serde_json::Value = serde_json::from_str(&line["wf: build cache ".len()..]).unwrap_or_default();
        if let Some(why) = stats["degraded"].as_str() {
            return Err(format!("the cache went cold ({why})"));
        }
    }
    match seeded_counts(stderr) {
        Some((present, checked)) if present < checked => Err(format!("seeded {present} of {checked}: the store acknowledged writes it does not hold")),
        _ => Ok(()),
    }
}

/// How many of the entries a stage wrote were found in the store when asked for again, as (present, checked), summed over the
/// stage's cache lines; `None` when the stage wrote nothing it could look up.
pub fn seeded_counts(stderr: &str) -> Option<(u64, u64)> {
    let mut sum: Option<(u64, u64)> = None;
    for line in stderr.lines().filter(|l| l.starts_with("wf: build cache {")) {
        let stats: serde_json::Value = serde_json::from_str(&line["wf: build cache ".len()..]).unwrap_or_default();
        if let (Some(checked), Some(present)) = (stats["seeded"]["checked"].as_u64(), stats["seeded"]["present"].as_u64()) {
            let (p, c) = sum.unwrap_or((0, 0));
            sum = Some((p + present, c + checked));
        }
    }
    sum
}

/// The verdict of a stage that passed on a push to dev: an event, and `ci.gate.seeded{stage}` as 1 or 0.
fn report_seeding(stage: &str, seeded: &Result<(), String>, counts: Option<(u64, u64)>) {
    use wf_bucket::ci_metrics::{event, Gauges, ALL};
    let why = seeded.as_ref().err().cloned().unwrap_or_default();
    let (present, checked) = counts.unwrap_or((0, 0));
    event(
        if seeded.is_ok() { "info" } else { "warn" },
        "gate.seeding",
        &[("stage", stage.to_string()), ("seeded", seeded.is_ok().to_string()), ("present", present.to_string()), ("checked", checked.to_string()), ("why", why)],
    );
    if checked > 0 {
        eprintln!("pushed: {stage} seeded {present} of {checked} entries it wrote");
    }
    Gauges::new().set("ci.gate.seeded", stage, ALL, f64::from(u8::from(seeded.is_ok()))).send();
}

/// Runs one stage with its stderr shown as it comes; for a push to dev also says whether the run seeded the cache.
fn run_stage(mut cmd: Command, to_dev: bool) -> (bool, Result<(), String>, Option<(u64, u64)>) {
    use std::io::BufRead;
    let Ok(mut child) = cmd.stdout(Stdio::inherit()).stderr(Stdio::piped()).spawn() else { return (false, Ok(()), None) };
    let mut seen = String::new();
    if let Some(err) = child.stderr.take() {
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            eprintln!("{line}");
            if to_dev && (line.starts_with("wf: build cache") || line.contains("reads only for this run")) {
                seen.push_str(&line);
                seen.push('\n');
            }
        }
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    (ok, if to_dev { seeding(&seen) } else { Ok(()) }, seeded_counts(&seen))
}

fn is_test_binary(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    let hashed = name.rsplit_once('-').is_some_and(|(_, h)| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()));
    // a hard link is the uplifted copy of a binary the next stage runs (`target/debug/<bin>`); a test executable has no other name
    hashed && std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.mode() & 0o111 != 0 && m.nlink() == 1)
}

/// The `target` directories a stage compiles into: `CARGO_TARGET_DIR`, else each `target` within three levels of the repo that holds a `debug`.
fn target_dirs(root: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if let Some(dir) = std::env::var_os("CARGO_TARGET_DIR").filter(|v| !v.is_empty()) {
        if depth == 0 {
            found.push(PathBuf::from(dir));
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
        match e.file_name().to_str() {
            Some("target") => found.push(e.path()),
            Some("node_modules" | ".git" | "references") => {}
            _ if depth < 3 => target_dirs(&e.path(), depth + 1, found),
            _ => {}
        }
    }
}

/// Deletes the test executables a passed stage left under the target directories (`debug/deps` and `<name>/debug/deps` of each) and returns the bytes freed.
/// Libraries, dependency files, the uplifted binaries and incremental state stay, so the next stage links instead of recompiling the world.
pub fn prune_test_binaries(root: &Path) -> u64 {
    let mut targets = Vec::new();
    target_dirs(root, 0, &mut targets);
    let mut freed = 0;
    for target in targets {
        let nested = std::fs::read_dir(&target).into_iter().flatten().flatten().map(|e| e.path().join("debug/deps"));
        for deps in std::iter::once(target.join("debug/deps")).chain(nested) {
            for e in std::fs::read_dir(&deps).into_iter().flatten().flatten() {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                if is_test_binary(&e.path()) && std::fs::remove_file(e.path()).is_ok() {
                    freed += size;
                }
            }
        }
    }
    freed
}

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &PUSHED)?;
    let base = p.val("base").unwrap_or("origin/dev").to_string();
    let root = repo_root();
    let mut refs = String::new();
    if !std::io::stdin().is_terminal() {
        let _ = std::io::stdin().read_to_string(&mut refs);
    }
    let to_dev = pushes_to_dev(&refs);
    if dev_push_sha(&refs).is_some_and(|sha| !contains_main(&root, sha)) {
        wf_kit::wf_err!("wf: not pushing: dev does not contain origin/main: git fetch origin main && git merge origin/main, resolve conflicts (main is a squash of dev, so expect them; never rebase this merge), push. See CLAUDE.md, Land on dev");
        return Some(1);
    }
    let off_pin: Vec<String> = wf_bucket::pins::pins(&root).iter().filter(|p| !p.ok()).map(|p| format!("{} {} (pinned {})", p.tool, if p.actual.is_empty() { "missing" } else { &p.actual }, p.pinned)).collect();
    if !off_pin.is_empty() {
        wf_kit::wf_err!("wf: not pushing: {}; Nx hashes the tool versions, so no pass here would replay on CI. Run bash .agents/setup.sh, then push again (wf test drift --pins shows the same check)", off_pin.join(", "));
        return Some(1);
    }
    let scope = secrets::scope_violations(&root.join(".github/workflows")).unwrap_or_else(|e| vec![e]);
    if !scope.is_empty() {
        wf_kit::wf_err!("wf: not pushing: a CI job asks for a vault key past its class ({}); wf secret scope-check lists every job", scope.join("; "));
        return Some(1);
    }
    drop_stale_graph(&root);

    let planned = me(&root, &["test", "plan", "--changed", "--base", &base, "--json"]).stderr(Stdio::inherit()).output();
    let plan = match planned {
        Ok(o) if o.status.success() => o.stdout,
        Ok(o) => {
            wf_kit::wf_err!("wf: cannot plan the pushed commits against {base}: exit {}", o.status.code().unwrap_or(1));
            return Some(1);
        }
        Err(e) => {
            wf_kit::wf_err!("wf: cannot run the plan: {e}");
            return Some(1);
        }
    };
    let file = std::env::temp_dir().join(format!("wf-pushed-plan-{}.json", std::process::id()));
    if let Err(e) = std::fs::write(&file, &plan) {
        wf_kit::wf_err!("wf: cannot write the plan to {}: {e}", file.display());
        return Some(1);
    }
    let plan_arg = file.to_string_lossy().to_string();

    // Nx writes to the shared cache only a task it ran: a pass its local cache already holds would never reach CI, which then runs it again
    let local_cache = std::env::temp_dir().join(format!("wf-pushed-nx-{}", std::process::id()));
    let mut failed = Vec::new();
    let stages = match stages(&root) {
        Ok(stages) => stages,
        Err(message) => {
            wf_kit::wf_err!("{message}");
            return Some(1);
        }
    };
    for stage in &stages {
        eprintln!("pushed: {stage}{}", if to_dev { " (writes to the shared cache)" } else { "" });
        let mut cmd = me(&root, &["test", "run", "--stage", stage, "--plan", &plan_arg]);
        // A one-shot gate gains nothing from incremental compile state, which is gigabytes the sandbox's disk does not have
        cmd.env("CARGO_INCREMENTAL", "0");
        if to_dev {
            cmd.env("WF_PUSH_TO_DEV", "1").env("NX_CACHE_DIRECTORY", &local_cache);
        }
        if *stage == BROWSER && browser_would_run(&root, &plan_arg, to_dev, &local_cache) {
            // a test that runs needs the export CI's browser job builds; an all-replay stage needs nothing
            match browser_env(&root).and_then(|env| me(&root, &["test", "prepare", "--web"]).envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str()))).stdout(Stdio::null()).status().map_err(|e| e.to_string()).and_then(|s| if s.success() { Ok(()) } else { Err("`wf test prepare --web` failed".to_string()) }).map(|()| env)) {
                Ok(env) => {
                    // the sandbox reaches the tailnet only through tailscaled's SOCKS5 listener, so the browser must use it
                    cmd.envs(env).env("WF_BROWSER_TAILNET", "1");
                }
                Err(why) => {
                    wf_kit::wf_err!("wf: the browser e2e tests cannot run here: {why}");
                    failed.push(*stage);
                    continue;
                }
            }
        }
        let (ok, seeded, counts) = run_stage(cmd, to_dev);
        if ok && to_dev {
            report_seeding(stage, &seeded, counts);
        }
        // after a failure nothing more is deleted: the failed stage's executables are the evidence
        if ok && failed.is_empty() {
            let freed = prune_test_binaries(&root);
            if freed > 0 {
                eprintln!("pushed: {stage} passed, freed {} MB of test binaries", freed / 1_000_000);
            }
        }
        if !ok {
            failed.push(*stage);
        } else if let Err(why) = seeded {
            wf_kit::wf_err!("wf: {stage} passed here but did not seed the shared cache, so CI would run it again: {why}");
            failed.push(*stage);
        }
    }
    let _ = std::fs::remove_file(&file);
    let _ = std::fs::remove_dir_all(&local_cache);
    if failed.is_empty() {
        eprintln!("pushed: {} passed here, so CI replays them", stages.join(" and "));
        Some(0)
    } else {
        wf_kit::wf_err!("wf: not pushing: {} failed here; CI would run the same tasks", failed.join(" and "));
        Some(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_an_install_that_changed_when_the_gate_starts_then_the_cached_graph_is_dropped_once() {
        // Given a repo with a cached graph and an installed tree
        let dir = std::env::temp_dir().join(format!("wf-pushed-graph-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("node_modules")).unwrap();
        std::fs::create_dir_all(dir.join(".nx/workspace-data")).unwrap();
        std::fs::write(dir.join("node_modules/.modules.yaml"), "hoisted: jsdom 27").unwrap();

        // When the gate checks it for the first time, Then the graph is dropped
        drop_stale_graph(&dir);
        assert!(!dir.join(".nx/workspace-data").exists());

        // And an unchanged tree keeps the graph rebuilt after that
        std::fs::create_dir_all(dir.join(".nx/workspace-data")).unwrap();
        drop_stale_graph(&dir);
        assert!(dir.join(".nx/workspace-data").exists());

        // And a reinstall that moved the hoisting drops it again
        std::fs::write(dir.join("node_modules/.modules.yaml"), "hoisted: jsdom 30").unwrap();
        drop_stale_graph(&dir);
        assert!(!dir.join(".nx/workspace-data").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn given_the_ref_lines_of_a_push_when_asked_then_only_a_push_to_dev_counts() {
        // Given git's pre-push ref lines for a push to dev, one to a feature branch, and an empty push
        let dev = "refs/heads/claude/device-lab abc refs/heads/dev def\n";
        let feature = "refs/heads/x abc refs/heads/claude/device-lab def\n";

        // When each is read, Then only the first is a push to dev
        assert!(pushes_to_dev(dev));
        assert!(pushes_to_dev(&format!("{feature}{dev}")));
        assert!(!pushes_to_dev(feature));
        assert!(!pushes_to_dev(""));
    }
}
