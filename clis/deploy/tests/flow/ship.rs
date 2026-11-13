//! `wf ship`: planning what changed since each service's running commit, the status of every service, the promotion of `latest` (its `sh` script is compared byte for byte with `fixtures/deploy-scripts`), a
//! redeploy of a tag, and an EAS Update. vm-ops-api is a fake behind a fake tailnet; `git` and `pnpm` are argv-recording stubs.
use crate::support::{create_stub, fixtures_dir, repo_root, run_wf, with_stubs, MockReply, MockServer, RunOpts, RunResult};
use wf_vmops::testing::{answers, FakeVm, CI_VM_SEED, VM_SEED};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;
use pipeline::copy_closure::{copy_problems, copy_sources};
use std::time::Duration;

const BASE: &str = "http://vm-ops-api-main:8080";
const ALL_APPS: [&str; 9] = ["vm-ops-api", "remote-chat-agent", "leetcode-backend", "leetcode-auth-cookie-capture", "reporting-service", "t3-serve", "orca-serve", "vnc-auth-session", "agent-shell"];
/// The exec-script job a promotion starts, with the id the start route mints.
const JOB1: &str = "0b9f5c2e-8a1d-4c3e-9f7a-000000000001";

pub(crate) struct Ship {
    pub(crate) vm: FakeVm,
    pub(crate) dir: tempfile::TempDir,
    /// The verification bucket the plan reads the branch's latest-update pointer from: no pointer until `latest_update` serves one.
    bucket: MockServer,
}

impl Ship {
    pub(crate) fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("deploy-").tempdir().unwrap();
        std::fs::create_dir(dir.path().join("bin")).unwrap();
        // `diff --name-only` lists the files of $GIT_CHANGED under the paths asked about; `show REV:PATH` prints
        // $STUB_DIR/show/REV/PATH when it exists. Everything else that reads a tree (`show`, `ls-tree`) is the real git on this repository,
        // with the running commits the tests name read as $GIT_AS (HEAD unless set), and `hash-object` answers $STUB_HASH when set.
        create_stub(
            &dir.path().join("bin/git"),
            "echo \"$*\" >> \"$STUB_DIR/git.log\"\n\
             case \"$1\" in\n\
             cat-file) exit \"${GIT_KNOWN:-0}\";;\n\
             diff) [ -n \"$GIT_DIFF_FAIL\" ] && exit 128; while [ \"$1\" != -- ]; do shift; done; shift\n\
               for f in $GIT_CHANGED; do for p in \"$@\"; do case \"$f\" in \"$p\"|\"$p\"/*) echo \"$f\"; break;; esac; done; done | sort;;\n\
             *) if [ \"$1\" = show ]; then f=\"$STUB_DIR/show/${2%%:*}/${2#*:}\"; [ -f \"$f\" ] && { cat \"$f\"; exit 0; }; fi\n\
               if [ \"$1\" = hash-object ] && [ -n \"$STUB_HASH\" ]; then cat > /dev/null; echo \"$STUB_HASH\"; exit 0; fi\n\
               n=$#; i=0; while [ $i -lt $n ]; do b=${1%%:*}; case \"$b\" in abc123|def456|fed789|aaaaaaa|dev|main|feature/x-1.2|0123456789abcdef) if [ \"$b\" = \"$1\" ]; then a=${GIT_AS:-HEAD}; else a=${GIT_AS:-HEAD}:${1#*:}; fi;; *) a=$1;; esac; shift; set -- \"$@\" \"$a\"; i=$((i+1)); done\n\
               exec /usr/bin/git -C \"$REAL_REPO\" \"$@\";;\n\
             esac",
        );
        // `list` answers $PNPM_OUT, or $PNPM_OUT_<sha> for the commit it is asked about; for the commit $PNPM_FAIL_ON it prints
        // $PNPM_FAIL_OUT and exits 1 (pnpm 11 prints an error object on stdout when it cannot filter)
        create_stub(
            &dir.path().join("bin/pnpm"),
            "echo \"$*\" >> \"$STUB_DIR/pnpm.log\"\n\
             sha=$(echo \"$*\" | sed -n 's/.*\\.\\.\\.\\[\\([0-9a-f]*\\)\\].*/\\1/p')\n\
             if [ -n \"$sha\" ] && [ \"$sha\" = \"$PNPM_FAIL_ON\" ]; then printf \"%s\" \"$PNPM_FAIL_OUT\"; exit 1; fi\n\
             eval \"out=\\${PNPM_OUT_$sha-\\$PNPM_OUT}\"\n\
             printf \"%s\" \"$out\"",
        );
        Self { vm: FakeVm::start(), dir, bucket: MockServer::start(None) }
    }

    /// The pointer `wf ship eas --pointer` stored: the update of branch dev published from `commit`.
    fn latest_update(&self, commit: &str) {
        self.latest_pointer(MockReply::json(200, json!({ "branch": "dev", "commit": commit, "group": "g-1" })));
    }

    fn latest_pointer(&self, reply: MockReply) {
        self.bucket.routes(&[("GET /o/verification/eas-updates/latest-dev.json", reply)]);
    }

    fn opts(&self, env: &[(&str, &str)]) -> RunOpts {
        let mut opts = self.vm.opts().bin_dir(&self.dir.path().join("bin")).cwd(self.dir.path()).timeout(Duration::from_secs(120)).env("STUB_DIR", &self.dir.path().display().to_string()).env("REAL_REPO", &repo_root().display().to_string()).env("WF_DEPLOY_POLL_MS", "10").env("WF_DEPLOY_RETRY_MS", "10").env("VERIFICATION_ARTIFACTS_READ_URL", &format!("http://127.0.0.1:{}/o", self.bucket.port));
        for (key, value) in env {
            opts = opts.env(key, value);
        }
        opts
    }

    /// Runs `wf <args>` after forgetting the stub logs of the previous step.
    pub(crate) fn wf(&self, args: &[&str], env: &[(&str, &str)]) -> RunResult {
        for log in ["git.log", "pnpm.log"] {
            let _ = std::fs::remove_file(self.dir.path().join(log));
        }
        // a build names its images' hashes in the script a golden holds, so it takes a fixed one; a plan hashes the real tree
        let fixed = [("STUB_HASH", "0123456789abcdef01234567")];
        let env: Vec<(&str, &str)> = env.iter().copied().chain(fixed.into_iter().filter(|_| !args.contains(&"--changed"))).collect();
        run_wf(args, &self.opts(&env))
    }

    pub(crate) fn log(&self, name: &str) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join(name)).map(|t| t.lines().filter(|l| !l.is_empty()).map(String::from).collect()).unwrap_or_default()
    }

    fn url(&self, n: usize) -> String {
        format!("{BASE}{}", self.vm.requests()[n - 1].url)
    }

    fn method(&self, n: usize) -> String {
        self.vm.requests()[n - 1].method.clone()
    }

    fn expect_script(&self, n: usize, golden: &str) {
        let expected = std::fs::read_to_string(fixtures_dir().join("deploy-scripts").join(golden)).unwrap();
        assert_eq!(self.vm.requests()[n - 1].body, expected, "the script of call {n} against {golden}");
    }
}

fn ps(app: &str, image: &str) -> String {
    answers::reply(200, &answers::running(app, image))
}

fn health(commit: &str) -> String {
    answers::reply(200, &answers::health(commit))
}

fn started(job: &str) -> String {
    answers::reply(202, &answers::job_started(job))
}

/// The answer of a redeploy of `app` at `tag` from `git_ref` whose sibling exited 0.
fn redeployed(app: &str, tag: &str, git_ref: &str) -> Value {
    answers::redeploy(app, tag, git_ref, 0, "[redeploy] up\n", "")
}

fn ps_url(app: &str) -> String {
    format!("{BASE}/services/{app}/ps")
}

/// The services in the order the plan asks about them, each with the image its leg builds.
const PLANNED: [(&str, &str); 7] = [("remote-chat-agent", "remote-chat-agent"), ("leetcode-backend", "leetcode-backend"), ("leetcode-auth-cookie-capture", "leetcode-auth-cookie-capture"), ("reporting-service", "reporting-service"), ("t3-serve", "t3-serve"), ("orca-serve", "orca-serve"), ("vnc-auth-session", "vnc-auth-capture")];

fn running(vm_ops_commit: &str) -> Vec<String> {
    let mut replies: Vec<String> = PLANNED.iter().map(|(app, image)| ps(app, &format!("reg/{image}:sha-abc123"))).collect();
    replies.push(health(vm_ops_commit));
    // the plan asks about agent-shell last
    replies.push(ps("agent-shell", "reg/agent-shell:sha-abc123"));
    replies
}

/// The planned legs: each app with why it ships.
fn legs(stdout: &str) -> Vec<(String, Vec<String>)> {
    let plan: Value = serde_json::from_str(stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    plan["legs"].as_array().unwrap().iter().map(|l| (l["app"].as_str().unwrap().to_string(), l["why"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect())).collect()
}

fn apps(stdout: &str) -> Vec<String> {
    legs(stdout).into_iter().map(|(app, _)| app).collect()
}

fn why(stdout: &str, app: &str) -> Vec<String> {
    legs(stdout).into_iter().find(|(a, _)| a == app).unwrap_or_else(|| panic!("{app} not planned: {stdout}")).1
}

#[test]
fn the_ship_plan_deploys_the_services_whose_image_inputs_changed_since_each_one_s_running_commit() {
    let s = Ship::new();

    // Given --force, When the plan is made, Then every service is planned (no Metro server) without asking anything
    let r = s.wf(&["ship", "--changed", "--force"], &[]);
    assert_eq!((r.exit_code, apps(&r.stdout)), (0, ALL_APPS.map(String::from).to_vec()));
    assert_eq!(why(&r.stdout, "vnc-auth-session"), ["--force"]);
    assert!(s.vm.requests().is_empty());
    // and with --github-output the legs and the update's publish flag are appended to the workflow's output file
    let gh_output = s.dir.path().join("gh-output");
    let r = s.wf(&["ship", "--changed", "--force", "--github-output"], &[("GITHUB_OUTPUT", &gh_output.display().to_string())]);
    let plan: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(std::fs::read_to_string(&gh_output).unwrap(), format!("legs={}\neas_update=true\n", plan["legs"]));
    assert_eq!((plan["easUpdate"].clone(), s.bucket.requests().len()), (json!({ "publish": true, "why": ["--force"] }), 0));

    // Given every service and vm-ops-api run commits whose image inputs hash the same as HEAD's (the stub reads each as HEAD), When the plan is made
    let mut replies: Vec<String> = PLANNED.iter().zip(["abc123", "abc123", "def456", "abc123", "abc123", "abc123", "abc123", "fed789"]).map(|((app, image), sha)| ps(app, &format!("reg/{image}:sha-{sha}"))).collect();
    replies.push(health("aaaaaaa"));
    replies.push(ps("agent-shell", "reg/agent-shell:sha-abc123"));
    s.vm.replies(&replies);
    s.latest_update("abc123");
    let r = s.wf(&["ship", "--changed"], &[("PNPM_OUT", "[{\"name\":\"@app/unrelated\"}]")]);
    // Then nothing is scheduled and no update is published, only the latest update's commit is asked of pnpm, and the dev Metro's bridge is not asked
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "{\"legs\":[],\"easUpdate\":{\"publish\":false,\"why\":[]}}\n"));
    assert_eq!(s.bucket.requests().iter().map(|q| format!("{} {}", q.method, q.url)).collect::<Vec<_>>(), ["GET /o/verification/eas-updates/latest-dev.json"]);
    assert_eq!(s.log("pnpm.log"), ["--filter ...[abc123] --changed-files-ignore-pattern=**/__tests__/** --changed-files-ignore-pattern=**/*.test.* --changed-files-ignore-pattern=**/*.md list --depth -1 --json"]);
    assert_eq!(s.url(PLANNED.len() + 1), format!("{BASE}/health"), "the double answers what the plan asks");
    assert!(s.vm.requests().iter().all(|q| !q.url.contains("dev-bridge") && !q.url.contains("metro-runner")), "{:?}", s.vm.requests().iter().map(|q| q.url.clone()).collect::<Vec<_>>());
    let signed = &s.vm.requests()[0];
    s.vm.assert_signed(signed, "GET", &signed.url, "");

    // Given an unreachable service and a running commit the checkout cannot hash (the empty tree has no Dockerfile)
    let mut replies = running("abc123");
    replies[2] = answers::reply(502, &answers::engine_down());
    s.vm.replies(&replies);
    let r = s.wf(&["ship", "--changed"], &[("PNPM_OUT", "[]"), ("GIT_AS", "4b825dc642cb6eb9a060e54bf8d69288fbee4904")]);
    // Then each is planned with why, the unreachable one with a warning
    assert_eq!(apps(&r.stdout), ALL_APPS.map(String::from).to_vec());
    assert!(why(&r.stdout, "leetcode-backend")[0].starts_with("its image inputs at abc123 cannot be hashed:"), "{}", r.stdout);
    assert_eq!(why(&r.stdout, "leetcode-auth-cookie-capture"), ["its running commit is unknown"]);
    assert!(r.stderr.contains("wf: leetcode-auth-cookie-capture status unavailable, planning it as changed:"), "{}", r.stderr);
    // and the desktop image copies exactly the cargo closure of its listener and the registry the secrets crate embeds, not the rest of wf
    let root = repo_root();
    assert_eq!(copy_problems(&root, "clis/vnc-auth-capture/Dockerfile", &["clis/vnc-auth-capture/listener"], &["clis/secrets/src/data/registry.json"]).unwrap(), Vec::<String>::new());

    // Given running commits the checkout does not know, Then every service is planned as changed without hashing or asking pnpm
    s.vm.replies(&running("abc123"));
    let r = s.wf(&["ship", "--changed"], &[("GIT_KNOWN", "1"), ("PNPM_OUT", "[]")]);
    assert_eq!(apps(&r.stdout), ALL_APPS.map(String::from).to_vec());
    assert_eq!(why(&r.stdout, "leetcode-backend"), ["its running commit abc123 is not in this checkout"]);
    assert!(s.log("pnpm.log").is_empty() && !s.log("git.log").iter().any(|l| l.starts_with("ls-tree")), "{:?}", s.log("git.log"));

    // Given the pnpm lockfile moved only a package nobody bundles, Then nothing ships and no update is published
    let show = |rev: &str, path: &str, text: &str| {
        let file = s.dir.path().join("show").join(rev).join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    };
    let pnpm_lock = |sonner: &str, vitest: &str| {
        format!(
            "lockfileVersion: '9.0'\nimporters:\n  frontend/mla-tamagui:\n    dependencies:\n      sonner-native:\n        specifier: ^0.21\n        version: 0.21.0(patch_hash={sonner})\n  clis/wf:\n    devDependencies:\n      vitest:\n        specifier: ^3\n        version: {vitest}\nsnapshots:\n  sonner-native@0.21.0(patch_hash={sonner}): {{}}\n  vitest@{vitest}: {{}}\n"
        )
    };
    show("HEAD", "pnpm-lock.yaml", &pnpm_lock("aaa", "3.1.0"));
    show("aaaaaaa", "pnpm-lock.yaml", &pnpm_lock("aaa", "3.0.0"));
    s.latest_update("aaaaaaa");
    s.vm.replies(&running("aaaaaaa"));
    let r = s.wf(&["ship", "--changed"], &[("PNPM_OUT", "[]"), ("GIT_CHANGED", "pnpm-lock.yaml")]);
    let plan: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!((r.exit_code, plan["legs"].clone(), plan["easUpdate"].clone()), (0, json!([]), json!({ "publish": false, "why": [] })), "{}{}", r.stdout, r.stderr);

    // Given it re-patched a package the app bundles, Then the update is published with the dependency that moved
    show("aaaaaaa", "pnpm-lock.yaml", &pnpm_lock("bbb", "3.1.0"));
    s.vm.replies(&running("aaaaaaa"));
    let r = s.wf(&["ship", "--changed"], &[("PNPM_OUT", "[]"), ("GIT_CHANGED", "pnpm-lock.yaml")]);
    let plan: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(plan["easUpdate"], json!({ "publish": true, "why": ["1 of its locked dependencies moved: sonner-native"] }), "{plan}");

    // Given --since, When the plan is made, Then that commit stands in for every running one and vm-ops-api is not asked
    s.vm.replies::<&str>(&[]);
    let r = s.wf(&["ship", "--changed", "--since", "abc123"], &[("PNPM_OUT", "[]")]);
    assert_eq!((r.exit_code, apps(&r.stdout)), (0, Vec::<String>::new()), "{}", r.stderr);
    assert_eq!(s.log("pnpm.log"), ["--filter ...[abc123] --changed-files-ignore-pattern=**/__tests__/** --changed-files-ignore-pattern=**/*.test.* --changed-files-ignore-pattern=**/*.md list --depth -1 --json"]);
    assert!(s.vm.requests().is_empty(), "{:?}", s.vm.requests().iter().map(|r| r.url.clone()).collect::<Vec<_>>());
}

#[test]
fn a_service_with_no_running_container_is_planned_whatever_changed() {
    let s = Ship::new();
    s.latest_update("abc123");

    // Given every service at the current commit, but remote-chat-agent's containers only created (a redeploy died half-way)
    let created = [("remote-chat-agent", "created", "Created", "reg/remote-chat-agent:sha-abc123"), ("remote-chat-agent-tailscale", "created", "Created", "tailscale/tailscale:v1.102.2")];
    let mut replies = running("abc123");
    replies[0] = answers::reply(200, &answers::ps("remote-chat-agent", &created));
    s.vm.replies(&replies);
    // When the plan is made with nothing changed
    let r = s.wf(&["ship", "--changed"], &[("PNPM_OUT", "[]")]);
    // Then only that service ships, because its tag names an image nothing runs
    assert_eq!((r.exit_code, apps(&r.stdout)), (0, vec!["remote-chat-agent".to_string()]), "{}{}", r.stdout, r.stderr);
    assert_eq!(why(&r.stdout, "remote-chat-agent"), ["its running commit is unknown"]);
    assert!(r.stderr.contains("wf: remote-chat-agent has no running container, planning it as changed"), "{}", r.stderr);

    // and once its own container runs again at that commit, nothing ships
    replies[0] = ps("remote-chat-agent", "reg/remote-chat-agent:sha-abc123");
    s.vm.replies(&replies);
    let r = s.wf(&["ship", "--changed"], &[("PNPM_OUT", "[]")]);
    assert_eq!((r.exit_code, apps(&r.stdout)), (0, Vec::<String>::new()), "{}{}", r.stdout, r.stderr);
}

#[test]
fn the_update_is_published_whenever_the_app_moved_since_the_pointer_whatever_the_services_run() {
    let s = Ship::new();
    let plan = |s: &Ship, env: &[(&str, &str)]| -> (RunResult, Value) {
        s.vm.replies(&running("aaaaaaa"));
        let r = s.wf(&["ship", "--changed"], env);
        let v = serde_json::from_str(r.stdout.trim()).unwrap_or(Value::Null);
        (r, v)
    };
    let behind = "[{\"name\":\"mla-tamagui\"}]";
    let publish = |v: &Value| (v["legs"].clone(), v["easUpdate"]["publish"].clone());

    // Given every service current, and a pointer at a commit the app changed since (a publish that failed on an earlier push)
    s.latest_update("bad1234");
    let (r, v) = plan(&s, &[("PNPM_OUT", "[]"), ("PNPM_OUT_bad1234", behind)]);
    // Then the update is published for the pointer's commit and no leg ships: what the services run does not decide it
    assert_eq!((r.exit_code, publish(&v)), (0, (json!([]), json!(true))), "{}{}", r.stdout, r.stderr);
    assert_eq!(v["easUpdate"]["why"], json!(["mla-tamagui changed since bad1234"]));
    assert!(s.log("pnpm.log").iter().any(|l| l.starts_with("--filter ...[bad1234] ")), "{:?}", s.log("pnpm.log"));
    // and a pointer at a commit the app did not change since is current, whatever the app changed since the commit vm-ops-api runs: nothing ships
    let (_, v) = plan(&s, &[("PNPM_OUT", "[]"), ("PNPM_OUT_aaaaaaa", behind)]);
    assert_eq!((apps(&v.to_string()), v["easUpdate"]["publish"].clone()), (Vec::<String>::new(), json!(false)), "{v}");

    // Given a pointer the plan cannot read: none yet, not JSON, and one that names no commit
    for (what, reply) in [("no pointer yet", MockReply::json(404, json!({}))), ("a body that is no JSON", MockReply::text(200, "<html>")), ("a pointer with no commit", MockReply::json(200, json!({ "group": "g-1" })))] {
        s.latest_pointer(reply);
        let (r, v) = plan(&s, &[("PNPM_OUT", "[]")]);
        // Then the update is published (the tests refuse to guess too, and nothing else would ever repair it), and the plan goes on
        assert_eq!((r.exit_code, publish(&v)), (0, (json!([]), json!(true))), "{what}: {}{}", r.stdout, r.stderr);
        assert!(v["easUpdate"]["why"][0].as_str().unwrap().starts_with("the commit of the latest update is unknown:"), "{what}: {v}");
    }
    // and a bucket that fails once is asked again, one that is down is asked three times and a missing pointer once
    s.bucket.clear();
    s.bucket.queue(vec![MockReply::json(503, json!({})), MockReply::json(200, json!({ "branch": "dev", "commit": "abc123", "group": "g-1" }))], false);
    let (_, v) = plan(&s, &[("PNPM_OUT", "[]"), ("WF_EAS_POINTER_BACKOFF_MS", "10")]);
    assert_eq!((s.bucket.requests().len(), v["easUpdate"]["publish"].clone()), (2, json!(false)), "{v}");
    s.bucket.clear();
    s.bucket.queue(vec![MockReply::json(503, json!({})); 3], false);
    let (_, v) = plan(&s, &[("PNPM_OUT", "[]"), ("WF_EAS_POINTER_BACKOFF_MS", "10")]);
    assert_eq!((s.bucket.requests().len(), v["easUpdate"]["publish"].clone()), (3, json!(true)), "{v}");
    assert!(v["easUpdate"]["why"][0].as_str().unwrap().contains("(3 tries)"), "{v}");
    s.bucket.queue(vec![], false);
    s.bucket.clear();
    s.latest_pointer(MockReply::json(404, json!({})));
    plan(&s, &[("PNPM_OUT", "[]")]);
    assert_eq!(s.bucket.requests().len(), 1);

    // Given pnpm cannot list what changed since the pointer's commit (it prints an error object and exits 1)
    s.latest_update("bad1234");
    let (r, v) = plan(&s, &[("PNPM_OUT", "[]"), ("PNPM_FAIL_ON", "bad1234"), ("PNPM_FAIL_OUT", "{\"error\":{\"code\":\"ERR_PNPM_FILTER_CHANGED\"}}")]);
    // Then that is no "nothing changed": the update is published, saying pnpm could not tell, and the legs are still planned
    assert_eq!((r.exit_code, publish(&v)), (0, (json!([]), json!(true))), "{}{}", r.stdout, r.stderr);
    let why = v["easUpdate"]["why"][0].as_str().unwrap();
    assert!(why.starts_with("cannot tell whether the app changed since the latest update (bad1234): pnpm exited 1 listing the packages changed since bad1234: {\"error\":{\"code\":\"ERR_PNPM_FILTER_CHANGED\"") , "{why}");

    // Given a pointer whose update baked in tailnet names, and a tailnet that answers another name for one
    let baked = |hosts: Value| MockReply::json(200, json!({ "branch": "dev", "commit": "abc123", "group": "g-1", "hosts": hosts }));
    s.latest_pointer(baked(json!(["leetcode-backend-prod-1", "remote-chat-agent-prod", "u.expo.dev"])));
    let (r, v) = plan(&s, &[("PNPM_OUT", "[]"), ("WF_TEST_TAILNET_NAMES", "leetcode-backend-prod,remote-chat-agent-prod")]);
    // Then the update is published for that, naming both names, though nothing in the app changed
    assert_eq!((r.exit_code, publish(&v), v["easUpdate"]["why"].clone()), (0, (json!([]), json!(true)), json!(["the tailnet name leetcode-backend-prod-1 the update baked in is now leetcode-backend-prod"])), "{}{}", r.stdout, r.stderr);
    // and where the tailnet answers the baked names, or answers nothing, it is current
    for names in ["leetcode-backend-prod-1,remote-chat-agent-prod", ""] {
        let (_, v) = plan(&s, &[("PNPM_OUT", "[]"), ("WF_TEST_TAILNET_NAMES", names)]);
        assert_eq!(v["easUpdate"], json!({ "publish": false, "why": [] }), "{names:?}");
    }
    // and --since stands in for the pointer: the bucket and the tailnet are not asked
    s.bucket.clear();
    s.vm.replies::<&str>(&[]);
    let r = s.wf(&["ship", "--changed", "--since", "abc123"], &[("PNPM_OUT", "[]"), ("WF_TEST_TAILNET_NAMES", "leetcode-backend-prod")]);
    assert_eq!((r.exit_code, s.bucket.requests().len()), (0, 0), "{}{}", r.stdout, r.stderr);
}

#[test]
fn ship_status_reports_every_service_and_vm_ops_api_and_retire_removes_an_app() {
    let s = Ship::new();

    // Given one running app and one vm-ops-api does not know, When their status is asked for
    let (status, history) = (answers::health("abc1234"), answers::redeploy_status("done", json!([])));
    let listed = answers::ps("leetcode-backend", &[("leetcode-backend", "running", "Up 2 hours", "r/lb:sha-1"), ("leetcode-backend-tailscale", "exited", "Exited (0) 1 hour ago", "tailscale/tailscale:v1")]);
    s.vm.replies(&[answers::reply(200, &listed), answers::reply(404, &answers::unknown_app()), answers::reply(200, &status), answers::reply(200, &history)]);
    let r = s.wf(&["ship", "--status", "leetcode-backend", "broken"], &[]);
    // Then health, the redeploy state and each app print, the failure as an error entry
    assert_eq!(r.exit_code, 0);
    let out: Value = serde_json::from_str(&r.stdout).unwrap();
    assert_eq!(
        out,
        json!({
            "health": status,
            "redeploy": history,
            "services": {
                "leetcode-backend": listed["containers"],
                "broken": { "error": format!("vm-ops-api /services/broken/ps returned HTTP 404: {}", answers::unknown_app()) },
            },
        })
    );
    assert_eq!(out.as_object().unwrap().keys().collect::<Vec<_>>(), ["health", "redeploy", "services"]);
    assert_eq!([s.url(1), s.url(2), s.url(3), s.url(4)], [ps_url("leetcode-backend"), ps_url("broken"), format!("{BASE}/health"), format!("{BASE}/redeploy/status")]);

    // Given no app names, Then every known app is asked about, in order, then health and the redeploy state, and nothing is asked
    // of the Metro (its container is no app of the sweep, and the dev-bridge is not read)
    let apps = ["leetcode-backend", "remote-chat-agent", "leetcode-auth-cookie-capture", "agent-shell", "vnc-auth-session", "reporting-service", "t3-serve", "orca-serve"];
    let mut replies: Vec<String> = apps.iter().map(|app| answers::reply(200, &answers::ps(app, &[]))).collect();
    replies.extend([answers::reply(200, &status), answers::reply(200, &history)]);
    s.vm.replies(&replies);
    let r = s.wf(&["ship", "--status"], &[]);
    let asked: Vec<String> = (1..=apps.len()).map(|i| s.url(i)).collect();
    assert_eq!(asked, apps.map(ps_url).to_vec());
    assert_eq!([s.url(apps.len() + 1), s.url(apps.len() + 2)], [format!("{BASE}/health"), format!("{BASE}/redeploy/status")], "{}", r.stderr);
    assert!(s.vm.requests().iter().all(|q| !q.url.contains("dev-bridge") && !q.url.contains("metro-runner")), "{:?}", s.vm.requests().iter().map(|q| q.url.clone()).collect::<Vec<_>>());
    let keys: Vec<String> = serde_json::from_str::<Value>(&r.stdout).unwrap()["services"].as_object().unwrap().keys().cloned().collect();
    assert!(!keys.iter().any(|k| k == "mla-tamagui" || k == "metro-runner"), "{keys:?}");

    // Given the old Metro name asked for, Then it is only an app vm-ops-api may not know: one ps, no special reading of the dev trunk
    s.vm.replies(&[answers::reply(404, &answers::unknown_app()), answers::reply(200, &status), answers::reply(200, &history)]);
    let r = s.wf(&["ship", "--status", "mla-tamagui"], &[]);
    assert_eq!(serde_json::from_str::<Value>(&r.stdout).unwrap()["services"]["mla-tamagui"], json!({ "error": format!("vm-ops-api /services/mla-tamagui/ps returned HTTP 404: {}", answers::unknown_app()) }));
    assert_eq!(s.url(1), ps_url("mla-tamagui"));
    assert!(s.vm.requests().iter().all(|q| !q.url.contains("dev-bridge")));

    // Given vm-ops-api goes away before its health check answers (a swap in flight), Then the whole command fails
    s.vm.replies(&[answers::reply(200, &answers::ps("reporting-service", &[])), "drop".to_string()]);
    let r = s.wf(&["ship", "--status", "reporting-service"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("wf: vm-ops-api request to /health failed"), "{}", r.stderr);

    // Given the redeploy log cannot be read (its sibling cannot pull the image), Then the command fails with the whole
    // status body, the log, its stderr and the history kept
    let mut unreadable = answers::redeploy_status("", json!([{ "key": "service-agent-shell", "outcome": "redeploy.failed", "exitCode": 1 }]));
    unreadable["stderr"] = json!("Unable to find image 'ns/vm-ops-api:sha-1234567' locally\n");
    s.vm.replies(&[answers::reply(200, &answers::ps("reporting-service", &[])), answers::reply(200, &status), answers::reply(502, &unreadable)]);
    let r = s.wf(&["ship", "--status", "reporting-service"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains(&format!("wf: vm-ops-api /redeploy/status returned HTTP 502: {unreadable}")), "{}", r.stderr);

    // When an app is retired, Then its retire route is posted and the reply printed
    let retired = answers::retire("vm-gateway", 0, "[retire] vm-gateway down\n", "");
    s.vm.replies(&[answers::reply(200, &retired)]);
    let r = s.wf(&["ship", "retire", "vm-gateway"], &[]);
    assert_eq!((r.exit_code, r.stdout.clone()), (0, format!("{}\n", serde_json::to_string_pretty(&retired).unwrap())));
    assert_eq!((s.method(1), s.url(1)), ("POST".to_string(), format!("{BASE}/services/vm-gateway/retire")));
}

#[test]
fn the_retired_metro_server_is_no_app_to_ship_and_its_flag_and_help_are_gone() {
    let s = Ship::new();

    // When the Metro server's old name is shipped in each form the deploy leg and an agent use
    let forms: [&[&str]; 2] = [
        &["ship", "mla-tamagui", "--ref", "0123456789abcdef", "--wait-serving"],
        &["ship", "mla-tamagui", "--ref", "dev", "--redeploy-only"],
    ];
    for args in forms {
        let r = s.wf(args, &[]);
        // Then it says the app is no service and what publishes it, and nothing was sent to vm-ops-api (nor the dev-bridge)
        assert_eq!(r.exit_code, 1, "{args:?}: {}{}", r.stdout, r.stderr);
        assert!(
            r.stderr.contains("mla-tamagui is no service: CI's eas-update job publishes its EAS Update; by hand, `wf ship eas --initial-route <route>`"),
            "{args:?}: {}",
            r.stderr
        );
        assert!(s.vm.requests().is_empty(), "{args:?}: {:?}", s.vm.requests().iter().map(|q| q.url.clone()).collect::<Vec<_>>());
    }

    // When a real service is shipped with the same flags, Then it still reaches vm-ops-api (the refusal is the name's, not the flags')
    s.vm.replies(&[answers::reply(404, &answers::unknown_app())]);
    let r = s.wf(&["ship", "leetcode-backend", "--ref", "dev", "--redeploy-only"], &[]);
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert_eq!((s.method(1), s.url(1)), ("POST".to_string(), format!("{BASE}/services/leetcode-backend/redeploy?tag=latest&ref=dev")));
    assert!(!r.stderr.contains("is no service"), "{}", r.stderr);

    // When the flag that wiped Metro's caches is given to any app, and the help is read
    let flag = s.wf(&["ship", "leetcode-backend", "--ref", "dev", "--clear-cache"], &[]);
    let help = s.wf(&["ship", "--help"], &[]);
    // Then the flag is refused as an unknown argument, and the help names neither it nor a Metro server
    assert_eq!(flag.exit_code, 2, "{}{}", flag.stdout, flag.stderr);
    assert!(flag.stderr.contains("unexpected argument '--clear-cache'"), "{}", flag.stderr);
    let text = help.stdout.to_lowercase();
    assert!(help.exit_code == 0 && text.contains("--wait-serving") && !text.contains("metro") && !text.contains("clear-cache"), "{}", help.stdout);
}

fn done(tag: &str, exit: i64) -> String {
    answers::reply(200, &answers::job(JOB1, Some(exit), &format!("[vm-build] building\n[vm-build] done tag={tag}\n")))
}

fn building(output: &str) -> String {
    answers::reply(200, &answers::job(JOB1, None, output))
}

#[test]
fn a_redeploy_of_a_pushed_tag_carries_wipe_state_and_recreate_all_and_a_ship_that_would_build_is_refused() {
    let s = Ship::new();

    // Given --wipe-state or --recreate-all, When a pushed tag is redeployed, Then the redeploy carries it
    for (flag, query) in [("--wipe-state", "leetcode-backend/redeploy?tag=sha-abc&ref=dev&wipeState=1"), ("--recreate-all", "remote-chat-agent/redeploy?tag=sha-abc&ref=dev&recreate=all")] {
        let app = query.split('/').next().unwrap();
        s.vm.replies_then_last(&[answers::reply(200, &redeployed(app, "sha-abc", "dev"))]);
        let r = s.wf(&["ship", app, "--ref", "dev", "--redeploy-only", "--tag", "sha-abc", flag], &[]);
        assert_eq!(r.exit_code, 0, "{}", r.stderr);
        assert_eq!(s.url(1), format!("{BASE}/services/{query}"));
    }

    // Given an app with an image and no --redeploy-only, Then wf ship refuses to build, names the CI path and the redeploy of a pushed tag, and sends nothing
    let s = Ship::new();
    s.vm.replies::<&str>(&[]);
    for app in ["leetcode-backend", "vm-ops-api"] {
        let r = s.wf(&["ship", app, "--ref", "dev"], &[]);
        assert_eq!(r.exit_code, 1, "{app}: {}", r.stderr);
        assert!(r.stderr.contains("is built by CI, never by wf ship") && r.stderr.contains("--redeploy-only --tag sha-<8>"), "{app}: {}", r.stderr);
    }
    assert!(s.vm.requests().is_empty());
}

#[test]
fn the_retired_build_only_flag_is_refused_as_an_unknown_option_that_names_the_ci_build_and_the_redeploy_of_its_tag() {
    let s = Ship::new();
    s.vm.replies::<&str>(&[]);

    // When the old flag is given, with or without an app, a ref, a tag or the flags that went with it
    let forms: [&[&str]; 4] = [
        &["ship", "leetcode-backend", "--ref", "dev", "--build-only"],
        &["ship", "remote-chat-agent", "leetcode-backend", "--ref", "dev", "--build-only", "--tag", "t1"],
        &["ship", "vm-ops-api-rs", "watchdog", "--ref", "dev", "--build-only", "--no-wait", "--tag", "t1"],
        &["ship", "--build-only", "mla-tamagui"],
    ];
    for args in forms {
        let r = s.wf(args, &[]);

        // Then it exits 1 saying the option is unknown and what replaced it, and nothing was sent to vm-ops-api
        assert_eq!(r.exit_code, 1, "{args:?}: {}", r.stderr);
        assert!(r.stderr.contains("unknown option --build-only") && r.stderr.contains("wf ship build <app> --tag <tag>") && r.stderr.contains("--redeploy-only --tag <tag>"), "{args:?}: {}", r.stderr);
    }
    assert!(s.vm.requests().is_empty());

    // And the old no-wait flag, which only went with it, is an unknown argument to clap
    let r = s.wf(&["ship", "leetcode-backend", "--ref", "dev", "--redeploy-only", "--no-wait"], &[]);
    assert!(r.exit_code == 2 && r.stderr.contains("unexpected argument '--no-wait'"), "{}{}", r.stdout, r.stderr);
}

const AS_CI_JOB: [(&str, &str); 2] = [("WF_VM_CI_PORT", "8081"), ("VM_OPS_API_CI_AUTH_TOKEN", CI_VM_SEED)];

#[test]
fn a_gradual_redeploy_names_only_the_services_whose_image_inputs_changed() {
    let s = Ship::new();
    let running = |opencode_image: &str| {
        answers::reply(200, &answers::ps("remote-chat-agent", &[
            ("remote-chat-agent", "running", "Up 2 hours", "reg/remote-chat-agent:sha-abc123"),
            ("remote-chat-agent-opencode", "running", "Up 2 hours", opencode_image),
            ("remote-chat-agent-stt", "running", "Up 2 hours", "reg/remote-chat-agent-stt:sha-abc123"),
        ]))
    };
    let up = answers::reply(200, &redeployed("remote-chat-agent", "sha-def456", "def456"));
    let redeploy = ["ship", "remote-chat-agent", "--ref", "def456", "--redeploy-only", "--tag", "sha-def456"];

    // Given every image runs a commit whose inputs hash the same as the target's, When the tag is redeployed
    s.vm.replies_then_last(&[running("reg/opencode-server:sha-abc123"), up.clone()]);
    let r = s.wf(&redeploy, &[("STUB_HASH", "same")]);

    // Then no service is named (tags present and empty), so every service keeps the tag it runs
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(s.url(2), format!("{BASE}/services/remote-chat-agent/redeploy?tag=sha-def456&ref=def456&tags="));

    // Given opencode runs a tag that is no commit, so it cannot be shown unchanged, When the tag is redeployed
    s.vm.replies_then_last(&[running("reg/opencode-server:latest"), up.clone()]);
    let r = s.wf(&redeploy, &[("STUB_HASH", "same")]);

    // Then opencode alone is named: the app's own service and stt keep the tag they run
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(s.url(2), format!("{BASE}/services/remote-chat-agent/redeploy?tag=sha-def456&ref=def456&tags=opencode%3Asha-def456"));

    // Given the checkout cannot hash the running commits (the empty tree has no Dockerfile), When the tag is redeployed, Then every service moves: no tags are sent
    s.vm.replies_then_last(&[running("reg/opencode-server:sha-abc123"), up]);
    let r = s.wf(&redeploy, &[("GIT_AS", "4b825dc642cb6eb9a060e54bf8d69288fbee4904")]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(s.url(2), format!("{BASE}/services/remote-chat-agent/redeploy?tag=sha-def456&ref=def456"));
}

#[test]
fn a_ci_job_redeploys_over_the_ci_port_with_the_ci_seed_and_a_caller_without_a_ci_port_uses_the_admin_port_and_seed() {
    let s = Ship::new();
    let up = redeployed("leetcode-backend", "sha-1234567", "dev");
    let redeploy = ["ship", "leetcode-backend", "--ref", "dev", "--redeploy-only", "--tag", "sha-1234567"];
    let path = "/services/leetcode-backend/redeploy?tag=sha-1234567&ref=dev";

    // Given a CI job (WF_VM_CI_PORT and the CI seed in its environment), When it redeploys a tag
    s.vm.replies_then_last(&[answers::reply(200, &up)]);
    let r = s.wf(&redeploy, &AS_CI_JOB);
    // Then the call went to the CI port, signed with the CI seed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(s.vm.tailnet.destinations(), ["vm-ops-api-main:8081"]);
    s.vm.assert_signed_with(CI_VM_SEED, &s.vm.requests()[0], "POST", path, "");

    // Given the sandbox or agent-shell (no CI port), When the same redeploy runs
    let before = s.vm.tailnet.destinations().len();
    s.vm.replies_then_last(&[answers::reply(200, &up)]);
    let r = s.wf(&redeploy, &[]);
    // Then it went to the admin port, signed with the admin seed
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(s.vm.tailnet.destinations()[before..], ["vm-ops-api-main:8080"]);
    s.vm.assert_signed_with(VM_SEED, &s.vm.requests()[0], "POST", path, "");
}

#[test]
fn a_ci_job_asking_an_admin_only_route_keeps_to_the_admin_port_and_the_admin_seed() {
    let s = Ship::new();

    // Given a CI job, When it retires an app, a route the CI port does not serve
    s.vm.replies(&[answers::reply(200, &answers::retire("vm-gateway", 0, "gone\n", ""))]);
    let r = s.wf(&["ship", "retire", "vm-gateway"], &AS_CI_JOB);

    // Then the call is on the admin port with the admin seed, never the CI port
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(s.vm.tailnet.destinations(), ["vm-ops-api-main:8080"]);
    s.vm.assert_signed_with(VM_SEED, &s.vm.requests()[0], "POST", "/services/vm-gateway/retire", "");
}

#[test]
fn a_redeploy_of_a_tag_is_sent_once_retried_only_when_unanswered_and_a_bad_command_line_sends_nothing() {
    let s = Ship::new();

    // Given a tag, When it is redeployed from a ref, Then that tag is asked for and the reply printed
    // (remote-chat-agent runs three images, so its running containers are asked first: none are listed here, so every service moves)
    let up = redeployed("remote-chat-agent", "v1", "main");
    s.vm.replies_then_last(&[answers::reply(200, &answers::ps("remote-chat-agent", &[])), answers::reply(200, &up)]);
    let r = s.wf(&["ship", "remote-chat-agent", "--ref", "main", "--redeploy-only", "--tag", "v1"], &[]);
    assert_eq!((r.exit_code, r.stdout.clone()), (0, format!("{}\n", serde_json::to_string_pretty(&up).unwrap())));
    assert_eq!(s.url(2), format!("{BASE}/services/remote-chat-agent/redeploy?tag=v1&ref=main"));

    // Given no tag, Then latest is redeployed; Given characters that need encoding, Then they are form-encoded, and a tag
    // docker refuses is refused
    s.vm.replies_then_last(&[answers::reply(200, &redeployed("leetcode-backend", "latest", "dev"))]);
    s.wf(&["ship", "leetcode-backend", "--ref", "dev", "--redeploy-only"], &[]);
    assert_eq!(s.url(1), format!("{BASE}/services/leetcode-backend/redeploy?tag=latest&ref=dev"));
    s.vm.replies_then_last(&[answers::reply(400, &json!({ "error": "invalid tag format" }))]);
    let r = s.wf(&["ship", "leetcode-backend", "--ref", "feature/a+b", "--redeploy-only", "--tag", "v 1"], &[]);
    assert_eq!(s.url(1), format!("{BASE}/services/leetcode-backend/redeploy?tag=v+1&ref=feature%2Fa%2Bb"));
    assert!(r.exit_code == 1 && r.stderr.contains("returned HTTP 400: {\"error\":\"invalid tag format\"}"), "{}", r.stderr);

    // Given an HTTP error, Then it is final: exit 1 with the sibling's output, no retry
    let failed = answers::redeploy("leetcode-backend", "latest", "dev", 4, "[redeploy] pulling\n", "Error response from daemon: manifest unknown\n");
    s.vm.replies_then_last(&[answers::reply(502, &failed)]);
    let r = s.wf(&["ship", "leetcode-backend", "--ref", "dev", "--redeploy-only"], &[]);
    assert_eq!(r.exit_code, 1);
    let shown = "returned HTTP 502: exitCode=4\n--- script stdout ---\n[redeploy] pulling\n--- script stderr ---\nError response from daemon: manifest unknown";
    assert!(r.stderr.contains(shown), "{}", r.stderr);
    assert_eq!(s.vm.requests().len(), 1);

    // Given a request that is never answered once, Then it retries and succeeds
    s.vm.replies_then_last(&["drop".to_string(), answers::reply(200, &redeployed("leetcode-backend", "v1", "dev"))]);
    let r = s.wf(&["ship", "leetcode-backend", "--ref", "dev", "--redeploy-only", "--tag", "v1"], &[]);
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert!(r.stderr.contains("wf: redeploy request failed, retrying: vm-ops-api request to /services/leetcode-backend/redeploy?tag=v1&ref=dev failed:"), "{}", r.stderr);
    assert_eq!(s.vm.requests().len(), 2);

    // Given a command line that cannot ship, Then it exits 1 with the reason and sends nothing
    let refused: [(&[&str], &str); 2] = [
        (&["a", "b", "--ref", "dev", "--redeploy-only"], "ship takes exactly one <service>"),
        (&["nope", "--ref", "dev"], "unknown image(s) nope -- expected one of:"),
    ];
    s.vm.replies::<&str>(&[]);
    for (args, message) in refused {
        let mut full = vec!["ship"];
        full.extend_from_slice(args);
        let r = s.wf(&full, &[]);
        assert_eq!(r.exit_code, 1, "{args:?}");
        assert!(r.stderr.contains(message), "{args:?}: {}", r.stderr);
    }
    assert!(s.vm.requests().is_empty());
}

#[test]
fn promote_latest_moves_latest_in_a_vm_job_that_must_end_0_and_refuses_what_it_cannot_promote() {
    let s = Ship::new();

    // Given a job that ends 0, When vm-ops-api and its watchdog are promoted to a tag that served
    s.vm.replies_then_last(&[started(JOB1), answers::reply(200, &answers::job(JOB1, Some(0), "[vm-build] promoted tag=sha-abc1234\n"))]);
    let r = s.wf(&["ship", "--promote-latest", "vm-ops-api-rs", "watchdog", "--tag", "sha-abc1234"], &[]);
    // Then the job's output is streamed, only its script was started and polled (nothing redeployed), and the script moves each latest
    assert_eq!((r.exit_code, r.stdout.as_str()), (0, "[vm-build] promoted tag=sha-abc1234\n"), "{}", r.stderr);
    assert_eq!((s.vm.requests().len(), s.url(1), s.url(2)), (2, format!("{BASE}/services/agent-shell/exec-script"), format!("{BASE}/services/agent-shell/exec-script/{JOB1}")));
    s.expect_script(1, "promote-latest.sh");

    // Given a job that ends non-zero, Then the command fails with that exit code
    s.vm.replies_then_last(&[started(JOB1), answers::reply(200, &answers::job(JOB1, Some(3), "Error: failed to copy manifest\n"))]);
    let r = s.wf(&["ship", "--promote-latest", "vm-ops-api-rs", "--tag", "sha-abc1234"], &[]);
    assert_eq!(r.exit_code, 3);
    assert!(r.stderr.contains("wf: promoting latest failed with exit code 3 -- see output above"), "{}", r.stderr);

    // Given a command line that cannot promote, Then it exits 1 with the reason and sends nothing
    let s = Ship::new();
    let refused: [(&[&str], &str); 4] = [
        (&["vm-ops-api-rs"], "wf: --promote-latest needs --tag <tag>"),
        (&["vm-ops-api-rs", "--tag", "a b"], "Error: unsafe tag: \"a b\""),
        (&["nope", "--tag", "t1"], "wf: unknown image(s) nope -- expected one of:"),
        (&["vm-ops-api-rs", "--tag", "t1", "--redeploy-only"], "wf: --promote-latest takes no --redeploy-only"),
    ];
    s.vm.replies::<&str>(&[]);
    for (args, message) in refused {
        let mut full = vec!["ship", "--promote-latest"];
        full.extend_from_slice(args);
        let r = s.wf(&full, &[]);
        assert_eq!(r.exit_code, 1, "{args:?}");
        assert!(r.stderr.contains(message), "{args:?}: {}", r.stderr);
    }
    assert!(s.vm.requests().is_empty());
}

const PNPM_PUBLISHES: &str = "#!/bin/sh\necho \"$*\" >> \"$0.log\"\nd=frontend/mla-tamagui/dist\nmkdir -p $d/_expo/static/js/ios\n\
    echo '[{\"id\":\"u1\",\"group\":\"g-123\",\"platform\":\"ios\"},{\"id\":\"u2\",\"group\":\"g-123\",\"platform\":\"android\"}]' > $d/eas-update.json\n\
    echo '{\"version\":3}' > $d/_expo/static/js/ios/entry-abc.hbc.map\n\
    echo '{\"platform\":\"all\",\"env\":[\"EXPO_PUBLIC_APP_ENV\",\"EXPO_PUBLIC_DEVICE_LAB\"],\"hosts\":[\"leetcode-backend-prod\"],\"exportS\":61.2,\"uploadS\":48.5}' > $d/eas-update-export.json\n\
    echo \"published update abc123\"\nexit 0";
/// The script's own default: one group for iOS alone.
const PNPM_PUBLISHES_IOS: &str = "#!/bin/sh\necho \"$*\" >> \"$0.log\"\nd=frontend/mla-tamagui/dist\nmkdir -p $d\n\
    echo '[{\"id\":\"u1\",\"group\":\"g-ios\",\"platform\":\"ios\"}]' > $d/eas-update.json\necho '{\"platform\":\"ios\",\"exportS\":30.1,\"uploadS\":20.2}' > $d/eas-update-export.json\nexit 0";
const PNPM_FAILS: &str = "#!/bin/sh\necho \"$*\" >> \"$0.log\"\necho \"eas: build failed\" >&2\nexit 3";
/// `pnpm --filter <app> run publish:eas -- <args>` as pnpm runs it: the app's real publish script, in the app's directory; the script's `pnpm exec nx run <app>:export:eas` runs that target's command (`expo export`) through the npx double.
const PNPM_RUNS_THE_SCRIPT: &str = "#!/bin/sh\nif [ \"$1\" = exec ]; then exec npx expo export $EXPO_EXPORT_PLATFORM_FLAGS --output-dir dist --clear --source-maps $EXPO_EXPORT_FLAGS; fi\nf=$2; shift 5\ncd frontend/$f && exec node scripts/publish-eas-update.js \"$@\"";
/// `npx expo export` and `npx eas-cli update`, each taking a moment, with the baked-in env the export saw.
const NPX_EXPORTS_AND_UPLOADS: &str = "#!/bin/sh\necho \"$* | lab=$EXPO_PUBLIC_DEVICE_LAB backend=$EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL\" >> \"$0.log\"\n\
    case \"$1\" in\n  expo) sleep 0.3; mkdir -p dist/_expo/static/js/android; echo '{\"version\":3}' > dist/_expo/static/js/android/entry-1.hbc.map;;\n\
    eas-cli) sleep 0.2; echo '[{\"id\":\"u1\",\"group\":\"g-all\",\"platform\":\"ios\"},{\"id\":\"u2\",\"group\":\"g-all\",\"platform\":\"android\"}]';;\nesac";

struct Eas {
    /// A bucket that takes the source maps and the summaries.
    bucket: crate::support::MockServer,
    /// u.expo.dev: the group's manifest is not served at first, then it is.
    updates: crate::support::MockServer,
}

impl Eas {
    fn new() -> Self {
        let bucket = crate::support::MockServer::start(None);
        bucket.queue(vec![crate::support::MockReply::json(200, json!({}))], true);
        let updates = crate::support::MockServer::start(None);
        updates.queue(vec![crate::support::MockReply::json(404, json!({ "error": "no update" })), crate::support::MockReply::json(200, json!({}))], true);
        Eas { bucket, updates }
    }

    fn run(&self, bin: &Path, cwd: &Path, args: &[&str], token: Option<&str>, env: &[(&str, &str)]) -> (RunResult, Vec<String>) {
        let mut full = vec!["ship", "eas"];
        full.extend_from_slice(args);
        let mut opts = RunOpts::default().bin_dir(bin).cwd(cwd).clean_env().env("SECRETS_BACKEND", "macos").env("SECRETS_LOG_BETTERSTACK", "false")
            .env("VERIFICATION_ARTIFACTS_PAR_URL", &format!("http://127.0.0.1:{}/p/", self.bucket.port))
            .env("VERIFICATION_ARTIFACTS_READ_URL", "http://bucket.test/o")
            .env("WF_EAS_UPDATES_URL", &format!("http://127.0.0.1:{}", self.updates.port));
        if let Some(token) = token {
            opts = opts.env("EXPO_TOKEN", token);
        }
        for (k, v) in env {
            opts = opts.env(k, v);
        }
        let r = run_wf(&full, &opts);
        let calls: Vec<String> = std::fs::read_to_string(bin.join("pnpm.log")).map(|t| t.lines().map(String::from).collect()).unwrap_or_default();
        let _ = std::fs::remove_file(bin.join("pnpm.log"));
        (r, calls)
    }

    fn put(&self, path: &str) -> Option<Value> {
        self.bucket.requests().iter().find(|q| q.method == "PUT" && q.url == path).map(|q| serde_json::from_str(&q.body).unwrap_or(Value::Null))
    }

    fn puts(&self) -> Vec<String> {
        self.bucket.requests().iter().filter(|q| q.method == "PUT").map(|q| q.url.clone()).collect()
    }
}

fn last_json(r: &RunResult) -> Value {
    serde_json::from_str(r.stdout.lines().last().unwrap_or("")).unwrap_or_else(|_| panic!("{}{}", r.stdout, r.stderr))
}

/// The app's config as the publish reads it: the project id and runtime version the manifest is asked for with.
fn with_app_config(cwd: &Path, app: &str) -> String {
    let config = std::fs::read_to_string(repo_root().join("frontend/mla-tamagui/app.config.js")).unwrap();
    std::fs::create_dir_all(cwd.join("frontend").join(app)).unwrap();
    std::fs::write(cwd.join("frontend").join(app).join("app.config.js"), &config).unwrap();
    config
}

#[test]
fn ship_eas_publishes_the_app_as_an_eas_update_through_its_publish_script() {
    let eas = Eas::new();
    with_stubs(&[("pnpm", PNPM_PUBLISHES)], |ctx| {
        let config = with_app_config(&ctx.cwd, "mla-tamagui");
        // Given a token and every option, Then the publish script runs with them, --platform included, and the update is reported
        let (r, calls) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--branch", "feature/x", "--initial-route", "/leetcode", "--message", "try it", "--platform", "all", "--pointer"], Some("tok"), &[]);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        assert_eq!(calls, ["--filter mla-tamagui run publish:eas -- --initial-route /leetcode --branch feature/x --message try it --platform all"]);
        let summary = last_json(&r);
        assert_eq!((summary["branch"].as_str(), summary["initialRoute"].as_str(), summary["group"].as_str()), (Some("feature/x"), Some("/leetcode"), Some("g-123")), "{summary}");
        assert_eq!((summary["platform"].clone(), summary["platforms"].clone()), (json!("all"), json!(["ios", "android"])), "{summary}");
        assert_eq!(summary["link"], "https://expo.dev/accounts/nuesslerm/projects/mla-tamagui/updates/g-123");
        // and the script's export facts are in it: its times, the names it baked in and the hosts they name
        assert_eq!((summary["timing"]["exportS"].as_f64(), summary["timing"]["uploadS"].as_f64()), (Some(61.2), Some(48.5)), "{summary}");
        assert_eq!((summary["env"].clone(), summary["hosts"].clone()), (json!(["EXPO_PUBLIC_APP_ENV", "EXPO_PUBLIC_DEVICE_LAB"]), json!(["leetcode-backend-prod"])), "{summary}");
        assert!(summary["memory"]["peakRssMb"].is_number() && summary["memory"]["vm"].is_null(), "no VM sampling unless WF_VM_SAMPLE_SECS is set: {summary}");
        // and servedS is measured: the manifest a device asks for answered 404 once, then 200 a second later
        let served = summary["timing"]["servedS"].as_f64().unwrap_or_else(|| panic!("no servedS: {summary}"));
        assert!((1.0..15.0).contains(&served), "{summary}");
        assert_eq!(summary["served"]["status"], 200, "{summary}");
        let asked = eas.updates.requests();
        assert_eq!(asked.len(), 2, "{asked:?}");
        for q in &asked {
            assert_eq!((q.method.as_str(), q.url.as_str()), ("GET", "/5d273b55-a8fc-4e4a-bae7-14d1a9f7eb57/group/g-123"));
            let h = |k: &str| q.headers.get(k).cloned().unwrap_or_default();
            assert_eq!((h("expo-platform"), h("expo-protocol-version"), h("accept")), ("android".into(), "1".into(), "multipart/mixed,application/expo+json,application/json".into()));
            assert!(config.contains(&format!("runtimeVersion: '{}'", h("expo-runtime-version"))), "the runtime version comes from app.config.js: {:?}", h("expo-runtime-version"));
        }
        // and its source map is in the bucket under the update group, so a device stack maps back to file:line
        assert_eq!(summary["maps"]["uploaded"], 1, "{summary}");
        assert!(eas.put("/p/verification/eas-updates/g-123/_expo/static/js/ios/entry-abc.hbc.map").is_some(), "{:?}", eas.puts());
        // and with --pointer the summary is stored twice: under the group, and as the branch's latest update, which the e2e tests read
        for key in ["g-123/summary.json", "latest-feature-x.json"] {
            let stored = eas.put(&format!("/p/verification/eas-updates/{key}")).unwrap_or_else(|| panic!("no PUT of {key}"));
            assert_eq!((stored["group"].clone(), stored["platforms"].clone(), stored["timing"]["servedS"].as_f64()), (json!("g-123"), json!(["ios", "android"]), Some(served)), "{key}");
        }
        assert_eq!(summary["pointers"]["latest"], "http://bucket.test/o/verification/eas-updates/latest-feature-x.json", "{summary}");

        // Given no options, Then the route is the root, the branch and message have defaults, the script picks the platform,
        // and no branch pointer is written
        eas.bucket.clear();
        let (r, calls) = eas.run(&ctx.bin_dir, &ctx.cwd, &[], Some("tok"), &[]);
        assert_eq!(r.exit_code, 0);
        assert_eq!(calls.len(), 1);
        assert!(calls[0].starts_with("--filter mla-tamagui run publish:eas -- --initial-route / --branch ") && calls[0].contains(" --message ") && !calls[0].contains("--platform"), "{}", calls[0]);
        assert!(!eas.puts().iter().any(|u| u.contains("/latest-")) && last_json(&r)["pointers"]["latest"].is_null(), "{:?}", eas.puts());

        // Given --frontend, Then that package is published
        let (_, calls) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--frontend", "other-app", "--branch", "b", "--message", "m"], Some("tok"), &[]);
        assert_eq!(calls, ["--filter other-app run publish:eas -- --initial-route / --branch b --message m"]);

        // Given no token, an option ship eas does not have, or a platform EAS does not publish, Then nothing is published
        let (r, calls) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--branch", "b", "--message", "m"], None, &[]);
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("EXPO_TOKEN is not resolvable"), "{}", r.stderr);
        assert!(calls.is_empty());
        let (r, calls) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--bogus"], Some("tok"), &[]);
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("unexpected argument '--bogus'"), "{}", r.stderr);
        assert!(calls.is_empty());
        let (r, calls) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--platform", "web"], Some("tok"), &[]);
        assert_eq!(r.exit_code, 2);
        assert!(r.stderr.contains("--platform takes all, ios or android, not \"web\""), "{}", r.stderr);
        assert!(calls.is_empty());
    });
    with_stubs(&[("pnpm", PNPM_FAILS)], |ctx| {
        // Given the publish script fails, Then wf exits 1 with the tail of its output
        let (r, _) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--branch", "b", "--message", "m"], Some("tok"), &[]);
        assert_eq!(r.exit_code, 1);
        assert!(r.stderr.contains("exit 3") && r.stderr.contains("eas: build failed"), "{}", r.stderr);
    });
}

#[test]
fn a_default_publish_is_asked_for_as_its_own_platform_and_leaves_the_branch_pointer_alone() {
    let eas = Eas::new();
    with_stubs(&[("pnpm", PNPM_PUBLISHES_IOS)], |ctx| {
        with_app_config(&ctx.cwd, "mla-tamagui");
        // Given the script's default publish, one group for iOS alone, on a dev checkout
        let (r, calls) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--branch", "dev", "--message", "m"], Some("tok"), &[]);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        assert!(!calls[0].contains("--platform"), "{}", calls[0]);
        // Then the manifest is asked for as an iOS device asks (EAS has no Android update in the group)
        let summary = last_json(&r);
        assert_eq!((summary["platforms"].clone(), summary["served"]["platform"].clone(), summary["served"]["status"].clone()), (json!(["ios"]), json!("ios"), json!(200)), "{summary}");
        let platforms: BTreeSet<String> = eas.updates.requests().iter().map(|q| q.headers.get("expo-platform").cloned().unwrap_or_default()).collect();
        assert_eq!(platforms, BTreeSet::from(["ios".to_string()]));
        // And only the group's summary is stored: latest-dev.json, which the Android job loads, still names CI's last publish
        assert!(eas.put("/p/verification/eas-updates/g-ios/summary.json").is_some(), "{:?}", eas.puts());
        assert!(eas.put("/p/verification/eas-updates/latest-dev.json").is_none(), "{:?}", eas.puts());
        assert_eq!(summary["pointers"], json!({ "summary": "http://bucket.test/o/verification/eas-updates/g-ios/summary.json" }), "{summary}");
    });
}

#[test]
fn latest_prints_the_update_the_branch_pointer_names_and_publishes_nothing() {
    let s = Ship::new();
    // Given the pointer names CI's last publish of dev, and no EXPO_TOKEN or publish script anywhere
    let link = "https://expo.dev/accounts/a/projects/p/updates/g-ci";
    s.latest_pointer(MockReply::json(200, json!({ "published": true, "branch": "dev", "commit": "abc1234", "group": "g-ci", "platforms": ["ios", "android"], "link": link, "env": ["EXPO_PUBLIC_DEVICE_LAB"] })));
    // When the latest update is asked for, by default and for dev by name
    let by_default = s.wf(&["ship", "eas", "--latest"], &[]);
    let by_name = s.wf(&["ship", "eas", "--latest", "--branch", "dev"], &[]);
    // Then one JSON line carries the link to open in Expo Go with its group and commit, and nothing was built, published or stored
    let line = json!({ "branch": "dev", "commit": "abc1234", "group": "g-ci", "platforms": ["ios", "android"], "link": link });
    assert_eq!((by_default.exit_code, last_json(&by_default)), (0, line.clone()), "{}{}", by_default.stdout, by_default.stderr);
    assert_eq!((by_name.exit_code, last_json(&by_name)), (0, line), "{}{}", by_name.stdout, by_name.stderr);
    assert!(s.vm.requests().is_empty() && s.bucket.requests().iter().all(|q| q.method == "GET"), "{:?}", s.bucket.requests().iter().map(|q| format!("{} {}", q.method, q.url)).collect::<Vec<_>>());

    // When the bucket has no pointer yet
    s.latest_pointer(MockReply::json(404, json!({})));
    let none = s.wf(&["ship", "eas", "--latest"], &[]);
    // Then it fails in the same JSON shape with the reason and the next step
    let failed = last_json(&none);
    assert_eq!(none.exit_code, 1, "{}{}", none.stdout, none.stderr);
    assert!(failed["ok"] == json!(false) && failed["error"].as_str().is_some_and(|e| e.contains("HTTP 404")) && failed["next"].as_str().is_some_and(|n| n.contains("wf ship eas --initial-route")), "{failed}");

    // When a publishing option rides along, Then it is a usage error that read nothing
    let before = s.bucket.requests().len();
    let mixed = s.wf(&["ship", "eas", "--latest", "--pointer"], &[]);
    assert_eq!(mixed.exit_code, 2, "{}{}", mixed.stdout, mixed.stderr);
    assert!(mixed.stderr.contains("--latest only reads the pointer") && s.bucket.requests().len() == before, "{}", mixed.stderr);
}

#[test]
fn with_pointer_a_branch_pointer_that_was_not_stored_fails_the_publish() {
    let eas = Eas::new();
    // Given a bucket that takes the source map but answers 500 to the summaries
    eas.bucket.queue(vec![], false);
    eas.bucket.routes(&[
        ("PUT /p/verification/eas-updates/g-123/_expo/static/js/ios/entry-abc.hbc.map", crate::support::MockReply::json(200, json!({}))),
        ("PUT /p/verification/eas-updates/g-123/summary.json", crate::support::MockReply::json(500, json!({ "code": "InternalError" }))),
    ]);
    with_stubs(&[("pnpm", PNPM_PUBLISHES)], |ctx| {
        with_app_config(&ctx.cwd, "mla-tamagui");
        // When CI publishes with --pointer
        let (r, _) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--branch", "dev", "--message", "m", "--platform", "all", "--pointer"], Some("tok"), &[]);
        // Then the update is reported, but the run fails naming the stale pointer, so the Android job is not handed an older update
        assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
        let summary = last_json(&r);
        assert!(summary["group"] == "g-123" && summary["pointers"]["error"].as_str().is_some_and(|e| e.contains("500")), "{summary}");
        assert!(r.stderr.contains("verification/eas-updates/latest-dev.json still names an earlier one") && r.stderr.contains("500"), "{}", r.stderr);
        assert!(eas.put("/p/verification/eas-updates/latest-dev.json").is_none(), "{:?}", eas.puts());

        // When the same publish runs without --pointer, Then the failed summary is reported but the publish counts as done
        let (r, _) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--branch", "dev", "--message", "m", "--platform", "all"], Some("tok"), &[]);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        assert!(last_json(&r)["pointers"]["error"].is_string(), "{}", r.stdout);
    });
}

#[test]
fn an_update_eas_never_serves_is_reported_as_not_served_and_still_stored() {
    let eas = Eas::new();
    // Given u.expo.dev that never serves the group, and a 2 s wait for it
    eas.updates.queue(vec![crate::support::MockReply::json(404, json!({ "error": "no update" }))], true);
    with_stubs(&[("pnpm", PNPM_PUBLISHES)], |ctx| {
        with_app_config(&ctx.cwd, "mla-tamagui");
        let started = std::time::Instant::now();
        let (r, _) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--branch", "dev", "--message", "m", "--platform", "all", "--pointer"], Some("tok"), &[("WF_EAS_SERVED_TIMEOUT_S", "2")]);
        // Then the publish still counts, servedS is no number, and the error carries the wait and EAS's last answer
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        assert!((2.0..15.0).contains(&started.elapsed().as_secs_f64()), "{:?}", started.elapsed());
        let summary = last_json(&r);
        assert!(summary["timing"]["servedS"].is_null(), "{summary}");
        let error = summary["served"]["error"].as_str().unwrap_or_default();
        assert!(error.contains("not served within 2 s") && error.contains("HTTP 404"), "{summary}");
        // And the summaries are stored with that error, so the pointer's reader sees it too
        for key in ["g-123/summary.json", "latest-dev.json"] {
            let stored = eas.put(&format!("/p/verification/eas-updates/{key}")).unwrap_or_else(|| panic!("no PUT of {key}"));
            assert_eq!(stored["served"]["error"].as_str(), Some(error), "{key}");
        }
    });
}

#[test]
fn the_publish_script_exports_ios_and_android_for_all_with_the_env_baked_in_and_times_export_and_upload() {
    let eas = Eas::new();
    with_stubs(&[("pnpm", PNPM_RUNS_THE_SCRIPT), ("npx", NPX_EXPORTS_AND_UPLOADS)], |ctx| {
        // Given the app's real publish script, with expo and eas-cli doubled
        with_app_config(&ctx.cwd, "mla-tamagui");
        let scripts = ctx.cwd.join("frontend/mla-tamagui/scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::copy(repo_root().join("frontend/mla-tamagui/scripts/publish-eas-update.js"), scripts.join("publish-eas-update.js")).unwrap();
        // When CI publishes one group for every platform, with the device lab's env and a backend token
        let env = [("EXPO_PUBLIC_DEVICE_LAB", "1"), ("EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL", "http://leetcode-backend-prod:8789"), ("EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN", "flow-token-value")];
        let (r, _) = eas.run(&ctx.bin_dir, &ctx.cwd, &["--branch", "dev", "--message", "m", "--platform", "all", "--pointer"], Some("tok"), &env);
        assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
        // Then the export is iOS and Android (expo's own `all` adds web), with the env baked in, and the upload is one group for all
        let npx: Vec<String> = std::fs::read_to_string(ctx.bin_dir.join("npx.log")).unwrap().lines().map(String::from).collect();
        assert_eq!(npx.len(), 2, "{npx:?}");
        assert!(npx[0].starts_with("expo export --platform ios --platform android --output-dir dist --clear --source-maps"), "{}", npx[0]);
        assert!(npx[0].ends_with("| lab=1 backend=http://leetcode-backend-prod:8789"), "{}", npx[0]);
        assert!(npx[1].starts_with("eas-cli update --branch dev --message m --platform all --skip-bundler --input-dir dist"), "{}", npx[1]);
        // And the script's facts reach the summary: each phase as long as its double took, the names baked in and their hosts
        let summary = last_json(&r);
        assert_eq!((summary["group"].clone(), summary["platform"].clone(), summary["platforms"].clone()), (json!("g-all"), json!("all"), json!(["ios", "android"])), "{summary}");
        let (export, upload) = (summary["timing"]["exportS"].as_f64().unwrap_or(-1.0), summary["timing"]["uploadS"].as_f64().unwrap_or(-1.0));
        assert!((0.3..10.0).contains(&export) && (0.2..10.0).contains(&upload), "{summary}");
        let names: Vec<&str> = summary["env"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        for name in ["EXPO_PUBLIC_APP_ENV", "EXPO_PUBLIC_DEVICE_LAB", "EXPO_PUBLIC_INITIAL_ROUTE", "EXPO_PUBLIC_LEETCODE_BACKEND_BASE_URL", "EXPO_PUBLIC_LEETCODE_BACKEND_TOKEN"] {
            assert!(names.contains(&name), "{name}: {summary}");
        }
        assert_eq!(summary["hosts"], json!(["leetcode-backend-prod"]), "{summary}");
        // And no value is published: the token is in neither the line nor the stored pointer
        let stored = eas.put("/p/verification/eas-updates/latest-dev.json").expect("the dev pointer is stored");
        assert!(!r.stdout.contains("flow-token-value") && !stored.to_string().contains("flow-token-value"), "{}", r.stdout);
    });
}

const REGISTRY: &str = "clis/secrets/src/data/registry.json";

#[test]
fn every_image_that_copies_part_of_clis_copies_exactly_the_cargo_closure_of_what_it_builds() {
    // Given each Dockerfile that copies from clis/ and the crates it builds (a backend image copies no crate of clis/: the shared libraries are
    // registry dependencies), When its COPY lines are compared with the path closure of those crates, Then none lacks or adds a crate
    let root = repo_root();
    let images: [(&str, &[&str], &[&str]); 4] = [
        ("clis/orca-serve/Dockerfile", &["clis/orca-serve"], &["clis/Cargo.toml", "clis/Cargo.lock"]),
        ("clis/t3-serve/Dockerfile", &["clis/t3-serve"], &["clis/Cargo.toml", "clis/Cargo.lock"]),
        ("clis/vnc-auth-capture/Dockerfile", &["clis/vnc-auth-capture/listener"], &[REGISTRY]),
        ("infra/oracle-cloud/logger-cli/Dockerfile", &["clis/telemetry"], &[]),
    ];
    for (dockerfile, roots, extra) in images {
        assert_eq!(copy_problems(&root, dockerfile, roots, extra).unwrap(), Vec::<String>::new(), "{dockerfile}");
    }
    // and no Dockerfile that copies from clis/ is missing from the list
    let listed: BTreeSet<&str> = images.iter().map(|(d, _, _)| *d).collect();
    let found: BTreeSet<String> = ["backend", "infra", "clis"].iter().flat_map(|top| dockerfiles_copying_clis(&root.join(top), &root)).collect();
    assert_eq!(found.iter().map(String::as_str).collect::<BTreeSet<_>>(), listed);
}

#[test]
fn no_image_copies_the_source_of_a_versioned_library_except_the_one_that_builds_it() {
    // Given libraries.json and every Dockerfile, When their COPY sources are read, Then none copies a library's directory: a service takes
    // the pinned version from the registry, so a library edit with no release cannot move its image hash
    let root = repo_root();
    let catalog: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("libraries.json")).unwrap()).unwrap();
    let libraries: Vec<String> = catalog["libraries"].as_array().unwrap().iter().map(|l| l["path"].as_str().unwrap().to_string()).collect();
    assert!(!libraries.is_empty());
    let mut copying = Vec::new();
    for top in ["backend", "infra", "clis"] {
        for dockerfile in dockerfiles_under(&root.join(top), &root) {
            if dockerfile == "infra/oracle-cloud/logger-cli/Dockerfile" {
                continue;
            }
            let text = std::fs::read_to_string(root.join(&dockerfile)).unwrap();
            for line in text.lines().filter(|l| l.starts_with("COPY ") && !l.contains("--from")) {
                let words: Vec<&str> = line.split_whitespace().skip(1).filter(|w| !w.starts_with("--")).collect();
                for source in &words[..words.len().saturating_sub(1)] {
                    let source = source.trim_start_matches("./").trim_end_matches('/');
                    if libraries.iter().any(|lib| source == lib || source.starts_with(&format!("{lib}/")) || lib.starts_with(&format!("{source}/"))) {
                        copying.push(format!("{dockerfile}: {source}"));
                    }
                }
            }
        }
    }
    assert_eq!(copying, Vec::<String>::new());
}

fn dockerfiles_under(dir: &Path, root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return found };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !["node_modules", "target", ".git"].contains(&name.as_str()) {
                found.extend(dockerfiles_under(&path, root));
            }
        } else if name.starts_with("Dockerfile") {
            found.push(path.strip_prefix(root).unwrap().display().to_string());
        }
    }
    found
}

fn dockerfiles_copying_clis(dir: &Path, root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let (path, name) = (entry.path(), entry.file_name().to_string_lossy().into_owned());
        if path.is_dir() {
            if !["node_modules", "target", ".git"].contains(&name.as_str()) {
                found.extend(dockerfiles_copying_clis(&path, root));
            }
        } else if name == "Dockerfile" && copy_sources(&std::fs::read_to_string(&path).unwrap()).iter().any(|p| p.starts_with("clis/")) {
            found.push(path.strip_prefix(root).unwrap().to_string_lossy().into_owned());
        }
    }
    found
}

#[test]
fn a_dockerfile_that_lacks_a_crate_of_the_closure_or_copies_one_outside_it_is_reported() {
    // Given an image building crate app, which depends on kit, which depends on base
    let dir = tempfile::tempdir().unwrap();
    let write = |path: &str, text: &str| {
        std::fs::create_dir_all(dir.path().join(path).parent().unwrap()).unwrap();
        std::fs::write(dir.path().join(path), text).unwrap();
    };
    write("svc/app/Cargo.toml", "[package]\nname = \"app\"\n[dependencies]\nkit = { path = \"../../clis/kit\" }\n");
    write("clis/kit/Cargo.toml", "[package]\nname = \"kit\"\n[dependencies]\nbase = { path = \"../base\" }\n");
    write("clis/base/Cargo.toml", "[package]\nname = \"base\"\n");
    // When its Dockerfile copies kit and an unrelated crate but not base
    write("svc/app/Dockerfile", "COPY clis/kit ./clis/kit\nCOPY clis/other ./clis/other\nCOPY --from=planner /src /src\n");
    // Then it is told what is missing and what is extra
    let problems = copy_problems(dir.path(), "svc/app/Dockerfile", &["svc/app"], &[]).unwrap();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].contains("does not COPY clis/base") && problems[1].contains("copies clis/other"), "{problems:?}");
}

/// A commit on top of this repository's `HEAD` that appends a line to `file` and changes nothing else, made in a throwaway clone that
/// shares the object store (no checkout), so the real repository gets no new objects and no ref.
fn commit_touching(clone: &Path, file: &str) -> String {
    let run = |args: &[&str], stdin: Option<&str>| -> String {
        let mut cmd = std::process::Command::new("git");
        cmd.args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).current_dir(clone).stdin(if stdin.is_some() { std::process::Stdio::piped() } else { std::process::Stdio::null() }).stdout(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        if let Some(text) = stdin {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    let original = run(&["show", &format!("HEAD:{file}")], None);
    let blob = run(&["hash-object", "-w", "--stdin"], Some(&format!("{original}\n# a change to this file only\n")));
    run(&["read-tree", "HEAD"], None);
    run(&["update-index", "--cacheinfo", &format!("100644,{blob},{file}")], None);
    let tree = run(&["write-tree"], None);
    run(&["commit-tree", &tree, "-p", "HEAD", "-m", "a change to one file"], None)
}

#[test]
fn a_change_to_one_compose_project_moves_the_deploy_hash_of_that_app_alone() {
    // Given a throwaway clone of this repository at its HEAD (no checkout)
    let clone = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("git").args(["clone", "-q", "--shared", "--no-checkout"]).arg(repo_root()).arg(clone.path()).status().unwrap();
    assert!(status.success());
    let hashes = |rev: &str| -> Vec<(String, String)> { ALL_APPS.iter().map(|app| (app.to_string(), pipeline::image_hash::leg_hash(Some(clone.path()), rev, app).unwrap())).collect() };
    let before = hashes("HEAD");

    // When only the compose file of agent-shell changes
    let changed = commit_touching(clone.path(), "infra/oracle-cloud/agent-shell/docker-compose.prod.yml");
    let after = hashes(&changed);

    // Then agent-shell's deploy hash moves and no other app's does
    let moved: Vec<&str> = before.iter().zip(&after).filter(|(b, a)| b.1 != a.1).map(|(b, _)| b.0.as_str()).collect();
    assert_eq!(moved, ["agent-shell"]);
}

#[test]
fn a_change_to_the_tools_layer_of_agent_shell_moves_its_image_hash_and_leaves_its_sessions_and_dind_images_alone() {
    // Given a throwaway clone of this repository at its HEAD (no checkout)
    let clone = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("git").args(["clone", "-q", "--shared", "--no-checkout"]).arg(repo_root()).arg(clone.path()).status().unwrap();
    assert!(status.success());
    let images = ["agent-shell", "agent-shell-sessions", "agent-shell-dind"];
    let hashes = |rev: &str| -> Vec<String> { images.iter().map(|image| pipeline::image_hash::hash(Some(clone.path()), rev, image).unwrap()).collect() };
    let moved = |before: &[String], after: &[String]| -> Vec<&'static str> { images.iter().copied().zip(before.iter().zip(after)).filter(|(_, (b, a))| b != a).map(|(image, _)| image).collect() };
    let before = hashes("HEAD");

    // When only the tools Dockerfile changes, Then only the tools image moves
    let tools = commit_touching(clone.path(), "infra/oracle-cloud/agent-shell/Dockerfile");
    assert_eq!(moved(&before, &hashes(&tools)), ["agent-shell"]);

    // When the entrypoint the three roles share changes, Then all three move
    let entrypoint = commit_touching(clone.path(), "infra/oracle-cloud/agent-shell/entrypoint.sh");
    assert_eq!(moved(&before, &hashes(&entrypoint)), images);

    // When only the sessions Dockerfile changes (its pinned tools tag, say), Then only the sessions image moves
    let sessions = commit_touching(clone.path(), "infra/oracle-cloud/agent-shell/Dockerfile.sessions");
    assert_eq!(moved(&before, &hashes(&sessions)), ["agent-shell-sessions"]);
}

#[test]
fn an_app_serves_the_commit_its_own_image_was_built_from_when_only_a_sidecar_image_changed() {
    // Given a throwaway clone of this repository at its HEAD (no checkout)
    let clone = tempfile::tempdir().unwrap();
    let status = std::process::Command::new("git").args(["clone", "-q", "--shared", "--no-checkout"]).arg(repo_root()).arg(clone.path()).status().unwrap();
    assert!(status.success());
    let unchanged = |rev: &str, image: &str| pipeline::image_hash::unchanged_between(Some(clone.path()), "HEAD", rev, image);

    // When only the opencode image's Dockerfile changes, Then remote-chat-agent's own image is unchanged and opencode's is not
    let opencode = commit_touching(clone.path(), "backend/remote-chat-agent/opencode-server/Dockerfile");
    assert_eq!((unchanged(&opencode, "remote-chat-agent"), unchanged(&opencode, "opencode-server")), (true, false));

    // When the app's own Dockerfile changes, Then its own image is changed
    let own = commit_touching(clone.path(), "backend/remote-chat-agent/Dockerfile");
    assert!(!unchanged(&own, "remote-chat-agent"));

    // And a commit the clone does not know cannot be shown unchanged
    assert!(!unchanged("0000000000000000000000000000000000000000", "remote-chat-agent"));
}

/// The commit of this repository that `HEAD` names, which `wf ship vm-ops-api --ref HEAD` resolves to the same 40 characters.
fn head() -> String {
    let out = std::process::Command::new("git").args(["rev-parse", "HEAD"]).current_dir(repo_root()).output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// The smoke gate of an app that has one: what `POST /services/<app>/smoke` answers, what its throwaway container answers over the tailnet
/// (the double answers every tailnet request itself), and what the stop answers.
fn gate_up(app: &str) -> Vec<String> {
    let up = json!({ "app": app, "exitCode": 0, "stdout": format!("[smoke] {app} healthy on port 18999\n"), "stderr": "" });
    let stopped = json!({ "app": app, "exitCode": 0, "stdout": format!("[smoke] {app} stopped\n"), "stderr": "" });
    vec![answers::reply(200, &up), health("abc"), answers::reply(200, &stopped)]
}

/// vm-ops-api's gate: its smoke start, its signed probe, its stop.
fn self_gate_up(sha: &str, tag: &str) -> Vec<String> {
    vec![answers::reply(200, &json!({ "smoke": "up", "port": 18999, "ref": sha, "tag": tag })), health(sha), answers::reply(200, &json!({ "exitCode": 0, "stdout": "stopped\n", "stderr": "" }))]
}

/// What `POST /redeploy` answers once its smoke test passed, and what `/redeploy/status` holds after a swap that failed and was restored.
fn swapping(sha: &str, tag: &str) -> String {
    answers::reply(202, &json!({ "started": true, "swapping": sha, "tag": tag }))
}

fn status_with_failure(sha: &str) -> String {
    answers::reply(200, &json!({ "log": "pulling\nexit=1 restored=ok\n", "stderr": "", "history": [{ "key": "vm-ops-api", "ref": sha, "tag": "sha-x", "outcome": "redeploy.failed", "exitCode": 1, "restored": "ok" }] }))
}

#[test]
fn vm_ops_api_redeploys_a_tag_ci_already_pushed_without_building_and_still_proves_it_first() {
    // Given the runner pushed the tag and the service proves its image, swaps, and serves the commit
    let s = Ship::new();
    let (sha, tag) = (head(), format!("sha-{}", &head()[..8]));
    s.vm.replies_then_last(&[&self_gate_up(&sha, &tag)[..], &[swapping(&sha, &tag), health(&sha)]].concat());

    // When it is shipped with --redeploy-only at that tag
    let r = s.wf(&["ship", "vm-ops-api", "--redeploy-only", "--ref", "HEAD", "--tag", &tag, "--wait-serving"], &[]);

    // Then no build job was started: the smoke test is the first request, then the swap and /health
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!((s.method(1), s.url(1)), ("POST".to_string(), format!("{BASE}/redeploy/smoke?ref={sha}&tag={tag}")));
    assert_eq!((s.method(4), s.url(4)), ("POST".to_string(), format!("{BASE}/redeploy?ref={sha}&tag={tag}")));
    assert!(s.vm.requests().iter().all(|q| !q.url.contains("exec-script")), "{:?}", s.vm.requests().iter().map(|q| q.url.clone()).collect::<Vec<_>>());

    // and without the tag it refuses before sending anything
    let before = s.vm.requests().len();
    let r = s.wf(&["ship", "vm-ops-api", "--redeploy-only", "--ref", "HEAD"], &[]);
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("vm-ops-api --redeploy-only needs --tag <tag> that CI pushed") && s.vm.requests().len() == before, "{}", r.stderr);
}

#[test]
fn redeploy_only_with_smoke_proves_the_pushed_tag_before_the_swap_and_smoke_without_redeploy_only_is_refused() {
    // Given a service with a smoke test
    let s = Ship::new();
    s.vm.replies_then_last(&[&gate_up("remote-chat-agent")[..], &[answers::reply(200, &answers::ps("remote-chat-agent", &[])), answers::reply(200, &redeployed("remote-chat-agent", "sha-abc", "dev"))]].concat());

    // When its pushed tag is redeployed with --smoke
    let r = s.wf(&["ship", "remote-chat-agent", "--redeploy-only", "--smoke", "--ref", "dev", "--tag", "sha-abc"], &[]);

    // Then the smoke container is started for the tag, probed, stopped, and only then is the app redeployed on it
    assert_eq!(r.exit_code, 0, "{}", r.stderr);
    assert_eq!(s.url(1), format!("{BASE}/services/remote-chat-agent/smoke?tag=sha-abc&ref=dev"));
    assert_eq!(s.url(3), format!("{BASE}/services/remote-chat-agent/smoke/stop"));
    assert_eq!(s.url(5), format!("{BASE}/services/remote-chat-agent/redeploy?tag=sha-abc&ref=dev"));

    // Given a smoke test that fails, Then nothing is redeployed
    let s = Ship::new();
    s.vm.replies_then_last(&[answers::reply(502, &json!({ "error": "smoke test failed, nothing was swapped" }))]);
    let r = s.wf(&["ship", "remote-chat-agent", "--redeploy-only", "--smoke", "--ref", "dev", "--tag", "sha-abc"], &[]);
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(s.vm.requests().iter().all(|q| !q.url.contains("/redeploy")), "{:?}", s.vm.requests().iter().map(|q| q.url.clone()).collect::<Vec<_>>());

    // Given --smoke without --redeploy-only, Then it is refused
    let r = s.wf(&["ship", "remote-chat-agent", "--smoke", "--ref", "dev"], &[]);
    assert!(r.exit_code == 1 && r.stderr.contains("--smoke applies to --redeploy-only"), "{}", r.stderr);
}

#[test]
fn a_vm_ops_api_image_that_fails_its_smoke_test_fails_the_ship_with_the_log_and_nothing_waits_on_health() {
    // Given a service that refuses the pushed image with the throwaway container's log
    let s = Ship::new();
    let (sha, tag) = (head(), format!("sha-{}", &head()[..8]));
    let refused = answers::reply(502, &json!({ "error": "smoke test failed, nothing was swapped", "reason": "[smoke] vm-ops-api did not answer on port 18999\nthread 'main' panicked: no such file" }));
    s.vm.replies_then_last(&[refused]);

    // When its pushed tag is shipped and its serving awaited
    let r = s.wf(&["ship", "vm-ops-api", "--redeploy-only", "--ref", "HEAD", "--tag", &tag, "--wait-serving"], &[]);

    // Then it fails at once carrying that log, and /health was never asked
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("smoke test failed, nothing was swapped") && r.stderr.contains("panicked: no such file"), "{}", r.stderr);
    assert!(s.vm.requests().iter().all(|q| q.url != "/health"), "{:?}", s.vm.requests().iter().map(|q| q.url.clone()).collect::<Vec<_>>());
    let _ = sha;
}

#[test]
fn a_vm_ops_api_swap_that_recorded_its_failure_fails_the_ship_at_once_with_the_recorded_outcome() {
    // Given a swap that spawned, a service still on the old commit, and a failed outcome recorded for this commit, restored
    let s = Ship::new();
    let (sha, tag) = (head(), format!("sha-{}", &head()[..8]));
    s.vm.replies_then_last(&[&self_gate_up(&sha, &tag)[..], &[swapping(&sha, &tag), health("0000000000000000000000000000000000000000"), status_with_failure(&sha)]].concat());

    // When its pushed tag is shipped and its serving awaited
    let started_at = std::time::Instant::now();
    let r = s.wf(&["ship", "vm-ops-api", "--redeploy-only", "--ref", "HEAD", "--tag", &tag, "--wait-serving"], &[]);

    // Then it fails at the first round, not at the limit, naming the failure, its exit and that the old stack was restored
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains(&format!("vm-ops-api redeploy failed at {sha} (exit 1, restored ok)")) && r.stderr.contains("exit=1 restored=ok"), "{}", r.stderr);
    assert!(started_at.elapsed() < std::time::Duration::from_secs(60), "waited {:?}", started_at.elapsed());
}

#[test]
fn a_vm_job_that_goes_silent_past_its_limit_fails_with_the_job_it_left_running() {
    let s = Ship::new();

    // Given a promotion job that is still running on every poll and prints nothing new
    s.vm.replies_then_last(&[started(JOB1), building("[vm-build] promoting\n")]);

    // When the job is waited for past a 200 ms silence limit
    let r = s.wf(&["ship", "--promote-latest", "vm-ops-api-rs", "--tag", "t1"], &[("WF_VM_JOB_LIMIT_MS", "200")]);

    // Then the ship fails at the limit, naming the job that keeps running on the VM and how to read it
    assert_eq!(r.exit_code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains(&format!("VM job {JOB1} printed nothing for 0 s")) && r.stderr.contains(&format!("wf vm exec --status {JOB1}")), "{}", r.stderr);
}

#[test]
fn a_vm_job_whose_output_tail_keeps_changing_is_waited_for_however_long_it_takes() {
    let s = Ship::new();

    // Given a job whose output grows on every poll for well past a 200 ms limit (polls every 10 ms), then finishes
    // (the poll carries only a tail of the output: past a point its length stops growing while its text keeps changing)
    let mut replies = vec![started(JOB1)];
    replies.extend((1..=60).map(|n| building(&format!("{}[vm-build] copying manifest {n}\n", "x".repeat(8) ))));
    replies.push(done("t1", 0));
    s.vm.replies_then_last(&replies);

    // When the job is waited for
    let r = s.wf(&["ship", "--promote-latest", "vm-ops-api-rs", "--tag", "t1"], &[("WF_VM_JOB_LIMIT_MS", "200")]);

    // Then the limit, which is on silence, never trips and the job ends 0
    assert_eq!(r.exit_code, 0, "{}{}", r.stdout, r.stderr);
}

#[test]
fn a_smoke_container_that_does_not_answer_over_the_tailnet_is_stopped_and_nothing_is_swapped() {
    // Given a smoke test that passes inside the container, and a tailnet that never answers it
    let s = Ship::new();
    let (sha, tag) = (head(), format!("sha-{}", &head()[..8]));
    let up = answers::reply(200, &json!({ "smoke": "up", "port": 18999, "ref": sha, "tag": tag }));
    s.vm.replies_then_last(&[up, answers::reply(503, &json!({}))]);

    // When its pushed tag is shipped
    let r = s.wf(&["ship", "vm-ops-api", "--redeploy-only", "--ref", "HEAD", "--tag", &tag, "--wait-serving"], &[]);

    // Then it fails naming the tailnet, the container was told to stop, and no swap was requested
    assert_eq!(r.exit_code, 1, "{}", r.stderr);
    assert!(r.stderr.contains("did not answer over the tailnet") && r.stderr.contains("nothing was swapped"), "{}", r.stderr);
    let urls: Vec<String> = s.vm.requests().iter().map(|q| q.url.clone()).collect();
    assert!(urls.iter().any(|u| u == "/redeploy/smoke/stop") && !urls.iter().any(|u| u.starts_with("/redeploy?")), "{urls:?}");
}

#[test]
fn an_app_without_a_health_is_confirmed_serving_by_the_sha_tag_of_its_running_image() {
    let s = Ship::new();

    // Given the desktop app, which has no /health, redeployed at a commit and its container running the image tagged with that commit
    let up = redeployed("vnc-auth-session", "sha-0123456", "0123456789abcdef");
    s.vm.replies_then_last(&[answers::reply(200, &up), answers::reply(200, &answers::running("vnc-auth-session", "ns/vnc-auth-capture:sha-0123456"))]);

    // When it is shipped with --wait-serving
    let r = s.wf(&["ship", "vnc-auth-session", "--ref", "0123456789abcdef", "--redeploy-only", "--tag", "sha-0123456", "--wait-serving"], &[]);

    // Then the deploy is confirmed from the image tag, with no /health asked of the app
    assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("\"serving\":\"serving 0123456 after"), "{}", r.stdout);
    assert!(!r.stdout.contains("not confirmed"), "{}", r.stdout);
}
