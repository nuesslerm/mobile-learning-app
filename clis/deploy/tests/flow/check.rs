//! `wf check --deployed`: the verification of a deployed commit, first as a plan printed without any network, then tier by tier against
//! fakes (the staged gate and the budget are in `pipeline`, the scan for dead references in `wf`). Repositories are real throwaway git;
//! `pnpm`, `node`, `oci` and `gh` are argv-recording stubs; vm-ops-api and the services' health sit behind a fake tailnet, the bucket is a
//! local server.
use crate::support::{create_stub, ed25519_private_key_one_line, node_script, repo_root, run_wf, MockReply, MockServer, MockTailnet, RunOpts, RunResult, Upstream};
use wf_vmops::testing::answers;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

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

struct Landed {
    dir: tempfile::TempDir,
    sha: String,
}

impl Landed {
    fn new() -> Self {
        let l = Self { dir: tempfile::Builder::new().prefix("wf-flow-dry-").tempdir().unwrap(), sha: String::new() };
        let root = l.dir.path();
        git(root, &["init", "-q", "-b", "main"]);
        put(root, "package.json", &json!({ "name": "root", "private": true }).to_string());
        put(root, "pnpm-workspace.yaml", "packages:\n  - 'libraries/*'\n  - 'backend/*'\n");
        put(root, "libraries/core/package.json", &json!({ "name": "@lib/core", "version": "1.0.0", "dependencies": {} }).to_string());
        put(root, "backend/api/package.json", &json!({ "name": "@app/api", "version": "1.0.0", "dependencies": { "@lib/core": "workspace:*" } }).to_string());
        git(root, &["add", "-A"]);
        git(root, &["commit", "-q", "-m", "base"]);
        l
    }

    fn land(&mut self, rels: &[&str]) -> &str {
        for rel in rels {
            put(self.dir.path(), rel, &format!("{rel}\n"));
        }
        git(self.dir.path(), &["add", "-A"]);
        git(self.dir.path(), &["commit", "-q", "-m", &format!("change {}", rels.join(" "))]);
        self.sha = git(self.dir.path(), &["rev-parse", "HEAD"]);
        &self.sha
    }

    fn dry_at(&self, target: &str, args: &[&str]) -> RunResult {
        let mut full = vec!["check", "--deployed", target, "--dry-run"];
        full.extend_from_slice(args);
        run_wf(&full, &RunOpts::default().cwd(self.dir.path()).clean_env())
    }

    fn dry(&self, args: &[&str]) -> RunResult {
        self.dry_at("HEAD", args)
    }
}

fn steps(r: &RunResult) -> Vec<String> {
    r.stdout.lines().filter_map(|l| l.strip_prefix("  - ")).map(String::from).collect()
}

fn has_all(r: &RunResult, wanted: &[String]) {
    let got = steps(r);
    for want in wanted {
        assert!(got.contains(want), "missing step {want:?} in {got:#?}");
    }
}

#[test]
fn a_deployed_commit_s_verification_plan_is_printed_from_the_paths_it_changed_without_the_network() {
    let mut l = Landed::new();

    // Given a docs-only change, Then no app changed, tier 1 is skipped and the rest of the plan is listed in order
    l.land(&["docs/notes.md"]);
    let (sha, short) = (l.sha.clone(), l.sha[..8].to_string());
    let out = l.dry(&[]);
    assert_eq!(out.exit_code, 0);
    assert!(out.stdout.starts_with("changed apps: none\nplan (dry run, no network):\n"), "{}", out.stdout);
    assert_eq!(
        steps(&out),
        [
            "tier 1: skipped (no code file changed)".to_string(),
            "eas update: skipped (no frontend change or --no-publish-eas)".to_string(),
            "read tier 2 record: OCI verify/<sha>.tier2.json".to_string(),
            format!("oci os object put verify/{sha}.json"),
            format!("on failure: append a fix unit to docs/plans/fix-{short}/units.jsonl"),
        ]
    );
    // and no step asks the dev bridge or the Metro
    assert!(!out.stdout.contains("metro") && !out.stdout.contains("dev-bridge") && !out.stdout.contains("app-flows"), "{}", out.stdout);
    // and the EAS flags (the last of publish and no-publish wins), the report, PR and timing options and an abbreviated sha are echoed
    assert!(steps(&l.dry(&["--publish-eas", "--eas-branch", "x", "--initial-route=/y"])).contains(&"eas update via publish:eas --branch x --initial-route /y".to_string()));
    assert!(steps(&l.dry(&["--publish-eas", "--no-publish-eas", "--skip-tier1"])).contains(&"eas update: skipped (no frontend change or --no-publish-eas)".to_string()));
    has_all(&l.dry(&["--publish", "--pr", "12"]), &[format!("PUT report HTML to verification/verify/dev/{sha}/index.html"), "rewrite PR #12 description block".into()]);
    assert!(steps(&l.dry_at(&sha[..10], &["--skip-tier1"])).contains(&format!("oci os object put verify/{sha}.json")));

    // Given a backend change, Then the app is health-gated, its e2e suite listed, and tier 1 runs on the package closure in a clean worktree
    l.land(&["backend/remote-chat-agent/src/a.md", "backend/api/src/index.ts"]);
    let short = l.sha[..8].to_string();
    let out = l.dry(&[]);
    assert!(out.stdout.contains("changed apps: remote-chat-agent\n"), "{}", out.stdout);
    has_all(
        &out,
        &[
            format!("tier 1: git worktree add --detach <tmpdir>/wf-verify-tier1-* {short} (removed after)"),
            "tier 1: pnpm install --prefer-offline --frozen-lockfile".to_string(),
            "tier 1: typecheck: pnpm --filter @app/api run --if-present typecheck".to_string(),
            format!("oci os object put verify/{}.tier1.json", l.sha),
            format!("GET remote-chat-agent /health until commit == {short} (timeout 1800000 ms, every 15000 ms)"),
            "sh: REMOTE_CHAT_AGENT_BASE_URL=\"${REMOTE_CHAT_AGENT_BASE_URL:-http://remote-chat-agent-prod:8787}\" pnpm --filter @app/remote-chat-agent run test:e2e".to_string(),
        ],
    );
    assert_eq!(
        steps(&out).into_iter().find(|s| s.starts_with("tier 1: test:unit + test:flow: ")).unwrap(),
        "tier 1: test:unit + test:flow: env -u GIT_INDEX_FILE -u GIT_DIR -u GIT_WORK_TREE -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES -u GIT_COMMON_DIR -u GIT_PREFIX pnpm --filter @app/api run --if-present \"/^test:(unit|flow)$/\""
    );

    // and the gate's timeout and poll interval are echoed on the health line they govern
    assert!(steps(&l.dry(&["--gate-timeout", "60", "--poll", "0.5"])).contains(&format!("GET remote-chat-agent /health until commit == {short} (timeout 60000 ms, every 500 ms)")));

    // Given a library change, its dependants join the tier 1 filters; a code change outside any package affects none
    l.land(&["libraries/core/src/index.ts"]);
    assert!(steps(&l.dry(&[])).contains(&"tier 1: typecheck: pnpm --filter @app/api --filter @lib/core run --if-present typecheck".to_string()));
    l.land(&["tools/x.ts"]);
    assert!(steps(&l.dry(&[])).contains(&"tier 1: no workspace package affected".to_string()));

    // Given a frontend change, Then it has no health to gate on and an EAS update is planned
    l.land(&["frontend/mla-tamagui/app/index.md"]);
    let out = l.dry(&["--skip-tier1"]);
    assert!(out.stdout.contains("changed apps: mla-tamagui\n"));
    assert!(!out.stdout.contains("metro") && !out.stdout.contains("dev-bridge"), "{}", out.stdout);
    has_all(
        &out,
        &[
            "tier 1: skipped (--skip-tier1)".into(),
            "mla-tamagui: no HTTP /health, not gated".into(),
            "eas update via publish:eas --branch dev --initial-route /".into(),
            "read tier 1 + tier 2 records: OCI verify/<sha>.tier1.json, verify/<sha>.tier2.json".into(),
        ],
    );

    // Given only the retired Metro runner's files changed, Then no app changed
    l.land(&["infra/oracle-cloud/metro-runner/docker-compose.prod.yml"]);
    assert!(l.dry(&["--skip-tier1"]).stdout.starts_with("changed apps: none\n"));

    // Given only the crate the desktop's listener is changed, Then the desktop is the app that changed
    l.land(&["clis/wf-login/src/vnc/engine.rs"]);
    let out = l.dry(&["--skip-tier1"]);
    assert!(out.stdout.starts_with("changed apps: vnc-auth-session\n"), "{}", out.stdout);
    has_all(&out, &["vnc-auth-session: no HTTP /health, not gated".into()]);

    // Given several apps, Then they list in path order, vm-ops-api once; and --base takes the apps from that range
    let first = l.land(&["backend/leetcode-backend/a.md", "infra/oracle-cloud/vm-ops-api/b.md", "infra/oracle-cloud/watchdog/c.md", "infra/oracle-cloud/agent-shell/d.md", "clis/vnc-auth-capture/e.md"]).to_string();
    let out = l.dry(&["--skip-tier1"]);
    assert!(out.stdout.contains("changed apps: leetcode-backend, vnc-auth-session, agent-shell, vm-ops-api\n"), "{}", out.stdout);
    has_all(&out, &[format!("GET vm-ops-api /health until commit == {} (timeout 1800000 ms, every 15000 ms)", &l.sha[..8]), "agent-shell: no HTTP /health, not gated".into(), "vnc-auth-session: no HTTP /health, not gated".into()]);
    l.land(&["infra/oracle-cloud/agent-shell/b2.md"]);
    assert!(l.dry(&["--base", &first, "--skip-tier1"]).stdout.contains("changed apps: agent-shell\n"));
    assert!(l.dry(&["--base", &format!("{first}~1"), "--skip-tier1"]).stdout.contains("changed apps: leetcode-backend, vnc-auth-session, agent-shell, vm-ops-api\n"));

    // Given an unknown commit or a bad option, Then it exits 2 with the reason
    let out = l.dry_at("nope", &[]);
    assert_eq!(out.exit_code, 2);
    assert!(out.stderr.contains("verify: nope is not a commit in this checkout (git fetch origin dev?)"), "{}", out.stderr);
    for (flags, message) in [(&["--gate-timeout", "0"][..], "--gate-timeout must be a positive number of seconds"), (&["--poll", "abc"], "--poll must be a positive number of seconds"), (&["--pr", "x"], "--pr must be a PR number")] {
        let out = l.dry(flags);
        assert_eq!(out.exit_code, 2, "{flags:?}");
        assert!(out.stderr.contains(message), "{flags:?}: {}", out.stderr);
    }

    // When the plan is made in this repository itself (a depth-1 CI checkout has no HEAD~1), Then it prints without the network as well
    let real = run_wf(&["check", "--deployed", "HEAD", "--dry-run", "--base", "HEAD"], &RunOpts::default().cwd(&repo_root()));
    assert_eq!(real.exit_code, 0, "{}{}", real.stdout, real.stderr);
    assert!(real.stdout.contains("plan (dry run, no network)"));
}

/// The bucket: PUTs under /par/ are kept and GETs read them back, so a stored pointer is also what the public read link serves.
const PAR_BUCKET: &str = r#"
const http = require('node:http'); const fs = require('node:fs'); const path = require('node:path');
const dir = process.argv[1];
const s = http.createServer((q, r) => {
  if (q.url.startsWith('/par/')) {
    const chunks = [];
    q.on('data', (c) => chunks.push(c));
    q.on('end', () => {
      const file = path.join(dir, q.url.slice(5).split('?')[0].replace(/\//g, '__'));
      if (q.method === 'PUT') { fs.writeFileSync(file, Buffer.concat(chunks)); r.writeHead(200).end(); }
      else if (fs.existsSync(file)) r.writeHead(200).end(fs.readFileSync(file));
      else r.writeHead(404).end();
    });
    return;
  }
  r.writeHead(404).end();
});
s.listen(0, '127.0.0.1', () => console.log(s.address().port));
"#;

/// The landed range, the stubs (`oci` keeps objects by name, `gh` keeps a PR description, `pnpm` logs), the bucket, and
/// vm-ops-api plus the services' health behind a fake tailnet.
struct Deployed {
    _root: tempfile::TempDir,
    repo: PathBuf,
    stubs: PathBuf,
    par: PathBuf,
    server: Child,
    port: u16,
    api: MockServer,
    vm_ops: MockServer,
    tailnet: MockTailnet,
    key: String,
    sha: String,
}

impl Drop for Deployed {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

impl Deployed {
    fn new() -> Self {
        let root = tempfile::Builder::new().prefix("wf-flow-tier3-").tempdir().unwrap();
        let (repo, stubs, par) = (root.path().join("repo"), root.path().join("stubs"), root.path().join("par"));
        for dir in [&repo, &stubs, &par, &stubs.join("objs"), &stubs.join("bin")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let mut server = node_script(PAR_BUCKET).arg(&par).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
        let mut first = String::new();
        BufReader::new(server.stdout.take().unwrap()).read_line(&mut first).unwrap();
        let port: u16 = first.trim().parse().unwrap();
        let (api, vm_ops) = (MockServer::start(None), MockServer::start(None));
        let tailnet = MockTailnet::start(Upstream::Map(HashMap::from([("remote-chat-agent-prod:8787".to_string(), api.port), ("vm-ops-api-main:8080".to_string(), vm_ops.port), (format!("127.0.0.1:{port}"), port)])));
        let d = Self { _root: root, repo, stubs, par, server, port, api, vm_ops, tailnet, key: ed25519_private_key_one_line(), sha: String::new() };
        git(&d.repo, &["init", "-q", "-b", "main"]);
        put(&d.repo, "frontend/mla-tamagui/app.config.js", "module.exports = { extra: { eas: { projectId: 'proj-1' } } };\n");
        put(&d.repo, "package.json", &json!({ "name": "root", "private": true }).to_string());
        put(&d.repo, "pnpm-workspace.yaml", "packages:\n  - 'libraries/*'\n");
        put(&d.repo, "libraries/core/package.json", &json!({ "name": "@lib/core", "version": "1.0.0" }).to_string());
        git(&d.repo, &["add", "-A"]);
        git(&d.repo, &["commit", "-q", "-m", "base"]);
        let bin = d.stubs.join("bin");
        create_stub(
            &bin.join("oci"),
            &[
                "echo \"$@\" >> \"$STUB_DIR/oci.log\"",
                "op=\"$3\"; name=\"\"; file=\"\"",
                "while [ $# -gt 0 ]; do case \"$1\" in --name) name=\"$2\";; --file) file=\"$2\";; esac; shift; done",
                "key=\"$STUB_DIR/objs/$(echo \"$name\" | tr / _)\"",
                "if [ \"$op\" = put ]; then cp \"$file\" \"$key\"; exit 0; fi",
                "if [ \"$op\" = get ]; then",
                "  if [ -f \"$key\" ]; then cp \"$key\" \"$file\"; exit 0; fi",
                "  echo '{\"status\": 404, \"code\": \"ObjectNotFound\"}' >&2; exit 1",
                "fi",
                "exit 0",
            ]
            .join("\n"),
        );
        create_stub(&bin.join("gh"), &["printf \"%s\\n\" \"$@\" >> \"$STUB_DIR/gh.log\"", "case \"$*\" in *PATCH*) exit 0;; *) printf \"Old description\";; esac"].join("\n"));
        create_stub(&bin.join("pnpm"), &["if [ \"$1\" = ls ]; then printf '[{\"name\":\"@lib/core\",\"path\":\"%s/libraries/core\"}]' \"$PWD\"; exit 0; fi", "echo \"$@\" >> \"$STUB_DIR/pnpm.log\"", "exit ${PNPM_EXIT:-0}"].join("\n"));
        d
    }

    /// Lands a commit carrying a `Unit:` trailer, with tier 2's record for it already in the bucket.
    fn land(&mut self, rels: &[&str]) {
        for rel in rels {
            put(&self.repo, rel, &format!("{rel}\n"));
        }
        git(&self.repo, &["add", "-A"]);
        git(&self.repo, &["commit", "-q", "-m", "land\n\nUnit: greenfield/p4-07"]);
        self.sha = git(&self.repo, &["rev-parse", "HEAD"]);
        std::fs::write(self.stubs.join("objs").join(format!("verify_{}.tier2.json", self.sha)), json!({ "tier": 2, "status": "pass", "ms": 5, "items": [] }).to_string()).unwrap();
    }

    fn short(&self) -> &str {
        &self.sha[..7]
    }

    /// The branch's latest-update pointer as CI stores it, readable at the bucket's public base.
    fn store_pointer(&self, group: &str, commit: &str) {
        let pointer = json!({ "published": true, "branch": "dev", "commit": commit, "group": group, "link": format!("https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/{group}") });
        std::fs::write(self.par.join("verification__eas-updates__latest-dev.json"), pointer.to_string()).unwrap();
    }

    /// `wf check --deployed <sha> --base <sha>~1 <args>`, the services reporting `HEALTH_COMMIT` (the sha by default).
    fn verify(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        for log in ["pnpm.log", "gh.log", "oci.log"] {
            let _ = std::fs::remove_file(self.stubs.join(log));
        }
        let lookup = |key: &str| env.iter().find(|(k, _)| *k == key).map(|(_, v)| v.to_string());
        let health = lookup("HEALTH_COMMIT").unwrap_or_else(|| self.short().to_string());
        self.api.clear();
        self.api.routes(&[("GET /health", MockReply::json(200, json!({ "commit": health })))]);
        self.vm_ops.clear();
        self.vm_ops.routes(&[("GET /health", MockReply::json(200, answers::health(&health)))]);
        let base = format!("{}~1", self.sha);
        let mut full = vec!["check", "--deployed", self.sha.as_str(), "--base", base.as_str()];
        full.extend_from_slice(args);
        let defaults: Vec<(&str, String)> = vec![
            ("STUB_DIR", self.stubs.display().to_string()),
            ("WF_TAILNET_SOCKS", self.tailnet.address()),
            ("VM_OPS_API_AUTH_TOKEN", "tok".into()),
            ("VM_GATEWAY_REQUEST_SIGNING_PRIVATE_KEY", self.key.clone()),
            ("GOLDEN_SCREENSHOTS_BUCKET_URL_PREFIX", "https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/ns1/b/shots/o/".into()),
            ("VERIFICATION_ARTIFACTS_PAR_URL", format!("http://127.0.0.1:{}/par/", self.port)),
            ("EXPO_TOKEN", "expo-tok".into()),
            ("VERIFICATION_ARTIFACTS_READ_URL", format!("http://127.0.0.1:{}/par", self.port)),
        ];
        let mut opts = RunOpts::default().bin_dir(&self.stubs.join("bin")).cwd(&self.repo).clean_env().timeout(Duration::from_secs(180));
        for (key, value) in &defaults {
            opts = opts.env(key, &lookup(key).unwrap_or_else(|| value.clone()));
        }
        for (key, value) in env.iter().filter(|(k, _)| !defaults.iter().any(|(d, _)| d == k)) {
            opts = opts.env(key, value);
        }
        run_wf(&full, &opts)
    }

    fn log(&self, name: &str) -> String {
        std::fs::read_to_string(self.stubs.join(format!("{name}.log"))).unwrap_or_default()
    }

    fn object(&self, key: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.stubs.join("objs").join(key.replace('/', "_"))).unwrap()).unwrap()
    }
}

#[test]
fn a_deployed_commit_is_verified_tier_by_tier_reported_to_the_bucket_and_the_pr_and_a_failure_leaves_a_fix_unit() {
    let mut d = Deployed::new();

    // Given a landed frontend and backend range that is deployed, and CI's pointer naming the latest update of dev
    d.land(&["frontend/mla-tamagui/app/leetcode.tsx", "backend/remote-chat-agent/src/server.ts"]);
    d.store_pointer("g-7", "0123456789abcdef");
    // When it is verified with --publish and --pr
    let r = d.verify(&["--skip-tier1", "--publish", "--pr", "7", "--gate-timeout", "30", "--poll", "1"], &[]);
    // Then every tier reports, the report lands in the bucket with its HTML, and the PR's verify block is rewritten
    assert!(!r.stderr.contains("rejected"), "{}", r.stderr);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    let sha = d.sha.clone();
    for part in ["changed apps: remote-chat-agent, mla-tamagui", "tier 1: skip", "tier 2: pass", "tier 3: pass", &format!("report: oci verify/{sha}.json")] {
        assert!(r.stdout.contains(part), "{part}: {}", r.stdout);
    }
    assert!(r.stdout.contains(&format!("report html: https://objectstorage.eu-frankfurt-1.oraclecloud.com/n/frbwmn2jazha/b/mobile-learning-verification-artifacts/o/verification/verify/dev/{sha}/index.html")));
    let report = d.object(&format!("verify/{sha}.json"));
    assert_eq!((report["sha"].clone(), report["units"].clone()), (json!(sha), json!(["greenfield/p4-07"])));
    assert_eq!(report["tiers"].as_array().unwrap().iter().map(|t| t["status"].as_str().unwrap()).collect::<Vec<_>>(), ["skip", "pass", "pass"]);
    assert_eq!(
        report["tiers"][2]["items"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect::<Vec<_>>(),
        ["deploy gate", "remote-chat-agent:e2e", "eas update"]
    );
    assert_eq!(report["deployed"], json!([{ "app": "remote-chat-agent", "commit": d.short(), "healthOk": true }]));
    let update = json!({ "link": "https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/g-7", "deepLink": "exp://u.expo.dev/proj-1/group/g-7", "commit": "0123456789abcdef", "group": "g-7" });
    assert_eq!(report["update"], update);
    let pnpm = d.log("pnpm");
    assert!(pnpm.contains("--filter @app/remote-chat-agent run test:e2e"), "{pnpm}");
    assert!(pnpm.contains(&format!("--filter mla-tamagui run publish:eas -- --initial-route / --branch dev --message dev@{} tier 3", &sha[..8])), "{pnpm}");
    let pointer = "verification__eas-updates__latest-dev.json";
    let mut put_files: Vec<String> = std::fs::read_dir(&d.par).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != pointer).collect();
    put_files.sort();
    let mut expected = vec![format!("verification__verify__dev__{sha}__index.html"), format!("verification__verify__dev__{sha}__report.json"), "verification__verify__index.html".to_string(), "verification__verify__index.json".to_string()];
    expected.sort();
    assert_eq!(put_files, expected);
    // and CI's pointer to the latest update is read, never rewritten
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(d.par.join(pointer)).unwrap()).unwrap()["group"], json!("g-7"));
    let html = std::fs::read_to_string(d.par.join(format!("verification__verify__dev__{sha}__index.html"))).unwrap();
    for part in [format!("Verify dev@{}", &sha[..8]), "<a href=\"https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/g-7\">Open the update</a>".into(), "<code>exp://u.expo.dev/proj-1/group/g-7</code>".into(), "built from <code>01234567</code>".into()] {
        assert!(html.contains(&part), "{part}: {html}");
    }
    assert!(!html.contains("metro-runner"), "{html}");
    let gh = d.log("gh");
    for part in [
        "--method",
        "PATCH",
        "repos/:owner/:repo/pulls/7",
        "<!-- wf-verify -->",
        "Old description",
        "**Try it:** [Open the update](https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/g-7) · `exp://u.expo.dev/proj-1/group/g-7` · built from `01234567`",
        &format!("**Verify:** `dev@{}` · flow — · visual ✅ · e2e ✅", &sha[..8]),
    ] {
        assert!(gh.contains(part), "{part}: {gh}");
    }
    assert!(!gh.contains("Metro") && !gh.contains("metro-runner"), "{gh}");
    // and the commit's services were asked about their health only: nothing went to the dev bridge
    assert!(d.api.requests().iter().any(|q| q.url == "/health"), "{:?}", d.api.requests());
    assert!(d.vm_ops.requests().is_empty(), "{:?}", d.vm_ops.requests());

    // Given no pointer is stored, Then the PR block says no update is published for the commit
    std::fs::remove_file(d.par.join(pointer)).unwrap();
    let r = d.verify(&["--skip-tier1", "--pr", "7", "--gate-timeout", "30", "--poll", "1"], &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(d.log("gh").contains("**Try it:** no update published for this commit"), "{}", d.log("gh"));
    assert_eq!(d.object(&format!("verify/{sha}.json"))["update"], Value::Null);
    // and when the bucket cannot be read at all, Then it says so instead of claiming there is none
    let r = d.verify(&["--skip-tier1", "--pr", "7", "--gate-timeout", "30", "--poll", "1"], &[("VERIFICATION_ARTIFACTS_READ_URL", "http://127.0.0.1:1/par"), ("WF_EAS_POINTER_BACKOFF_MS", "1")]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(d.log("gh").contains("**Try it:** update link unavailable (cannot read http://127.0.0.1:1/par/verification/eas-updates/latest-dev.json"), "{}", d.log("gh"));

    // Given no EXPO_TOKEN for a frontend change, Then the EAS update is skipped with a note
    d.land(&["frontend/mla-tamagui/app/settings.tsx"]);
    let r = d.verify(&["--skip-tier1", "--pr", "7", "--gate-timeout", "30", "--poll", "1"], &[("EXPO_TOKEN", "")]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("note: EAS Update skipped: EXPO_TOKEN not resolvable"), "{}", r.stdout);
    assert!(!d.log("pnpm").contains("publish:eas"));
    // and nothing was exercised, so tier 3 is skipped with no item and the PR block marks e2e as not run instead of green
    assert!(r.stdout.contains("tier 3: skip"), "{}", r.stdout);
    let tier3 = d.object(&format!("verify/{}.json", d.sha))["tiers"][2].clone();
    assert_eq!((tier3["status"].clone(), tier3["items"].clone()), (json!("skip"), json!([])));
    assert!(d.log("gh").contains("· e2e —"), "{}", d.log("gh"));

    // Given a vm-ops-api change, Then the gate asks vm-ops-api's /health for its commit and nothing else
    d.land(&["infra/oracle-cloud/vm-ops-api/docker-compose.yml"]);
    let r = d.verify(&["--skip-tier1", "--gate-timeout", "30", "--poll", "1"], &[]);
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("tier 3: pass"), "{}", r.stdout);
    let asked: Vec<String> = d.vm_ops.requests().iter().map(|q| format!("{} {}", q.method, q.url)).collect();
    assert!(!asked.is_empty() && asked.iter().all(|a| a == "GET /health"), "{asked:?}");
    let r = d.verify(&["--skip-tier1", "--gate-timeout", "1", "--poll", "1"], &[("HEALTH_COMMIT", "bbbbbbb")]);
    assert!(r.stdout.contains("pending: vm-ops-api (commit bbbbbbb)"), "{}{}", r.stdout, r.stderr);
    // a failed verification appends a fix unit; this one is not the subject here
    std::fs::remove_dir_all(d.repo.join("docs/plans")).unwrap();

    // Given a landed backend change
    d.land(&["backend/remote-chat-agent/src/routes.ts"]);
    // Given a deploy that never reports the commit, Then tier 3 fails, nothing else runs, and a fix unit is appended with how to land it
    let r = d.verify(&["--skip-tier1", "--gate-timeout", "1", "--poll", "1"], &[("HEALTH_COMMIT", "aaaaaaa")]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stdout.contains("tier 3: fail"));
    assert!(Regex::new(r"x deploy gate: timeout after \d+ ms; pending: remote-chat-agent \(commit aaaaaaa\)").unwrap().is_match(&r.stdout), "{}", r.stdout);
    let short8 = d.sha[..8].to_string();
    let id = format!("fix-{short8}-01");
    assert!(r.stdout.contains(&format!("fix unit {id} appended to docs/plans/fix-{short8}/units.jsonl; land it on dev with:")) && r.stdout.contains("  git push origin dev"), "{}", r.stdout);
    let unit: Value = serde_json::from_str(std::fs::read_to_string(d.repo.join(format!("docs/plans/fix-{short8}/units.jsonl"))).unwrap().trim()).unwrap();
    assert_eq!((unit["id"].as_str(), unit["tier"].as_str(), unit["needs"].as_str()), (Some(id.as_str()), Some("e2e"), Some("deployed")));
    assert_eq!(unit["files"], json!(["backend/remote-chat-agent/src/routes.ts"]));
    assert_eq!(d.log("pnpm"), "");
    std::fs::remove_dir_all(d.repo.join("docs/plans")).unwrap();

    // Given tier 1 is not skipped for a changed workspace package, Then install and the gate run in a clean worktree and the tier is recorded
    d.land(&["libraries/core/index.ts"]);
    let r = d.verify(&["--gate-timeout", "30", "--poll", "1"], &[]);
    let pnpm = d.log("pnpm");
    assert!(pnpm.contains("install --prefer-offline --frozen-lockfile") && pnpm.contains("--filter @lib/core run --if-present typecheck"), "{pnpm}");
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("tier 1: pass"));
    assert!(d.log("oci").contains(&format!("--name verify/{}.tier1.json", d.sha)));
    assert_eq!(git(&d.repo, &["worktree", "list"]).lines().count(), 1);
    // and a failing install fails tier 1 with its exit code
    let r = d.verify(&["--gate-timeout", "30", "--poll", "1"], &[("PNPM_EXIT", "3")]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stdout.contains("tier 1: fail") && r.stdout.contains("x pnpm install --prefer-offline --frozen-lockfile: exit 3"), "{}", r.stdout);

    // Given an unknown commit or a bad PR number, Then it exits 2 before doing anything
    let r = run_wf(&["check", "--deployed", "deadbeefdeadbeef", "--skip-tier1"], &RunOpts::default().cwd(&d.repo).bin_dir(&d.stubs.join("bin")).clean_env());
    assert!(r.exit_code == 2 && r.stderr.contains("is not a commit in this checkout"), "{}", r.stderr);
    let r = d.verify(&["--skip-tier1", "--pr", "x"], &[]);
    assert!(r.exit_code == 2 && r.stderr.contains("--pr must be a PR number"), "{}", r.stderr);
}
