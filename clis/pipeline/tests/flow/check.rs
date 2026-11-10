//! `wf check --staged` and `wf check --budget`: the pre-commit gate over what is staged, through Nx and the shared build cache, and the
//! simplicity budget; and what a test stage and the CI steps do with the cache and the checkout. Repositories are real throwaway git;
//! `pnpm`, `node` and `nx` are argv-recording stubs, the bucket is a local server.
use crate::support::{node_path, repo_root, run_wf, MockReply, MockServer, RunOpts, RunResult};
use regex::Regex;
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"]).args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn put(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn executable(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const PACKAGES: [(&str, &str, &[&str]); 4] = [("@lib/a", "libraries/a", &[]), ("@cli/tool", "clis/tool", &["@lib/a"]), ("@app/svc", "backend/svc", &["@cli/tool"]), ("@lib/b", "libraries/b", &[])];

/// Records every call with its cwd, answers `pnpm ls -r` with the workspace, and exits per a table of argv matches.
fn pnpm_stub(node: &str) -> String {
    format!(
        "#!{node}\nconst fs = require('fs');\nconst dir = process.env.STUB_DIR;\nconst args = process.argv.slice(2);\nfs.appendFileSync(dir + '/calls.log', JSON.stringify({{ tool: 'pnpm', args, cwd: process.cwd() }}) + '\\n');\nif (args[0] === 'ls') {{ process.stdout.write(fs.readFileSync(dir + '/ls.json', 'utf8')); process.exit(0); }}\nif (args[0] === '--version') {{ console.log(process.env.STUB_PNPM_VERSION || '11.5.2'); process.exit(0); }}\nconst exits = JSON.parse(fs.readFileSync(dir + '/exits.json', 'utf8'));\nconst line = args.join(' ');\nconst hit = exits.find((e) => line.includes(e.match));\nif (hit) {{ process.stderr.write(hit.stderr || ''); process.exit(hit.code); }}\n"
    )
}

/// The node the tool pin check asks for its version: records the call.
fn node_stub(node: &str) -> String {
    format!(
        "#!{node}\nconst fs = require('fs');\nfs.appendFileSync(process.env.STUB_DIR + '/calls.log', JSON.stringify({{ tool: 'node', args: process.argv.slice(2), cwd: process.cwd() }}) + '\\n');\nif (process.argv[2] === '--version') {{ console.log(process.env.STUB_NODE_VERSION || 'v24.13.1'); process.exit(0); }}\n"
    )
}

/// The workspace's `nx`: records the call (and the build cache it was given), answers a failure per `exits.json`, and otherwise
/// does what Nx does for a task when `STUB_NX_HASH` is set (asks the cache server, stores `blob` on a miss) and prints two task
/// headers and the closing line of a real Nx run with one of them replayed.
fn nx_stub(node: &str) -> String {
    format!(
        "#!{node}\nconst fs = require('fs');\nconst dir = process.env.STUB_DIR;\nconst args = process.argv.slice(2);\nconst env = process.env;\nfs.appendFileSync(dir + '/calls.log', JSON.stringify({{ tool: 'nx', args, cwd: process.cwd(), cache: env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER || null, noProxy: env.NO_PROXY || null, skip: env.NX_SKIP_NX_CACHE || null }}) + '\\n');\nconst exits = JSON.parse(fs.readFileSync(dir + '/exits.json', 'utf8'));\nconst line = args.join(' ');\nconst hit = exits.find((e) => line.includes(e.match));\nif (hit) {{ process.stderr.write(hit.stderr || ''); process.exit(hit.code); }}\n(async () => {{\n  if (env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER && env.STUB_NX_HASH) {{\n    const url = env.NX_SELF_HOSTED_REMOTE_CACHE_SERVER + '/v1/cache/' + env.STUB_NX_HASH + '-' + args[2].replace(/[^a-z]/g, '');\n    const headers = {{ Authorization: 'Bearer ' + env.NX_SELF_HOSTED_REMOTE_CACHE_ACCESS_TOKEN }};\n    if ((await fetch(url, {{ headers }})).status === 404) await fetch(url, {{ method: 'PUT', headers, body: 'blob' }});\n  }}\n  console.log('\\n> nx run a:lint  [local cache]\\n\\n> nx run a:typecheck\\n\\nNx read the output from the cache instead of running the command for 1 out of 2 tasks.');\n}})();\n"
    )
}

/// The `wf` the gate calls for `mimic verify`: records the call and exits per `exits.json`.
fn wf_stub(node: &str) -> String {
    format!(
        "#!{node}\nconst fs = require('fs');\nconst dir = process.env.STUB_DIR;\nconst args = process.argv.slice(2);\nfs.appendFileSync(dir + '/calls.log', JSON.stringify({{ tool: 'wf', args, cwd: process.cwd() }}) + '\\n');\nconst exits = JSON.parse(fs.readFileSync(dir + '/exits.json', 'utf8'));\nconst hit = exits.find((e) => args.join(' ').includes(e.match));\nif (hit) {{ process.stderr.write(hit.stderr || ''); process.exit(hit.code); }}\n"
    )
}

/// A throwaway git repository with a four-package workspace and the files the budget reads, and a directory of stubs.
struct Workspace {
    repo: tempfile::TempDir,
    stub: tempfile::TempDir,
}

impl Workspace {
    fn new() -> Self {
        let w = Self { repo: tempfile::Builder::new().prefix("wf-flow-staged-").tempdir().unwrap(), stub: tempfile::Builder::new().prefix("wf-flow-staged-stub-").tempdir().unwrap() };
        git(w.repo.path(), &["init", "-q"]);
        w.put("lefthook.yml", "pre-commit:\n  jobs:\n    - name: a\n      run: x\n");
        w.put("CLAUDE.md", "one\n");
        w.put("docs/known-gaps.md", "one\n");
        w.put("package.json", &json!({ "name": "root-ws", "scripts": {} }).to_string());
        w.put("mise.toml", "[tools]\nnode = \"24.13.1\"\npnpm = \"11.5.2\"\n");
        w.put(".agents/config/hooks.json", &json!({ "claude": { "hooks": {} } }).to_string());
        for (name, dir, deps) in PACKAGES {
            let deps: serde_json::Map<String, Value> = deps.iter().map(|d| (d.to_string(), json!("workspace:*"))).collect();
            w.put(&format!("{dir}/package.json"), &json!({ "name": name, "dependencies": deps }).to_string());
        }
        let mut ls = vec![json!({ "name": "root", "path": w.repo.path() })];
        ls.extend(PACKAGES.iter().map(|(name, dir, _)| json!({ "name": name, "path": w.repo.path().join(dir) })));
        std::fs::write(w.stub.path().join("ls.json"), Value::Array(ls).to_string()).unwrap();
        std::fs::create_dir(w.stub.path().join("bin")).unwrap();
        let node = node_path().expect("node on PATH").display().to_string();
        executable(&w.stub.path().join("bin/pnpm"), &pnpm_stub(&node));
        executable(&w.stub.path().join("bin/node"), &node_stub(&node));
        executable(&w.stub.path().join("bin/nx"), &nx_stub(&node));
        executable(&w.stub.path().join("bin/wf"), &wf_stub(&node));
        w
    }

    fn put(&self, rel: &str, content: &str) {
        put(self.repo.path(), rel, content);
    }

    /// `wf check --staged <args>` with pnpm exiting per `exits`; the calls of earlier steps are forgotten.
    fn staged(&self, args: &[&str], exits: Value, env: &[(&str, &str)]) -> RunResult {
        let _ = std::fs::remove_file(self.stub.path().join("calls.log"));
        std::fs::write(self.stub.path().join("exits.json"), exits.to_string()).unwrap();
        let mut full = vec!["check", "--staged"];
        full.extend_from_slice(args);
        let stub = self.stub.path().display().to_string();
        let node = self.stub.path().join("bin/node").display().to_string();
        let nx = self.stub.path().join("bin/nx").display().to_string();
        let mut opts = RunOpts::default().bin_dir(&self.stub.path().join("bin")).cwd(self.repo.path()).clean_env().env("STUB_DIR", &stub).env("WF_NODE", &node).env("WF_NX_BIN", &nx);
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    /// `wf test run --stage typecheck-lint-unit-tests` over one package, with `extra` after the stage and `env` added.
    fn stage(&self, extra: &[&str], env: &[(&str, &str)]) -> RunResult {
        self.stage_over(&["@lib/a"], extra, env)
    }

    /// The same stage over a plan that typechecks each of `packages`.
    fn stage_over(&self, packages: &[&str], extra: &[&str], env: &[(&str, &str)]) -> RunResult {
        let _ = std::fs::remove_file(self.stub.path().join("calls.log"));
        std::fs::write(self.stub.path().join("exits.json"), "[]").unwrap();
        let plan = self.stub.path().join("plan.json");
        let tests: Vec<Value> = packages.iter().map(|p| json!({ "kind": "typecheck", "package": p })).collect();
        std::fs::write(&plan, json!({ "stages": { "typecheck-lint-unit-tests": tests } }).to_string()).unwrap();
        let mut full = vec!["test", "run", "--stage", "typecheck-lint-unit-tests", "--plan", plan.to_str().unwrap()];
        full.extend_from_slice(extra);
        let nx = self.stub.path().join("bin/nx").display().to_string();
        let stub = self.stub.path().display().to_string();
        let mut opts = RunOpts::default().bin_dir(&self.stub.path().join("bin")).cwd(self.repo.path()).clean_env().env("STUB_DIR", &stub).env("WF_NX_BIN", &nx).env("WF_TEST_REPO", &self.repo.path().display().to_string());
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    /// What each `nx` call of the last run saw of the build cache: its server, `NO_PROXY` and `NX_SKIP_NX_CACHE`.
    fn nx_calls(&self) -> Vec<Value> {
        let log = std::fs::read_to_string(self.stub.path().join("calls.log")).unwrap_or_default();
        log.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()).filter(|c| c["tool"] == "nx").collect()
    }

    /// Each call as `<tool> <argv> @<cwd relative to the repo>`.
    fn calls(&self) -> Vec<String> {
        let repo = self.repo.path().canonicalize().unwrap();
        std::fs::read_to_string(self.stub.path().join("calls.log"))
            .map(|log| {
                log.trim()
                    .lines()
                    .map(|l| serde_json::from_str::<Value>(l).unwrap())
                    // the pin check asks node and pnpm for their versions; that is not a step of the gate
                    .filter(|c| c["args"] != json!(["--version"]))
                    .map(|c| {
                        let args: Vec<&str> = c["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
                        let rel = PathBuf::from(c["cwd"].as_str().unwrap()).strip_prefix(&repo).map(|p| p.display().to_string()).unwrap_or_default();
                        format!("{} {} @{}", c["tool"].as_str().unwrap(), args.join(" "), if rel.is_empty() { ".".to_string() } else { rel })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

const AFFECTED: [&str; 3] = ["@lib/a", "@cli/tool", "@app/svc"];
const LS: &str = "pnpm ls -r --depth -1 --json @.";
const NX_FLAGS: &str = "--parallel=4 --outputStyle=stream --nxBail";

fn nx(targets: &str, pkgs: &[&str]) -> String {
    format!("nx run-many -t {targets} -p {} {NX_FLAGS} @.", pkgs.join(","))
}

fn lint(pkgs: &[&str]) -> String {
    nx("lint", pkgs)
}

fn gate(pkgs: &[&str]) -> String {
    nx("typecheck,test:unit,test:flow", pkgs)
}

#[test]
fn the_staged_gate_runs_through_nx() {
    let w = Workspace::new();

    // Given nothing staged, Then only the budget is measured
    let r = w.staged(&[], json!([]), &[]);
    assert_eq!(r.exit_code, 0);
    assert!(w.calls().is_empty() && r.stdout.contains("msg=\"known-gaps-lines\""), "{}", r.stdout);

    // Given a changed file in a leaf package, Then Nx lints that package, then typechecks and tests every dependent in one call
    w.put("libraries/a/x.ts", "export const x = 1;\n");
    let r = w.staged(&["--files", "libraries/a/x.ts"], json!([]), &[]);
    assert_eq!(r.exit_code, 0);
    assert_eq!(w.calls(), [LS.to_string(), lint(&["@lib/a"]), gate(&AFFECTED)]);
    assert!(r.stdout.contains("msg=\"pre-commit-jobs\""));
    // And what Nx replayed is said, so a recommit shows what the cache served
    assert!(r.stdout.contains("verify: nx lint over 1 projects: 1 of 2 tasks replayed from the cache\n"), "{}", r.stdout);
    assert!(r.stdout.contains("verify: nx typecheck,test:unit,test:flow over 3 projects: 1 of 2 tasks replayed from the cache\n"), "{}", r.stdout);

    // Given files in the git index, Then added and modified code is gated, and deletions and non-code are not
    w.put("libraries/b/gone.ts", "old\n");
    w.put("notes.md", "# n\n");
    git(w.repo.path(), &["add", "-A"]);
    git(w.repo.path(), &["commit", "-qm", "base"]);
    w.put("libraries/a/x.ts", "export const x = 2;\n");
    std::fs::remove_file(w.repo.path().join("libraries/b/gone.ts")).unwrap();
    w.put("notes.md", "# changed\n");
    git(w.repo.path(), &["add", "-A"]);
    assert_eq!(w.staged(&[], json!([]), &[]).exit_code, 0);
    assert_eq!(w.calls(), [LS.to_string(), lint(&["@lib/a"]), gate(&AFFECTED)]);
    git(w.repo.path(), &["commit", "-qm", "staged"]);

    // Given a package nothing depends on, Then only it is typechecked and tested
    w.put("libraries/b/y.ts", "export {};\n");
    assert_eq!(w.staged(&["--files", "libraries/b/y.ts"], json!([]), &[]).exit_code, 0);
    assert_eq!(w.calls(), [LS.to_string(), lint(&["@lib/b"]), gate(&["@lib/b"])]);

    // Given a code file no package owns, Then it says nothing is affected
    w.put("tools.json", "{}");
    let r = w.staged(&["--files", "tools.json"], json!([]), &[]);
    assert_eq!(r.exit_code, 0);
    assert!(r.stdout.contains("verify: no workspace package affected\n"));
    assert_eq!(w.calls(), [LS]);

    // Given a lint failure, Then the gate still runs and Nx's exit code is returned
    let r = w.staged(&["--files", "libraries/b/y.ts"], json!([{ "match": "-t lint", "code": 3 }]), &[]);
    assert_eq!((r.exit_code, w.calls().len()), (3, 3));

    // Given a failing typecheck, Then its output is shown and its code is returned
    let r = w.staged(&["--files", "libraries/b/y.ts"], json!([{ "match": "typecheck", "code": 2, "stderr": "TS2322 broke\n" }]), &[]);
    assert_eq!(r.exit_code, 2);
    assert!(r.stderr.contains("TS2322 broke"), "{}", r.stderr);

    // Given a change to the editor bundle's inputs, Then its freshness check runs before the gate, and a stale bundle stops it
    w.put("frontend/libraries/logging/src/z.ts", "export {};\n");
    let bundle = nx("check:editor-bundle", &["mla-tamagui"]);
    let lint_root = lint(&["root-ws"]);
    assert_eq!(w.staged(&["--files", "frontend/libraries/logging/src/z.ts"], json!([]), &[]).exit_code, 0);
    assert_eq!(w.calls(), [LS.to_string(), lint_root.clone(), bundle.clone()]);
    let stale = json!([{ "match": "check:editor-bundle", "code": 4 }]);
    assert_eq!(w.staged(&["--files", "frontend/libraries/logging/src/z.ts"], stale, &[]).exit_code, 4);
    assert_eq!(w.calls(), [LS.to_string(), lint_root, bundle]);

    // Given a gated budget over its limit, Then lint ran, the gate did not, and the failure names the budget
    w.put("CLAUDE.md", &"x".repeat(10_000));
    let r = w.staged(&["--files", "libraries/b/y.ts"], json!([]), &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("simplicity budget exceeded: root-claude-md-bytes"), "{}", r.stderr);
    assert_eq!(w.calls(), [LS.to_string(), lint(&["@lib/b"])]);
    w.put("CLAUDE.md", "one\n");
}

/// The bucket as a loopback fake: a read of an absent object answers 404 and a write of one of `names` (objects of generation 0
/// under `nx/`) 200, every request recorded. The environment that gives `wf` its two links, with a `NO_PROXY` to be extended.
fn fake_bucket(names: &[&str]) -> (MockServer, Vec<(&'static str, String)>) {
    let bucket = MockServer::start(None);
    let writes: Vec<(String, MockReply)> = names.iter().map(|name| (format!("PUT /write/o/nx/g0/{name}"), MockReply::text(200, ""))).collect();
    bucket.routes(&writes.iter().map(|(route, reply)| (route.as_str(), reply.clone())).collect::<Vec<_>>());
    let base = format!("http://127.0.0.1:{}", bucket.port);
    let env = vec![("BUILD_CACHE_READ_PAR_URL", format!("{base}/read/o/")), ("BUILD_CACHE_WRITE_PAR_URL", format!("{base}/write/o/")), ("NO_PROXY", "example.invalid".to_string()), ("STUB_NX_HASH", "h".to_string())];
    (bucket, env)
}

fn borrowed<'a>(env: &'a [(&'static str, String)]) -> Vec<(&'a str, &'a str)> {
    env.iter().map(|(key, value)| (*key, value.as_str())).collect()
}

fn bucket_requests(bucket: &MockServer, method: &str) -> usize {
    bucket.requests().iter().filter(|r| r.method == method).count()
}

/// The one `wf: build cache {...}` line of a run, as JSON.
fn cache_stats(stderr: &str) -> Value {
    let lines: Vec<&str> = stderr.lines().filter_map(|l| l.strip_prefix("wf: build cache {")).collect();
    assert_eq!(lines.len(), 1, "one stats line expected: {stderr}");
    serde_json::from_str(&format!("{{{}", lines[0])).unwrap()
}

#[test]
fn the_staged_gate_gives_nx_the_build_cache_and_writes_to_it_only_from_a_clean_commit_on_dev() {
    let w = Workspace::new();
    let git = |args: &[&str]| git(w.repo.path(), args);
    let refusal = |r: &RunResult| r.stderr.lines().find(|l| l.starts_with("wf: build cache reads only")).map(str::to_string);
    // Given a clean checkout whose HEAD is origin/dev, the pinned tools, and the bucket's two links
    w.put("libraries/a/x.ts", "export const x = 1;\n");
    git(&["add", "-A"]);
    git(&["commit", "-qm", "base"]);
    git(&["update-ref", "refs/remotes/origin/dev", "HEAD"]);
    let (bucket, links) = fake_bucket(&["h-lint", "h-typechecktestunittestflow"]);
    let gate = |extra: &[(&'static str, &'static str)]| {
        bucket.clear();
        let mut env = borrowed(&links);
        env.extend(extra.iter().copied());
        w.staged(&["--files", "libraries/a/x.ts"], json!([]), &env)
    };

    // When the gate runs, Then each Nx call is given the loopback server with the loopback kept out of the proxy, and both
    // entries are written to the bucket
    let r = gate(&[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let seen = w.nx_calls();
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|c| c["cache"].as_str().is_some_and(|u| u.starts_with("http://127.0.0.1:")) && c["noProxy"] == "example.invalid,127.0.0.1,localhost"), "{seen:?}");
    assert_eq!((bucket_requests(&bucket, "PUT"), refusal(&r)), (2, None));
    // And the run reports what it spent on one stderr line: the generation, a read and a write per call
    let mut stats = cache_stats(&r.stderr);
    let latency = stats.as_object_mut().unwrap().remove("latencyMs").unwrap();
    assert_eq!(stats.as_object_mut().unwrap().remove("seeded").unwrap()["checked"], json!(2), "the two writes are looked up again");
    assert_eq!(stats, json!({ "hits": 0, "misses": 2, "puts": 2, "skipped": 0, "quotaExceeded": 0, "droppedPuts": 0, "lastFailure": null, "requests": 5, "bytes": 8, "degraded": null }));
    // And how long the store took to answer, per kind of call: two reads and two writes went out
    assert_eq!((latency["get"]["n"].clone(), latency["put"]["n"].clone(), latency["exists"]["n"].clone()), (json!(2), json!(2), json!(0)), "{latency}");

    // When a file is only staged, Then it is what the commit will hold and the run still writes
    w.put("libraries/a/y.ts", "export const y = 1;\n");
    git(&["add", "libraries/a/y.ts"]);
    assert_eq!(bucket_requests(&bucket, "PUT"), 2);
    let r = gate(&[]);
    assert_eq!((r.exit_code, bucket_requests(&bucket, "PUT"), refusal(&r)), (0, 2, None));

    // When a file is untracked, Then the run reads only: the reason is on stderr and no write reaches the bucket, while the
    // reads did arrive and Nx was still given the server
    w.put("scratch.txt", "x\n");
    let r = gate(&[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(refusal(&r).is_some_and(|l| l.contains("dirty: the work tree has changes: scratch.txt")), "{}", r.stderr);
    assert_eq!((bucket_requests(&bucket, "PUT"), bucket_requests(&bucket, "GET")), (0, 3));
    assert_eq!((cache_stats(&r.stderr)["puts"].clone(), w.nx_calls().iter().filter(|c| c["cache"].is_string()).count()), (json!(0), 2));

    // When the commit is not in origin/dev, Then it reads only
    std::fs::remove_file(w.repo.path().join("scratch.txt")).unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-qm", "local"]);
    let r = gate(&[]);
    assert!(refusal(&r).is_some_and(|l| l.contains("not-on-dev: HEAD is not known to be an ancestor of origin/dev")), "{}", r.stderr);
    assert_eq!((bucket_requests(&bucket, "PUT"), bucket_requests(&bucket, "GET")), (0, 3));

    // When the commit is on dev again but node is not the pinned one, Then it reads only
    git(&["update-ref", "refs/remotes/origin/dev", "HEAD"]);
    let r = gate(&[("STUB_NODE_VERSION", "v22.22.0")]);
    assert!(refusal(&r).is_some_and(|l| l.contains("tools-unpinned: node 22.22.0 is not the pinned 24.13.1")), "{}", r.stderr);
    assert_eq!(bucket_requests(&bucket, "PUT"), 0);
    // And with the pinned tools it writes again
    assert_eq!((gate(&[]).exit_code, bucket_requests(&bucket, "PUT")), (0, 2));
}

#[test]
fn a_test_stage_starts_the_build_cache_shim_unless_it_is_fresh_or_a_link_is_missing() {
    let w = Workspace::new();
    w.put("libraries/a/x.ts", "export const x = 1;\n");
    git(w.repo.path(), &["add", "-A"]);
    git(w.repo.path(), &["commit", "-qm", "base"]);
    git(w.repo.path(), &["update-ref", "refs/remotes/origin/dev", "HEAD"]);
    let (bucket, links) = fake_bucket(&["h-typecheck"]);
    let ran = |extra: &[&str], env: &[(&str, &str)]| {
        bucket.clear();
        let r = w.stage(extra, env);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        r
    };

    // Given a clean commit on dev and both links, When the stage runs, Then Nx is given the shim and its result is written
    let r = ran(&[], &borrowed(&links));
    assert_eq!(w.nx_calls().iter().map(|c| c["cache"].is_string()).collect::<Vec<_>>(), [true]);
    assert_eq!((bucket_requests(&bucket, "PUT"), cache_stats(&r.stderr)["puts"].clone()), (1, json!(1)));

    // When it runs fresh, Then Nx is told to skip its cache, is given no server, and nothing reaches the bucket
    let r = ran(&["--fresh"], &borrowed(&links));
    assert_eq!(w.nx_calls().iter().map(|c| (c["cache"].is_string(), c["skip"].clone())).collect::<Vec<_>>(), [(false, json!("true"))]);
    assert_eq!((bucket.requests().len(), r.stderr.contains("wf: build cache")), (0, false));

    // When a link does not resolve, Then the run is the cold one it was before, now saying so: no server and nothing reaches the bucket
    let one: Vec<(&str, &str)> = borrowed(&links).into_iter().filter(|(k, _)| *k != "BUILD_CACHE_WRITE_PAR_URL").collect();
    let r = ran(&[], &one);
    assert_eq!(w.nx_calls().iter().map(|c| c["cache"].is_string()).collect::<Vec<_>>(), [false]);
    assert_eq!((bucket.requests().len(), r.stderr.contains("wf: build cache disabled for this run, no link"), r.stderr.contains("wf: build cache {")), (0, true, false), "{}", r.stderr);
}

#[test]
fn a_test_stage_writes_to_the_build_cache_beside_ignored_files_and_reads_only_beside_an_unignored_one() {
    let w = Workspace::new();
    w.put(".gitignore", "/e2e-tests.jsonl\n.e2e-results/\n");
    w.put("libraries/a/x.ts", "export const x = 1;\n");
    git(w.repo.path(), &["add", "-A"]);
    git(w.repo.path(), &["commit", "-qm", "base"]);
    git(w.repo.path(), &["update-ref", "refs/remotes/origin/dev", "HEAD"]);
    let (bucket, links) = fake_bucket(&["h-typecheck"]);
    let ran = || {
        bucket.clear();
        let r = w.stage(&[], &borrowed(&links));
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        r
    };

    // Given what an e2e job leaves in its checkout, both ignored: the record it tees and the result Nx restores on a hit
    w.put("e2e-tests.jsonl", "{}\n");
    w.put(".e2e-results/android/t.json", "{}\n");
    // When the stage runs, Then the tree is clean for the guard and the result is written
    let r = ran();
    assert_eq!((r.stderr.contains("wf: build cache reads only"), bucket_requests(&bucket, "PUT"), cache_stats(&r.stderr)["puts"].clone()), (false, 1, json!(1)), "{}", r.stderr);

    // When a generated file is not ignored, Then the run reads only and the refusal names it
    w.put("e2e-tests.log", "{}\n");
    let r = ran();
    assert!(r.stderr.contains("dirty: the work tree has changes: e2e-tests.log"), "{}", r.stderr);
    assert_eq!((bucket_requests(&bucket, "PUT"), cache_stats(&r.stderr)["puts"].clone()), (0, json!(0)));
}

/// The script of the `Summary` step of the e2e workflow, as the runner hands it to bash.
fn e2e_summary_script() -> String {
    let yml = std::fs::read_to_string(repo_root().join(".github/workflows/e2e-tests.yml")).unwrap();
    let (_, step) = yml.split_once("      - name: Summary\n").expect("a Summary step");
    let (_, run) = step.split_once("        run: |\n").expect("its run block");
    run.lines().take_while(|l| l.starts_with("          ")).map(|l| &l[10..]).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_e2e_summary_step_lists_a_replayed_pass_that_lacks_a_fresh_units_fields() {
    // Given a stage summary whose first unit is a pass replayed from a seeded record (a verdict, no args, duration or note)
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("e2e-runs")).unwrap();
    let units = json!([
        { "test": "editor-typing", "verdict": "pass", "cached": true },
        { "test": "voice-edit", "args": ["chunk=1"], "verdict": "pass", "durationS": 41.5, "bundle": "https://b.example/x", "note": "", "cached": false },
        { "test": "tamil-writing", "args": ["part=write"], "verdict": "fail", "durationS": 3.25, "note": "no ink", "next": "look", "cached": false }
    ]);
    std::fs::write(dir.path().join("e2e-runs/summary.json"), json!({ "ok": false, "stage": "android-e2e-tests", "units": units }).to_string()).unwrap();
    let page = dir.path().join("page.md");

    // When the step's script runs with the job's env
    let out = Command::new("bash")
        .args(["-e", "-c", &e2e_summary_script()])
        .env("RUNNER_TEMP", dir.path())
        .env("GITHUB_STEP_SUMMARY", &page)
        .env("PLATFORM", "android")
        .env("SHA", "ef90df980456dfb6cf479933f56393be15458446")
        .output()
        .unwrap();

    // Then it succeeds and the page has one line per unit, the replayed one marked, with no `null` in any
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&page).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "### android e2e tests for ef90df980",
            "- editor-typing: pass (replayed) in 0 s",
            "- voice-edit chunk=1: pass in 41.5 s · [bundle](https://b.example/x)",
            "- tamil-writing part=write: fail in 3.25 s · no ink · next: look",
        ]
    );
}

#[test]
fn no_ci_step_writes_a_file_into_the_checkout_that_git_does_not_ignore() {
    // Given every workflow and action, whose steps run in the runner's kept checkout (a dirty tree keeps the build cache read-only)
    let root = repo_root();
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join(".github/workflows")).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "yml")).collect();
    files.extend(std::fs::read_dir(root.join(".github/actions")).unwrap().map(|e| e.unwrap().path().join("action.yml")).filter(|p| p.is_file()));
    assert!(files.len() > 3, "{files:?}");
    // When each `| tee <path>` and `> <path>` of a `run:` script names a relative path
    let write = Regex::new(r#"(?:\|\s*tee(?:\s+-a)?|>>?)\s+"?([A-Za-z._][^\s"|;&)]*)"#).unwrap();
    let run_key = Regex::new(r"^(\s*)(- )?run:(.*)$").unwrap();
    let mut offenders = Vec::new();
    for file in files {
        let mut block: Option<usize> = None;
        for (n, line) in std::fs::read_to_string(&file).unwrap().lines().enumerate() {
            let indent = line.len() - line.trim_start().len();
            if block.is_some_and(|key| !line.trim().is_empty() && indent <= key) {
                block = None;
            }
            let script = match (block, run_key.captures(line)) {
                (Some(_), _) => line,
                (None, Some(c)) if matches!(c[3].trim(), "|" | "|-" | ">" | ">-") => {
                    block = Some(c[1].len() + c.get(2).map_or(0, |dash| dash.len()));
                    continue;
                }
                (None, Some(c)) => c.get(3).unwrap().as_str(),
                (None, None) => continue,
            };
            if script.trim_start().starts_with('#') {
                continue;
            }
            for target in write.captures_iter(script).map(|c| c[1].to_string()) {
                let ignored = Command::new("git").args(["check-ignore", "-q", &target]).current_dir(&root).status().unwrap().success();
                if !ignored {
                    offenders.push(format!("{}:{}: {target}", file.strip_prefix(&root).unwrap().display(), n + 1));
                }
            }
        }
    }
    // Then git ignores it: such a file would otherwise make every later cache write of the job's stages refuse
    assert!(offenders.is_empty(), "write it under $RUNNER_TEMP or ignore it in .gitignore: {offenders:?}");
}

fn lefthook(jobs: usize) -> String {
    let rows: String = (0..jobs).map(|i| format!("    - name: job{i}\n      run: echo {i}\n")).collect();
    format!("pre-commit:\n  parallel: true\n  jobs:\n{rows}post-land:\n  jobs:\n    - name: x\n      run: echo x\n")
}

fn lines(n: usize) -> String {
    format!("{}\n", (0..n - 1).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n"))
}

fn hooks(commands: &[&[&str]]) -> String {
    let events: serde_json::Map<String, Value> = commands
        .iter()
        .enumerate()
        .map(|(i, cs)| (format!("Event{i}"), json!([{ "matcher": "*", "hooks": cs.iter().map(|c| json!({ "type": "command", "command": c })).collect::<Vec<_>>() }])))
        .collect();
    json!({ "claude": { "hooks": events } }).to_string()
}

fn measured(level: &str, name: &str, value: usize, limit: usize, gated: bool) -> Regex {
    Regex::new(&format!(r#"^ts=\d+ level={level} source=verify\.budget msg="{name}" value={value} limit={limit} gated={gated}$"#)).unwrap()
}

#[test]
fn the_simplicity_budget_measures_the_repo_and_fails_only_on_gated_limits_unless_strict() {
    let dir = tempfile::Builder::new().prefix("wf-flow-budget-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let reset = || {
        put(root, "lefthook.yml", &lefthook(2));
        put(root, "CLAUDE.md", &"x".repeat(7_300));
        put(root, "docs/known-gaps.md", &lines(53));
        put(root, "package.json", &json!({ "scripts": { "a": "1", "b": "2" } }).to_string());
        put(root, ".agents/config/hooks.json", &hooks(&[&["one"]]));
    };
    let budget = |args: &[&str]| {
        let mut full = vec!["check", "--budget"];
        full.extend_from_slice(args);
        run_wf(&full, &RunOpts::default().cwd(root).clean_env())
    };
    let on = |r: &RunResult, re: &str| Regex::new(re).unwrap().is_match(&format!("{}{}", r.stdout, r.stderr));

    // Given a repo within every budget, Then seventeen info lines print on stdout and it exits 0
    reset();
    let r = budget(&[]);
    assert_eq!((r.exit_code, r.stderr.as_str()), (0, ""));
    let out: Vec<&str> = r.stdout.trim().split('\n').collect();
    assert_eq!(out.len(), 18);
    let expected = [("pre-commit-jobs", 2, 5, true), ("root-claude-md-bytes", 7_300, 9_216, true), ("known-gaps-lines", 53, 200, true), ("concepts-dangling-links", 0, 0, true), ("backend-core-io-violations", 0, 0, true), ("cargo-dep-violations", 0, 0, true), ("blast-radius-violations", 0, 0, true), ("workspace-crossing-violations", 0, 0, true), ("ci-scope-violations", 0, 0, true), ("unpinned-third-party-actions", 0, 0, true), ("env-file-in-artifacts", 0, 0, true), ("secrets-inherit", 0, 0, true), ("handrolled-recorder-violations", 0, 0, true), ("cargo-task-npm-inputs", 0, 0, true), ("versioned-library-path-deps", 0, 0, true), ("wf-kit-lines", 0, 1200, true), ("root-scripts", 2, 15, false), ("agent-hooks", 1, 4, false)];
    for (line, (name, value, limit, gated)) in out.iter().zip(expected) {
        assert!(measured("info", name, value, limit, gated).is_match(line), "{line}");
    }

    // Given CLAUDE.md over its limit, Then a warn line and the failure name the budget; exactly the limit in bytes still fits
    put(root, "CLAUDE.md", &"x".repeat(9_217));
    let r = budget(&[]);
    assert_eq!(r.exit_code, 1);
    assert!(on(&r, r#"level=warn source=verify\.budget msg="root-claude-md-bytes over budget" value=9217 limit=9216 gated=true"#), "{}", r.stderr);
    assert!(r.stderr.contains("wf: simplicity budget exceeded: root-claude-md-bytes 9217>9216"));
    assert_eq!(r.stdout.trim().split('\n').count(), 17);
    put(root, "CLAUDE.md", &"x".repeat(9_216));
    let r = budget(&[]);
    assert!(r.exit_code == 0 && on(&r, r#"msg="root-claude-md-bytes" value=9216 "#), "{}", r.stdout);
    put(root, "CLAUDE.md", &"x".repeat(9_217));
    assert_eq!(budget(&[]).exit_code, 1);

    // Given too many pre-commit jobs and a long known-gaps file, Then both are listed
    reset();
    put(root, "lefthook.yml", &lefthook(6));
    put(root, "docs/known-gaps.md", &lines(201));
    let r = budget(&[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("wf: simplicity budget exceeded: pre-commit-jobs 6>5, known-gaps-lines 201>200"), "{}", r.stderr);

    // Given the kit over its line cap, Then the failure names it
    reset();
    put(root, "clis/wf-kit/src/lib.rs", &"x\n".repeat(1201));
    let r = budget(&[]);
    assert!(r.exit_code == 1 && r.stderr.contains("wf: simplicity budget exceeded: wf-kit-lines 1201>1200"), "{}", r.stderr);
    std::fs::remove_dir_all(root.join("clis")).unwrap();

    // Given an ungated budget over its limit, Then it only warns until --strict
    reset();
    let scripts: serde_json::Map<String, Value> = (0..16).map(|i| (format!("s{i}"), json!("x"))).collect();
    put(root, "package.json", &json!({ "scripts": scripts }).to_string());
    let (plain, strict) = (budget(&[]), budget(&["--strict"]));
    assert_eq!((plain.exit_code, strict.exit_code), (0, 1));
    assert!(on(&plain, r#"level=warn source=verify\.budget msg="root-scripts over budget" value=16 limit=15 gated=false"#), "{}", plain.stderr);
    assert!(strict.stderr.contains("wf: simplicity budget exceeded: root-scripts 16>15"));

    // Given hook commands repeated across events, Then distinct commands are counted
    reset();
    put(root, ".agents/config/hooks.json", &hooks(&[&["a", "b"], &["b", "c"], &["a"]]));
    assert!(on(&budget(&[]), r#"msg="agent-hooks" value=3 limit=4 gated=false"#));
    put(root, ".agents/config/hooks.json", &hooks(&[&["a", "b", "c", "d", "e"]]));
    assert!(on(&budget(&[]), r#"msg="agent-hooks over budget" value=5 limit=4 gated=false"#));

    // Given CONCEPTS.md links, Then only relative targets that do not exist are dangling
    reset();
    put(root, "docs/there.md", "x");
    put(
        root,
        "CONCEPTS.md",
        &[
            "[ok](docs/there.md) [anchor](#top) [web](https://example.com/x) [mail](mailto:a@b.c)",
            "[up](docs/../docs/there.md) [missing](docs/gone.md) [also](nothing/here.md#frag)",
            "[spaced](docs/there.md and words) [dir](docs) [empty]() [abs](/definitely/not/here)",
            "[gone-up](docs/missing/../there.md) [nested]( docs/there.md)",
        ]
        .join("\n"),
    );
    let r = budget(&[]);
    assert!(r.exit_code == 1 && on(&r, r#"msg="concepts-dangling-links over budget" value=3 limit=0 gated=true"#), "{}", r.stderr);

    // Given a backend core that depends on reqwest, in a workspace whose tokio enables fs, Then both are counted and the budget fails
    put(root, "backend/Cargo.toml", "[workspace]\nmembers = [\"svc/core\"]\n[workspace.dependencies]\ntokio = { version = \"1\", features = [\"macros\", \"fs\"] }\n");
    put(root, "backend/svc/core/Cargo.toml", "[dependencies]\nreqwest = { workspace = true }\n");
    let r = budget(&[]);
    assert!(r.exit_code == 1 && on(&r, r#"msg="backend-core-io-violations over budget" value=2 limit=0 gated=true"#), "{}", r.stderr);
    // And a service with a workspace of its own is checked too: its core asking tokio for net is a third finding
    put(root, "backend/svc2/Cargo.toml", "[workspace]\nmembers = [\"core\"]\n[workspace.dependencies]\ntokio = { version = \"1\", features = [\"macros\"] }\n");
    put(root, "backend/svc2/core/Cargo.toml", "[dependencies]\ntokio = { workspace = true, features = [\"net\"] }\n");
    let r = budget(&[]);
    assert!(r.exit_code == 1 && on(&r, r#"msg="backend-core-io-violations over budget" value=3 limit=0 gated=true"#), "{}", r.stderr);
    std::fs::remove_dir_all(root.join("backend")).unwrap();

    // Given two rules that match does-io, one naming the wire-contract crates by glob and one a module inside an adapter crate, a rule that does not match
    // does-io and names another crate, and crates: one under the first glob that depends on axum and asks tokio for process, a clean one, the adapter
    // crate that depends on reqwest, the crate of the other rule that depends on hyper, and one under no rule that depends on reqwest
    let rule = |id: &str, glob: &str, matches: &str| format!("id: {id}\nfiles:\n  - \"{glob}\"\nignores:\n  - \"**/src/testing.rs\"\nrule:\n  matches: {matches}\n");
    put(root, "backend/arch-rules/contract-crate-does-no-io.yml", &rule("contract-crate-does-no-io", "**/libraries/agent-api/*/src/**/*.rs", "does-io"));
    put(root, "backend/arch-rules/pure-module-does-no-io.yml", &rule("pure-module-does-no-io", "**/clis/adapter/src/domain.rs", "does-io"));
    put(root, "backend/arch-rules/other-layer.yml", &rule("other-layer", "**/clis/layered/src/**/*.rs", "something-else"));
    // and a rule that names a crate of a service as ast-grep reads it, relative to backend/, with that crate and a sibling that no rule names
    put(root, "backend/arch-rules/service-core-crate-does-no-io.yml", &rule("service-core-crate-does-no-io", "**/svc/driver/src/**/*.rs", "does-io"));
    put(root, "backend/svc/driver/Cargo.toml", "[dependencies]\nreqwest = \"0.12\"\n");
    put(root, "backend/svc/unruled/Cargo.toml", "[dependencies]\nreqwest = \"0.12\"\n");
    put(root, "libraries/agent-api/wire/Cargo.toml", "[dependencies]\naxum = \"0.8\"\ntokio = { version = \"1\", features = [\"process\"] }\n");
    put(root, "libraries/agent-api/clean/Cargo.toml", "[dependencies]\nserde = \"1\"\n");
    put(root, "clis/adapter/Cargo.toml", "[dependencies]\nreqwest = \"0.12\"\n");
    put(root, "clis/layered/Cargo.toml", "[dependencies]\nhyper = \"1\"\n");
    put(root, "libraries/elsewhere/Cargo.toml", "[dependencies]\nreqwest = \"0.12\"\n");
    // Then the findings of every crate a does-io rule names are counted (two in the wire crate, one in the adapter crate, one in the service's crate) and the
    // budget fails; the clean crate, the crate of the other rule and the ones under no rule are not
    let r = budget(&[]);
    assert!(r.exit_code == 1 && on(&r, r#"msg="backend-core-io-violations over budget" value=4 limit=0 gated=true"#), "{}", r.stderr);
    // And without the rules the same crates are nobody's to check
    std::fs::remove_dir_all(root.join("backend")).unwrap();
    assert!(on(&budget(&[]), r#"msg="backend-core-io-violations" value=0 "#));
    std::fs::remove_dir_all(root.join("libraries")).unwrap();
    std::fs::remove_dir_all(root.join("clis")).unwrap();

    // Given a service workspace whose `core` member is also named by a purity rule, and the core depends on reqwest
    put(root, "backend/svc/Cargo.toml", "[workspace]\nmembers = [\"core\", \"rs\"]\n[workspace.dependencies]\ntokio = { version = \"1\" }\n");
    put(root, "backend/svc/core/Cargo.toml", "[dependencies]\nreqwest = \"0.12\"\n");
    put(root, "backend/arch-rules/svc-core-crate-does-no-io.yml", &rule("svc-core-crate-does-no-io", "**/svc/core/src/**/*.rs", "does-io"));
    // Then the one finding is counted once, not once for the member and once for the rule
    let r = budget(&[]);
    assert!(r.exit_code == 1 && on(&r, r#"msg="backend-core-io-violations over budget" value=1 limit=0 gated=true"#), "{}", r.stderr);
    std::fs::remove_dir_all(root.join("backend")).unwrap();

    // Given a pure crate (named by a does-io rule) on an adapter library, and the same library used by a crate no rule names
    put(root, "backend/arch-rules/svc-core-crate-does-no-io.yml", &rule("svc-core-crate-does-no-io", "**/svc/core/src/**/*.rs", "does-io"));
    put(root, "backend/svc/core/Cargo.toml", "[package]\nname = \"svc-core\"\n[dependencies]\nservice-kit = { path = \"../../service-kit\" }\n");
    put(root, "backend/svc/rs/Cargo.toml", "[package]\nname = \"svc\"\n[dependencies]\nservice-kit = { path = \"../../service-kit\" }\n");
    put(root, "backend/service-kit/Cargo.toml", "[package]\nname = \"service-kit\"\n");
    // Then only the pure crate is a finding, and it is named
    let r = budget(&[]);
    assert!(r.exit_code == 1 && on(&r, r#"msg="cargo-dep-violations over budget" value=1 limit=0 gated=true"#), "{}", r.stderr);
    assert!(r.stderr.contains("the pure crate svc-core depends on the adapter library service-kit"), "{}", r.stderr);
    std::fs::remove_dir_all(root.join("backend")).unwrap();

    // Given the exec testkit as a dev-dependency of rca-exec and rca-service, and an adapter library with only a contract crate below it
    put(root, "clis/exec-testkit/Cargo.toml", "[package]\nname = \"exec-testkit\"\n");
    put(root, "clis/rca-exec/Cargo.toml", "[package]\nname = \"rca-exec\"\n[dev-dependencies]\nexec-testkit = { path = \"../exec-testkit\" }\n");
    put(root, "backend/remote-chat-agent/rs/Cargo.toml", "[package]\nname = \"rca-service\"\n[dev-dependencies]\nexec-testkit = { path = \"../../../clis/exec-testkit\" }\n");
    put(root, "libraries/agent-api/agent-api-proto/Cargo.toml", "[package]\nname = \"agent-api-proto\"\n");
    put(root, "libraries/agent-api/agent-api-client/Cargo.toml", "[package]\nname = \"agent-api-client\"\n[dependencies]\nagent-api-proto = { path = \"../agent-api-proto\" }\n");
    put(root, "clis/t3-serve/core/Cargo.toml", "[package]\nname = \"t3-core\"\n[dependencies]\nagent-api-client = { path = \"../../../libraries/agent-api/agent-api-client\" }\n");
    put(root, "clis/t3-serve/vcs/Cargo.toml", "[package]\nname = \"t3-vcs\"\n[dependencies]\nt3-core = { path = \"../core\" }\n");
    // Then nothing is counted
    assert!(on(&budget(&[]), r#"msg="cargo-dep-violations" value=0 "#));
    // And the testkit as a normal dependency of a mapper, and the client library dev-depending on the area crate that consumes it through the core, are one finding each
    put(root, "clis/t3-serve/core/Cargo.toml", "[package]\nname = \"t3-core\"\n[dependencies]\nagent-api-client = { path = \"../../../libraries/agent-api/agent-api-client\" }\nexec-testkit = { path = \"../../exec-testkit\" }\n");
    put(root, "libraries/agent-api/agent-api-client/Cargo.toml", "[package]\nname = \"agent-api-client\"\n[dependencies]\nagent-api-proto = { path = \"../agent-api-proto\" }\n[dev-dependencies]\nt3-vcs = { path = \"../../../clis/t3-serve/vcs\" }\n");
    let r = budget(&[]);
    assert!(r.exit_code == 1 && on(&r, r#"msg="cargo-dep-violations over budget" value=2 limit=0 gated=true"#), "{}", r.stderr);
    assert!(r.stderr.contains("t3-core depends on exec-testkit outside dev-dependencies") && r.stderr.contains("the adapter library agent-api-client depends on t3-vcs, which consumes it"), "{}", r.stderr);
    // And a manifest that cannot be read is a finding too, so a broken one cannot hide a rule
    put(root, "clis/t3-serve/core/Cargo.toml", "[package\nname = ");
    let r = budget(&[]);
    assert!(on(&r, r#"msg="cargo-dep-violations over budget" value=1 limit=0 gated=true"#) && r.stderr.contains("clis/t3-serve/core/Cargo.toml cannot be read"), "{}", r.stderr);
    std::fs::remove_dir_all(root.join("clis")).unwrap();
    std::fs::remove_dir_all(root.join("libraries")).unwrap();
    std::fs::remove_dir_all(root.join("backend")).unwrap();

    // Given libraries.json naming service-kit, a service that depends on it by path and a library manifest that does so beside a version and registry
    put(root, "libraries.json", &json!({ "libraries": [{ "name": "service-kit", "path": "backend/service-kit" }, { "name": "telemetry", "path": "clis/telemetry" }] }).to_string());
    put(root, "backend/svc/Cargo.toml", "[dependencies]\nservice-kit = { version = \"0.1\", registry = \"mla\" }\n");
    put(root, "backend/service-kit/Cargo.toml", "[dependencies]\ntelemetry = { version = \"0.1\", registry = \"mla\", path = \"../../clis/telemetry\" }\n");
    // Then registry dependencies and a library's own sibling path pass
    assert!(on(&budget(&[]), r#"msg="versioned-library-path-deps" value=0 "#));
    // And a path dependency on the library, under its own key or a renamed one, fails the budget naming the manifest
    put(root, "backend/svc/Cargo.toml", "[dependencies]\nservice-kit = { path = \"../service-kit\" }\nkit = { package = \"telemetry\", path = \"../../clis/telemetry\" }\n");
    let r = budget(&[]);
    assert!(r.exit_code == 1 && on(&r, r#"msg="versioned-library-path-deps over budget" value=2 limit=0 gated=true"#), "{}", r.stderr);
    assert!(r.stderr.contains("backend/svc/Cargo.toml: service-kit") && r.stderr.contains("backend/svc/Cargo.toml: telemetry"), "{}", r.stderr);
    std::fs::remove_dir_all(root.join("backend")).unwrap();
    std::fs::remove_file(root.join("libraries.json")).unwrap();

    // Given no CONCEPTS.md, known-gaps or CLAUDE.md, and no pre-commit section, Then each counts as zero
    std::fs::remove_file(root.join("CONCEPTS.md")).unwrap();
    std::fs::remove_file(root.join("CLAUDE.md")).unwrap();
    std::fs::remove_dir_all(root.join("docs")).unwrap();
    put(root, "lefthook.yml", "post-land:\n  jobs:\n    - name: x\n");
    let r = budget(&[]);
    assert_eq!(r.exit_code, 0);
    for name in ["root-claude-md-bytes", "known-gaps-lines", "concepts-dangling-links", "pre-commit-jobs"] {
        assert!(on(&r, &format!(r#"msg="{name}" value=0 "#)), "{name}: {}", r.stdout);
    }

    // Given no package.json, Then it fails with the read error
    std::fs::remove_file(root.join("package.json")).unwrap();
    let r = budget(&[]);
    assert!(r.exit_code != 0 && format!("{}{}", r.stdout, r.stderr).contains("package.json"));
}


#[test]
fn the_budget_counts_unpinned_actions_env_files_in_artifacts_and_inherited_secrets() {
    // Given a repo whose workflows pin every third-party action by sha, exclude the env file from artifacts and pass no secrets wholesale
    let dir = tempfile::Builder::new().prefix("wf-flow-hygiene-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    put(root, "lefthook.yml", &lefthook(2));
    put(root, "CLAUDE.md", "x");
    put(root, "docs/known-gaps.md", "x");
    put(root, "package.json", "{\"scripts\":{}}");
    put(root, ".agents/config/hooks.json", &hooks(&[&["one"]]));
    let sha = "11d5960a326750d5838078e36cf38b85af677262";
    let clean = format!("jobs:\n  a:\n    steps:\n      - uses: actions/checkout@{sha} # v4\n      - uses: ./.github/actions/local\n      - uses: actions/upload-artifact@{sha} # v4\n        with:\n          path: |\n            out/\n            .env*\n            !.env*\n  b:\n    uses: ./.github/workflows/other.yml\n");
    put(root, ".github/workflows/ci.yml", &clean);
    put(root, ".github/actions/local/action.yml", &format!("runs:\n  using: composite\n  steps:\n    - uses: actions/cache@{sha}\n      with:\n        path: ~/.cache\n"));
    let budget = || run_wf(&["check", "--budget"], &RunOpts::default().cwd(root).clean_env());
    let on = |r: &RunResult, re: &str| Regex::new(re).unwrap().is_match(&format!("{}{}", r.stdout, r.stderr));
    // When the budget runs, Then the three metrics are 0 (the exit code is the CI job-scope rule's, which wants the real jobs)
    let r = budget();
    for name in ["unpinned-third-party-actions", "env-file-in-artifacts", "secrets-inherit"] {
        assert!(on(&r, &format!(r#"msg="{name}" value=0 limit=0 gated=true"#)), "{name}: {}", r.stdout);
    }

    // Given a third-party action pinned by a tag, Then it is counted
    put(root, ".github/workflows/ci.yml", &clean.replace(&format!("actions/checkout@{sha} # v4"), "actions/checkout@v4"));
    let r = budget();
    assert!(on(&r, r#"msg="unpinned-third-party-actions over budget" value=1 limit=0 gated=true"#), "{}", r.stderr);
    assert!(r.stderr.contains("ci.yml:4: actions/checkout@v4"), "{}", r.stderr);

    // Given an artifact path with a .env file and no exclusion, and a cache step in an action doing the same
    put(root, ".github/workflows/ci.yml", &clean.replace("            !.env*\n", ""));
    let r = budget();
    assert!(on(&r, r#"msg="env-file-in-artifacts over budget" value=1 limit=0 gated=true"#), "{}", r.stderr);
    put(root, ".github/actions/local/action.yml", &format!("runs:\n  using: composite\n  steps:\n    - uses: actions/cache@{sha}\n      with:\n        path: apps/web/.env\n"));
    assert!(on(&budget(), r#"msg="env-file-in-artifacts over budget" value=2 limit=0 gated=true"#));

    // Given a called workflow that inherits every secret, Then it is counted
    put(root, ".github/actions/local/action.yml", "runs:\n  using: composite\n  steps: []\n");
    put(root, ".github/workflows/ci.yml", &clean.replace("    uses: ./.github/workflows/other.yml\n", "    uses: ./.github/workflows/other.yml\n    secrets: inherit\n"));
    let r = budget();
    assert!(on(&r, r#"msg="secrets-inherit over budget" value=1 limit=0 gated=true"#), "{}", r.stderr);
}

#[test]
fn the_pre_push_hook_runs_the_sandbox_stages_and_the_agent_cannot_skip_it() {
    // Given the repository's lefthook config and the agent's deny list
    let lefthook = std::fs::read_to_string(repo_root().join("lefthook.yml")).unwrap();
    let deny = std::fs::read_to_string(repo_root().join("libraries/hook-runtime/src/deny.json")).unwrap();

    // When the pre-push section is read
    let section = lefthook.split("\npre-push:\n").nth(1).and_then(|rest| rest.split("\npost-land:").next()).unwrap_or_default();

    // Then, while a pre-push section exists (the owner removed it on 9 Nov 2026 while the hosted CI takes over), it runs `wf check --pushed` with git's ref lines on stdin, and `git push --no-verify` is denied
    assert!(section.is_empty() || (section.contains("run: wf check --pushed") && section.contains("use_stdin: true")), "{section}");
    assert!(deny.contains(r#""args_sequential": ["^push$"], "args_any_of": ["^--no-verify$"]"#), "git push --no-verify is not denied");
}

#[test]
fn the_pushed_gate_plans_the_push_runs_every_sandbox_stage_and_stops_the_push_on_a_red_one() {
    // Given a repository, and a stub `wf` that records each call, prints an empty plan, says the browser stage is all replays, and
    // fails the flow stage unless STUB_PASS is set
    let dir = tempfile::Builder::new().prefix("wf-flow-pushed-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let log = root.join("calls.log");
    let stub = root.join("stub-wf");
    executable(
        &stub,
        &format!(
            "#!/bin/sh\necho \"$* | nx=${{NX_CACHE_DIRECTORY:-shared}} push=${{WF_PUSH_TO_DEV:-no}}\" >> {}\n[ -z \"$STUB_UNSEEDED\" ] || case \"$*\" in *\"--stage flow-tests\"*) echo 'wf: build cache reads only for this run, dirty: 1 file' >&2;; esac\ncase \"$*\" in\n  *\"secret get OPENCODE_GO_API_KEY\"*) [ -z \"$STUB_NOKEY\" ] && echo model-key || exit 1;;\n  *\"test plan\"*) echo '{{\"stages\":{{}}}}';;\n  *--probe*) echo '{{\"ok\":true,\"ran\":[],\"cached\":[]}}';;\n  *\"--stage flow-tests\"*) [ -n \"$STUB_PASS\" ] || exit 1;;\nesac\n",
            log.display()
        ),
    );
    let run = |input: &str, pass: bool| {
        let mut opts = RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).input(input);
        if pass {
            opts = opts.env("STUB_PASS", "1");
        }
        std::fs::write(&log, "").unwrap();
        let r = run_wf(&["check", "--pushed"], &opts);
        (r, std::fs::read_to_string(&log).unwrap())
    };

    // When a push to a feature branch runs with the flow stage red
    let (r, calls) = run("refs/heads/x aaa refs/heads/claude/device-lab bbb\n", false);

    // Then the push is planned against origin/dev, the four stages run in CI's order (the red one does not skip the rest), nothing is
    // written to the shared cache, a browser stage that is all replays asks for no export, and the gate exits 1 naming the stage
    let lines: Vec<&str> = calls.lines().collect();
    assert!(lines[0].starts_with("test plan --changed --base origin/dev --json"), "{calls}");
    let stages: Vec<&str> = lines.iter().filter(|l| l.contains("test run --stage") && !l.contains("--probe")).map(|l| l.split_whitespace().nth(3).unwrap()).collect();
    assert_eq!(stages, ["typecheck-lint-unit-tests", "flow-tests", "integration-tests", "browser-e2e-tests", "cli-e2e-tests"], "{calls}");
    assert!(lines.iter().all(|l| l.ends_with("push=no")) && !calls.contains("vm net"), "{calls}");
    // Then a push that is not to dev keeps the developer's own Nx cache
    assert!(lines.iter().all(|l| l.contains("nx=shared")), "{calls}");
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("not pushing: flow-tests failed here"), "{}", r.stderr);

    // When a push to dev passes every stage
    let (r, calls) = run("refs/heads/claude/device-lab aaa refs/heads/dev bbb\n", true);

    // Then each stage runs with the write scope of a push to dev, and the gate exits 0
    assert!(calls.lines().filter(|l| l.contains("test run --stage")).all(|l| l.ends_with("push=1") && !l.contains("nx=shared")), "{calls}");
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("passed here, so CI replays them"), "{}", r.stderr);

    // When a stage of that push passes but says it only read the shared cache (a dirty tree: its passes reach no one)
    std::fs::write(&log, "").unwrap();
    let opts = RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).env("STUB_PASS", "1").env("STUB_UNSEEDED", "1").input("refs/heads/claude/device-lab aaa refs/heads/dev bbb\n");
    let unseeded = run_wf(&["check", "--pushed"], &opts);

    // Then the push is refused naming the stage, because CI would run it again
    assert_eq!(unseeded.exit_code, 1, "{}{}", unseeded.stdout, unseeded.stderr);
    assert!(unseeded.stderr.contains("flow-tests passed here but did not seed the shared cache"), "{}", unseeded.stderr);

    // When the developer switches the suite off (WF_PUSH_CLI_E2E=0), the push goes through on the other four stages
    let stages_of = |calls: &str| -> Vec<String> { calls.lines().filter(|l| l.contains("test run --stage") && !l.contains("--probe")).map(|l| l.split_whitespace().nth(3).unwrap().to_string()).collect() };
    std::fs::write(&log, "").unwrap();
    let opts = RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).env("STUB_PASS", "1").env("WF_PUSH_CLI_E2E", "0").input("refs/heads/claude/device-lab aaa refs/heads/dev bbb\n");
    let r = run_wf(&["check", "--pushed"], &opts);
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_eq!(stages_of(&calls), ["typecheck-lint-unit-tests", "flow-tests", "integration-tests", "browser-e2e-tests"], "{calls}");
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);

    // And when the vault has no model key the push is refused before any stage runs, and the refusal names the way out
    std::fs::write(&log, "").unwrap();
    let opts = RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).env("STUB_PASS", "1").env("STUB_NOKEY", "1").input("refs/heads/claude/device-lab aaa refs/heads/dev bbb\n");
    let r = run_wf(&["check", "--pushed"], &opts);
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_ne!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("OPENCODE_GO_API_KEY") && r.stderr.contains("WF_PUSH_CLI_E2E=0"), "{}", r.stderr);
    assert!(stages_of(&calls).is_empty(), "{calls}");

    // And a push to dev with the key runs the suite last, writing to the shared cache like every other stage, so CI replays it
    std::fs::write(&log, "").unwrap();
    let opts = RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).env("STUB_PASS", "1").input("refs/heads/claude/device-lab aaa refs/heads/dev bbb\n");
    let with_e2e = run_wf(&["check", "--pushed"], &opts);
    let calls = std::fs::read_to_string(&log).unwrap();
    assert_eq!(stages_of(&calls).last().map(String::as_str), Some("cli-e2e-tests"), "{calls}");
    assert!(calls.lines().filter(|l| l.contains("--stage cli-e2e-tests")).all(|l| l.ends_with("push=1")), "{calls}");
    assert_eq!(with_e2e.exit_code, 0, "{}{}", with_e2e.stdout, with_e2e.stderr);
}

#[test]
fn the_pushed_gate_refuses_a_push_from_a_node_other_than_the_pinned_one_before_planning_anything() {
    // Given a repository pinning a node no machine has, and a stub `wf` that records each call
    let dir = tempfile::Builder::new().prefix("wf-flow-pushed-pins-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("mise.toml"), "[tools]\nnode = \"0.0.1\"\n").unwrap();
    let log = root.join("calls.log");
    let stub = root.join("stub-wf");
    executable(&stub, &format!("#!/bin/sh\necho \"$*\" >> {}\n", log.display()));

    // When the gate runs for a push to dev
    let opts = RunOpts::default().cwd(root).env("WF_PUSHED_EXE", stub.to_str().unwrap()).input("refs/heads/x aaa refs/heads/dev bbb\n");
    let r = run_wf(&["check", "--pushed"], &opts);

    // Then the push is refused naming the tool and its pin, and no plan or stage ran
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("node") && r.stderr.contains("pinned 0.0.1") && r.stderr.contains("setup.sh"), "{}", r.stderr);
    assert!(std::fs::read_to_string(&log).unwrap_or_default().is_empty());
}

#[test]
fn the_pushed_gate_refuses_a_push_to_dev_that_lacks_a_main_commit_and_proceeds_once_dev_contains_it() {
    // Given a repository whose origin/main holds a commit dev lacks, and a stub `wf` that records each call
    let dir = tempfile::Builder::new().prefix("wf-flow-pushed-main-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "dev"]);
    put(root, "a.txt", "a");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "base"]);
    git(root, &["checkout", "-q", "-b", "main"]);
    put(root, "m.txt", "m");
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "main only"]);
    git(root, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(root, &["checkout", "-q", "dev"]);
    let log = root.join("calls.log");
    let stub = root.join("stub-wf");
    executable(&stub, &format!("#!/bin/sh\necho \"$*\" >> {}\ncase \"$*\" in *\"test plan\"*) echo '{{\"stages\":{{}}}}';; *--probe*) echo '{{\"ok\":true,\"ran\":[],\"cached\":[]}}';; esac\n", log.display()));
    let run = |lines: &str| {
        let _ = std::fs::write(&log, "");
        let opts = RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).env("WF_PUSH_CLI_E2E", "0").input(lines);
        let r = run_wf(&["check", "--pushed"], &opts);
        (r, std::fs::read_to_string(&log).unwrap_or_default())
    };

    let rev = |name: &str| String::from_utf8(std::process::Command::new("git").args(["rev-parse", name]).current_dir(root).output().unwrap().stdout).unwrap().trim().to_string();
    let (dev_sha, main_sha) = (rev("dev"), rev("main"));

    // When the gate runs for a push to dev
    let (r, calls) = run(&format!("refs/heads/dev {dev_sha} refs/heads/dev bbb\n"));

    // Then it refuses with the next step and runs no stage
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("dev does not contain origin/main: git fetch origin main && git merge origin/main, resolve conflicts"), "{}", r.stderr);
    assert!(calls.is_empty(), "{calls}");

    // When the pushed commit is main's (dev is pushed from another ref than the checked out one), and when dev is deleted
    let (r_main, _) = run(&format!("refs/heads/x {main_sha} refs/heads/dev bbb\n"));
    let (r_delete, _) = run("(delete) 0000000000000000000000000000000000000000 refs/heads/dev bbb\n");

    // Then the verdict follows the pushed commit, not HEAD, and a deletion is not gated
    assert!(!r_main.stderr.contains("does not contain origin/main"), "{}", r_main.stderr);
    assert!(!r_delete.stderr.contains("does not contain origin/main"), "{}", r_delete.stderr);

    // When a push to a feature branch runs with the same checkout
    let (_, calls) = run("refs/heads/x aaa refs/heads/claude/x bbb\n");

    // Then the rule is for dev only: the push is planned
    assert!(calls.contains("test plan"), "{calls}");

    // When dev merges main and the gate runs again for a push to dev
    git(root, &["merge", "-q", "--no-edit", "main"]);
    let (r, calls) = run(&format!("refs/heads/dev {} refs/heads/dev bbb\n", rev("dev")));

    // Then the gate proceeds to plan and run its stages
    assert!(calls.contains("test plan") && calls.contains("test run --stage"), "{calls}");
    assert!(!r.stderr.contains("does not contain origin/main"), "{}", r.stderr);
}

#[test]
fn the_pushed_gate_prepares_the_export_for_a_browser_stage_that_would_run_even_when_the_tailnet_has_no_dns() {
    // Given a stub `wf` for which the browser stage would run, whose tailnet join works but whose services probe and name lookup find
    // nothing (the sandbox's userspace tailnet), and a vault that holds the gateway token
    let dir = tempfile::Builder::new().prefix("wf-flow-pushed-browser-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let log = root.join("calls.log");
    let stub = root.join("stub-wf");
    executable(
        &stub,
        &format!(
            "#!/bin/sh\necho \"$* | api=${{EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL:-none}}\" >> {}\ncase \"$*\" in\n  *\"test plan\"*) echo '{{\"stages\":{{}}}}';;\n  *--probe*) echo '{{\"ok\":true,\"ran\":[\"editor-typing\"],\"cached\":[]}}';;\n  *\"vm ops probe\"*|*\"vm net resolve\"*) exit 1;;\n  *\"secret get VM_GATEWAY_AUTH_SEED\"*) echo token-value;;\n  *\"secret get\"*) exit 1;;\nesac\n",
            log.display()
        ),
    );

    // When the gate runs
    let r = run_wf(&["check", "--pushed"], &RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).env("WF_PUSH_CLI_E2E", "0").input(""));
    let calls = std::fs::read_to_string(&log).unwrap();

    // Then it joins the tailnet, bakes the bare tailnet names, prepares the web export before the stage and gives the stage the same
    // environment, says the services were not probed, and passes
    let at = |needle: &str| calls.lines().position(|l| l.contains(needle)).unwrap_or(usize::MAX);
    let ran = calls.lines().position(|l| l.contains("test run --stage browser-e2e-tests --plan") && !l.contains("--probe")).unwrap_or(usize::MAX);
    assert!(at("vm net up") < at("test prepare --web") && at("test prepare --web") < ran, "{calls}");
    assert!(calls.lines().any(|l| l.starts_with("test prepare --web") && l.ends_with("api=http://leetcode-backend-prod:8789")), "{calls}");
    assert!(calls.lines().any(|l| l.starts_with("test run --stage browser-e2e-tests --plan") && !l.contains("--probe") && l.ends_with("api=http://leetcode-backend-prod:8789")), "{calls}");
    assert!(r.stderr.contains("services were not probed from here"), "{}", r.stderr);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_script_that_records_into_a_fixtures_path_fails_the_budget_unless_the_allowlist_names_it_and_mimic_is_exempt() {
    let dir = tempfile::Builder::new().prefix("wf-flow-recorders-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    put(root, "lefthook.yml", &lefthook(2));
    put(root, "CLAUDE.md", &lines(73));
    put(root, "docs/known-gaps.md", &lines(53));
    put(root, "package.json", &json!({ "scripts": { "a": "1" } }).to_string());
    put(root, ".agents/config/hooks.json", &hooks(&[&["one"]]));
    let budget = || run_wf(&["check", "--budget"], &RunOpts::default().cwd(root).clean_env());
    let on = |r: &RunResult, re: &str| Regex::new(re).unwrap().is_match(&format!("{}{}", r.stdout, r.stderr));
    let over = |n: usize| format!(r#"msg="handrolled-recorder-violations over budget" value={n} limit=0 gated=true"#);

    // Given recorded interactions with their provenance, and the harness's own sources, Then nothing is counted
    put(root, "clis/x/fixtures/set/cassette.json", "{}");
    put(root, "clis/x/fixtures/set/interactions.jsonl", "");
    put(root, "tooling/mimic-drivers/src/generate-all.ts", "writeFileSync('fixtures/a.json', '')");
    assert!(on(&budget(), r#"msg="handrolled-recorder-violations" value=0 "#));

    // Given a generate script under a fixtures path, a script that writes into a vectors path, interactions with no cassette.json and a RECORD_ switch in a test
    put(root, "clis/x/fixtures/generate.mjs", "// regenerates the goldens");
    put(root, "clis/y/tools/dump.ts", "import { writeFileSync } from 'node:fs'\nwriteFileSync('clis/y/vectors/v.json', JSON.stringify(1))");
    put(root, "clis/z/cassettes/set/interactions.jsonl", "");
    put(root, "clis/w/tests/flow.rs", "fn rewrite() { if std::env::var_os(\"RECORD_W_FIXTURES\").is_some() {} }");
    // Then each is one finding, named by its path
    let r = budget();
    assert!(r.exit_code == 1 && on(&r, &over(4)), "{}", r.stderr);
    for path in ["clis/x/fixtures/generate.mjs", "clis/y/tools/dump.ts", "clis/z/cassettes/set/interactions.jsonl", "clis/w/tests/flow.rs"] {
        assert!(r.stderr.contains(path), "{path}: {}", r.stderr);
    }

    // And an allowlist entry removes a finding, and an entry whose file is gone is a finding itself
    let entry = |p: &str| json!({ "path": p, "why": "kept" });
    put(root, "clis/mimic/recorder-allowlist.json", &json!({ "allow": [entry("clis/x/fixtures/generate.mjs"), entry("clis/gone/generate.mjs")] }).to_string());
    let r = budget();
    assert!(on(&r, &over(4)) && r.stderr.contains("clis/gone/generate.mjs no longer exists"), "{}", r.stderr);
    assert!(!r.stderr.contains("clis/x/fixtures/generate.mjs:"), "{}", r.stderr);
}

#[test]
fn staging_a_cassette_set_or_what_records_it_runs_mimic_verify_and_a_stale_set_fails_the_gate() {
    let w = Workspace::new();
    let verifies = |w: &Workspace| -> Vec<String> { w.calls().into_iter().filter(|c| c.starts_with("wf mimic verify")).collect() };
    w.put("clis/tool/fixtures/set/mimic.toml", "");
    w.put("clis/tool/fixtures/set/cassette.json", "{}");
    w.put("clis/tool/fixtures/set/interactions.jsonl", "");
    // Given a staged file of a cassette set, When the gate runs, Then every set is verified offline, once
    let r = w.staged(&["--files", "clis/tool/fixtures/set/interactions.jsonl"], json!([]), &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let calls = verifies(&w);
    assert!(calls.len() == 1 && calls[0].contains("--all --root") && !calls[0].contains("--live"), "{calls:?}");
    // And a file no set or recorder owns verifies nothing
    assert_eq!(w.staged(&["--files", "libraries/b/notes.txt"], json!([]), &[]).exit_code, 0);
    assert!(verifies(&w).is_empty());
    // And a stale set fails the gate with the verifier's own words
    let r = w.staged(&["--files", "clis/tool/fixtures/set/interactions.jsonl"], json!([{ "match": "mimic verify", "code": 1, "stderr": "set is stale\n" }]), &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("set is stale"), "{}", r.stderr);

    // Given the reference's own code changed and its toolchain is not installed, Then it is verified offline only
    let r = w.staged(&["--files", "tooling/mimic-drivers/src/cli.ts"], json!([]), &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(verifies(&w).iter().all(|c| !c.contains("--live")), "{:?}", verifies(&w));
    // And with the toolchain and the pinned Orca checkout present, the reference is re-recorded and diffed too
    w.put("tooling/mimic-drivers/node_modules/.keep", "");
    w.put("references/orca/src/.keep", "");
    let r = w.staged(&["--files", "tooling/mimic-drivers/src/cli.ts"], json!([]), &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let calls = verifies(&w);
    assert!(calls.len() == 2 && calls[1].ends_with("--live @."), "{calls:?}");
}

#[test]
fn staging_a_file_of_a_fixture_tree_runs_the_provenance_scan_and_an_unclassified_file_fails_the_gate() {
    let w = Workspace::new();
    let scans = |w: &Workspace| -> Vec<String> { w.calls().into_iter().filter(|c| c.starts_with("wf mimic verify --fixtures")).collect() };
    w.put("libs/api/fixture-provenance.json", "{}");
    w.put("libs/api/crate/tests/fixtures/a.json", "{}");
    // Given a staged fixture file, When the gate runs, Then its tree is scanned once
    let r = w.staged(&["--files", "libs/api/crate/tests/fixtures/a.json"], json!([]), &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let calls = scans(&w);
    assert!(calls.len() == 1 && calls[0].contains("libs/api"), "{calls:?}");
    // And a file outside every fixture tree scans nothing
    assert_eq!(w.staged(&["--files", "libs/b/notes.txt"], json!([]), &[]).exit_code, 0);
    assert!(scans(&w).is_empty(), "{:?}", scans(&w));
    // And an unclassified file fails the gate with the scan's own words
    let r = w.staged(&["--files", "libs/api/crate/tests/fixtures/a.json"], json!([{ "match": "mimic verify --fixtures", "code": 1, "stderr": "unclassified: crate/tests/fixtures/a.json\n" }]), &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("unclassified"), "{}", r.stderr);
}

#[test]
fn staging_the_code_a_set_is_pinned_to_verifies_it_and_re_records_it_once_the_drivers_toolchain_is_installed() {
    let w = Workspace::new();
    let verifies = |w: &Workspace| -> Vec<String> { w.calls().into_iter().filter(|c| c.starts_with("wf mimic verify")).collect() };
    // Given a set recorded from a directory of this repository (the pin is that directory's git tree), named only in its mimic.toml
    w.put("backend/svc/cassettes/dsl/mimic.toml", "[reference]\ncommand = [\"node\"]\npin_tree = \"../../../../lib/dsl\"\n");
    w.put("lib/dsl/src/rule.ts", "export const rule = 1");
    w.put("lib/other/src/rule.ts", "export const rule = 1");

    // When a file of that directory is staged, Then every set is verified offline
    let r = w.staged(&["--files", "lib/dsl/src/rule.ts"], json!([]), &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let calls = verifies(&w);
    assert!(calls.len() == 1 && calls[0].contains("--all --root") && !calls[0].contains("--live"), "{calls:?}");
    // And a file of a directory no set is pinned to verifies nothing
    assert_eq!(w.staged(&["--files", "lib/other/src/rule.ts"], json!([]), &[]).exit_code, 0);
    assert!(verifies(&w).is_empty());
    // And the stale pin fails the gate with the verifier's own words
    let r = w.staged(&["--files", "lib/dsl/src/rule.ts"], json!([{ "match": "mimic verify", "code": 1, "stderr": "pin_hash is stale\n" }]), &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("pin_hash is stale"), "{}", r.stderr);

    // Given the drivers' toolchain is installed, When the pinned directory changed, Then that set alone is re-recorded in memory and diffed
    w.put("tooling/mimic-drivers/node_modules/.keep", "");
    let r = w.staged(&["--files", "lib/dsl/src/rule.ts"], json!([]), &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    let calls = verifies(&w);
    assert!(calls.len() == 2 && calls[1].contains("backend/svc/cassettes/dsl") && calls[1].contains(" --live") && !calls[1].contains("--all"), "{calls:?}");
}

#[test]
fn the_pushed_gate_reports_per_stage_whether_the_run_seeded_the_shared_cache() {
    // Given a repository, a stub `wf` whose flow stage passes but says it only read the shared cache, and a metrics source
    let dir = tempfile::Builder::new().prefix("wf-flow-seeding-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let stub = root.join("stub-wf");
    executable(
        &stub,
        "#!/bin/sh\ncase \"$*\" in *\"--stage flow-tests\"*) echo 'wf: build cache reads only for this run, dirty: 1 file' >&2;; esac\ncase \"$*\" in\n  *\"test plan\"*) echo '{\"stages\":{}}';;\n  *--probe*) echo '{\"ok\":true,\"ran\":[],\"cached\":[]}';;\nesac\n",
    );
    let source = MockServer::start(None);

    // When a push to dev runs every stage
    let opts = RunOpts::default()
        .cwd(root)
        .clean_env()
        .env("WF_PUSHED_EXE", stub.to_str().unwrap())
        .env("WF_PUSH_CLI_E2E", "0")
        .env("WF_CI_METRICS_BASE", &format!("http://127.0.0.1:{}", source.port))
        .input("refs/heads/claude/device-lab aaa refs/heads/dev bbb\n");
    let r = run_wf(&["check", "--pushed"], &opts);

    // Then the push is refused for the unseeded stage, and each stage has an event with its verdict and a gauge of 1 or 0
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    let verdicts: Vec<&str> = r.stderr.lines().filter(|l| l.contains("msg=\"gate.seeding\"")).collect();
    assert_eq!(verdicts.len(), 4, "{}", r.stderr);
    assert!(verdicts.iter().any(|l| l.contains("stage=flow-tests") && l.contains("seeded=false") && l.contains("level=warn") && l.contains("reads only for this run")), "{verdicts:?}");
    assert!(verdicts.iter().any(|l| l.contains("stage=integration-tests") && l.contains("seeded=true") && l.contains("level=info")), "{verdicts:?}");
    let seeded: std::collections::BTreeMap<String, f64> = crate::support::otlp::gauges(&source.requests()).into_iter().filter(|g| g.metric == "ci.gate.seeded").map(|g| (g.scope, g.value)).collect();
    assert_eq!(seeded.get("flow-tests"), Some(&0.0), "{seeded:?}");
    assert_eq!(seeded.get("integration-tests"), Some(&1.0), "{seeded:?}");
    assert_eq!(seeded.len(), 4, "{seeded:?}");
}

#[test]
fn the_pushed_gate_refuses_a_stage_whose_acknowledged_writes_the_store_does_not_hold() {
    // Given a stub `wf` whose flow stage wrote 10 entries and finds 7 of them when it asks again, and whose integration stage finds all 4
    let dir = tempfile::Builder::new().prefix("wf-flow-seeded-n-of-m-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let stub = root.join("stub-wf");
    executable(
        &stub,
        "#!/bin/sh\ncase \"$*\" in\n  *\"--stage flow-tests\"*) echo 'wf: build cache {\"seeded\":{\"checked\":10,\"present\":7}}' >&2;;\n  *\"--stage integration-tests\"*) echo 'wf: build cache {\"seeded\":{\"checked\":4,\"present\":4}}' >&2;;\nesac\ncase \"$*\" in\n  *\"test plan\"*) echo '{\"stages\":{}}';;\n  *--probe*) echo '{\"ok\":true,\"ran\":[],\"cached\":[]}';;\nesac\n",
    );

    // When a push to dev runs every stage
    let opts = RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).env("WF_PUSH_CLI_E2E", "0").env("WF_CI_METRICS", "off").input("refs/heads/claude/x aaa refs/heads/dev bbb\n");
    let r = run_wf(&["check", "--pushed"], &opts);

    // Then the push is refused for the stage that lost entries, with the counts, and the stage that kept them says so
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("flow-tests passed here but did not seed the shared cache") && r.stderr.contains("seeded 7 of 10"), "{}", r.stderr);
    assert!(r.stderr.contains("pushed: integration-tests seeded 4 of 4 entries it wrote"), "{}", r.stderr);
    assert!(!r.stderr.contains("integration-tests passed here but did not seed"), "{}", r.stderr);
}

#[test]
fn the_pushed_gate_deletes_a_passed_stages_test_executables_and_keeps_a_failed_stages() {
    // Given a target directory, and a stub `wf` whose every stage leaves a test executable in it and a library beside it, and whose
    // flow stage fails when STUB_FAIL is set
    let dir = tempfile::Builder::new().prefix("wf-flow-prune-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    let deps = root.join("tgt/debug/deps");
    std::fs::create_dir_all(&deps).unwrap();
    let stub = root.join("stub-wf");
    executable(
        &stub,
        &format!(
            "#!/bin/sh\ncase \"$*\" in\n  *\"test plan\"*) echo '{{\"stages\":{{}}}}';;\n  *--probe*) echo '{{\"ok\":true,\"ran\":[],\"cached\":[]}}';;\n  *\"test run --stage\"*) s=$4; printf '#!/bin/sh\\n' > {d}/$s-0123456789abcdef; chmod +x {d}/$s-0123456789abcdef; echo lib > {d}/lib$s-0123456789abcdef.rlib; [ \"$s\" = flow-tests ] && [ -n \"$STUB_FAIL\" ] && exit 1;;\nesac\nexit 0\n",
            d = deps.display()
        ),
    );
    let bin = root.join("tgt/debug/thin");
    executable(&bin, "#!/bin/sh\n");
    std::fs::hard_link(&bin, deps.join("thin-0123456789abcdef")).unwrap();
    let run = |fail: bool| {
        let mut opts = RunOpts::default().cwd(root).clean_env().env("WF_PUSHED_EXE", stub.to_str().unwrap()).env("CARGO_TARGET_DIR", root.join("tgt").to_str().unwrap()).env("WF_PUSH_CLI_E2E", "0").input("refs/heads/x aaa refs/heads/claude/device-lab bbb\n");
        if fail {
            opts = opts.env("STUB_FAIL", "1");
        }
        run_wf(&["check", "--pushed"], &opts)
    };
    let left = || {
        let mut names: Vec<String> = std::fs::read_dir(&deps).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };

    // When the flow stage fails
    let r = run(true);

    // Then the executable of the stage that passed before the failure is gone, the failed stage's and the later ones are kept for debugging, and libraries and the hard-linked binary stay
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!(
        left(),
        ["browser-e2e-tests-0123456789abcdef", "flow-tests-0123456789abcdef", "integration-tests-0123456789abcdef", "libbrowser-e2e-tests-0123456789abcdef.rlib", "libflow-tests-0123456789abcdef.rlib", "libintegration-tests-0123456789abcdef.rlib", "libtypecheck-lint-unit-tests-0123456789abcdef.rlib", "thin-0123456789abcdef"],
        "{}",
        r.stderr
    );

    // When every stage passes
    let r = run(false);

    // Then no stage's executable is left, the passing run says how much it freed, and the binary the next stage runs is intact
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(left().iter().all(|n| n.starts_with("lib") || n.starts_with("thin-")), "{:?}", left());
    assert!(bin.exists() && deps.join("thin-0123456789abcdef").exists());
}

#[test]
fn a_workflow_job_that_no_class_covers_fails_the_budget_and_the_budget_passes_without_it() {
    let dir = tempfile::Builder::new().prefix("wf-flow-ci-scope-").tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q"]);
    put(root, "lefthook.yml", &lefthook(2));
    put(root, "CLAUDE.md", &lines(73));
    put(root, "docs/known-gaps.md", &lines(53));
    put(root, "package.json", &json!({ "scripts": { "a": "1" } }).to_string());
    put(root, ".agents/config/hooks.json", &hooks(&[&["one"]]));
    let budget = || run_wf(&["check", "--budget"], &RunOpts::default().cwd(root).clean_env());
    let on = |r: &RunResult, re: &str| Regex::new(re).unwrap().is_match(&format!("{}{}", r.stdout, r.stderr));
    let workflow = |key: &str| format!("name: probe\non: workflow_dispatch\njobs:\n  stray:\n    runs-on: ubuntu-latest\n    steps:\n      - run: wf secret get {key}\n");

    // Given a planted workflow whose job reads a key but has no class in job-scopes.json
    put(root, ".github/workflows/probe.yml", &workflow("TEST_RUNS_ORDS_BASE_URL"));
    // When the budget runs
    let unclassified = budget();
    // Then the budget is over and names the job that has no class (and every declared job this planted tree lacks)
    assert!(unclassified.exit_code == 1 && on(&unclassified, r#"msg="ci-scope-violations over budget" value=\d+ limit=0 gated=true"#), "{}{}", unclassified.stdout, unclassified.stderr);
    assert!(unclassified.stderr.contains("probe.yml:stray"), "{}", unclassified.stderr);

    // Given the planted workflow is removed
    std::fs::remove_dir_all(root.join(".github")).unwrap();
    // When the budget runs, Then the scope check counts nothing
    assert!(on(&budget(), r#"msg="ci-scope-violations" value=0 "#));
}

#[test]
fn a_shard_runs_its_share_of_the_packages_and_the_shards_together_run_each_package_once() {
    // Given a plan that typechecks four packages
    let w = Workspace::new();
    let packages = ["@lib/b", "@app/svc", "@lib/a", "@cli/tool"];
    let projects = |w: &Workspace| -> Vec<String> {
        w.nx_calls().iter().filter_map(|c| {
            let args: Vec<&str> = c["args"].as_array()?.iter().filter_map(Value::as_str).collect();
            let at = args.iter().position(|a| *a == "-p")?;
            Some(args.get(at + 1)?.split(',').map(str::to_string).collect::<Vec<_>>())
        }).flatten().collect()
    };

    // When the first and the second of two shards run
    let first = w.stage_over(&packages, &["--shard", "1/2"], &[]);
    let first_projects = projects(&w);
    let second = w.stage_over(&packages, &["--shard", "2/2"], &[]);
    let second_projects = projects(&w);

    // Then both pass, each package ran in exactly one of them, and the split is by sorted name, round-robin
    assert_eq!((first.exit_code, second.exit_code), (0, 0), "{}{}", first.stderr, second.stderr);
    let mut all: Vec<String> = first_projects.iter().chain(second_projects.iter()).cloned().collect();
    all.sort();
    assert_eq!(all, ["@app/svc", "@cli/tool", "@lib/a", "@lib/b"], "{first_projects:?} {second_projects:?}");
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    assert_eq!((sorted(first_projects), sorted(second_projects)), (vec!["@app/svc".to_string(), "@lib/a".to_string()], vec!["@cli/tool".to_string(), "@lib/b".to_string()]));

    // and a malformed shard is refused before anything runs
    let bad = w.stage_over(&packages, &["--shard", "3/2"], &[]);
    assert_eq!(bad.exit_code, 2, "{}", bad.stdout);
    assert!(w.nx_calls().is_empty());
}
