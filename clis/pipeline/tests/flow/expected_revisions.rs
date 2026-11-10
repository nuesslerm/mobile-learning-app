//! What the deployment verification stage tells each deployed-service test (owner, 23 Oct): the commit its service should run. A push that
//! leaves a service's image inputs alone ships no leg, so the service keeps the commit it was built from and the test must
//! expect that one; a service whose image inputs moved since, or that cannot be asked, is held to the pushed commit. vm-ops-api is a
//! fake behind a fake tailnet; `git`, `pnpm` and `nx` are stubs, and nx writes the `WF_EXPECTED_REVISIONS` its tasks would see.
use crate::support::{create_stub, run_wf, RunResult};
use wf_vmops::testing::{answers, FakeVm};
use serde_json::{json, Value};
use std::time::Duration;

const PUSHED: &str = "def4567890abcdef1234567890abcdef12345678";
/// The stage's services in the order the plan lists them (by package name), each with its package.
const SERVICES: [(&str, &str); 4] = [("leetcode-auth-cookie-capture", "@app/leetcode-auth-cookie-capture"), ("leetcode-backend", "@app/leetcode-backend"), ("remote-chat-agent", "@app/remote-chat-agent"), ("reporting-service", "@backend/reporting-service")];

struct Stage {
    vm: FakeVm,
    dir: tempfile::TempDir,
}

impl Stage {
    fn new() -> Self {
        let dir = tempfile::Builder::new().prefix("wf-expected-").tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        // a service's image inputs are hashed by the real git on this repository: the commit `abc1234` reads as HEAD (nothing moved) and
        // `bad5678` as the empty tree (which has no Dockerfile to hash)
        create_stub(
            &bin.join("git"),
            "case \"$1\" in cat-file) exit 0;; esac\n\
             n=$#; i=0; while [ $i -lt $n ]; do a=$(printf %s \"$1\" | sed -e 's#^abc1234\\(:\\|$\\)#HEAD\\1#' -e 's#^bad5678\\(:\\|$\\)#4b825dc642cb6eb9a060e54bf8d69288fbee4904\\1#'); shift; set -- \"$@\" \"$a\"; i=$((i+1)); done\n\
             exec /usr/bin/git -C \"$REAL_REPO\" \"$@\"",
        );
        create_stub(&bin.join("pnpm"), "printf \"%s\" \"$PNPM_OUT\"");
        create_stub(&bin.join("nx"), "printf '%s' \"$WF_EXPECTED_REVISIONS\" > \"$STUB_DIR/expected.json\"");
        let tests: Vec<Value> = SERVICES.iter().map(|(_, package)| json!({ "kind": "deployment-verification", "package": package })).collect();
        std::fs::write(dir.path().join("plan.json"), json!({ "sha": PUSHED, "stages": { "deployment-verification-tests": tests } }).to_string()).unwrap();
        Self { vm: FakeVm::start(), dir }
    }

    /// Runs the stage once vm-ops-api has told each service's running image (none: it cannot be asked) and pnpm says which packages
    /// changed since; returns the stage's result, its summary line, and the expectations nx's tasks saw.
    fn run(&self, running: [Option<String>; 4], changed_packages: &str) -> (RunResult, Value, Value) {
        let replies: Vec<String> = SERVICES.iter().zip(running).map(|((app, _), image)| match image {
            Some(image) => answers::reply(200, &answers::running(app, &image)),
            None => answers::reply(502, &answers::engine_down()),
        }).collect();
        self.vm.replies(&replies);
        let dir = self.dir.path().display().to_string();
        let opts = self.vm.opts().bin_dir(&self.dir.path().join("bin")).cwd(self.dir.path()).env("STUB_DIR", &dir).env("REAL_REPO", &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").display().to_string()).env("WF_TEST_REPO", &dir).env("WF_NX_BIN", &format!("{dir}/bin/nx")).env("WF_BLOB_CACHE", "off").env("PNPM_OUT", changed_packages).env("WF_DEPLOY_RETRY_MS", "10").timeout(Duration::from_secs(120));
        let r = run_wf(&["test", "run", "--stage", "deployment-verification-tests", "--plan", &format!("{dir}/plan.json")], &opts);
        let summary = r.stdout.lines().rev().find(|l| l.starts_with("{\"ok\"")).and_then(|l| serde_json::from_str(l).ok()).unwrap_or_else(|| panic!("no summary: {}{}", r.stdout, r.stderr));
        let seen = std::fs::read_to_string(self.dir.path().join("expected.json")).unwrap_or_default();
        let expected = serde_json::from_str(&seen).unwrap_or_else(|e| panic!("nx saw no WF_EXPECTED_REVISIONS ({e}): {seen:?}\n{}{}", r.stdout, r.stderr));
        (r, summary, expected)
    }
}

fn built(app: &str, sha: &str) -> Option<String> {
    Some(format!("reg/{app}:sha-{sha}"))
}

#[test]
fn the_deployment_verification_stage_expects_the_commit_each_service_should_run() {
    let s = Stage::new();
    let [auth, backend, agent, reporting] = SERVICES.map(|(app, _)| app);

    // Given a push that left every service's image inputs alone, so each still runs the commit its image was built from
    let (r, summary, expected) = s.run([built(auth, "abc1234"), built(backend, "abc1234"), built(agent, "abc1234"), built(reporting, "abc1234")], "[{\"name\":\"@cli/wf\"}]");
    // Then each test is told that commit, not the pushed one, and the stage says why
    assert_eq!(expected, json!({ auth: "abc1234", backend: "abc1234", agent: "abc1234", reporting: "abc1234" }), "{}{}", r.stdout, r.stderr);
    assert_eq!((r.exit_code, summary["expectedRevisions"][1].clone()), (0, json!({ "app": backend, "commit": "abc1234", "why": "nothing its image is built from changed since" })), "{}{}", r.stdout, r.stderr);

    // Given leetcode-backend runs a commit whose image inputs cannot be hashed, reporting-service already runs the pushed commit, and
    // remote-chat-agent cannot be asked
    let (r, summary, expected) = s.run([built(auth, "abc1234"), built(backend, "bad5678"), None, built(reporting, &PUSHED[..7])], "[]");
    // Then those three are held to the pushed commit, so one running anything older fails its test, and the untouched one keeps its own
    assert_eq!(expected, json!({ auth: "abc1234", backend: PUSHED, agent: PUSHED, reporting: PUSHED }), "{}{}", r.stdout, r.stderr);
    let why = |i: usize| summary["expectedRevisions"][i]["why"].as_str().unwrap_or_default().chars().take(44).collect::<String>();
    assert_eq!((why(1), why(2)), ("its image inputs at bad5678 cannot be hashed".to_string(), "its running commit is unknown".to_string()), "{summary}");
}
