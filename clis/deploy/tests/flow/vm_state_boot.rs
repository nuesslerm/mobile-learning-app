//! The restore of a fresh VM: `restore-state.js` and `provision-apps.js` as `infra/oracle-cloud/cloud-init.sh.tftpl` embeds them, run with node,
//! the host's real restic on a repository `wf vm state` made, and `docker`, `bws` and GitHub doubled. Nothing touches the machine's own
//! docker or paths: the rendered scripts are pointed at a temp directory. Given/When/Then.
use crate::vm_state::{database, files, write, State};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

const BIG: u64 = 100_000_000;
const PASSWORD: &str = "test-restic-password-never-printed-0123456789";

fn template() -> String {
    std::fs::read_to_string(crate::support::repo_root().join("infra/oracle-cloud/cloud-init.sh.tftpl")).expect("the cloud-init template")
}

/// The body of the heredoc `<<'NAME'` of the template.
fn heredoc(name: &str) -> String {
    let t = template();
    let open = format!("<<'{name}'\n");
    let start = t.find(&open).unwrap_or_else(|| panic!("no {name} in the template")) + open.len();
    let end = t[start..].find(&format!("\n{name}\n")).expect("the heredoc ends") + start;
    t[start..end].to_string()
}

struct Boot {
    dir: tempfile::TempDir,
}

const DOCKER: &str = r#"echo "$PWD $*" >> "$STUB_LOG"
case "$1 $2" in
  "volume create") mkdir -p "$STUB_VOLS/$3" ;;
  "volume inspect") echo "$STUB_VOLS/$5" ;;
esac
exit 0"#;

impl Boot {
    fn new() -> Boot {
        let dir = tempfile::Builder::new().prefix("wf-boot-").tempdir().unwrap();
        for sub in ["bin", "vols", "home"] {
            std::fs::create_dir(dir.path().join(sub)).unwrap();
        }
        crate::support::create_stub(&dir.path().join("bin/docker"), DOCKER);
        Boot { dir }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn volume(&self, docker_name: &str) -> PathBuf {
        let path = self.p("vols").join(docker_name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn path_env(&self) -> String {
        format!("{}:{}", self.p("bin").display(), std::env::var("PATH").unwrap_or_default())
    }

    fn command(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        c.env("PATH", self.path_env()).env("STUB_LOG", self.p("docker.log")).env("STUB_VOLS", self.p("vols")).env("VM_STATE_RESTORE_PAUSE_MS", "1").env("HOME", self.p("home")).env("RESTIC_CACHE_DIR", self.p("home/cache"));
        c
    }

    /// `restore-state.js` on these volumes `(app, volume, docker name)` against this repository, with this password.
    fn restore(&self, repo: &Path, password: Option<&str>, volumes: &[(&str, &str, &str)]) -> (i32, Vec<Value>, String) {
        let script = self.p("restore-state.js");
        std::fs::write(&script, heredoc("RESTORESTATEJS")).unwrap();
        let list: Vec<Value> = volumes.iter().map(|(a, v, d)| json!({ "app": a, "volume": v, "docker": d })).collect();
        let mut c = self.command("node");
        c.arg(&script).arg(Value::Array(list).to_string()).env("RESTIC_REPOSITORY", repo);
        if let Some(p) = password {
            c.env("RESTIC_PASSWORD", p);
        }
        let out = c.output().expect("node runs");
        let rows = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect();
        (out.status.code().unwrap_or(-1), rows, String::from_utf8_lossy(&out.stderr).into_owned())
    }

    fn docker_log(&self) -> Vec<String> {
        std::fs::read_to_string(self.p("docker.log")).unwrap_or_default().lines().map(String::from).collect()
    }
}

type Tree = Vec<(String, Vec<u8>)>;

fn tree(dir: &Path) -> Tree {
    files(dir).into_iter().map(|f| (f.clone(), std::fs::read(dir.join(&f)).unwrap())).collect()
}

/// An orca-state and a workspace (with a database) backed up and verified by `wf vm state`; the repository and the trees they came from.
fn backed_up(s: &State, verified: bool) -> (Tree, Tree) {
    let orca = s.volume("orca-serve", "orca-state");
    write(orca.join("runtime-id"), "runtime-restored");
    write(orca.join("devices.json"), "[{\"id\":\"phone\"}]");
    let ws = s.volume("agent-shell", "agent-workspace");
    write(ws.join("home/.claude/.credentials.json"), "{\"login\":true}");
    database(&ws.join("home/.codex/sessions.db"), 300);
    for (app, name, plan) in [("orca-serve", "orca-state", s.plan("orca-serve", "orca-state", &[], BIG)), ("agent-shell", "agent-workspace", s.plan("agent-shell", "agent-workspace", &["./home/wf-bin"], BIG))] {
        assert_eq!(s.local("backup", &plan, 1_793_000_000_000).exit_code, 0, "{app}/{name}");
        if verified {
            assert_eq!(s.local("verify", &plan, 1_793_000_000_000).exit_code, 0, "{app}/{name}");
        }
    }
    let live_db_free = |t: Tree| t.into_iter().filter(|(f, _)| !f.ends_with("sessions.db") && !f.contains("sessions.db-")).collect::<Vec<_>>();
    (tree(&orca), live_db_free(tree(&ws)))
}

#[test]
fn given_a_verified_backup_when_cloud_init_restores_an_empty_volume_then_the_files_and_databases_are_back_exactly() {
    // Given volumes backed up and verified by `wf vm state`, and a fresh VM's empty docker volumes
    let s = State::new();
    let (orca, ws) = backed_up(&s, true);
    let boot = Boot::new();
    let (orca_vol, ws_vol) = (boot.volume("orca-serve_orca-state"), boot.volume("agent-workspace"));

    // When restore-state.js runs with the repository and its password
    let (code, rows, err) = boot.restore(&s.repo(), Some(PASSWORD), &[("orca-serve", "orca-state", "orca-serve_orca-state"), ("agent-shell", "agent-workspace", "agent-workspace")]);

    // Then it exits 0, says it restored each, the files are byte-identical and the restored database passes its integrity check
    assert_eq!(code, 0, "{rows:?} {err}");
    assert!(rows.iter().all(|r| r["ok"] == true && r["status"] == "restored"), "{rows:?}");
    assert_eq!(tree(&orca_vol), orca);
    let restored: Vec<_> = tree(&ws_vol).into_iter().filter(|(f, _)| !f.ends_with("sessions.db")).collect();
    assert_eq!(restored, ws);
    let check = Command::new("sqlite3").arg(ws_vol.join("home/.codex/sessions.db")).arg("PRAGMA integrity_check;").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&check.stdout).trim(), "ok");
    assert!(!rows.iter().any(|r| r.to_string().contains(PASSWORD)));
}

#[test]
fn given_no_snapshot_to_restore_when_cloud_init_runs_then_it_continues_without_error_and_says_so() {
    // Given a first boot ever (no repository), and a repository that exists but never saw this volume
    let s = State::new();
    let (_, _) = backed_up(&s, true);
    let boot = Boot::new();
    boot.volume("alpha_alpha-state");

    // When the script runs against a repository that does not exist, and against one that has no snapshot of the volume
    let (first_code, first, _) = boot.restore(&s.dir.path().join("no-such-repository"), Some(PASSWORD), &[("alpha", "alpha-state", "alpha_alpha-state")]);
    let (other_code, other, _) = boot.restore(&s.repo(), Some(PASSWORD), &[("alpha", "alpha-state", "alpha_alpha-state")]);

    // Then both exit 0 and say why the volume stays empty
    assert_eq!((first_code, other_code), (0, 0), "{first:?} {other:?}");
    assert!(first[0]["note"].as_str().unwrap().contains("first boot"), "{first:?}");
    assert!(other[0]["note"].as_str().unwrap().contains("never backed up"), "{other:?}");
    assert!(files(&boot.p("vols/alpha_alpha-state")).is_empty());
}

#[test]
fn given_a_volume_that_holds_data_when_cloud_init_runs_then_it_is_left_untouched() {
    // Given a verified backup and a volume that already holds a newer file
    let s = State::new();
    backed_up(&s, true);
    let boot = Boot::new();
    let vol = boot.volume("orca-serve_orca-state");
    write(vol.join("devices.json"), "newer than the backup");

    // When the script runs
    let (code, rows, _) = boot.restore(&s.repo(), Some(PASSWORD), &[("orca-serve", "orca-state", "orca-serve_orca-state")]);

    // Then it succeeds, reports it kept the volume, and wrote nothing into it
    assert_eq!(code, 0, "{rows:?}");
    assert_eq!(rows[0]["status"], "kept");
    assert_eq!(tree(&vol), vec![("devices.json".to_string(), b"newer than the backup".to_vec())]);
}

#[test]
fn given_a_repository_that_cannot_be_read_or_has_no_verified_snapshot_when_cloud_init_runs_then_it_fails_loudly_and_never_mistakes_it_for_a_first_boot() {
    // Given a repository whose snapshots were never verified, and the right and the wrong password
    let s = State::new();
    backed_up(&s, false);
    let boot = Boot::new();
    let vol = boot.volume("orca-serve_orca-state");
    let volumes = [("orca-serve", "orca-state", "orca-serve_orca-state")];

    // When the script runs with the right password (nothing verified), with a wrong one and with none
    let (unverified, u_rows, _) = boot.restore(&s.repo(), Some(PASSWORD), &volumes);
    let (wrong, w_rows, _) = boot.restore(&s.repo(), Some("another-password-entirely-0000000000"), &volumes);
    let (missing, m_rows, _) = boot.restore(&s.repo(), None, &volumes);

    // Then each exits 1 naming the volume and why, none says "first boot", and the volume stays empty
    assert_eq!((unverified, wrong, missing), (1, 1, 1), "{u_rows:?} {w_rows:?} {m_rows:?}");
    assert!(u_rows[0]["error"].as_str().unwrap().contains("none is verified"), "{u_rows:?}");
    assert!(w_rows[0]["error"].as_str().unwrap().contains("could not be read"), "{w_rows:?}");
    assert!(m_rows[0]["error"].as_str().unwrap().contains("VM_STATE_RESTIC_PASSWORD"), "{m_rows:?}");
    for rows in [&u_rows, &w_rows, &m_rows] {
        assert!(rows.iter().all(|r| r["ok"] == false && r["docker"] == "orca-serve_orca-state"), "{rows:?}");
    }
    assert!(files(&vol).is_empty());
    assert!(!w_rows.iter().any(|r| r.to_string().contains("another-password")));
}

/// `provision-apps.js` as rendered for three apps (vm-ops-api, `alpha` with a state volume, `beta` without), pointed at a temp directory,
/// with GitHub, bws and docker doubled and the state repository a directory.
fn provision(boot: &Boot, repo: &Path, password: &str, extra_env: &[(&str, &str)]) -> (i32, String) {
    let base = boot.dir.path().display().to_string();
    let apps = json!([
        { "name": "vm-ops-api", "repoPath": "vm-ops-api", "composeFile": "c.yml" },
        { "name": "alpha", "repoPath": "alpha", "composeFile": "c.yml", "stateVolumes": [{ "name": "alpha-state", "maxBytes": 1000 }] },
        { "name": "beta", "repoPath": "beta", "composeFile": "c.yml" },
    ]);
    let groups: Vec<Value> = ["vm-ops-api", "alpha", "beta"].iter().map(|a| json!({ "app": a, "required": [], "optional": [] })).collect();
    let mut js = heredoc("PROVISIONAPPSJS").replace("$${", "${");
    for (k, v) in [("${secret_groups_json}", Value::Array(groups).to_string()), ("${vm_apps_json}", apps.to_string()), ("${provision_ref}", "main".into()), ("${image_tag_overrides_json}", "{}".into()), ("${vm_state_bucket}", "testbucket".into()), ("${vm_state_namespace}", "ns".into()), ("${vm_state_region}", "r".into())] {
        js = js.replace(k, &v);
    }
    js = js.replace("'rclone:oos:testbucket'", "process.env.TEST_STATE_REPO").replace("/home/ubuntu", &format!("{base}/ubuntu")).replace("/usr/local/bin/", &format!("{base}/bin/"));
    let restore = heredoc("RESTORESTATEJS");
    std::fs::create_dir_all(boot.p("ubuntu/vm-ops-api")).unwrap();
    std::fs::write(boot.p("ubuntu/restore-state.js"), restore).unwrap();
    std::fs::write(boot.p("ubuntu/vm-ops-api/.env.local"), "BWS_ADMIN_ACCESS_TOKEN=token\n").unwrap();
    std::fs::write(boot.p("provision-apps.js"), js).unwrap();
    let secrets = json!([{ "key": "VM_STATE_RESTIC_PASSWORD", "value": password }]).to_string();
    crate::support::create_stub(&boot.p("bin/bws"), &format!("printf '%s' '{secrets}'"));
    std::fs::write(
        boot.p("shim.js"),
        "const https = require('https'); const { EventEmitter } = require('events');\nhttps.get = (url, opts, cb) => { const req = new EventEmitter(); req.destroy = () => {}; const res = new EventEmitter(); res.statusCode = 200; setImmediate(() => { cb(res); res.emit('data', Buffer.from(url.endsWith('.env.defaults') ? 'X=1\\n' : 'services: {}\\n')); res.emit('end'); }); return req; };\n",
    )
    .unwrap();
    let out = boot.command("node").arg("--require").arg(boot.p("shim.js")).arg(boot.p("provision-apps.js")).env("HOST_LABEL", "main").env("TEST_STATE_REPO", repo).envs(extra_env.iter().copied()).output().expect("node runs");
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

#[test]
fn given_a_restorable_backup_when_provisioning_runs_then_the_volume_is_restored_before_the_first_app_starts() {
    // Given a verified backup of alpha's state volume and a fresh VM
    let s = State::new();
    let vol = s.volume("alpha", "alpha-state");
    write(vol.join("session.json"), "{\"session\":\"kept\"}");
    assert_eq!(s.local("backup", &s.plan("alpha", "alpha-state", &[], BIG), 1_793_000_000_000).exit_code, 0);
    assert_eq!(s.local("verify", &s.plan("alpha", "alpha-state", &[], BIG), 1_793_000_000_000).exit_code, 0);
    let boot = Boot::new();

    // When cloud-init provisions the three apps
    let (code, log) = provision(&boot, &s.repo(), PASSWORD, &[]);

    // Then the volume was created and filled before any compose up, and every app started
    assert_eq!(code, 0, "{log}");
    let calls = boot.docker_log();
    let created = calls.iter().position(|c| c.contains("volume create alpha_alpha-state")).expect("the state volume is created");
    let first_up = calls.iter().position(|c| c.contains("compose") && c.contains(" up -d")).expect("an app starts");
    assert!(created < first_up, "{calls:?}");
    assert_eq!(std::fs::read_to_string(boot.p("vols/alpha_alpha-state/session.json")).unwrap(), "{\"session\":\"kept\"}");
    assert_eq!(calls.iter().filter(|c| c.contains(" up -d")).count(), 3, "{calls:?}");
}

#[test]
fn given_a_backup_that_cannot_be_read_when_provisioning_runs_then_the_app_that_owns_the_state_is_not_started_and_the_rest_are() {
    // Given a verified backup and a vault whose restic password is wrong
    let s = State::new();
    write(s.volume("alpha", "alpha-state").join("session.json"), "{}");
    assert_eq!(s.local("backup", &s.plan("alpha", "alpha-state", &[], BIG), 1_793_000_000_000).exit_code, 0);
    assert_eq!(s.local("verify", &s.plan("alpha", "alpha-state", &[], BIG), 1_793_000_000_000).exit_code, 0);
    let boot = Boot::new();

    // When cloud-init provisions the three apps
    let (code, log) = provision(&boot, &s.repo(), "another-password-entirely-0000000000", &[]);

    // Then alpha was never started (not even empty), the others were, and provisioning fails loudly naming the volume
    let started: Vec<String> = boot.docker_log().into_iter().filter(|c| c.contains(" up -d")).collect();
    assert_eq!(code, 1, "{log}");
    assert!(started.iter().all(|c| !c.contains("/alpha ")), "alpha was started: {started:?}");
    assert_eq!(started.len(), 2, "{started:?}");
    assert!(log.contains("not starting alpha") && log.contains("alpha_alpha-state"), "{log}");
    assert!(!log.contains("another-password"), "the password reached the log");
    assert!(files(&boot.p("vols/alpha_alpha-state")).is_empty());
}

/// The workspace layout step of agent-shell's real `entrypoint.sh` (the block that runs while the entrypoint is root), pointed at a temp
/// directory and run with bash, `chown`, `chmod`, `usermod` and `wf` doubled and `runuser` running the command as the current user.
fn run_workspace_init(workspace: &Path) -> (i32, String) {
    let text = std::fs::read_to_string(crate::support::repo_root().join("infra/oracle-cloud/agent-shell/entrypoint.sh")).unwrap();
    let start = text.find("if role_has dind tools && [ -d /workspace ]; then").expect("the workspace step in entrypoint.sh");
    let end = text[start..].find("\nfi\n").expect("the step ends") + start + 4;
    let block = text[start..end].replace("/workspace", &workspace.display().to_string());
    let boot = Boot::new();
    for stub in ["chown", "chmod", "usermod"] {
        crate::support::create_stub(&boot.p("bin").join(stub), "exit 0");
    }
    crate::support::create_stub(&boot.p("bin/wf"), "echo \"wf $*\" >> \"$STUB_LOG\"");
    crate::support::create_stub(&boot.p("bin/runuser"), "while [ \"$1\" != \"--\" ]; do shift; done; shift; exec \"$@\"");
    let script = format!("set -euo pipefail\nAGENT_SHELL_ROLE=all\nrole_has() {{ return 0; }}\n{block}\n");
    let out = boot.command("bash").arg("-c").arg(script).output().unwrap();
    (out.status.code().unwrap_or(-1), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

#[test]
fn given_a_restored_workspace_when_agent_shell_starts_then_its_home_and_worktrees_are_byte_identical_and_a_fresh_volume_gets_its_layout() {
    // Given a workspace volume a backup restored: logins, shell history, a worktree, a repo, and a file in the toolchain tree
    let restored = tempfile::tempdir().unwrap();
    for (path, body) in [("home/.claude/.credentials.json", "{\"login\":\"restored\"}"), ("home/.codex/auth.json", "tok"), ("home/.bash_history", "ls\n"), ("home/.gitconfig", "[user]\n"), ("worktrees/feature/src/lib.rs", "// work\n"), ("repos/app/README.md", "x"), ("tools/npm/bin/tool", "#!/bin/sh\n"), (".agent-env/profile", "A=1\n")] {
        write(restored.path().join(path), body);
    }
    let before = tree(restored.path());

    // When agent-shell's entrypoint runs its workspace step on it, and on a fresh empty volume
    let (code, out) = run_workspace_init(restored.path());
    let fresh = tempfile::tempdir().unwrap();
    let (fresh_code, fresh_out) = run_workspace_init(fresh.path());

    // Then every restored file is byte-identical (nothing reset, rewritten or removed), and the fresh volume gets the directories the tools expect
    assert_eq!(code, 0, "{out}");
    assert_eq!(tree(restored.path()), before);
    assert_eq!(fresh_code, 0, "{fresh_out}");
    for dir in ["home", "repos", "worktrees", "tools/npm/bin", "tools/mise", "tools/cargo", "cache/pnpm", ".agent-env"] {
        assert!(fresh.path().join(dir).is_dir(), "{dir} was not created on a fresh volume");
    }
}

fn real_restic() -> String {
    String::from_utf8(Command::new("sh").arg("-c").arg("command -v restic").output().unwrap().stdout).unwrap().trim().to_string()
}

/// A `restic` ahead of the real one that fails the first `fails` calls of each sub-command in `subs` with a transient error (a restore
/// failure first leaves a half-written file in its target), then runs the real restic.
fn flaky_restic(boot: &Boot, subs: &[&str], fails: u32) {
    let script = format!(
        "S=$1; case \" {subs} \" in *\" $S \"*) N=$(cat {dir}/n-$S 2>/dev/null || echo 0); N=$((N+1)); echo $N > {dir}/n-$S\n if [ $N -le {fails} ]; then\n if [ $S = restore ]; then T=; for a in \"$@\"; do [ \"$P\" = --target ] && T=$a; P=$a; done; [ -n \"$T\" ] && echo half > \"$T/half-written\"; fi\n echo 'Fatal: connection reset by peer' >&2; exit 1; fi;; esac\nexec {real} \"$@\"",
        subs = subs.join(" "),
        dir = boot.dir.path().display(),
        real = real_restic(),
    );
    crate::support::create_stub(&boot.p("bin/restic"), &script);
}

fn attempts(stderr: &str) -> Vec<Value> {
    stderr.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|r| r["event"] == "vm-state-restore-attempt").collect()
}

#[test]
fn given_transient_failures_when_cloud_init_restores_then_listings_and_restores_are_retried_each_attempt_is_logged_and_the_volume_comes_back_exactly() {
    // Given a verified backup, and a restic whose first two snapshot listings and first two restores fail (a restore leaving half a file behind)
    let s = State::new();
    let (orca, _) = backed_up(&s, true);
    let boot = Boot::new();
    let vol = boot.volume("orca-serve_orca-state");
    flaky_restic(&boot, &["snapshots", "restore"], 2);

    // When restore-state.js runs
    let (code, rows, err) = boot.restore(&s.repo(), Some(PASSWORD), &[("orca-serve", "orca-state", "orca-serve_orca-state")]);

    // Then it succeeds, every failed attempt is one line saying the step and that it retries, and the volume is the backup byte for byte (the half file is gone)
    assert_eq!(code, 0, "{rows:?} {err}");
    assert_eq!(rows[0]["status"], "restored", "{rows:?}");
    let tried = attempts(&err);
    for step in ["snapshots orca-serve.orca-state.files", "restore"] {
        let mine: Vec<&Value> = tried.iter().filter(|r| r["step"].as_str().unwrap().starts_with(step)).collect();
        assert_eq!(mine.len(), 2, "{step}: {tried:?}");
        assert!(mine.iter().all(|r| r["retrying"] == true && r["error"].as_str().unwrap().contains("connection reset")), "{mine:?}");
    }
    assert_eq!(tree(&vol), orca);
}

#[test]
fn given_a_restore_that_keeps_failing_when_provisioning_runs_then_the_app_is_not_started_and_the_log_names_the_command_to_rerun() {
    // Given a verified backup of alpha's volume and a restic whose restores always fail, with three attempts allowed
    let s = State::new();
    write(s.volume("alpha", "alpha-state").join("session.json"), "{}");
    assert_eq!(s.local("backup", &s.plan("alpha", "alpha-state", &[], BIG), 1_793_000_000_000).exit_code, 0);
    assert_eq!(s.local("verify", &s.plan("alpha", "alpha-state", &[], BIG), 1_793_000_000_000).exit_code, 0);
    let boot = Boot::new();
    flaky_restic(&boot, &["restore"], 1000);

    // When cloud-init provisions the three apps
    let (code, log) = provision(&boot, &s.repo(), PASSWORD, &[("VM_STATE_RESTORE_ATTEMPTS", "3")]);

    // Then alpha is not started, the others are, every attempt was logged, and the log says what to rerun once the cause is fixed
    let started: Vec<String> = boot.docker_log().into_iter().filter(|c| c.contains(" up -d")).collect();
    assert_eq!(code, 1, "{log}");
    assert_eq!(started.len(), 2, "{started:?}");
    assert!(started.iter().all(|c| !c.contains("/alpha ")), "{started:?}");
    assert_eq!(log.matches("\"event\":\"vm-state-restore-attempt\"").count(), 3, "{log}");
    assert!(log.contains("after 3 attempts") && log.contains("HOST_LABEL=main node ") && log.contains("provision-apps.js"), "{log}");
    assert!(files(&boot.p("vols/alpha_alpha-state")).is_empty());
}

/// A bucket whose repository does not exist (restic says so) and whose listing by rclone fails `listing_fails` times before it works.
fn missing_repository_behind_a_policy(boot: &Boot, listing_fails: u32) {
    crate::support::create_stub(&boot.p("bin/restic"), "[ \"$1\" = cat ] && { echo 'Fatal: unable to open config file: Stat: object not found. Is there a repository at the following location?' >&2; exit 10; }\nexit 1");
    let script = format!("N=$(cat {d}/rclone-n 2>/dev/null || echo 0); N=$((N+1)); echo $N > {d}/rclone-n\nif [ $N -le {listing_fails} ]; then echo 'directory not found' >&2; exit 3; fi\nexit 0", d = boot.dir.path().display());
    crate::support::create_stub(&boot.p("bin/rclone"), &script);
}

#[test]
fn given_an_instance_principal_policy_that_has_not_propagated_when_cloud_init_reads_the_bucket_then_it_waits_and_a_listable_empty_bucket_is_the_first_boot() {
    // Given a bucket that answers "no repository" while rclone cannot list it for three tries (the policy is new), then can
    let boot = Boot::new();
    boot.volume("alpha_alpha-state");
    missing_repository_behind_a_policy(&boot, 3);
    let volumes = [("alpha", "alpha-state", "alpha_alpha-state")];

    // When restore-state.js runs against it
    let (code, rows, err) = boot.restore(Path::new("rclone:oos:testbucket"), Some(PASSWORD), &volumes);

    // Then it kept asking (each wait logged) and, once the bucket lists, continues empty as a first boot
    assert_eq!(code, 0, "{rows:?} {err}");
    assert!(rows[0]["note"].as_str().unwrap().contains("first boot"), "{rows:?}");
    assert!(attempts(&err).len() >= 3 && attempts(&err).iter().all(|r| r["step"] == "repository" && r["retrying"] == true), "{err}");
    assert_eq!(std::fs::read_to_string(boot.p("rclone-n")).unwrap().trim(), "4");
}

#[test]
fn given_a_policy_that_never_takes_effect_when_cloud_init_reads_the_bucket_then_it_fails_after_the_window_and_never_calls_it_a_first_boot() {
    // Given a bucket that says "no repository" and can never be listed, and a wait window of a quarter second instead of fifteen minutes
    let boot = Boot::new();
    let vol = boot.volume("alpha_alpha-state");
    missing_repository_behind_a_policy(&boot, 1_000_000);
    let script = boot.p("restore-state.js");
    std::fs::write(&script, heredoc("RESTORESTATEJS")).unwrap();

    // When restore-state.js runs
    let out = boot
        .command("node")
        .arg(&script)
        .arg(json!([{ "app": "alpha", "volume": "alpha-state", "docker": "alpha_alpha-state" }]).to_string())
        .env("RESTIC_REPOSITORY", "rclone:oos:testbucket")
        .env("RESTIC_PASSWORD", PASSWORD)
        .env("VM_STATE_RESTORE_IAM_WAIT_MS", "250")
        .output()
        .unwrap();

    // Then it exits 1 with an error about the policy and the wait, saying nothing of a first boot, and the volume was not touched
    let rows: Vec<Value> = String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    assert_eq!(out.status.code(), Some(1), "{rows:?}");
    assert!(rows[0]["error"].as_str().unwrap().contains("waiting") && rows[0]["error"].as_str().unwrap().contains("policy"), "{rows:?}");
    assert!(!rows.iter().any(|r| r.to_string().contains("first boot")), "{rows:?}");
    assert!(files(&vol).is_empty());
}

const CURL: &str = r#"for a in "$@"; do last="$a"; done
case "$last" in
  *"artifacts?name=clis-arm"*) echo '{"artifacts":[{"id":7,"expired":true},{"id":9,"expired":false,"workflow_run":{"head_sha":"abc123"}}]}' ;;
  *"/artifacts/9/zip") out=""; prev=""; for a in "$@"; do [ "$prev" = "-o" ] && out="$a"; prev="$a"; done; cp "$STUB_ZIP" "$out" ;;
  *) exit 22 ;;
esac"#;

/// `seedClis` of the template on a volume with this content, GitHub doubled; the log lines it printed and the volume's directory.
fn seed(boot: &Boot, existing_wf: bool) -> (String, PathBuf) {
    let t = template();
    let start = t.find("function seedClis()").expect("seedClis in the template");
    let end = t[start..].find("\nconst HOST_LABEL").expect("seedClis ends") + start;
    let vol = boot.volume("clis");
    if existing_wf {
        std::fs::create_dir_all(vol.join("current")).unwrap();
        std::fs::write(vol.join("current/wf"), "published").unwrap();
    }
    crate::support::create_stub(&boot.p("bin/curl"), CURL);
    let zip = boot.p("clis.zip");
    let make = "import zipfile,sys; z=zipfile.ZipFile(sys.argv[1],'w'); z.writestr('wf','#!/bin/sh\\necho seeded'); z.writestr('wf-mimic','x')";
    assert!(Command::new("python3").args(["-c", make]).arg(&zip).status().unwrap().success());
    let script = boot.p("seed.js");
    let header = "const fs=require('fs');const {execFileSync}=require('child_process');const PROVISION_REPO='o/r';const githubToken='tok';const log=(m)=>console.log(m);\n";
    std::fs::write(&script, format!("{header}{}\nseedClis();\n", t[start..end].replace("$$", "$"))).unwrap();
    let out = boot.command("node").arg(&script).env("STUB_ZIP", &zip).output().expect("node runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    (String::from_utf8_lossy(&out.stdout).into_owned(), vol)
}

#[test]
fn given_a_fresh_vm_with_an_empty_clis_volume_when_cloud_init_seeds_it_then_the_newest_unexpired_build_is_current_and_runs() {
    // Given an empty clis volume and a repository whose newest unexpired clis-arm artifact holds wf
    let boot = Boot::new();
    // When cloud-init seeds the volume
    let (log, vol) = seed(&boot, false);
    // Then current points at the bootstrap tree, whose wf is executable and is the one of the unexpired artifact
    assert!(log.contains("seeded from artifact 9"), "{log}");
    assert_eq!(std::fs::read_link(vol.join("current")).unwrap(), PathBuf::from("bootstrap"));
    let out = Command::new(vol.join("current/wf")).output().expect("the seeded wf runs");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "seeded");
    assert!(vol.join("bootstrap/wf-mimic").exists() && !vol.join(".seed").exists());
}

#[test]
fn given_a_clis_volume_that_already_holds_a_wf_when_cloud_init_seeds_it_then_the_published_build_is_left_alone() {
    // Given a volume whose current already has a wf
    let boot = Boot::new();
    // When cloud-init seeds the volume
    let (log, vol) = seed(&boot, true);
    // Then nothing is downloaded or replaced
    assert!(log.contains("already holds a wf"), "{log}");
    assert_eq!(std::fs::read_to_string(vol.join("current/wf")).unwrap(), "published");
    assert!(!vol.join("bootstrap").exists());
}
