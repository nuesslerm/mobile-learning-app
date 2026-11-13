//! The deploy plan's contract (owner, 18 Oct): a push ships exactly the legs whose image is built from what it changed, and
//! nothing else. The workspace is a git repository made from this repository's files; each case commits one edit on top of
//! the same base and asks `wf ship --changed --since <base>`, the real planner with real git and pnpm, which legs ship and whether
//! the push publishes an EAS Update (what the app bundles changed since the commit of the latest update, which `--since` stands in for).
use crate::support::{repo_root, run_wf, RunOpts};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The services with a Cargo workspace of their own.
const OWN_WORKSPACE: [&str; 6] = ["reporting-service", "leetcode-backend", "leetcode-auth-cookie-capture", "vm-ops-api", "watchdog", "remote-chat-agent"];

/// The services built on service-kit and the telemetry crate.
const RUST_SERVICES: [&str; 4] = ["remote-chat-agent", "leetcode-backend", "leetcode-auth-cookie-capture", "reporting-service"];

struct Repo {
    dir: tempfile::TempDir,
    base: String,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

impl Repo {
    /// The repository's tracked and new files (not the ignored ones), committed as the base.
    fn copy() -> Repo {
        let repo = repo_root();
        let dir = tempfile::Builder::new().prefix("wf-flow-legs-").tempdir().unwrap();
        let listed = Command::new("git").args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"]).current_dir(&repo).output().unwrap();
        for file in String::from_utf8_lossy(&listed.stdout).split('\0').filter(|f| !f.is_empty() && !f.starts_with("references/")) {
            let (from, to) = (repo.join(file), dir.path().join(file));
            if from.is_file() && !from.is_symlink() {
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::copy(&from, &to).unwrap();
            }
        }
        for args in [&["init", "-q", "-b", "base"][..], &["config", "user.email", "flow@test"], &["config", "user.name", "flow"], &["config", "commit.gpgsign", "false"], &["add", "-A"], &["commit", "-qm", "base", "--no-verify"]] {
            git(dir.path(), args);
        }
        let base = git(dir.path(), &["rev-parse", "HEAD"]);
        Repo { dir, base }
    }

    /// Commits `edit` on top of the base and returns the plan `wf ship --changed --since <base>` prints.
    fn plan_after(&self, edit: impl FnOnce(&Path)) -> Value {
        self.plan_with(&[], edit)
    }

    /// The same, with `extra` flags after `--since`.
    fn plan_with(&self, extra: &[&str], edit: impl FnOnce(&Path)) -> Value {
        git(self.dir.path(), &["checkout", "-q", "--detach", &self.base]);
        edit(self.dir.path());
        git(self.dir.path(), &["add", "-A"]);
        git(self.dir.path(), &["commit", "-qm", "case", "--no-verify", "--allow-empty"]);
        let mut args = vec!["ship", "--changed", "--since", self.base.as_str()];
        args.extend_from_slice(extra);
        let r = run_wf(&args, &RunOpts::default().cwd(self.dir.path()).timeout(Duration::from_secs(120)));
        assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
        serde_json::from_str(r.stdout.trim()).unwrap_or_else(|e| panic!("{e}: {}", r.stdout))
    }

    /// The legs of that plan, with why.
    fn legs_after(&self, edit: impl FnOnce(&Path)) -> Vec<(String, Vec<String>)> {
        let plan = self.plan_after(edit);
        plan["legs"].as_array().unwrap().iter().map(|l| (l["app"].as_str().unwrap().to_string(), l["why"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect())).collect()
    }
}

/// Appends a line the file's language ignores.
fn append(dir: &Path, rel: &str) {
    let file = dir.join(rel);
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    text.push_str(match Path::new(rel).extension().and_then(|e| e.to_str()) {
        Some("rs" | "ts" | "tsx" | "js") => "\n// deploy legs case\n",
        Some("lua") => "\n-- deploy legs case\n",
        _ => "\n",
    });
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, text).unwrap();
}

/// Rewrites the checksum of `krate` in a Cargo.lock: the lockfile now resolves another build of it.
fn rechecksum(dir: &Path, lock: &str, krate: &str) {
    let file = dir.join(lock);
    let text = std::fs::read_to_string(&file).unwrap();
    let at = text.find(&format!("\nname = \"{krate}\"\n")).unwrap_or_else(|| panic!("no {krate} in {lock}"));
    let sum = at + text[at..].find("checksum = \"").unwrap() + "checksum = \"".len();
    let flipped = if &text[sum..sum + 1] == "0" { "1" } else { "0" };
    std::fs::write(file, format!("{}{flipped}{}", &text[..sum], &text[sum + 1..])).unwrap();
}

fn apps(legs: &[(String, Vec<String>)]) -> Vec<&str> {
    legs.iter().map(|(app, _)| app.as_str()).collect()
}

#[test]
fn a_push_ships_exactly_the_legs_built_from_what_it_changed() {
    let repo = Repo::copy();

    // Given nothing changed since the base, Then nothing ships and no update is published
    let none = repo.plan_after(|_| {});
    assert_eq!((apps(&repo.legs_after(|_| {})), none["easUpdate"].clone()), (Vec::<&str>::new(), json!({ "publish": false, "why": [] })));

    let rust: Vec<&str> = RUST_SERVICES.to_vec();
    let watchdog_and_rust: Vec<&str> = ["vm-ops-api"].into_iter().chain(RUST_SERVICES).collect();
    // these services have a workspace of their own (backend/<service>/Cargo.toml and Cargo.lock): the shared backend workspace manifest
    // and lockfile are not an input of their images
    // the last element: whether the push publishes an EAS Update, which carries what the app bundles; no leg ships the app, and the
    // retired Metro runner's files plan nothing
    let cases: Vec<(&str, &str, Vec<&str>, bool)> = vec![
        ("an in-app feature's code", "frontend/mla-tamagui/features/tamil-writing/data/tamilCourse.ts", vec![], true),
        ("a library the app imports", "frontend/libraries/tamagui-design-system/composites/collapsing-header.tsx", vec![], true),
        ("the Metro server's compose project", "infra/oracle-cloud/metro-runner/docker-compose.prod.yml", vec![], false),
        ("one service's code", "backend/leetcode-backend/rs/src/main.rs", vec!["leetcode-backend"], false),
        ("the agent service's code", "backend/remote-chat-agent/rs/src/api/app.rs", vec!["remote-chat-agent"], false),
        // the wire contracts (libraries/agent-api) are copied crate by crate; what .dockerignore leaves out of the context is in no image
        ("the agent.threads group's code, a crate of the agent service", "backend/remote-chat-agent/group-agent/src/group.rs", vec!["remote-chat-agent"], false),
        ("the workspace groups' code, a crate of the agent service", "backend/remote-chat-agent/group-workspace/src/worktrees.rs", vec!["remote-chat-agent"], false),
        ("a wire contract crate's code, which the agent service builds with", "libraries/agent-api/exec-fs/src/lib.rs", vec!["remote-chat-agent", "t3-serve"], false),
        ("a wire contract crate's recorded exchanges, which only tests read", "libraries/agent-api/exec-fs/tests/fixtures/read_file.json", vec![], false),
        ("a wire contract crate's loader of those exchanges", "libraries/agent-api/exec-fs/src/testing.rs", vec![], false),
        ("the wire contracts' own workspace lockfile, which no image reads", "libraries/agent-api/Cargo.lock", vec![], false),
        ("the agent.threads contract's code, which the agent service builds its action table with", "libraries/agent-api/agent-api-agent/src/events.rs", vec!["remote-chat-agent", "t3-serve", "orca-serve"], false),
        ("the attachments contract's code, which the agent service, the T3 server and the Orca server build with", "libraries/agent-api/agent-api-attachments/src/lib.rs", vec!["remote-chat-agent", "t3-serve", "orca-serve"], false),
        ("the catalog contract's code, which the agent service and the Orca server build with (the T3 server only copies it for the workspace manifest)", "libraries/agent-api/agent-api-catalog/src/lib.rs", vec!["remote-chat-agent", "orca-serve"], false),
        ("the catalog group's code, a crate of the agent service", "backend/remote-chat-agent/group-catalog/src/service.rs", vec!["remote-chat-agent"], false),
        ("the voice edit DSL cassette, which the agent service's flow test replays and no image holds", "backend/remote-chat-agent/rs/tests/cassettes/voice-edit-dsl/interactions.jsonl", vec![], false),
        ("the Rust port's mimic driver, an example of the service core that the cassette test runs", "backend/remote-chat-agent/service-core/examples/mimic_voice_dsl.rs", vec![], false),
        ("the Orca mobile bundle's mimic driver, an example of map-mobile that wf-mimic replays", "clis/orca-serve/map-mobile/examples/mimic_orca_bundle.rs", vec![], false),
        ("the agent service's integration test, which replays the DSL cassette through wf-mimic and is no image's input", "backend/remote-chat-agent/rs/tests/integration.rs", vec![], false),
        ("the Orca chain's integration test of the mobile bundle replay", "clis/orca-serve/tests/integration/mimic_x2.rs", vec![], false),
        ("the Orca oracle's integration test of the dual-target replay", "clis/orca-serve/tests/integration/mimic_x6.rs", vec![], false),
        ("the Orca oracle's cassette", "clis/orca-serve/cassettes/orca-files/cassette.json", vec![], false),
        ("the voice edit DSL library the cassette pins, which the app bundles and the services never read", "frontend/libraries/voice-edit-dsl/src/validator.ts", vec![], true),
        ("the agent.threads contract's recorded exchanges", "libraries/agent-api/agent-api-agent/tests/fixtures/thread.get.json", vec![], false),
        ("the agent.threads contract's recorded events", "libraries/agent-api/agent-api-agent/tests/fixtures/thread-events.jsonl", vec![], false),
        ("the agent API client, which the T3 and Orca servers build with", "libraries/agent-api/agent-api-client/src/client.rs", vec!["t3-serve", "orca-serve"], false),
        ("the wire crate's integration test, which runs the wf-mimic binary and is no image's input", "libraries/agent-api/agent-api-proto/tests/integration.rs", vec![], false),
        ("the wire crate's example, the driver that test replays", "libraries/agent-api/agent-api-proto/examples/mimic_driver.rs", vec![], false),
        ("the test kit's code, which the agent service resolves as a dev-dependency and never builds", "libraries/agent-api/exec-testkit/src/cli.rs", vec![], false),
        ("the test kit's recordings", "libraries/agent-api/exec-testkit/cassettes/exec-recordings/interactions.jsonl", vec![], false),
        ("the test kit's manifest, which the images copy to resolve the dev-dependency", "libraries/agent-api/exec-testkit/Cargo.toml", vec!["remote-chat-agent", "t3-serve", "orca-serve"], false),
        // the shared libraries are registry dependencies: an edit with no released version is in no image, only a service's lockfile moves it
        ("a library the agent service and the desktop's listener take from the registry", "libraries/rca-core/src/file_store.rs", vec![], false),
        ("a service's crate", "backend/reporting-service/rs/src/main.rs", vec!["reporting-service"], false),
        ("the T3 server's code, which builds from the clis workspace", "clis/t3-serve/core/src/usecases/peers.rs", vec!["t3-serve"], false),
        ("the T3 server's compose project, beside its Dockerfile", "clis/t3-serve/docker-compose.prod.yml", vec!["t3-serve"], false),
        ("the Orca server's code, which builds from the clis workspace", "clis/orca-serve/map-mobile/src/lib.rs", vec!["orca-serve"], false),
        ("the Orca agent-session mapper, which the Orca image builds from the clis workspace", "clis/orca-serve/map-agent/src/fold.rs", vec!["orca-serve"], false),
        ("the Orca server's compose project, beside its Dockerfile", "clis/orca-serve/docker-compose.prod.yml", vec!["orca-serve"], false),
        ("the agent API contracts the T3 and Orca servers build with", "libraries/agent-api/agent-api-workspace/src/edit.rs", vec!["remote-chat-agent", "t3-serve", "orca-serve"], false),
        ("a blank line in a service's own workspace manifest", "backend/reporting-service/Cargo.toml", vec![], false),
        ("the kit every Rust service and the watchdog take from the registry", "backend/service-kit/src/lib.rs", vec![], false),
        // the image hash reads the part of the workspace manifest a service's crates use, not the file: a blank line moves nothing
        ("a blank line in the shared backend workspace manifest", "backend/Cargo.toml", vec![], false),
        ("the telemetry crate every Rust image takes from the registry", "clis/telemetry/src/oneshot.rs", vec![], false),
        ("the watchdog, which ships with vm-ops-api", "backend/watchdog/rs/src/main.rs", vec!["vm-ops-api"], false),
        ("vm-ops-api's compose project", "infra/oracle-cloud/vm-ops-api/docker-compose.yml", vec!["vm-ops-api"], false),
        ("vm-ops-api's code", "backend/vm-ops-api/rs/src/main.rs", vec!["vm-ops-api"], false),
        ("the dashboard vm-ops-api embeds", "backend/vm-ops-api/rs/static/opencode-log/index.html", vec!["vm-ops-api"], false),
        ("the browser driver the desktop's listener compiles", "clis/cdp/src/lib.rs", vec!["vnc-auth-session"], false),
        ("the kit the desktop's listener builds on", "clis/wf-kit/src/time.rs", vec!["t3-serve", "orca-serve", "vnc-auth-session"], false),
        ("the secrets crate the desktop's listener builds on", "clis/secrets/src/secrets/mod.rs", vec!["vnc-auth-session"], false),
        ("the VM crate the desktop's listener reaches the tailnet through", "clis/wf-vmops/src/vmops.rs", vec!["vnc-auth-session"], false),
        ("the login crate the desktop's listener is", "clis/wf-login/src/vnc/engine.rs", vec!["vnc-auth-session"], false),
        ("wf outside the listener, which no image holds", "clis/wf/src/main.rs", vec![], false),
        ("the mimic core's code, of which the Orca and T3 images copy only the manifest and stub the library, so no image rebuilds", "clis/mimic/core/src/differ.rs", vec![], false),
        ("the mimic CLI's adapters, which no image holds", "clis/mimic/src/adapters/stdio.rs", vec![], false),
        ("the mimic core's manifest, which the Orca and T3 images copy to resolve the dev-dependency of their replay tests", "clis/mimic/core/Cargo.toml", vec!["t3-serve", "orca-serve"], false),
        ("the T3 server's imported cassettes, read by its tests only", "clis/t3-serve/cassettes/t3-v0.1.6/interactions.jsonl", vec![], false),
        ("the CLIs' lockfile", "clis/Cargo.lock", vec![], false),
        ("the Nx config", "nx.json", vec![], false),
        ("an e2e test case", "e2e/editor-typing.lua", vec![], false),
        ("a feature's unit test", "frontend/mla-tamagui/features/leetcode/auth/__tests__/webBridgeClient.test.ts", vec![], false),
        ("a service's readme", "backend/leetcode-backend/README.md", vec![], false),
        ("a doc", "docs/plans/nx-cache/plan.md", vec![], false),
        ("the pnpm lockfile, no package moved", "pnpm-lock.yaml", vec![], false),
        ("a markdown file inside the app", "frontend/mla-tamagui/AGENTS.md", vec![], false),
        ("the workspace file that sets how packages install", "pnpm-workspace.yaml", vec![], true),
    ];
    for (what, file, expected, update) in cases {
        // When one file changes, Then exactly the legs built from it ship, and the update is published when the app bundles it
        let plan = repo.plan_after(|dir| append(dir, file));
        let legs: Vec<(String, Vec<String>)> = plan["legs"].as_array().unwrap().iter().map(|l| (l["app"].as_str().unwrap().to_string(), l["why"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect())).collect();
        assert_eq!(apps(&legs), expected, "{what}: {file}: {legs:?}");
        assert_eq!(plan["easUpdate"]["publish"], update, "{what}: {file}: {}", plan["easUpdate"]);
    }
    assert!(rust.iter().all(|s| watchdog_and_rust.contains(s)));

    // When a package the app bundles is patched anew (pnpm writes the new patch hash wherever the lockfile names it), Then
    // no leg ships and the update is published, saying which package moved
    let plan = repo.plan_after(|dir| {
        let lock = std::fs::read_to_string(dir.join("pnpm-lock.yaml")).unwrap();
        let hash = lock.lines().find_map(|l| l.strip_prefix("  sonner-native@0.26.4: ")).unwrap().to_string();
        std::fs::write(dir.join("pnpm-lock.yaml"), lock.replace(&hash, &format!("{}0", &hash[..hash.len() - 1]))).unwrap();
    });
    assert_eq!(plan["legs"], json!([]));
    assert_eq!(plan["easUpdate"], json!({ "publish": true, "why": ["1 of its locked dependencies moved: sonner-native"] }));

    // When the plan is written for CI's jobs (--github-output) after an app change and after a backend change, and with --force
    let out = repo.dir.path().join("gh-output");
    let gh = |edit: &str| {
        let _ = std::fs::remove_file(&out);
        git(repo.dir.path(), &["checkout", "-q", "--detach", &repo.base]);
        append(repo.dir.path(), edit);
        git(repo.dir.path(), &["add", "-A"]);
        git(repo.dir.path(), &["commit", "-qm", "case", "--no-verify"]);
        let r = run_wf(&["ship", "--changed", "--since", &repo.base, "--github-output"], &RunOpts::default().cwd(repo.dir.path()).env("GITHUB_OUTPUT", &out.display().to_string()).timeout(Duration::from_secs(120)));
        assert_eq!(r.exit_code, 0, "{}\n{}", r.stdout, r.stderr);
        std::fs::read_to_string(&out).unwrap()
    };
    let (app, backend) = (gh("frontend/mla-tamagui/features/tamil-writing/data/tamilCourse.ts"), gh("backend/leetcode-backend/rs/src/main.rs"));
    // Then the eas-update job's condition is a line of its own beside the legs
    assert!(app.lines().any(|l| l == "eas_update=true") && app.lines().any(|l| l.starts_with("legs=")), "{app}");
    assert!(backend.lines().any(|l| l == "eas_update=false") && backend.contains("leetcode-backend"), "{backend}");
    let forced = run_wf(&["ship", "--changed", "--force"], &RunOpts::default().cwd(repo.dir.path()).timeout(Duration::from_secs(120)));
    assert_eq!(serde_json::from_str::<Value>(forced.stdout.trim()).unwrap()["easUpdate"], json!({ "publish": true, "why": ["--force"] }), "{}", forced.stdout);

    // When the lockfile of a service with its own workspace moves the same crate, Then that service alone ships
    for service in OWN_WORKSPACE {
        let legs = repo.legs_after(|dir| rechecksum(dir, &format!("backend/{service}/Cargo.lock"), "tokio"));
        // The watchdog ships in vm-ops-api's leg
        assert_eq!(apps(&legs), vec![if service == "watchdog" { "vm-ops-api" } else { service }]);
    }

    // When a service's lockfile pins a new release of service-kit, Then that service alone ships
    for service in OWN_WORKSPACE.iter().copied() {
        let legs = repo.legs_after(|dir| rechecksum(dir, &format!("backend/{service}/Cargo.lock"), "service-kit"));
        assert_eq!(apps(&legs), vec![if service == "watchdog" { "vm-ops-api" } else { service }]);
    }

    // When pnpm cannot read the workspace (a broken pnpm-workspace.yaml: it exits 1 with nothing on stdout); the services need none of it
    git(repo.dir.path(), &["checkout", "-q", "--detach", &repo.base]);
    std::fs::write(repo.dir.path().join("pnpm-workspace.yaml"), "packages: [\n").unwrap();
    git(repo.dir.path(), &["add", "-A"]);
    git(repo.dir.path(), &["commit", "-qm", "broken workspace", "--no-verify"]);
    let broken = run_wf(&["ship", "--changed", "--since", &repo.base], &RunOpts::default().cwd(repo.dir.path()).timeout(Duration::from_secs(120)));
    // Then the update is published saying pnpm could not tell, instead of reading its silence as "nothing changed"
    assert_eq!(broken.exit_code, 0, "{}\n{}", broken.stdout, broken.stderr);
    let why = serde_json::from_str::<Value>(broken.stdout.trim()).unwrap()["easUpdate"]["why"][0].as_str().unwrap().to_string();
    assert!(why.contains("pnpm exited 1 listing the packages changed since"), "{why}");
}

#[test]
fn an_edit_of_the_backend_kit_workspace_manifest_or_lockfile_ships_no_service() {
    let repo = Repo::copy();
    // Given every service has a Cargo workspace of its own, When the manifest or the lockfile of backend/ (the service-kit's own workspace) changes
    let manifest = repo.legs_after(|dir| append(dir, "backend/Cargo.toml"));
    let lock = repo.legs_after(|dir| rechecksum(dir, "backend/Cargo.lock", "tokio"));
    // Then no service image is rebuilt: none of them copies it
    assert_eq!((apps(&manifest), apps(&lock)), (Vec::<&str>::new(), Vec::<&str>::new()));
}

#[test]
fn an_edit_under_the_git_contract_rebuilds_exactly_the_services_whose_dockerfile_copies_it() {
    let root = repo_root();
    let registry: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(root.join("infra/oracle-cloud/vm-apps.json")).unwrap()).unwrap();
    // Given the services whose Dockerfile copies the crate, read from the Dockerfiles
    let mut copying: Vec<String> = registry
        .iter()
        .filter(|a| std::fs::read_to_string(root.join(a["repoPath"].as_str().unwrap()).join("Dockerfile")).is_ok_and(|d| d.lines().any(|l| l.starts_with("COPY libraries/agent-api/agent-api-git "))))
        .map(|a| a["name"].as_str().unwrap().to_string())
        .collect();
    copying.sort();
    assert!(copying.len() > 1, "no service copies the crate: {copying:?}");

    // When its code changes, Then those services ship and no other
    let repo = Repo::copy();
    let legs = repo.legs_after(|dir| append(dir, "libraries/agent-api/agent-api-git/src/lib.rs"));
    let mut shipped: Vec<&str> = apps(&legs);
    shipped.sort();
    assert_eq!(shipped, copying.iter().map(String::as_str).collect::<Vec<_>>());
}

#[test]
fn a_named_service_deploys_whatever_changed_and_an_unknown_name_deploys_nothing() {
    // Given a base and a commit that touches no service's images
    let repo = Repo::copy();

    // When the plan is asked with --also naming reporting-service, vm-ops-api and a name that is no service
    let plan = repo.plan_with(&["--also", "reporting-service,vm-ops-api,no-such-service"], |_| {});

    // Then exactly the two real services are planned, vm-ops-api first, each because it was forced
    let legs: Vec<(String, Vec<String>)> = plan["legs"].as_array().unwrap().iter().map(|l| (l["app"].as_str().unwrap().to_string(), l["why"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect())).collect();
    assert_eq!(legs, [("vm-ops-api".to_string(), vec!["forced".to_string()]), ("reporting-service".to_string(), vec!["forced".to_string()])]);

    // and without --also the same commit plans no leg
    assert!(repo.plan_after(|_| {})["legs"].as_array().unwrap().is_empty());
}
