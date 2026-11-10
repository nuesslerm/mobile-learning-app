//! `wf offload plan <slug>` and `wf offload worker`: checking a units.jsonl, reading each unit's state off the remote's refs and trailers, and
//! running a plan once: claim, implement through `wf offload ask` (the rca service as a fake), gate, land with a `Unit:` trailer, release.
//! The repositories are real git (a bare origin and a clone); `pnpm` is an argv-recording stub and the rca service is a local listener that writes the files the prompt names.
use crate::support::{create_sh_stub, iso_from_ms, now_ms, run_wf, spawn_wf, wait_until, RunOpts, RunResult};
use regex::Regex;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A unit's id, deps, gate and brief.
type Unit<'a> = (&'a str, &'a [&'a str], Option<&'a str>, Option<&'a str>);

fn git_in(cwd: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(cwd).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn unit(overrides: Value) -> String {
    let mut base = json!({ "title": "t", "tier": "unit", "files": ["src/a.ts"], "brief": "do it", "deps": [], "gate": "pnpm test" });
    for (key, value) in overrides.as_object().unwrap() {
        base[key] = value.clone();
    }
    base.to_string()
}

/// A bare origin on `dev` and a clone of it, with a directory of stub tools.
struct Setting {
    root: tempfile::TempDir,
    origin: PathBuf,
    repo: PathBuf,
    bin: PathBuf,
    wt: PathBuf,
    rca: Rca,
}

const LAND: &str = "scratch/land";

const STUB_PNPM: &str = r#"
echo "$*" >> "$0.argv.log"
case "$1" in install) [ -n "$STUB_INSTALL_FAIL" ] && exit 1; exit 0;; esac
exit 1
"#;

/// What the fake rca service does with the next runs.
#[derive(Default)]
struct Scenario {
    /// Hang instead of answering.
    sleep: bool,
    /// Push an unrelated commit onto the land ref first.
    move_base: bool,
}

/// The rca service as a local listener: it answers `POST /runs` with the files the prompt's `FILES:` line names (`NOOP` in the prompt
/// writes nothing), and records each request body.
struct Rca {
    port: String,
    bodies: Arc<Mutex<Vec<Value>>>,
    scenario: Arc<Mutex<Scenario>>,
}

impl Rca {
    fn start(origin: PathBuf) -> Rca {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let (bodies, scenario) = (Arc::new(Mutex::new(Vec::new())), Arc::new(Mutex::new(Scenario::default())));
        let (seen, plan) = (Arc::clone(&bodies), Arc::clone(&scenario));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (seen, plan, origin) = (Arc::clone(&seen), Arc::clone(&plan), origin.clone());
                std::thread::spawn(move || Self::serve(stream, &seen, &plan, &origin));
            }
        });
        Rca { port, bodies, scenario }
    }

    fn serve(mut stream: std::net::TcpStream, seen: &Mutex<Vec<Value>>, plan: &Mutex<Scenario>, origin: &Path) {
        let mut raw = Vec::new();
        let mut buf = [0u8; 8192];
        let (head_end, length) = loop {
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                return;
            }
            raw.extend_from_slice(&buf[..n]);
            if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&raw[..at]).to_ascii_lowercase();
                let length = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                break (at + 4, length);
            }
        };
        while raw.len() < head_end + length {
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
        }
        let body: Value = serde_json::from_slice(&raw[head_end..]).unwrap_or(Value::Null);
        seen.lock().unwrap().push(body.clone());
        let (sleep, move_base) = {
            let p = plan.lock().unwrap();
            (p.sleep, p.move_base)
        };
        if sleep {
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        if move_base {
            let tmp = tempfile::tempdir().unwrap();
            let clone = tmp.path().join("c");
            git_in(tmp.path(), &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()]);
            let _ = Command::new("git").args(["checkout", "-q", "-B", LAND, &format!("origin/{LAND}")]).current_dir(&clone).output();
            std::fs::write(clone.join("other.txt"), "other\n").unwrap();
            git_in(&clone, &["add", "other.txt"]);
            git_in(&clone, &["commit", "-q", "-m", "other"]);
            git_in(&clone, &["push", "-q", "origin", &format!("HEAD:refs/heads/{LAND}")]);
        }
        let prompt = body["prompt"].as_str().unwrap_or_default();
        let changes: Vec<Value> = if prompt.contains("NOOP") {
            Vec::new()
        } else {
            prompt.lines().find_map(|l| l.strip_prefix("FILES: ")).unwrap_or_default().split(',').filter(|f| !f.is_empty()).map(|f| json!({ "path": f, "content": "content\n" })).collect()
        };
        let attempt = json!({ "index": 0, "choice": { "model": "minimax/MiniMax-M3" }, "verdict": { "ok": true } });
        let reply = json!({ "ok": true, "output": { "text": "ok", "toolEvents": [], "structured": true }, "attempt": attempt, "attempts": [attempt], "changes": changes }).to_string();
        let _ = write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}", reply.len());
    }

    /// The prompts the service was asked, as the old stub logged them.
    fn prompts(&self) -> String {
        self.bodies.lock().unwrap().iter().map(|b| format!("{}\n---\n", b["prompt"].as_str().unwrap_or_default())).collect()
    }
}

impl Setting {
    fn new() -> Setting {
        let root = tempfile::Builder::new().prefix("wf-flow-worker-").tempdir().unwrap();
        let base = root.path().canonicalize().unwrap();
        let (origin, repo, bin, wt) = (base.join("o/origin.git"), base.join("repo"), base.join("bin"), base.join("wt/trees"));
        std::fs::create_dir_all(origin.parent().unwrap()).unwrap();
        std::fs::create_dir(&bin).unwrap();
        git_in(&base, &["init", "-q", "--bare", "-b", "dev", origin.to_str().unwrap()]);
        git_in(&base, &["clone", "-q", origin.to_str().unwrap(), repo.to_str().unwrap()]);
        git_in(&repo, &["checkout", "-q", "-b", "dev"]);
        std::fs::write(repo.join("README.md"), "x\n").unwrap();
        git_in(&repo, &["add", "."]);
        git_in(&repo, &["commit", "-q", "-m", "init"]);
        git_in(&repo, &["push", "-q", "origin", "dev"]);
        create_sh_stub(&bin.join("pnpm"), STUB_PNPM);
        let rca = Rca::start(origin.clone());
        Setting { root, origin, repo, bin, wt, rca }
    }

    fn plan(&self, slug: &str, lines: &[String]) -> PathBuf {
        let path = self.repo.join("docs/plans").join(slug).join("units.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
        path
    }

    /// A plan of units writing `<id>.txt`, gated on that file unless a gate is given.
    fn units(&self, slug: &str, units: &[Unit]) {
        let lines: Vec<String> = units
            .iter()
            .map(|(id, deps, gate, brief)| json!({ "id": id, "title": format!("add {id}"), "tier": "unit", "files": [format!("{id}.txt")], "brief": brief.map(String::from).unwrap_or_else(|| format!("write {id}")), "deps": deps, "gate": gate.map(String::from).unwrap_or_else(|| format!("test -f {id}.txt")) }).to_string())
            .collect();
        self.plan(slug, &lines);
    }

    /// `wf` in the repo with a clean environment (read-only commands).
    fn wf(&self, args: &[&str]) -> RunResult {
        run_wf(args, &RunOpts::default().cwd(&self.repo).clean_env())
    }

    fn opts(&self, env: &[(&str, &str)]) -> RunOpts {
        let at = |name: &str| self.root.path().join(name).display().to_string();
        let mut opts = RunOpts::default()
            .cwd(&self.repo)
            .bin_dir(&self.bin)
            .timeout(Duration::from_secs(60))
            .env("WF_WORKER_TICK_MS", "40")
            .env("WF_WORKER_LAUNCH_SPACING_MS", "0")
            .env("CHAT_AGENT_API_SECRET", "s3cret")
            .env("RCA_HOST", "127.0.0.1")
            .env("RCA_PORT", &self.rca.port)
            .env("VITEST", "true")
            .env("SECRETS_BACKEND", "macos")
            .env("SECRETS_LOG_BETTERSTACK", "false")
            .env("OPENCODE_DISPATCH_RUN_LOG", &at("runs.jsonl"))
            .env("OPENCODE_POOL_LEDGER", &at("pool.json"))
            .env("WF_TAILNET_SOCKS", "");
        *self.rca.scenario.lock().unwrap() = Scenario { sleep: env.iter().any(|(k, _)| *k == "STUB_SLEEP"), move_base: env.iter().any(|(k, _)| *k == "STUB_MOVE_BASE") };
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        opts
    }

    /// `wf offload plan <slug> --once` onto the land ref, the prompts log cleared first.
    fn work(&self, slug: &str, extra: &[&str], env: &[(&str, &str)]) -> RunResult {
        self.rca.bodies.lock().unwrap().clear();
        let wt = self.wt.display().to_string();
        let mut args = vec!["offload", "plan", slug, "--once", "--land-ref", LAND, "--worktree-dir", &wt];
        args.extend_from_slice(extra);
        run_wf(&args, &self.opts(env))
    }

    fn remote(&self, args: &[&str]) -> String {
        git_in(&self.origin, args)
    }

    fn prompts(&self) -> String {
        self.rca.prompts()
    }

    fn refs(&self, prefix: &str) -> Vec<String> {
        self.remote(&["for-each-ref", "--format=%(refname)", prefix]).lines().filter(|l| !l.is_empty()).map(String::from).collect()
    }

    fn push_ref(&self, message: &str, name: &str) {
        let tree = git_in(&self.repo, &["hash-object", "-t", "tree", "-w", "/dev/null"]);
        let sha = git_in(&self.repo, &["commit-tree", tree.trim(), "-m", message]).trim().to_string();
        git_in(&self.repo, &["push", "-q", "origin", &format!("{sha}:{name}")]);
    }

    fn landed(&self, slug: &str, id: &str) {
        std::fs::write(self.repo.join(format!("{id}.txt")), id).unwrap();
        git_in(&self.repo, &["add", "-A"]);
        git_in(&self.repo, &["commit", "-qm", &format!("land {id}\n\nUnit: {slug}/{id}")]);
        git_in(&self.repo, &["push", "-q", "origin", "HEAD"]);
    }
}

fn lease(worker: &str, started_ago_ms: i128, beat_ago_ms: i128) -> String {
    json!({ "worker": worker, "attempt": 1, "model": "m", "startedAt": iso_from_ms(now_ms() - started_ago_ms), "heartbeatAt": iso_from_ms(now_ms() - beat_ago_ms) }).to_string()
}

/// What the log says happened, without the clock, the worker name or the durations.
fn story(stderr: &str) -> Vec<String> {
    stderr
        .split("\nwf: ")
        .next()
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with('{'))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .map(|e| ["event", "id", "reason", "exitCode"].iter().filter_map(|k| e.get(*k).filter(|v| !v.is_null()).map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string()))).collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn a_plan_is_checked_before_it_runs_and_its_state_is_read_off_the_remote() {
    let s = Setting::new();

    // When a valid plan is checked, in both spellings
    s.plan("p1", &[unit(json!({ "id": "b", "deps": ["a"], "files": ["src/b.ts", "src/c.ts"], "tier": "flow", "gate": "pnpm test:flow" })), unit(json!({ "id": "a" })), unit(json!({ "id": "c", "deps": ["b"] }))]);
    s.plan("one", &[unit(json!({ "id": "a" }))]);
    let checked = s.wf(&["offload", "plan", "p1", "--check"]);
    let worker = s.wf(&["offload", "worker", "check", "--plan", "one"]);
    // Then units print in dependency order with their gate and a count
    assert_eq!((checked.exit_code, checked.stdout.as_str()), (0, "a  unit  src/a.ts  gate: pnpm test\nb  flow  src/b.ts,src/c.ts  gate: pnpm test:flow\nc  unit  src/a.ts  gate: pnpm test\nok: 3 unit(s)\n"), "{}", checked.stderr);
    assert_eq!((worker.exit_code, worker.stdout.as_str()), (0, "a  unit  src/a.ts  gate: pnpm test\nok: 1 unit(s)\n"), "{}", worker.stderr);

    // When a plan with every kind of broken line is checked
    let path = s.plan(
        "broken",
        &[
            "{not json".into(),
            "# a comment".into(),
            String::new(),
            "[1]".into(),
            json!({ "id": "x" }).to_string(),
            unit(json!({ "id": "f", "files": ["1", "2", "3", "4"] })),
            unit(json!({ "id": "g", "files": ["a", "a"] })),
            unit(json!({ "id": "h", "tier": "bogus" })),
            unit(json!({ "id": "i", "needs": "gpu" })),
            unit(json!({ "id": "j", "deps": "a" })),
            unit(json!({ "id": "k", "gate": " " })),
            unit(json!({ "id": "ok" })),
            unit(json!({ "id": "ok" })),
        ],
    );
    let broken = s.wf(&["offload", "plan", "broken", "--check"]);
    // Then every error is listed with its line, the report is repeated as the failure message, and the exit code is 2
    assert_eq!((broken.exit_code, broken.stdout.as_str()), (2, ""));
    let first = &broken.stderr[..broken.stderr.find("\nwf: ").expect("the failure message") + 1];
    assert_eq!(broken.stderr, format!("{first}wf: {first}\n"));
    let lines: Vec<&str> = first.split('\n').collect();
    assert_eq!(lines[0], format!("{}:", path.display()));
    assert!(Regex::new(r"^ {2}1: not JSON: ").unwrap().is_match(lines[1]), "{}", lines[1]);
    assert_eq!(
        &lines[2..lines.len() - 1],
        [
            "  4: not an object",
            "  5: x: files must list 1-3 paths (got 0)",
            "  6: f: files must list 1-3 paths (got 4)",
            "  7: g: duplicate entry in files",
            "  8: h: missing or unknown tier (allowed: unit, contract, flow, integration, e2e, visual)",
            "  9: i: unknown needs (allowed: any, deployed)",
            "  10: j: deps must be an array of unit ids",
            "  11: k: missing gate",
            "  13: duplicate id ok"
        ]
    );

    // When a plan depends on an unknown unit, has a cycle, is empty, or is missing, or the slug has a space
    s.plan("unknown", &[unit(json!({ "id": "a", "deps": ["zzz"] }))]);
    s.plan("cycle", &[unit(json!({ "id": "a", "deps": ["b"] })), unit(json!({ "id": "b", "deps": ["c"] })), unit(json!({ "id": "c", "deps": ["a"] }))]);
    s.plan("empty", &["# nothing".into()]);
    let unknown = s.wf(&["offload", "plan", "unknown", "--check"]);
    let cycle = s.wf(&["offload", "plan", "cycle", "--check"]);
    let empty = s.wf(&["offload", "plan", "empty", "--check"]);
    let missing = s.wf(&["offload", "plan", "nope", "--check"]);
    let spaced = s.wf(&["offload", "worker", "check", "--plan", "bad slug"]);
    // Then each says what is wrong and exits 2
    assert!(unknown.stderr.contains("  -: a: depends on unknown unit zzz"), "{}", unknown.stderr);
    assert_eq!(cycle.exit_code, 2);
    assert!(cycle.stderr.contains("  -: cycle: a -> b -> c -> a"), "{}", cycle.stderr);
    assert!(empty.stderr.contains("  -: no units"), "{}", empty.stderr);
    assert_eq!(missing.exit_code, 2);
    let message = format!("no units file: {}\n", s.repo.join("docs/plans/nope/units.jsonl").display());
    assert_eq!(missing.stderr, format!("{message}wf: {message}\n"));
    assert_eq!(spaced.exit_code, 2);
    assert!(spaced.stderr.contains("--plan <slug> is required\nusage:\n") && spaced.stderr.contains("wf offload worker --plan <slug>"), "{}", spaced.stderr);

    // Given units that are landed, leased (fresh, stale, unreadable), failed, blocked and runnable
    s.plan(
        "state",
        &[
            unit(json!({ "id": "done1" })),
            unit(json!({ "id": "leased1", "files": ["src/l.ts"] })),
            unit(json!({ "id": "stale1", "files": ["src/s.ts"] })),
            unit(json!({ "id": "junk1", "files": ["src/j.ts"] })),
            unit(json!({ "id": "failed1", "files": ["src/f.ts"] })),
            unit(json!({ "id": "blocked1", "deps": ["leased1", "done1"], "files": ["src/b.ts"] })),
            unit(json!({ "id": "ready1", "deps": ["done1"], "files": ["src/r.ts"] })),
        ],
    );
    s.landed("state", "done1");
    s.push_ref(&lease("sandbox:h:1", 300_000, 1000), "refs/leases/state/leased1");
    s.push_ref(&lease("vm:h:2", 3_600_000, 3_000_000), "refs/leases/state/stale1");
    s.push_ref("not a lease", "refs/leases/state/junk1");
    s.push_ref("gate output", "refs/failed/state/failed1");
    s.plan("fresh", &[unit(json!({ "id": "a" })), unit(json!({ "id": "b", "deps": ["a"], "files": ["src/b.ts"] }))]);
    // When their status is asked for
    let status = s.wf(&["offload", "plan", "--status", "state"]);
    let fresh = s.wf(&["offload", "worker", "status", "--plan", "fresh"]);
    // Then each unit shows its state and a summary counts them; with no refs, roots are runnable and dependents blocked
    assert_eq!(status.exit_code, 0, "{}", status.stderr);
    assert_eq!(
        status.stdout,
        ["done1     done", "leased1   leased    by sandbox:h:1, 5m", "stale1    leased    by vm:h:2, 60m (stale)", "junk1     leased    by unknown, 0s (stale)", "failed1   failed", "blocked1  blocked   waiting on leased1", "ready1    runnable", "1 done, 3 leased, 1 failed, 1 blocked, 1 runnable", ""].join("\n")
    );
    assert_eq!((fresh.exit_code, fresh.stdout.as_str()), (0, "a   runnable\nb   blocked   waiting on a\n1 runnable, 1 blocked\n"));

    // Given a unit landed on another branch only
    s.plan("other", &[unit(json!({ "id": "x1" }))]);
    git_in(&s.repo, &["checkout", "-q", "-b", "integration"]);
    s.landed("other", "x1");
    // When its status is read from dev and from that branch
    let on_dev = s.wf(&["offload", "plan", "--status", "other"]);
    let on_integration = s.wf(&["offload", "plan", "--status", "other", "--land-ref", "integration"]);
    // Then the trailers are read from the branch asked for
    assert_eq!((on_dev.stdout.as_str(), on_integration.stdout.as_str()), ("x1  runnable\n1 runnable\n", "x1  done\n1 done\n"));
}

#[test]
fn a_worker_runs_a_plan_once_landing_what_passes_its_gate_and_releasing_what_cannot_run() {
    let s = Setting::new();
    let land_log = || s.remote(&["log", LAND, "--format=%s%n%(trailers:key=Unit,valueonly)"]);

    // When a plan of two chained units runs once
    s.units("chain", &[("u1", &[], None, None), ("u2", &["u1"], Some("test -f u1.txt && test -f u2.txt"), None)]);
    let r = s.work("chain", &[], &[]);
    // Then both are claimed, gated and landed in order with a Unit trailer, no lease or worktree is left behind
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(r.stdout, "landed 2 [u1 u2], failed 0 [], released 0 []\n");
    assert_eq!(story(&r.stderr), ["worker.unit.claimed u1", "worker.unit.gated u1", "worker.unit.landed u1", "worker.unit.claimed u2", "worker.unit.gated u2", "worker.unit.landed u2"]);
    assert!(land_log().contains("chain/u1") && land_log().contains("chain/u2"), "{}", land_log());
    assert_eq!(s.remote(&["show", &format!("{LAND}:u2.txt")]), "content\n");
    assert!(s.refs("refs/leases").is_empty());
    assert_eq!(git_in(&s.repo, &["worktree", "list"]).trim().lines().count(), 1);
    // and the implementer got the brief, the files and the gate after a frozen install, through the rca service with the unit's files
    assert!(s.prompts().contains("write u2\n\nFILES: u2.txt\nVERIFY: test -f u1.txt && test -f u2.txt"), "{}", s.prompts());
    let pnpm = std::fs::read_to_string(s.bin.join("pnpm.argv.log")).unwrap();
    assert!(pnpm.contains("install --prefer-offline --frozen-lockfile") && !pnpm.contains("rca"), "{pnpm}");
    let first = s.rca.bodies.lock().unwrap()[0].clone();
    assert_eq!((first["shape"].clone(), first["caller"].clone()), (json!("implementer:unit"), json!("worker")));

    // When the plan runs again
    let again = s.work("chain", &[], &[]);
    // Then nothing starts
    assert_eq!(again.stdout, "landed 0 [], failed 0 [], released 0 []\n", "{}", again.stderr);

    // When the land ref moves on an unrelated file while a unit works
    s.units("moved", &[("m1", &[], None, None)]);
    let moved = s.work("moved", &[], &[("STUB_MOVE_BASE", "1")]);
    // Then the unit is rebased onto it and still pushed
    assert_eq!(moved.stdout, "landed 1 [m1], failed 0 [], released 0 []\n", "{}", moved.stderr);
    assert_eq!((s.remote(&["show", &format!("{LAND}:other.txt")]), s.remote(&["show", &format!("{LAND}:m1.txt")])), ("other\n".to_string(), "content\n".to_string()));
    assert_eq!(s.remote(&["log", "-3", LAND, "--format=%s"]).trim().lines().collect::<Vec<_>>(), ["add m1", "other", "add u2"]);

    // When a unit's gate never passes, and it has a dependent
    s.units("red", &[("r1", &[], Some("echo GATE-SAID-NO; exit 3"), None), ("r2", &["r1"], None, None)]);
    let red = s.work("red", &[], &[]);
    // Then after two attempts (the second told the gate output) it is marked failed with that output, its dependent never starts,
    // and no lease or scratch branch is left
    assert_eq!(red.stdout, "landed 0 [], failed 1 [r1], released 0 []\n", "{}", red.stderr);
    assert_eq!(story(&red.stderr), ["worker.unit.claimed r1", "worker.unit.failed r1 3"]);
    assert_eq!(s.refs("refs/failed"), ["refs/failed/red/r1"]);
    assert_eq!(s.remote(&["log", "-1", "--format=%B", "refs/failed/red/r1"]).trim(), "GATE-SAID-NO");
    assert_eq!(s.prompts().matches("---\n").count(), 2);
    assert!(s.prompts().contains("Previous attempt failed. Fix this:\nGATE-SAID-NO"), "{}", s.prompts());
    assert!(s.refs("refs/leases").is_empty() && s.refs("refs/heads/scratch").iter().all(|r| r == &format!("refs/heads/{LAND}")), "{:?}", s.refs("refs/heads/scratch"));

    // When the implementer writes nothing
    s.units("noop", &[("n1", &[], None, Some("NOOP"))]);
    let noop = s.work("noop", &[], &[]);
    // Then it is told to make the edit, and then the unit fails
    assert_eq!(noop.stdout, "landed 0 [], failed 1 [n1], released 0 []\n", "{}", noop.stderr);
    assert!(s.prompts().contains("Previous attempt failed. Fix this:\nimplementer wrote nothing; make the edit now"), "{}", s.prompts());
    assert_eq!(s.remote(&["log", "-1", "--format=%B", "refs/failed/noop/n1"]).trim(), "implementer wrote nothing");

    // When the install fails
    s.units("install", &[("i1", &[], None, None)]);
    let install = s.work("install", &[], &[("STUB_INSTALL_FAIL", "1")]);
    // Then the unit is released three times and left to other workers, and no implementer ran
    assert_eq!(install.stdout, "landed 0 [], failed 0 [], released 3 [i1 i1 i1]\n", "{}", install.stderr);
    assert_eq!(story(&install.stderr).iter().filter(|line| *line == "worker.unit.released i1 install-failed").count(), 3);
    assert!(s.refs("refs/leases").is_empty() && s.prompts().is_empty());

    // When units are already failed on the remote, held by a live lease, or held by a stale one
    s.units("held", &[("h1", &[], None, None), ("h2", &[], None, None), ("h3", &[], None, None)]);
    s.push_ref("old failure", "refs/failed/held/h1");
    let at = iso_from_ms(now_ms());
    s.push_ref(&json!({ "worker": "other:host:1", "attempt": 1, "model": "rca", "startedAt": at, "heartbeatAt": at }).to_string(), "refs/leases/held/h2");
    let old = iso_from_ms(now_ms() - 3_600_000);
    s.push_ref(&json!({ "worker": "dead:host:1", "attempt": 1, "model": "rca", "startedAt": old, "heartbeatAt": old }).to_string(), "refs/leases/held/h3");
    let held = s.work("held", &[], &[]);
    // Then the failed and the live ones are left alone, and the stale lease is expired, reclaimed and landed
    assert_eq!(held.stdout, "landed 1 [h3], failed 0 [], released 0 []\n", "{}", held.stderr);
    assert_eq!(story(&held.stderr), ["worker.unit.expired h3", "worker.unit.claimed h3", "worker.unit.gated h3", "worker.unit.landed h3"]);
    assert_eq!(s.refs("refs/leases"), ["refs/leases/held/h2"]);

    // When the worktree dir is inside the repo, or --max is not positive
    let inside = run_wf(&["offload", "plan", "held", "--once", "--worktree-dir", &s.repo.join("trees").display().to_string()], &RunOpts::default().cwd(&s.repo).bin_dir(&s.bin));
    let max = s.work("held", &["--max", "0"], &[]);
    // Then the worker refuses before claiming anything
    assert_eq!(inside.exit_code, 2);
    assert!(inside.stderr.contains("--worktree-dir must be outside the repo"), "{}", inside.stderr);
    assert_eq!(max.exit_code, 2);
    assert!(max.stderr.contains("--max must be a positive integer"), "{}", max.stderr);

    // When a long-running worker gets SIGTERM while a unit is implementing
    s.units("term", &[("t1", &[], None, None)]);
    let wt = s.wt.display().to_string();
    let mut opts = s.opts(&[("STUB_SLEEP", "30")]);
    opts.timeout = None;
    let mut child = spawn_wf(&["offload", "plan", "term", "--land-ref", LAND, "--worktree-dir", &wt], &opts);
    wait_until(Duration::from_secs(30), "the unit to be claimed", || child.out().contains("worker.unit.claimed"));
    std::thread::sleep(Duration::from_millis(400));
    child.signal("TERM");
    // Then it releases the lease and exits 143
    let code = child.wait(Duration::from_secs(30));
    let out = child.out();
    assert_eq!(code, Some(143), "{out}");
    assert_eq!(story(&out), ["worker.unit.claimed t1", "worker.stopping", "worker.unit.released t1 stopping"]);
    assert!(out.contains("landed 0 [], failed 0 [], released 0 []\n"), "{out}");
    assert_eq!(s.refs("refs/leases"), ["refs/leases/held/h2"]);
}
