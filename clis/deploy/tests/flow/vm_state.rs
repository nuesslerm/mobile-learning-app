//! `wf vm state backup|restore|list|verify`: the agent-shell side (`--local`) over the real `restic` and `sqlite3` with a repository in a
//! temp directory (the one thing doubled is where the repository lives), and the relay a caller does through a fake vm-ops-api. Nothing
//! reaches a cloud. Given/When/Then.
use crate::support::{repo_root, run_wf, RunOpts, RunResult};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use wf_vmops::testing::{answers, FakeVm, CI_VM_SEED};

const DAY_MS: i64 = 86_400_000;
const T0: i64 = 1_793_000_000_000;
const PASSWORD: &str = "test-restic-password-never-printed-0123456789";
const BIG: u64 = 100_000_000;

pub(crate) struct State {
    pub dir: tempfile::TempDir,
}

fn hex_of(plan: &[Value]) -> String {
    Value::Array(plan.to_vec()).to_string().bytes().map(|b| format!("{b:02x}")).collect()
}

impl State {
    pub fn new() -> State {
        let dir = tempfile::Builder::new().prefix("wf-vm-state-").tempdir().unwrap();
        for sub in ["home", "vol", "tmp"] {
            std::fs::create_dir(dir.path().join(sub)).unwrap();
        }
        State { dir }
    }

    pub fn repo(&self) -> PathBuf {
        self.dir.path().join("repo")
    }

    /// An (empty) volume as agent-shell would mount it.
    pub fn volume(&self, app: &str, name: &str) -> PathBuf {
        let path = self.dir.path().join("vol").join(app).join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn entry(&self, app: &str, name: &str, excludes: &[&str], max: u64) -> Value {
        json!({ "name": name, "path": self.dir.path().join("vol").join(app).join(name), "excludes": excludes, "maxBytes": max })
    }

    /// The plan of one app with these volumes `(name, excludes, max)`.
    pub fn plan_of(&self, app: &str, volumes: &[(&str, &[&str], u64)]) -> String {
        hex_of(&[json!({ "app": app, "volumes": volumes.iter().map(|(n, e, m)| self.entry(app, n, e, *m)).collect::<Vec<_>>() })])
    }

    pub fn plan(&self, app: &str, name: &str, excludes: &[&str], max: u64) -> String {
        self.plan_of(app, &[(name, excludes, max)])
    }

    pub fn env_opts(&self) -> RunOpts {
        RunOpts::default()
            .clean_env()
            .env("HOME", &self.dir.path().join("home").display().to_string())
            .env("WF_VM_STATE_REPO", &self.repo().display().to_string())
            .env("VM_STATE_RESTIC_PASSWORD", PASSWORD)
            .env("WF_VM_STATE_ROOT", &self.dir.path().join("vol").display().to_string())
            .env("TMPDIR", &self.scratch().display().to_string())
    }

    /// Where the verbs make their scratch directories: it must be empty again after every run.
    pub fn scratch(&self) -> PathBuf {
        self.dir.path().join("tmp")
    }

    pub fn leftovers(&self) -> Vec<String> {
        files(&self.scratch())
    }

    pub fn local_with(&self, verb: &str, plan: &str, now: i64, opts: RunOpts) -> RunResult {
        run_wf(&["vm", "state", verb, "--local", "--plan", plan], &opts.env("WF_VM_STATE_NOW_MS", &now.to_string()))
    }

    pub fn local(&self, verb: &str, plan: &str, now: i64) -> RunResult {
        self.local_with(verb, plan, now, self.env_opts())
    }

    /// The real restic on the repository, as a person debugging would run it.
    pub fn restic(&self, args: &[&str]) -> String {
        let out = Command::new("restic").args(args).env("RESTIC_REPOSITORY", self.repo()).env("RESTIC_PASSWORD", PASSWORD).output().expect("restic is installed");
        assert!(out.status.success(), "restic {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn snapshots(&self, host: &str) -> Vec<Value> {
        serde_json::from_str(&self.restic(&["snapshots", "--json", "--host", host])).unwrap()
    }
}

pub(crate) fn line(r: &RunResult) -> Value {
    let last = r.stdout.lines().rev().find(|l| l.starts_with('{')).unwrap_or_else(|| panic!("no JSON line: {r:?}"));
    serde_json::from_str(last).unwrap()
}

pub(crate) fn files(dir: &Path) -> Vec<String> {
    let out = Command::new("find").arg(dir).args(["-type", "f"]).output().unwrap();
    let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout).lines().map(|l| l.strip_prefix(&format!("{}/", dir.display())).unwrap_or(l).to_string()).collect();
    names.sort();
    names
}

pub(crate) fn write(path: PathBuf, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A SQLite database with enough rows to span several pages, written through the real `sqlite3`.
pub(crate) fn database(path: &Path, rows: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let sql = format!("PRAGMA journal_mode=WAL; CREATE TABLE t(id INTEGER PRIMARY KEY, body TEXT); WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<{rows}) INSERT INTO t(body) SELECT hex(randomblob(200)) FROM n;");
    let out = Command::new("sqlite3").arg(path).arg(sql).output().expect("sqlite3 is installed");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    files(dir).into_iter().map(|f| (f.clone(), std::fs::read(dir.join(&f)).unwrap())).collect()
}

const WORKSPACE_EXCLUDES: [&str; 6] = ["./rca-asks", "./tools", "./home/.npm", "./home/wf-bin", "target", "node_modules"];

#[test]
fn a_backup_snapshots_what_matters_never_reads_what_the_plan_leaves_out_and_a_restore_brings_it_back_byte_for_byte() {
    // Given an agent workspace with sessions and credentials, regenerable bulk, a node_modules and the running wf, and a repository that does not exist yet
    let s = State::new();
    let ws = s.volume("ws-app", "agent-workspace");
    write(ws.join("home/.claude/.credentials.json"), "{\"token\":\"TOKEN-MARKER-123\"}");
    write(ws.join("home/.codex/auth.json"), "codex-login");
    write(ws.join("worktrees/feature/src/main.rs"), "fn main() {}");
    write(ws.join("rca-asks/ask-1/answer.txt"), "regenerable");
    write(ws.join("tools/mise/bin/node"), "toolchain");
    write(ws.join("home/.npm/_cacache/x"), "npm cache");
    write(ws.join("home/wf-bin/wf"), "running binary");
    write(ws.join("worktrees/feature/node_modules/dep/index.js"), "dep");
    let orca = s.volume("orca-serve", "orca-state");
    write(orca.join("runtime-id"), "runtime-1");
    write(orca.join("devices.json"), "[]");
    let plans = (s.plan("ws-app", "agent-workspace", &WORKSPACE_EXCLUDES, BIG), s.plan("orca-serve", "orca-state", &[], BIG));

    // When both apps are backed up
    let (a, b) = (s.local("backup", &plans.0, T0), s.local("backup", &plans.1, T0));

    // Then each is one snapshot of its volume, the repository was created, nothing the plan leaves out is in it, and no secret is readable in it
    assert_eq!((a.exit_code, b.exit_code), (0, 0), "{a:?} {b:?}");
    assert_eq!((line(&a)["ok"].clone(), line(&a)["apps"][0]["volumes"][0]["databases"].clone()), (json!(true), json!(0)));
    let listing = s.restic(&["ls", "--host", "ws-app.agent-workspace.files", "latest", "--long"]);
    for kept in ["home/.claude/.credentials.json", "home/.codex/auth.json", "worktrees/feature/src/main.rs"] {
        assert!(listing.contains(kept), "{kept} missing: {listing}");
    }
    for left_out in ["rca-asks", "tools/mise", ".npm", "wf-bin", "node_modules"] {
        assert!(!listing.contains(left_out), "{left_out} was backed up: {listing}");
    }
    let raw = Command::new("grep").args(["-r", "-l", "TOKEN-MARKER-123"]).arg(s.repo()).output().unwrap();
    assert!(raw.stdout.is_empty(), "the credential is readable in the repository: {}", String::from_utf8_lossy(&raw.stdout));

    // When the newest snapshots are verified, the volumes are emptied (a fresh VM's), and the stopped apps are restored
    assert_eq!(s.local("verify", &plans.0, T0).exit_code, 0);
    assert_eq!(s.local("verify", &plans.1, T0).exit_code, 0);
    let before = (tree(&ws), tree(&orca));
    for dir in [&ws, &orca] {
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::create_dir(dir).unwrap();
    }
    let (ra, rb) = (s.local("restore", &plans.0, T0), s.local("restore", &plans.1, T0));

    // Then every kept file is back byte for byte, and what was left out is not invented
    assert_eq!((ra.exit_code, rb.exit_code), (0, 0), "{ra:?} {rb:?}");
    let expected_ws: Vec<_> = before.0.iter().filter(|(f, _)| ["rca-asks", "tools/", "home/.npm", "wf-bin", "node_modules"].iter().all(|x| !f.contains(x))).cloned().collect();
    assert_eq!(tree(&ws), expected_ws);
    assert_eq!(tree(&orca), before.1);
    assert!(s.leftovers().is_empty(), "{:?}", s.leftovers());
}

#[test]
fn a_second_backup_of_an_unchanged_volume_uploads_next_to_nothing_and_a_changed_file_uploads_only_itself() {
    // Given a volume of two files, one of them large and incompressible, backed up once
    let s = State::new();
    let vol = s.volume("leetcode-backend", "leetcode-backend-data");
    let big: String = (0..40_000).map(|i| format!("{:08x}", (i as u32).wrapping_mul(2_654_435_761))).collect();
    write(vol.join("big.bin"), &big);
    write(vol.join("session.json"), "{\"session\":1}");
    let plan = s.plan("leetcode-backend", "leetcode-backend-data", &[], BIG);
    let first = line(&s.local("backup", &plan, T0));
    let added = |l: &Value| l["apps"][0]["volumes"][0]["dataAdded"].as_u64().unwrap();

    // When it is backed up again unchanged, and again after the small file changed
    let again = line(&s.local("backup", &plan, T0 + DAY_MS));
    write(vol.join("session.json"), "{\"session\":2}");
    let changed = line(&s.local("backup", &plan, T0 + 2 * DAY_MS));

    // Then the first uploaded the big file, the unchanged one almost nothing, and the change only a few hundred bytes
    assert!(added(&first) > 100_000, "{first}");
    assert!(added(&again) < 3_000, "{again}");
    assert!(added(&changed) < 3_000, "{changed}");
}

#[test]
fn a_volume_over_its_cap_is_refused_by_size_before_anything_is_read_and_a_cap_that_fits_is_not() {
    // Given a volume holding 5 000 bytes outside its excludes (and 5 000 000 inside them) and a cap of 4 000
    let s = State::new();
    let vol = s.volume("orca-serve", "orca-state");
    write(vol.join("devices.json"), &"d".repeat(5_000));
    write(vol.join("cache/blob"), &"c".repeat(5_000_000));
    let tight = s.plan("orca-serve", "orca-state", &["./cache"], 4_000);

    // When it is backed up
    let r = s.local("backup", &tight, T0);

    // Then it fails naming the size and the cap, the repository holds no snapshot, and with a cap that fits the same volume is backed up
    let out = line(&r);
    assert_eq!((r.exit_code, out["ok"].clone()), (1, json!(false)), "{r:?}");
    let error = out["apps"][0]["volumes"][0]["error"].as_str().unwrap();
    assert!(error.contains("5000 bytes") && error.contains("4000 byte cap") && error.contains("nothing was read"), "{error}");
    assert!(s.snapshots("orca-serve.orca-state.files").is_empty(), "a snapshot was made of a refused volume");
    assert_eq!(s.local("backup", &s.plan("orca-serve", "orca-state", &["./cache"], 6_000), T0).exit_code, 0);
}

#[test]
fn only_the_newest_fourteen_snapshots_of_a_volume_are_kept_and_all_of_them_still_restore() {
    // Given sixteen nightly backups of a volume that changes every night
    let s = State::new();
    let vol = s.volume("orca-serve", "orca-state");
    let plan = s.plan("orca-serve", "orca-state", &[], BIG);
    for night in 0..16 {
        write(vol.join("devices.json"), &format!("[{night}]"));
        assert_eq!(s.local("backup", &plan, T0 + night * DAY_MS).exit_code, 0, "night {night}");
    }

    // When the snapshots are listed and the newest is verified
    let list = line(&s.local("list", &plan, T0 + 16 * DAY_MS));
    let verify = s.local("verify", &plan, T0 + 16 * DAY_MS);

    // Then fourteen remain, the newest first and aged by their run, and the repository still checks
    let snapshots = list["apps"][0]["volumes"][0]["snapshots"].as_array().unwrap();
    assert_eq!((list["ok"].clone(), snapshots.len()), (json!(true), 14), "{list}");
    assert_eq!(snapshots[0]["ageSeconds"], json!(DAY_MS / 1000));
    assert_eq!(verify.exit_code, 0, "{verify:?}");
}

#[test]
fn a_repository_over_its_budget_fails_the_backup_and_a_backup_that_would_outgrow_it_says_so() {
    // Given a backup of a 300 KB incompressible volume under a generous budget
    let s = State::new();
    let vol = s.volume("vnc-auth-session", "vnc-auth-credentials");
    let noise: String = (0..40_000).map(|i| format!("{:08x}", (i as u32).wrapping_mul(2_654_435_761))).collect();
    write(vol.join("credentials.json"), &noise);
    let plan = s.plan("vnc-auth-session", "vnc-auth-credentials", &[], BIG);
    let ok = s.local_with("backup", &plan, T0, s.env_opts().env("WF_VM_STATE_BUDGET_BYTES", "5000000"));
    assert_eq!(ok.exit_code, 0, "{ok:?}");
    assert!(line(&ok)["repository"]["bytes"].as_u64().unwrap() > 100_000);

    // When the budget is lower than what the repository already holds, and when a new volume would take it past a budget just above it
    let held = line(&ok)["repository"]["bytes"].as_u64().unwrap();
    let refused = s.local_with("backup", &plan, T0 + DAY_MS, s.env_opts().env("WF_VM_STATE_BUDGET_BYTES", &(held - 1).to_string()));
    let bulk: String = (0..40_000).map(|i| format!("{:08x}", (i as u32).wrapping_mul(2_246_822_519))).collect();
    write(vol.join("more.bin"), &bulk);
    let grown = s.local_with("backup", &plan, T0 + 2 * DAY_MS, s.env_opts().env("WF_VM_STATE_BUDGET_BYTES", &(held + 1_000).to_string()));

    // Then both fail loudly with the figures, and the first changed nothing
    let first = line(&refused);
    assert_eq!((refused.exit_code, first["ok"].clone()), (1, json!(false)), "{refused:?}");
    assert!(first["error"].as_str().unwrap().contains("over its") && first["error"].as_str().unwrap().contains("budget"), "{first}");
    let second = line(&grown);
    assert_eq!((grown.exit_code, second["repository"]["ok"].clone()), (1, json!(false)), "{grown:?}");
    assert!(second["repository"]["error"].as_str().unwrap().contains("over its"), "{second}");
}

#[test]
fn a_sqlite_database_is_snapshotted_as_a_consistent_copy_without_its_wal_and_verify_checks_its_integrity() {
    // Given a workspace with a database in WAL mode (its -wal and -shm beside it) and a plain file
    let s = State::new();
    let ws = s.volume("agent-shell", "agent-workspace");
    database(&ws.join("home/.codex/sessions.db"), 300);
    write(ws.join("home/.codex/auth.json"), "login");
    let _holder = Command::new("sqlite3").arg(ws.join("home/.codex/sessions.db")).arg("PRAGMA journal_mode=WAL; INSERT INTO t(body) VALUES('x');").output().unwrap();
    write(ws.join("home/.codex/sessions.db-wal"), "a wal that must never be copied raw");
    let plan = s.plan("agent-shell", "agent-workspace", &WORKSPACE_EXCLUDES, BIG);

    // When it is backed up and verified
    let backup = s.local("backup", &plan, T0);
    let verify = s.local("verify", &plan, T0);

    // Then the files snapshot has neither the live database nor its wal, the sqlite snapshot has the copy, and verify checked one database
    assert_eq!((backup.exit_code, verify.exit_code), (0, 0), "{backup:?} {verify:?}");
    let files_listing = s.restic(&["ls", "--host", "agent-shell.agent-workspace.files", "latest"]);
    assert!(files_listing.contains("auth.json") && !files_listing.contains("sessions.db"), "{files_listing}");
    assert!(s.restic(&["ls", "--host", "agent-shell.agent-workspace.sqlite", "latest"]).contains("home/.codex/sessions.db"));
    assert_eq!(line(&verify)["apps"][0]["volumes"][0]["databasesChecked"], json!(1));
    assert!(s.leftovers().is_empty(), "the staged copy was left behind: {:?}", s.leftovers());
}

#[test]
fn a_database_that_fails_its_integrity_check_is_never_verified_and_never_restored() {
    // Given a database whose pages were damaged after a good backup was made, and then backed up again
    let s = State::new();
    let ws = s.volume("ws-app", "agent-workspace");
    let db = ws.join("home/.codex/sessions.db");
    database(&db, 400);
    let plan = s.plan("ws-app", "agent-workspace", &WORKSPACE_EXCLUDES, BIG);
    assert_eq!(s.local("backup", &plan, T0).exit_code, 0);
    assert_eq!(s.local("verify", &plan, T0).exit_code, 0);
    let mut bytes = std::fs::read(&db).unwrap();
    for b in &mut bytes[4096..16384] {
        *b = 0xA5;
    }
    std::fs::write(&db, bytes).unwrap();
    let backup = s.local("backup", &plan, T0 + DAY_MS);

    // When the newest is verified (if the damaged copy could be made at all)
    let verify = s.local("verify", &plan, T0 + DAY_MS);

    // Then the night fails (the copy or the check), and a restore still takes the older snapshot that verified, never the damaged one
    assert!(backup.exit_code == 1 || verify.exit_code == 1, "{backup:?} {verify:?}");
    let verified: Vec<Value> = s.snapshots("ws-app.agent-workspace.sqlite").into_iter().filter(|x| x["tags"].as_array().unwrap().contains(&json!("verified"))).collect();
    assert_eq!(verified.len(), 1, "only the good night is verified: {verified:?}");
    std::fs::remove_dir_all(&ws).unwrap();
    std::fs::create_dir(&ws).unwrap();
    assert_eq!(s.local("restore", &plan, T0 + DAY_MS).exit_code, 0);
    let restored = Command::new("sqlite3").arg(&db).arg("PRAGMA integrity_check;").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&restored.stdout).trim(), "ok");
}

#[test]
fn verify_covers_every_declared_volume_and_fails_for_one_that_was_never_backed_up() {
    // Given an app that declares two volumes of which only one was ever backed up
    let s = State::new();
    write(s.volume("t3-serve", "state-a").join("a.json"), "a");
    write(s.volume("t3-serve", "state-b").join("b.json"), "b");
    let only_a = s.plan_of("t3-serve", &[("state-a", &[], BIG)]);
    let both = s.plan_of("t3-serve", &[("state-a", &[], BIG), ("state-b", &[], BIG)]);
    assert_eq!(s.local("backup", &only_a, T0).exit_code, 0);

    // When both are verified and listed
    let verify = s.local("verify", &both, T0);
    let list = s.local("list", &both, T0);

    // Then the app fails and names the volume with no snapshot, the other volume still verified, and list agrees
    let out = line(&verify);
    assert_eq!((verify.exit_code, out["apps"][0]["ok"].clone()), (1, json!(false)), "{verify:?}");
    assert!(out["apps"][0]["error"].as_str().unwrap().contains("state-b: the repository holds no snapshot of t3-serve/state-b"), "{out}");
    assert_eq!(out["apps"][0]["volumes"][0]["ok"], json!(true));
    assert_eq!((list.exit_code, line(&list)["apps"][0]["ok"].clone()), (1, json!(false)));
}

#[test]
fn a_damaged_repository_fails_verify_and_a_volume_that_agent_shell_does_not_mount_or_outside_the_state_root_is_refused() {
    // Given a backed-up volume whose repository packs were all damaged afterwards
    let s = State::new();
    write(s.volume("orca-serve", "orca-state").join("devices.json"), &"d".repeat(50_000));
    let plan = s.plan("orca-serve", "orca-state", &[], BIG);
    assert_eq!(s.local("backup", &plan, T0).exit_code, 0);
    for pack in files(&s.repo().join("data")) {
        let path = s.repo().join("data").join(&pack);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.iter_mut().skip(100).for_each(|b| *b ^= 0xFF);
        // restic writes its packs read-only, which only root writes through
        use std::os::unix::fs::PermissionsExt;
        let mut mode = std::fs::metadata(&path).unwrap().permissions();
        mode.set_mode(mode.mode() | 0o200);
        std::fs::set_permissions(&path, mode).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }

    // When it is verified, and a plan names a volume nobody mounted and one outside the state root
    let verify = s.local("verify", &plan, T0);
    let unmounted = s.local("backup", &hex_of(&[json!({ "app": "orca-serve", "volumes": [{ "name": "x", "path": s.dir.path().join("vol/orca-serve/missing"), "excludes": [], "maxBytes": 1000 }] })]), T0);
    let outside = s.local("backup", &hex_of(&[json!({ "app": "orca-serve", "volumes": [{ "name": "x", "path": "/etc", "excludes": [], "maxBytes": 1000 }] })]), T0);

    // Then verify fails and the scratch is gone, and the two refusals say which path and why
    assert_eq!(verify.exit_code, 1, "{verify:?}");
    assert!(s.leftovers().is_empty(), "{:?}", s.leftovers());
    assert!(line(&unmounted)["apps"][0]["volumes"][0]["error"].as_str().unwrap().contains("is not mounted in agent-shell"));
    assert!(line(&outside)["apps"][0]["volumes"][0]["error"].as_str().unwrap().contains("not a mounted state volume"));
}

#[test]
fn a_restore_fills_only_an_empty_volume_from_a_verified_snapshot_and_otherwise_writes_nothing() {
    // Given a backed-up volume that was never verified
    let s = State::new();
    let vol = s.volume("orca-serve", "orca-state");
    write(vol.join("runtime-id"), "runtime-1");
    let plan = s.plan("orca-serve", "orca-state", &[], BIG);
    assert_eq!(s.local("backup", &plan, T0).exit_code, 0);
    let emptied = || {
        std::fs::remove_dir_all(&vol).unwrap();
        std::fs::create_dir(&vol).unwrap();
    };
    emptied();

    // When it is restored before verify, then after verify into a volume that holds data, then into the empty one
    let unverified = s.local("restore", &plan, T0);
    assert_eq!(s.local("verify", &plan, T0).exit_code, 0);
    write(vol.join("devices.json"), "paired-after");
    let occupied = s.local("restore", &plan, T0);
    emptied();
    let filled = s.local("restore", &plan, T0);

    // Then the unverified snapshot is never used, the occupied volume is refused with its file untouched, and the empty one is filled
    assert!(line(&unverified)["apps"][0]["error"].as_str().unwrap().contains("none is verified"), "{unverified:?}");
    assert!(line(&occupied)["apps"][0]["error"].as_str().unwrap().contains("is not empty"), "{occupied:?}");
    assert_eq!((filled.exit_code, std::fs::read_to_string(vol.join("runtime-id")).unwrap()), (0, "runtime-1".to_string()), "{filled:?}");
    assert!(!vol.join("devices.json").exists());
}

#[test]
fn the_restic_password_is_never_printed_a_missing_one_is_named_and_a_wrong_one_cannot_read_the_repository() {
    // Given a repository made with the right password
    let s = State::new();
    write(s.volume("orca-serve", "orca-state").join("devices.json"), "[]");
    let plan = s.plan("orca-serve", "orca-state", &[], BIG);
    assert_eq!(s.local("backup", &plan, T0).exit_code, 0);

    // When a verb runs with no password and with another one
    let none = s.local_with("verify", &plan, T0, RunOpts::default().clean_env().env("HOME", &s.dir.path().join("home").display().to_string()).env("WF_VM_STATE_REPO", &s.repo().display().to_string()).env("TMPDIR", &s.scratch().display().to_string()));
    let wrong = s.local_with("verify", &plan, T0, s.env_opts().env("VM_STATE_RESTIC_PASSWORD", "another-password-entirely-0000000000"));

    // Then each fails, the missing one names the secret, neither backs a repository up empty, and no output carries a password
    assert_eq!((none.exit_code, wrong.exit_code), (1, 1), "{none:?} {wrong:?}");
    assert!(line(&none)["error"].as_str().unwrap().contains("VM_STATE_RESTIC_PASSWORD"), "{none:?}");
    for r in [&none, &wrong] {
        assert!(!r.stdout.contains(PASSWORD) && !r.stderr.contains(PASSWORD) && !r.stdout.contains("another-password") && !r.stderr.contains("another-password"), "{r:?}");
    }
}

#[test]
fn nothing_plaintext_survives_any_run_and_a_killed_run_is_swept_by_the_next() {
    // Given a run killed outright that left its scratch, one still owned by a live process, and a stranger
    let s = State::new();
    write(s.volume("orca-serve", "orca-state").join("devices.json"), "device");
    let plan = s.plan("orca-serve", "orca-state", &[], BIG);
    let mut done = Command::new("true").spawn().unwrap();
    let dead = done.id();
    done.wait().unwrap();
    for (name, owner) in [("wf-vm-state-killed", Some(dead.to_string())), ("wf-vm-state-live", Some(std::process::id().to_string())), ("wf-vm-state-unowned", None)] {
        write(s.scratch().join(name).join("staged.db"), "plaintext");
        if let Some(pid) = owner {
            write(s.scratch().join(name).join(".wf-pid"), &pid);
        }
    }

    // When any verb runs, Then the dead run's scratch is gone and the others are left alone
    assert_eq!(s.local_with("backup", &plan, T0, s.env_opts().env("WF_VM_STATE_SWEEP_ROOT", &s.scratch().display().to_string())).exit_code, 0);
    let left: Vec<String> = std::fs::read_dir(s.scratch()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(!left.iter().any(|n| n == "wf-vm-state-killed") && left.iter().any(|n| n == "wf-vm-state-live") && left.iter().any(|n| n == "wf-vm-state-unowned"), "{left:?}");
    std::fs::remove_dir_all(s.scratch().join("wf-vm-state-live")).unwrap();
    std::fs::remove_dir_all(s.scratch().join("wf-vm-state-unowned")).unwrap();

    // and after a successful verify, a failed restore (a volume holding data) and a failed verify, the scratch is empty again
    assert_eq!(s.local("verify", &plan, T0).exit_code, 0);
    assert_eq!(s.local("restore", &plan, T0).exit_code, 1);
    assert!(s.leftovers().is_empty(), "{:?}", s.leftovers());
}

#[test]
fn from_a_caller_a_restore_is_refused_while_the_app_runs_has_no_force_and_otherwise_recreates_the_app() {
    // Given vm-ops-api that reports orca-serve running
    let vm = FakeVm::start();
    vm.replies(&[answers::reply(200, &answers::ps("orca-serve", &[("orca-serve", "running", "Up 2 hours", "img")]))]);

    // When a caller restores it, without --app, and with --force
    let running = run_wf(&["vm", "state", "restore", "--app", "orca-serve"], &vm.opts().cwd(&repo_root()));
    let bare = run_wf(&["vm", "state", "restore"], &vm.opts().cwd(&repo_root()));
    let forced = run_wf(&["vm", "state", "restore", "--app", "orca-serve", "--force"], &vm.opts().cwd(&repo_root()));

    // Then the first is refused with the steps and agent-shell was never asked, the others are refused before anything is asked
    let out = line(&running);
    assert_eq!((running.exit_code, out["ok"].clone()), (1, json!(false)), "{running:?}");
    assert!(out["error"].as_str().unwrap().contains("orca-serve is running") && out["next"].as_str().unwrap().contains("fresh VM"), "{out}");
    assert!(vm.requests().iter().all(|r| !r.url.contains("exec-script")), "{:?}", vm.requests().iter().map(|r| r.url.clone()).collect::<Vec<_>>());
    assert_eq!(line(&bare)["ok"], json!(false));
    assert!(line(&bare)["error"].as_str().unwrap().contains("needs --app"));
    assert_eq!(forced.exit_code, 2);
    assert!(forced.stderr.contains("no --force"), "{}", forced.stderr);
}

#[test]
fn from_a_caller_a_stopped_app_is_restored_on_agent_shell_and_recreated_but_agent_shell_never_is() {
    // Given vm-ops-api that reports orca-serve stopped and agent-shell that answers a restore
    let vm = FakeVm::start();
    let job = answers::job_id(1);
    let printed = json!({ "event": "vm-state", "verb": "restore", "ok": true, "apps": [{ "app": "orca-serve", "ok": true }] });
    vm.replies(&[
        answers::reply(200, &answers::ps("orca-serve", &[("orca-serve", "exited", "Exited (0) 1 hour ago", "img")])),
        answers::reply(202, &answers::job_started(&job)),
        answers::reply(200, &answers::job(&job, Some(0), &format!("noise\n{printed}\n"))),
        answers::reply(200, &json!({ "ok": true })),
    ]);

    // When a caller restores it
    let r = run_wf(&["vm", "state", "restore", "--app", "orca-serve"], &vm.opts().cwd(&repo_root()));

    // Then one signed exec job carries the plan (no --force), and the app is recreated from dev
    let out = line(&r);
    assert_eq!((r.exit_code, out["ok"].clone(), out["restarted"].clone()), (0, json!(true), json!(["orca-serve"])), "{r:?}");
    let requests = vm.requests();
    vm.assert_signed(&requests[1], "POST", "/services/agent-shell/exec-script", &requests[1].body);
    assert!(requests[1].body.contains("vm state restore --local --plan ") && !requests[1].body.contains("--force"), "{}", requests[1].body);
    assert!(requests[3].url.starts_with("/services/orca-serve/redeploy?") && requests[3].url.contains("recreate=all") && requests[3].url.contains("ref=dev"), "{}", requests[3].url);
}

#[test]
fn from_a_caller_the_script_on_agent_shell_removes_its_scratch_on_every_exit() {
    // Given agent-shell that answers a verify
    let vm = FakeVm::start();
    let job = answers::job_id(1);
    vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(0), &format!("{}\n", json!({ "event": "vm-state", "verb": "verify", "ok": true, "apps": [] }))))]);

    // When a caller verifies
    run_wf(&["vm", "state", "verify", "--app", "orca-serve"], &vm.opts().cwd(&repo_root()));

    // Then the script that runs there traps its exit, hangup and termination to remove the scratch directory it made privately
    let body = &vm.requests()[0].body;
    assert!(body.contains("trap") && body.contains("rm -rf") && body.contains("EXIT") && body.contains("TERM") && body.contains("umask 077"), "{body}");
    // and it makes that directory on agent-shell's tmpfs (SQLite's own temp files too) and refuses to run where /scratch is not one
    assert!(body.contains("D=$S/wf-vm-state-run.$$") && body.contains("SQLITE_TMPDIR=") && body.contains("/proc/mounts") && body.contains("/scratch tmpfs"), "{body}");
}

#[test]
fn the_script_on_agent_shell_makes_its_scratch_on_a_tmpfs_sized_for_the_largest_volume_and_refuses_to_run_without_one() {
    // Given agent-shell's compose file, the volumes' caps and the script a caller sends
    let root = repo_root();
    let compose = std::fs::read_to_string(root.join("infra/oracle-cloud/agent-shell/docker-compose.prod.yml")).unwrap();
    let apps: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(root.join("infra/oracle-cloud/vm-apps.json")).unwrap()).unwrap();
    let largest = apps.iter().flat_map(|a| a["stateVolumes"].as_array().into_iter().flatten()).filter_map(|v| v["maxBytes"].as_u64()).max().unwrap();
    let vm = FakeVm::start();
    let job = answers::job_id(1);
    vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(0), "{}\n"))]);
    run_wf(&["vm", "state", "verify", "--app", "orca-serve"], &vm.opts().cwd(&root));
    let script = vm.requests()[0].body.clone();

    // When the tmpfs the compose file mounts at /scratch is read, and the script is run where /scratch is no tmpfs (this machine's is not)
    let mount = compose.lines().map(str::trim).find_map(|l| l.strip_prefix("- /scratch:")).expect("a tmpfs mounted at /scratch for agent-shell");
    let size_mb: u64 = mount.split(',').find_map(|o| o.strip_prefix("size=")).and_then(|v| v.strip_suffix('m')).and_then(|v| v.parse().ok()).expect("a size in megabytes");
    let out = Command::new("sh").arg("-c").arg(&script).env_remove("WF_VM_STATE_SCRATCH").output().unwrap();

    // Then the tmpfs holds the largest cap and a quarter, and the script refuses to start, with a JSON line that says how to fix it and no directory made
    assert!(size_mb * 1024 * 1024 >= largest + largest / 4, "{size_mb} MB cannot hold the {largest} byte volume and its staged copies");
    assert!(compose.contains("tmpfs:"), "the mount is not under a tmpfs key");
    let said = String::from_utf8_lossy(&out.stdout);
    let refusal: Value = serde_json::from_str(said.lines().last().unwrap_or("{}")).unwrap_or(Value::Null);
    assert_eq!((out.status.code(), refusal["ok"].clone()), (Some(1), json!(false)), "{said} {}", String::from_utf8_lossy(&out.stderr));
    assert!(refusal["error"].as_str().unwrap().contains("no tmpfs at /scratch") && refusal["next"].as_str().unwrap().contains("redeploy agent-shell"), "{refusal}");
}

#[test]
fn a_caller_hands_agent_shell_a_plan_that_keeps_the_running_wf_out_of_the_workspace_archive() {
    // Given agent-shell that answers a list
    let vm = FakeVm::start();
    let job = answers::job_id(1);
    vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(0), &format!("{}\n", json!({ "event": "vm-state", "verb": "list", "ok": true, "apps": [] }))))]);

    // When a caller asks for the workspace's backups
    run_wf(&["vm", "state", "list", "--app", "agent-shell"], &vm.opts().cwd(&repo_root()));

    // Then the plan it sends names the workspace and excludes home/wf-bin
    let body = &vm.requests()[0].body;
    let word = body.split("--plan ").nth(1).unwrap().split(|c: char| !c.is_ascii_hexdigit()).next().unwrap();
    let bytes: Vec<u8> = (0..word.len() / 2).map(|i| u8::from_str_radix(&word[2 * i..2 * i + 2], 16).unwrap()).collect();
    let plan: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(plan[0]["volumes"][0]["path"], json!("/workspace"));
    assert!(plan[0]["volumes"][0]["excludes"].as_array().unwrap().contains(&json!("./home/wf-bin")), "{plan}");
}

#[test]
fn from_a_caller_an_agent_shell_that_is_being_redeployed_is_asked_again_until_it_answers() {
    // Given agent-shell that is down for two asks (a redeploy in flight) and then answers a verify
    let vm = FakeVm::start();
    let job = answers::job_id(1);
    let printed = json!({ "event": "vm-state", "verb": "verify", "ok": true, "apps": [] });
    vm.replies(&[
        answers::reply(503, &answers::agent_shell_not_running()),
        answers::reply(503, &answers::agent_shell_not_running()),
        answers::reply(202, &answers::job_started(&job)),
        answers::reply(200, &answers::job(&job, Some(0), &format!("{printed}\n"))),
    ]);

    // When a caller verifies
    let r = run_wf(&["vm", "state", "verify", "--app", "orca-serve"], &vm.opts().cwd(&repo_root()).env("WF_VM_STATE_RELAY_BACKOFF_MS", "1"));

    // Then the third ask is answered, and the first two are logged as retried
    assert_eq!((r.exit_code, line(&r)["ok"].clone()), (0, json!(true)), "{r:?}");
    assert!(r.stderr.contains("attempt 1/4") && r.stderr.contains("attempt 2/4"), "{}", r.stderr);

    // and an agent-shell that never answers gives up after four asks with the reason, never hanging
    vm.replies(&[answers::reply(503, &answers::agent_shell_not_running())]);
    let r = run_wf(&["vm", "state", "verify", "--app", "orca-serve"], &vm.opts().cwd(&repo_root()).env("WF_VM_STATE_RELAY_BACKOFF_MS", "1"));
    assert_eq!(r.exit_code, 1);
    assert!(r.stderr.contains("attempt 3/4") && !r.stderr.contains("attempt 4/4"), "{}", r.stderr);
}

#[test]
fn from_a_caller_an_app_without_state_volumes_is_refused_before_agent_shell_is_asked() {
    // Given a caller that names an app with no stateVolumes
    let vm = FakeVm::start();
    vm.replies(&[answers::reply(503, &answers::agent_shell_not_running())]);

    // When it backs it up
    let r = run_wf(&["vm", "state", "backup", "--app", "remote-chat-agent"], &vm.opts().cwd(&repo_root()));

    // Then the refusal names the app and the VM was never called
    assert_eq!((r.exit_code, line(&r)["ok"].clone()), (1, json!(false)), "{r:?}");
    assert!(line(&r)["error"].as_str().unwrap().contains("remote-chat-agent declares no stateVolumes"), "{r:?}");
    assert!(vm.requests().is_empty());

    // and an agent-shell that is down is an answer with the next step, not a hang
    vm.replies(&vec![answers::reply(503, &answers::agent_shell_not_running()); 4]);
    let r = run_wf(&["vm", "state", "list"], &vm.opts().cwd(&repo_root()).env("WF_VM_STATE_RELAY_BACKOFF_MS", "1"));
    assert_eq!(r.exit_code, 1);
    assert!(line(&r)["error"].as_str().unwrap().contains("not running"), "{r:?}");
    assert!(line(&r)["next"].is_string());
}

#[test]
fn every_state_volume_of_vm_apps_is_mounted_in_agent_shell_where_the_verb_looks_for_it() {
    // Given the volumes vm-apps.json declares and agent-shell's compose file, both read from the repository
    let root = repo_root();
    let apps: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(root.join("infra/oracle-cloud/vm-apps.json")).unwrap()).unwrap();
    let compose = std::fs::read_to_string(root.join("infra/oracle-cloud/agent-shell/docker-compose.prod.yml")).unwrap();
    let mut declared = 0;
    for app in &apps {
        let name = app["name"].as_str().unwrap();
        for volume in app["stateVolumes"].as_array().into_iter().flatten() {
            declared += 1;
            let v = volume["name"].as_str().unwrap();
            // Then the compose file attaches it at the path the verb uses, as the project's own volume name
            let at = if name == "agent-shell" { "/workspace".to_string() } else { format!("/state/{name}/{v}") };
            assert!(compose.lines().any(|l| l.trim_start().starts_with("- ") && l.trim_end().ends_with(&format!(":{at}"))), "{name}/{v}: no mount at {at}");
            if name != "agent-shell" {
                assert!(compose.contains(&format!("name: {name}_{v}")), "{name}/{v}: the top-level volume is not pinned to {name}_{v}");
            }
        }
    }
    assert!(declared >= 4, "{declared}");
}

#[test]
fn the_bucket_the_verbs_use_is_the_one_terraform_creates() {
    // Given the bucket's name as terraform builds it from the prefix variable
    let root = repo_root();
    let vars = std::fs::read_to_string(root.join("infra/oracle-cloud/variables.tf")).unwrap();
    let prefix = vars.split("variable \"name_prefix\"").nth(1).and_then(|b| b.lines().find(|l| l.trim_start().starts_with("default"))).and_then(|l| l.split('"').nth(1)).expect("name_prefix default").to_string();
    let tf = std::fs::read_to_string(root.join("infra/oracle-cloud/vm-state.tf")).unwrap();
    assert!(tf.contains("name           = \"${var.name_prefix}-vm-state\""));

    // When a verb reports its bucket
    let s = State::new();
    let out = line(&s.local("list", &s.plan("orca-serve", "orca-state", &[], BIG), T0));

    // Then it is that name
    assert_eq!(out["bucket"], json!(format!("{prefix}-vm-state")));
}


/// A `restic` ahead of the real one on PATH that runs `before` (a shell snippet; `$VOL` is the volume, `$N` the count of file-snapshot
/// backups so far) ahead of each backup of a files snapshot, then runs the real restic: the one way to change a volume while it is read.
fn restic_that_acts_during_the_read(s: &State, volume: &Path, before: &str) -> String {
    let real = String::from_utf8(Command::new("sh").arg("-c").arg("command -v restic").output().unwrap().stdout).unwrap().trim().to_string();
    let bin = s.dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let counter = s.dir.path().join("files-backups");
    let script = format!(
        "if [ \"$1\" = backup ]; then case \"$*\" in *kind:files*) N=$(cat {c} 2>/dev/null || echo 0); N=$((N+1)); echo $N > {c}; VOL={v}; {before} ;; esac; fi\nexec {real} \"$@\"",
        c = counter.display(),
        v = volume.display()
    );
    crate::support::create_stub(&bin.join("restic"), &script);
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default())
}

#[test]
fn a_database_that_appears_while_the_volume_is_read_is_read_again_once_and_never_kept_live() {
    // Given a volume with one database, and a restic that lets a second one appear, without the .db name, as the first read starts
    let s = State::new();
    let ws = s.volume("agent-shell", "agent-workspace");
    database(&ws.join("home/.codex/state"), 50);
    write(ws.join("home/.codex/auth.json"), "login");
    let plan = s.plan("agent-shell", "agent-workspace", &WORKSPACE_EXCLUDES, BIG);
    let appears = "[ $N -eq 1 ] && sqlite3 $VOL/home/.codex/late \"CREATE TABLE t(a); INSERT INTO t VALUES(1);\"";
    let path = restic_that_acts_during_the_read(&s, &ws, appears);

    // When it is backed up
    let r = s.local_with("backup", &plan, T0, s.env_opts().env("PATH", &path));

    // Then the second read stages both (databases are found by their magic bytes, whatever their names), the first read's snapshots were dropped, and nothing was read live
    let out = line(&r);
    assert_eq!((r.exit_code, out["apps"][0]["volumes"][0]["databases"].clone()), (0, json!(2)), "{r:?}");
    assert_eq!(s.snapshots("agent-shell.agent-workspace.files").len(), 1);
    assert_eq!(s.snapshots("agent-shell.agent-workspace.sqlite").len(), 1);
    let copies = s.restic(&["ls", "--host", "agent-shell.agent-workspace.sqlite", "latest"]);
    assert!(copies.contains("home/.codex/state") && copies.contains("home/.codex/late"), "{copies}");
    let files = s.restic(&["ls", "--host", "agent-shell.agent-workspace.files", "latest"]);
    assert!(files.contains("auth.json") && !files.contains("home/.codex/state") && !files.contains("home/.codex/late"), "{files}");

    // and a volume where a new database appears at every read fails the backup after the second, keeping neither
    let s = State::new();
    let ws = s.volume("agent-shell", "agent-workspace");
    database(&ws.join("home/.codex/state"), 50);
    let always = "sqlite3 $VOL/home/.codex/late$N \"CREATE TABLE t(a); INSERT INTO t VALUES(1);\"";
    let path = restic_that_acts_during_the_read(&s, &ws, always);
    let r = s.local_with("backup", &s.plan("agent-shell", "agent-workspace", &WORKSPACE_EXCLUDES, BIG), T0, s.env_opts().env("PATH", &path));
    let error = line(&r)["apps"][0]["volumes"][0]["error"].as_str().unwrap().to_string();
    assert!(r.exit_code == 1 && error.contains("appeared") && error.contains("twice in a row"), "{error}");
    assert!(s.snapshots("agent-shell.agent-workspace.files").is_empty() && s.snapshots("agent-shell.agent-workspace.sqlite").is_empty());
    assert!(s.leftovers().is_empty(), "{:?}", s.leftovers());
}

/// A program that lists the bucket's object versions the way `oci os object list-object-versions` answers: the first call says `first`
/// bytes, every later one `later`; a row without a size is a delete marker.
fn versions_listing(s: &State, first: u64, later: u64) -> String {
    let path = s.dir.path().join("versions");
    let counter = s.dir.path().join("versions-calls");
    let script = format!(
        "N=$(cat {c} 2>/dev/null || echo 0); N=$((N+1)); echo $N > {c}\nif [ $N -eq 1 ]; then T={first}; else T={later}; fi\necho \"{{\\\"data\\\":[{{\\\"name\\\":\\\"current\\\",\\\"size\\\":$T}},{{\\\"name\\\":\\\"gone\\\",\\\"is-delete-marker\\\":true}}]}}\"",
        c = counter.display()
    );
    crate::support::create_stub(&path, &script);
    path.display().to_string()
}

#[test]
fn the_bucket_is_counted_with_its_previous_versions_and_a_run_that_could_pass_its_budget_never_starts() {
    // Given a bucket whose listing says 900 bytes (versions the prune left included) and a volume capped at 1 000
    let s = State::new();
    write(s.volume("orca-serve", "orca-state").join("devices.json"), "[]");
    let plan = s.plan("orca-serve", "orca-state", &[], 1_000);
    let with = |listing: String, budget: &str| s.env_opts().env("WF_VM_STATE_VERSIONS_CMD", &listing).env("WF_VM_STATE_BUCKET_BUDGET_BYTES", budget);

    // When a backup runs under a budget of 1 800 (900 and a full 1 000 would pass it), and under one of 2 000
    let refused = s.local_with("backup", &plan, T0, with(versions_listing(&s, 900, 900), "1800"));
    assert!(s.snapshots("orca-serve.orca-state.files").is_empty(), "a run that could pass the budget read the volume");
    let allowed = s.local_with("backup", &plan, T0, with(versions_listing(&s, 900, 900), "2000"));

    // Then the first reads nothing and says what the bucket holds and what the run could add, and the second runs and reports the bucket's bytes
    let first = line(&refused);
    assert_eq!((refused.exit_code, first["ok"].clone()), (1, json!(false)), "{refused:?}");
    assert!(first["error"].as_str().unwrap().contains("holds 900 bytes counting previous object versions") && first["error"].as_str().unwrap().contains("could add 1000") && first["error"].as_str().unwrap().contains("nothing was read"), "{first}");
    assert_eq!((allowed.exit_code, line(&allowed)["repository"]["bucketBytes"].clone()), (0, json!(900)), "{allowed:?}");

    // and a bucket that grows past its budget during the run (the prune's deleted packs stay as versions) fails the run after it
    let _ = std::fs::remove_file(s.dir.path().join("versions-calls"));
    let grown = s.local_with("backup", &plan, T0 + DAY_MS, with(versions_listing(&s, 100, 9_000), "2000"));
    let out = line(&grown);
    assert_eq!((grown.exit_code, out["repository"]["ok"].clone()), (1, json!(false)), "{grown:?}");
    assert!(out["repository"]["error"].as_str().unwrap().contains("9000 bytes counting the previous object versions"), "{out}");
}

#[test]
fn a_restore_never_touches_agent_shells_own_workspace_and_says_it_is_restored_only_by_cloud_init() {
    // Given vm-ops-api that would answer anything, and a backed-up workspace
    let vm = FakeVm::start();
    let s = State::new();
    write(s.volume("agent-shell", "agent-workspace").join("home/.bash_history"), "ls");
    let plan = s.plan("agent-shell", "agent-workspace", &[], BIG);
    assert_eq!(s.local("backup", &plan, T0).exit_code, 0);
    assert_eq!(s.local("verify", &plan, T0).exit_code, 0);
    std::fs::remove_dir_all(s.dir.path().join("vol/agent-shell/agent-workspace")).unwrap();
    s.volume("agent-shell", "agent-workspace");

    // When a caller restores agent-shell, and the agent-shell side is asked to directly, into an empty workspace
    let caller = run_wf(&["vm", "state", "restore", "--app", "agent-shell"], &vm.opts().cwd(&repo_root()));
    let local = s.local("restore", &plan, T0);

    // Then both refuse naming cloud-init, nothing was asked of vm-ops-api and nothing was written
    for r in [&caller, &local] {
        assert_eq!(r.exit_code, 1, "{r:?}");
        let said = line(r);
        let error = said["error"].as_str().or_else(|| said["apps"][0]["error"].as_str()).unwrap();
        assert!(error.contains("only by cloud-init"), "{said}");
    }
    assert!(vm.requests().is_empty());
    assert!(files(&s.dir.path().join("vol/agent-shell/agent-workspace")).is_empty());
}

#[test]
fn from_a_ci_job_a_verify_is_started_and_read_back_over_the_ci_port_and_never_the_exec_route() {
    // Given vm-ops-api that starts the fixed verify job and then reports it finished with its one result line
    let vm = FakeVm::start();
    let job = answers::job_id(1);
    let printed = json!({ "event": "vm-state", "verb": "verify", "ok": true, "apps": [] });
    vm.replies(&[answers::reply(202, &answers::job_started(&job)), answers::reply(200, &answers::job(&job, Some(0), &format!("{printed}\n")))]);

    // When a CI job (the CI port and seed in its environment) verifies
    let opts = vm.opts().cwd(&repo_root()).env("WF_VM_CI_PORT", "8081").env("VM_OPS_API_CI_AUTH_TOKEN", CI_VM_SEED);
    let r = run_wf(&["vm", "state", "verify"], &opts);

    // Then the answer is the job's line, and both calls went to the CI port with the CI seed: the start, then the status of that job
    assert_eq!((r.exit_code, line(&r)["ok"].clone()), (0, json!(true)), "{r:?}");
    assert_eq!(vm.tailnet.destinations(), ["vm-ops-api-main:8081", "vm-ops-api-main:8081"]);
    let requests = vm.requests();
    vm.assert_signed_with(CI_VM_SEED, &requests[0], "POST", "/state/verify", "");
    vm.assert_signed_with(CI_VM_SEED, &requests[1], "GET", &format!("/state/jobs/{job}"), "");
}

#[test]
fn the_agent_shell_side_reads_its_plan_from_a_vm_apps_file_and_refuses_one_the_budgets_do_not_fit() {
    // Given a vm-apps.json that is not the committed one, so its volumes do not add up to the committed budgets
    let state = State::new();
    let file = state.dir.path().join("vm-apps.json");
    std::fs::write(&file, r#"[{"name":"orca-serve","stateVolumes":[{"name":"orca-state","path":"/state/orca-serve/orca-state","maxBytes":1}]}]"#).unwrap();

    // When the agent-shell side is run on it
    let r = run_wf(&["vm", "state", "verify", "--local", "--apps-file", &file.display().to_string()], &state.env_opts().cwd(&repo_root()));

    // Then it is refused for the budgets, naming the files that hold them, and nothing reached a repository
    assert_eq!((r.exit_code, line(&r)["ok"].clone()), (1, json!(false)), "{r:?}");
    assert!(line(&r)["error"].as_str().unwrap().contains("budget"), "{r:?}");
}

#[test]
fn a_passphrase_protected_oci_key_reaches_rclone_decrypted_in_a_private_directory_that_is_gone_after_the_run_and_the_passphrase_stays_off_every_command_line() {
    // Given an agent-shell whose OCI key is encrypted (pass_phrase in ~/.oci/config) and stubs for oci, openssl and restic that record what they get
    let s = State::new();
    let home = s.dir.path().join("home");
    write(home.join(".oci/config"), "[DEFAULT]\nuser=u\nregion=eu-frankfurt-1\nkey_file=/nowhere/oci_api_key.pem\npass_phrase=hunter2-passphrase\n");
    let bin = s.dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let seen = s.dir.path().join("seen");
    std::fs::create_dir_all(&seen).unwrap();
    crate::support::create_stub(&bin.join("oci"), "echo '{\"data\":\"ns\"}'");
    crate::support::create_stub(&bin.join("openssl"), &format!("echo \"$@\" > {d}/openssl-args; echo \"$WF_OCI_KEY_PASS\" > {d}/openssl-pass; printf 'PLAIN-KEY'", d = seen.display()));
    crate::support::create_stub(&bin.join("restic"), &format!("cp \"$RCLONE_CONFIG_OOS_CONFIG_FILE\" {d}/config; cp \"$(sed -n 's/^key_file=//p' \"$RCLONE_CONFIG_OOS_CONFIG_FILE\")\" {d}/key; echo \"$RCLONE_CONFIG_OOS_CONFIG_FILE\" > {d}/path; stat -c %a \"$RCLONE_CONFIG_OOS_CONFIG_FILE\" > {d}/mode; exit 1", d = seen.display()));
    let plan = s.plan("orca-serve", "orca-state", &[], BIG);

    // When a backup runs against the bucket repository (no WF_VM_STATE_REPO)
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
    let opts = RunOpts::default().clean_env().env("HOME", &home.display().to_string()).env("PATH", &path).env("VM_STATE_RESTIC_PASSWORD", PASSWORD).env("WF_VM_STATE_ROOT", &s.dir.path().join("vol").display().to_string()).env("TMPDIR", &s.scratch().display().to_string());
    let r = s.local_with("backup", &plan, T0, opts);

    // Then restic got a config without the passphrase pointing at a decrypted key, both private, the directory is removed after the run,
    // and the passphrase was passed to openssl through the environment only
    let read = |n: &str| std::fs::read_to_string(seen.join(n)).unwrap_or_default();
    let config = read("config");
    assert!(!config.contains("pass_phrase") && !config.contains("hunter2") && !config.contains("/nowhere/"), "{config}");
    assert!(config.contains("key_file=") && config.contains("region=eu-frankfurt-1"), "{config}");
    assert_eq!(read("key"), "PLAIN-KEY", "{r:?}");
    assert_eq!(read("mode").trim(), "600");
    assert!(!Path::new(read("path").trim()).exists(), "the decrypted key outlived the run");
    assert!(read("openssl-args").contains("env:WF_OCI_KEY_PASS") && !read("openssl-args").contains("hunter2"), "{}", read("openssl-args"));
    assert_eq!(read("openssl-pass").trim(), "hunter2-passphrase");
    assert!(!r.stdout.contains("hunter2") && !r.stderr.contains("hunter2"), "{r:?}");
}
