//! The work of `wf vm state`, as it runs on agent-shell: restic snapshots of each state volume into one repository (the versioned bucket),
//! listed, verified by a real restore into a private scratch directory, and restored into an empty volume. Every verb answers one JSON
//! object; a failure carries what the tool said and the next step.
//!
//! A volume is two kinds of snapshot, told apart by the snapshot's host: `<app>.<volume>.files` (the volume read in place, its live SQLite
//! databases left out) and `<app>.<volume>.sqlite` (the consistent `.backup` copies of those databases, staged in the private scratch
//! directory, which is removed on every way out). Tags: `app:`, `volume:`, `kind:`, `run:<stamp>`, `ms:<epoch ms>`, `sqlite` on a files
//! snapshot whose run has databases, and `verified` once `verify` restored it and checked every database.
use super::plan::{growth_bytes, restic_excludes, AppPlan, Volume};
use super::sqlite;
use super::tools::{newest, Restic, Snapshot, BUCKET};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

/// Why `wf vm state restore` never touches agent-shell: the restore would write into the workspace this very process runs from.
pub const WORKSPACE_RESTORE: &str = "agent-shell's workspace is not restored by this verb: it holds the running wf and the session that runs it, so it is restored only by cloud-init on a fresh VM, before agent-shell starts";

/// Snapshots kept per volume and kind (`restic forget --keep-last`): two weeks of nightly runs, the length the bucket's budget is sized for.
pub const KEEP: usize = 14;

pub struct Ctx<'a> {
    pub restic: Restic,
    pub now_ms: i64,
    pub tmp: &'a Path,
    /// What the live repository may hold (`vmStateRepositoryBytes` of the committed budget).
    pub repository_budget_bytes: u64,
    /// What the bucket may hold with its previous object versions (`vmStateBytes` of the committed budget).
    pub bucket_budget_bytes: u64,
}

fn stamp_of(ms: i64) -> String {
    let digits: String = wf_kit::time::iso(ms).chars().filter(char::is_ascii_digit).take(14).collect();
    format!("{digits}Z")
}

fn host_of(app: &str, volume: &str, kind: &str) -> String {
    format!("{app}.{volume}.{kind}")
}

/// A path the plan may name: agent-shell's own workspace or a volume mounted under `/state` (or under `WF_VM_STATE_ROOT`, which the
/// flow tests point at a temp directory), never anything else.
fn safe_path(path: &str) -> bool {
    let root = std::env::var("WF_VM_STATE_ROOT").ok().filter(|r| !r.is_empty());
    let under = |root: &str| path.starts_with(&format!("{}/", root.trim_end_matches('/')));
    (path == "/workspace" || under("/state") || root.as_deref().is_some_and(under)) && !path.split('/').any(|part| part == "..")
}

fn mounted(volume: &Volume) -> Result<(), String> {
    if !safe_path(&volume.path) {
        return Err(format!("the plan names {:?}, which is not a mounted state volume", volume.path));
    }
    if !Path::new(&volume.path).is_dir() {
        return Err(format!("{} is not mounted in agent-shell (redeploy agent-shell so its compose file attaches the state volumes)", volume.path));
    }
    Ok(())
}

fn age_s(ctx: &Ctx, created_ms: Option<i64>) -> Value {
    created_ms.map_or(Value::Null, |created| json!(((ctx.now_ms - created) / 1000).max(0)))
}

fn run_args(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

/// The summary line of `restic backup --json`.
fn summary(stdout: &[u8]) -> Value {
    String::from_utf8_lossy(stdout).lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).find(|v| v["message_type"] == "summary").unwrap_or(Value::Null)
}

fn backup_snapshot(ctx: &Ctx, host: &str, tags: &[String], excludes: &[String], path: &Path) -> Result<Value, String> {
    let mut args: Vec<String> = vec!["backup".into(), "--json".into(), "--host".into(), host.into()];
    for tag in tags {
        args.extend(["--tag".into(), tag.clone()]);
    }
    for exclude in excludes {
        args.extend(["--exclude".into(), exclude.clone()]);
    }
    args.push(path.display().to_string());
    let out = ctx.restic.exec(&run_args(&args))?;
    if out.code != 0 {
        return Err(format!("restic backup of {} exited {}: {}", path.display(), out.code, out.stderr));
    }
    Ok(summary(&out.stdout))
}

/// Why an attempt failed, and whether a second one may succeed (a database appeared while the volume was read).
type Failure = (bool, String);

/// What a volume's read turned out to be, checked after it: no database that was not staged. If one appeared, the snapshots this attempt
/// made are forgotten, so nothing read live is kept.
fn judged(ctx: &Ctx, volume: &Volume, databases: &[String], snapshots: &[&Value]) -> Result<(), Failure> {
    let ids: Vec<&str> = snapshots.iter().filter_map(|s| s["snapshot_id"].as_str()).collect();
    let dropped = |why: String| -> Failure {
        let forgot = ctx.restic.forget(&ids).err().map_or(String::new(), |e| format!(" (and the snapshots could not be dropped: {e})"));
        (false, format!("{why}: its snapshots were dropped{forgot}"))
    };
    let root = Path::new(&volume.path);
    if sqlite::find(root, &volume.excludes) != databases {
        let (_, message) = dropped(format!("a SQLite database appeared or went in {} while it was read, so it could have been read live", volume.path));
        return Err((true, message));
    }
    Ok(())
}

/// One read of a volume: its SQLite databases found and staged as consistent copies, then the volume (without them) and the copies
/// snapshotted, then judged. The staging directory is gone before this returns, whether it succeeds or fails.
fn backup_attempt(ctx: &Ctx, app: &str, stamp: &str, volume: &Volume, size: u64) -> Result<Value, Failure> {
    let root = Path::new(&volume.path);
    let databases = sqlite::find(root, &volume.excludes);
    if !databases.is_empty() && !sqlite::available() {
        return Err((false, format!("{} holds SQLite databases ({}) and sqlite3 is not installed on agent-shell, so no consistent copy can be made (ship the agent-shell image that installs it)", volume.path, databases.iter().take(3).cloned().collect::<Vec<_>>().join(", "))));
    }
    let stage = ctx.tmp.join(format!("{app}-{}-sqlite", volume.name));
    let _ = std::fs::remove_dir_all(&stage);
    let made = (|| {
        let base = |kind: &str| vec![format!("app:{app}"), format!("volume:{}", volume.name), format!("kind:{kind}"), format!("run:{stamp}"), format!("ms:{}", ctx.now_ms)];
        let mut excludes = restic_excludes(volume);
        let mut sqlite_snapshot = Value::Null;
        for db in &databases {
            let copy = stage.join(db);
            std::fs::create_dir_all(copy.parent().unwrap_or(&stage)).map_err(|e| (false, e.to_string()))?;
            sqlite::snapshot(&root.join(db), &copy).map_err(|e| (false, format!("{db}: no consistent copy: {e}")))?;
            excludes.extend(["", "-wal", "-shm", "-journal"].iter().map(|suffix| format!("{}/{db}{suffix}", volume.path.trim_end_matches('/'))));
        }
        if !databases.is_empty() {
            sqlite_snapshot = backup_snapshot(ctx, &host_of(app, &volume.name, "sqlite"), &base("sqlite"), &[], &stage).map_err(|e| (false, e))?;
        }
        let mut tags = base("files");
        if !databases.is_empty() {
            tags.push("sqlite".into());
        }
        let files_snapshot = backup_snapshot(ctx, &host_of(app, &volume.name, "files"), &tags, &excludes, root).map_err(|e| (false, e))?;
        let taken: Vec<&Value> = [&sqlite_snapshot, &files_snapshot].into_iter().filter(|s| !s.is_null()).collect();
        judged(ctx, volume, &databases, &taken)?;
        Ok(json!({ "ok": true, "volume": volume.name, "measuredBytes": size, "maxBytes": volume.max_bytes, "databases": databases.len(), "snapshot": files_snapshot["snapshot_id"], "sqliteSnapshot": sqlite_snapshot["snapshot_id"], "dataAdded": files_snapshot["data_added"] }))
    })();
    let _ = std::fs::remove_dir_all(&stage);
    made
}

/// One volume: measured before anything is read, then read (a second time when a database appeared during the first read).
fn backup_volume(ctx: &Ctx, app: &str, stamp: &str, volume: &Volume) -> Result<Value, String> {
    mounted(volume)?;
    let size = sqlite::measure(Path::new(&volume.path), &volume.excludes)?;
    if size > volume.max_bytes {
        return Err(format!("{} holds {size} bytes beyond what the plan leaves out, over its {} byte cap: nothing was read (if the growth is right raise maxBytes of the volume in vm-apps.json and the budgets with it, else find what grew)", volume.path, volume.max_bytes));
    }
    match backup_attempt(ctx, app, stamp, volume, size) {
        Err((true, _)) => backup_attempt(ctx, app, stamp, volume, size).map_err(|(_, why)| format!("{why} (twice in a row)")),
        other => other.map_err(|(_, why)| why),
    }
}

fn backup_app(ctx: &Ctx, plan: &AppPlan) -> Value {
    let stamp = stamp_of(ctx.now_ms);
    let mut rows = Vec::new();
    let mut failed = false;
    for volume in &plan.volumes {
        match backup_volume(ctx, &plan.app, &stamp, volume) {
            Ok(row) => rows.push(row),
            Err(error) => {
                failed = true;
                rows.push(json!({ "ok": false, "volume": volume.name, "error": error, "next": format!("fix the cause named in error, then rerun wf vm state backup --app {}", plan.app) }));
            }
        }
    }
    json!({ "app": plan.app, "ok": !failed, "stamp": stamp, "volumes": rows })
}

/// The repository's total and the bucket's, previous object versions included, against their budgets: either over fails the run, neither is
/// ever allowed to grow silently.
fn repository_row(ctx: &Ctx, ok_so_far: Option<String>) -> Value {
    let mut row = json!({ "repo": ctx.restic.repo(), "budgetBytes": ctx.repository_budget_bytes, "bucketBudgetBytes": ctx.bucket_budget_bytes });
    let mut errors: Vec<String> = ok_so_far.into_iter().collect();
    match ctx.restic.stored_bytes() {
        Ok(bytes) => {
            row["bytes"] = json!(bytes);
            if bytes > ctx.repository_budget_bytes {
                errors.push(format!("the repository holds {bytes} bytes, over its {} byte budget (vm_state_repository_bytes: twice the sum of the volumes' maxBytes)", ctx.repository_budget_bytes));
            }
        }
        Err(e) => errors.push(e),
    }
    match ctx.restic.bucket_bytes() {
        Ok(bytes) => {
            row["bucketBytes"] = json!(bytes);
            if bytes > ctx.bucket_budget_bytes {
                errors.push(format!("the bucket holds {bytes} bytes counting the previous object versions a prune leaves behind, over its {} byte budget (vm_state_budget_bytes: four times the sum of the volumes' maxBytes)", ctx.bucket_budget_bytes));
            }
        }
        Err(e) => errors.push(e),
    }
    row["ok"] = json!(errors.is_empty());
    if !errors.is_empty() {
        row["error"] = json!(errors.join("; "));
        row["next"] = json!("find what grew (wf vm state list), lower its maxBytes or exclude it, or raise the budgets in vm-apps.json, vm-state.tf and clis/blob-cache/data/budget.json together; the previous versions leave the bucket within a day of a prune");
    }
    row
}

fn open(ctx: &Ctx) -> Result<bool, String> {
    ctx.restic.exists()
}

pub fn backup(ctx: &Ctx, plans: &[AppPlan]) -> Value {
    match open(ctx) {
        Ok(true) => {}
        Ok(false) => {
            if let Err(error) = ctx.restic.init() {
                return fail("backup", &format!("the repository {} does not exist and could not be created: {error}", ctx.restic.repo()), "check the OCI key on agent-shell (wf cache guard --local reports it) and that rclone and restic are installed");
            }
        }
        Err(error) => return fail("backup", &error, "check VM_STATE_RESTIC_PASSWORD, the OCI key on agent-shell and that restic and rclone are installed"),
    }
    if let Ok(bytes) = ctx.restic.stored_bytes() {
        if bytes > ctx.repository_budget_bytes {
            return fail("backup", &format!("the repository already holds {bytes} bytes, over its {} byte budget: nothing was backed up", ctx.repository_budget_bytes), "find what grew (wf vm state list), then lower its maxBytes or raise the budgets in vm-apps.json, vm-state.tf and clis/blob-cache/data/budget.json together");
        }
    }
    // The bucket as it is billed, previous versions included, plus the most this run can add: a run that could pass the budget never starts.
    match ctx.restic.bucket_bytes() {
        Ok(bytes) if bytes + growth_bytes(plans) > ctx.bucket_budget_bytes => {
            return fail("backup", &format!("the bucket holds {bytes} bytes counting previous object versions and this run could add {} (every volume at its cap): over its {} byte budget, so nothing was read", growth_bytes(plans), ctx.bucket_budget_bytes), "previous versions leave the bucket a day after a prune: rerun tomorrow, or find what grew (wf vm state list) and lower its maxBytes, or raise the budgets in vm-apps.json, vm-state.tf and clis/blob-cache/data/budget.json together");
        }
        Ok(_) => {}
        Err(error) => return fail("backup", &format!("the bucket's size could not be counted: {error}"), "check the OCI key on agent-shell (oci os object list-object-versions must work)"),
    }
    let apps: Vec<Value> = plans.iter().map(|p| backup_app(ctx, p)).collect();
    let kept = KEEP.to_string();
    let pruned = ctx.restic.run(&["forget", "--group-by", "host", "--keep-last", &kept, "--prune"]).err();
    envelope("backup", apps, Some(repository_row(ctx, pruned)))
}

/// The newest snapshot pair of a volume: the files snapshot and, when its run has databases, the sqlite snapshot of the same run.
struct Pair {
    files: Snapshot,
    sqlite: Option<Snapshot>,
}

fn latest_pair(ctx: &Ctx, app: &str, volume: &str, verified_only: bool) -> Result<Pair, String> {
    let all = ctx.restic.snapshots(Some(&host_of(app, volume, "files")))?;
    if all.is_empty() {
        return Err(format!("the repository holds no snapshot of {app}/{volume}"));
    }
    let files = newest(all.iter().filter(|s| !verified_only || s.has("verified")).cloned()).ok_or_else(|| format!("{app}/{volume} has {} snapshots and none is verified (wf vm state verify tags the ones it restored)", all.len()))?;
    let sqlite = if files.has("sqlite") {
        let run = files.tag("run").unwrap_or_default().to_string();
        let found = ctx.restic.snapshots(Some(&host_of(app, volume, "sqlite")))?.into_iter().find(|s| s.tag("run") == Some(run.as_str()));
        Some(found.ok_or_else(|| format!("{app}/{volume}: the snapshot of run {run} has SQLite databases but the repository holds no sqlite snapshot of that run"))?)
    } else {
        None
    };
    Ok(Pair { files, sqlite })
}

fn restore_snapshot(ctx: &Ctx, snap: &Snapshot, target: &Path) -> Result<(), String> {
    let path = snap.paths.first().ok_or_else(|| format!("snapshot {} records no path", snap.id))?;
    let spec = format!("{}:{path}", snap.id);
    ctx.restic.run(&["restore", &spec, "--target", &target.display().to_string(), "--verify"]).map(|_| ())
}

/// A real restore of the newest snapshots into a private scratch directory (removed on every way out), `PRAGMA integrity_check` on every
/// database in it, and only then the `verified` tag.
fn verify_volume(ctx: &Ctx, app: &str, volume: &Volume) -> Result<Value, String> {
    let pair = latest_pair(ctx, app, &volume.name, false)?;
    let scratch = ctx.tmp.join(format!("verify-{app}-{}", volume.name));
    let checked = (|| {
        std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
        restore_snapshot(ctx, &pair.files, &scratch)?;
        if let Some(copies) = &pair.sqlite {
            restore_snapshot(ctx, copies, &scratch)?;
        }
        let databases = sqlite::find(&scratch, &[]);
        if pair.sqlite.is_some() && databases.is_empty() {
            return Err(format!("{app}/{}: the snapshot says it holds SQLite databases and the restore has none", volume.name));
        }
        if !databases.is_empty() && !sqlite::available() {
            return Err(format!("{app}/{} holds SQLite databases ({}) and sqlite3 is not installed on agent-shell to check them", volume.name, databases.iter().take(3).cloned().collect::<Vec<_>>().join(", ")));
        }
        for db in &databases {
            sqlite::integrity(&scratch.join(db)).map_err(|e| format!("{app}/{}: {db}: {e}", volume.name))?;
        }
        Ok(databases.len())
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    let databases = checked?;
    let mut ids = vec![pair.files.id.as_str()];
    ids.extend(pair.sqlite.as_ref().map(|s| s.id.as_str()));
    let mut args = vec!["tag", "--add", "verified"];
    args.extend(ids);
    ctx.restic.run(&args)?;
    Ok(json!({ "ok": true, "volume": volume.name, "snapshot": pair.files.id, "run": pair.files.tag("run"), "ageSeconds": age_s(ctx, pair.files.created_ms()), "databasesChecked": databases }))
}

fn envelope(verb: &str, apps: Vec<Value>, repository: Option<Value>) -> Value {
    let ok = !apps.is_empty() && apps.iter().all(|a| a["ok"] == true) && repository.as_ref().is_none_or(|r| r["ok"] == true);
    let mut out = json!({ "event": "vm-state", "verb": verb, "ok": ok, "bucket": BUCKET, "apps": apps });
    if let Some(repository) = repository {
        out["repository"] = repository;
    }
    if !ok {
        out["next"] = json!(if out["apps"].as_array().is_some_and(Vec::is_empty) { "no app declares stateVolumes in infra/oracle-cloud/vm-apps.json" } else { "read the failed rows: each carries the error and the next step" });
    }
    out
}

fn fail(verb: &str, error: &str, next: &str) -> Value {
    json!({ "event": "vm-state", "verb": verb, "ok": false, "bucket": BUCKET, "apps": [], "error": error, "next": next })
}

pub fn list(ctx: &Ctx, plans: &[AppPlan]) -> Value {
    match open(ctx) {
        Ok(true) => {}
        Ok(false) => return fail("list", "no backup repository exists yet: nothing was ever backed up", "wf vm state backup"),
        Err(error) => return fail("list", &error, "check VM_STATE_RESTIC_PASSWORD and the OCI key on agent-shell"),
    }
    let rows = plans
        .iter()
        .map(|plan| {
            let volumes: Vec<Value> = plan
                .volumes
                .iter()
                .map(|v| match ctx.restic.snapshots(Some(&host_of(&plan.app, &v.name, "files"))) {
                    Err(error) => json!({ "volume": v.name, "ok": false, "error": error }),
                    Ok(mut found) => {
                        found.sort_by_key(|s| std::cmp::Reverse(s.created_ms().unwrap_or(0)));
                        let rows: Vec<Value> = found.iter().map(|s| json!({ "snapshot": s.id, "run": s.tag("run"), "ageSeconds": age_s(ctx, s.created_ms()), "verified": s.has("verified"), "databases": s.has("sqlite") })).collect();
                        let ok = !rows.is_empty();
                        json!({ "volume": v.name, "ok": ok, "snapshots": rows, "latest": rows.first().cloned().unwrap_or(Value::Null) })
                    }
                })
                .collect();
            let ok = volumes.iter().all(|v| v["ok"] == true);
            let mut row = json!({ "app": plan.app, "ok": ok, "volumes": volumes });
            if !ok {
                row["error"] = json!("a volume has no snapshot");
                row["next"] = json!(format!("wf vm state backup --app {}", plan.app));
            }
            row
        })
        .collect();
    envelope("list", rows, None)
}

/// Every volume `vm-apps.json` declares, restored from its newest snapshot into a scratch directory and checked. A volume with no
/// snapshot, a snapshot that does not restore and a database that fails `integrity_check` each fail the app.
pub fn verify(ctx: &Ctx, plans: &[AppPlan]) -> Value {
    match open(ctx) {
        Ok(true) => {}
        Ok(false) => return fail("verify", "no backup repository exists: nothing was ever backed up", "wf vm state backup"),
        Err(error) => return fail("verify", &error, "check VM_STATE_RESTIC_PASSWORD and the OCI key on agent-shell"),
    }
    let structure = ctx.restic.run(&["check", "--read-data-subset", "5%"]).err();
    let rows = plans
        .iter()
        .map(|plan| {
            let mut volumes = Vec::new();
            let mut ages = Vec::new();
            for volume in &plan.volumes {
                match verify_volume(ctx, &plan.app, volume) {
                    Ok(row) => {
                        ages.push(row["ageSeconds"].as_u64());
                        volumes.push(row);
                    }
                    Err(error) => volumes.push(json!({ "ok": false, "volume": volume.name, "error": error })),
                }
            }
            let failed: Vec<String> = volumes.iter().filter(|v| v["ok"] != true).map(|v| format!("{}: {}", v["volume"].as_str().unwrap_or("?"), v["error"].as_str().unwrap_or("failed"))).collect();
            let age = if ages.iter().all(Option::is_some) { ages.iter().flatten().max().copied() } else { None };
            let mut row = json!({ "app": plan.app, "ok": failed.is_empty(), "volumes": volumes, "ageSeconds": age });
            if !failed.is_empty() {
                row["error"] = json!(failed.join("; "));
                row["next"] = json!(format!("the newest backup of {} is damaged or missing: wf vm state backup --app {0} writes a fresh one", plan.app));
            }
            row
        })
        .collect();
    let repository = repository_row(ctx, structure.map(|e| format!("restic check: {e}")));
    envelope("verify", rows, Some(repository))
}

/// What a volume holds that a restore would sit on top of: entries other than `lost+found` and the ones a backup leaves out.
fn occupants(volume: &Volume) -> Vec<String> {
    let skipped: BTreeSet<String> = volume.excludes.iter().map(|e| e.trim_start_matches("./").to_string()).collect();
    let Ok(read) = std::fs::read_dir(&volume.path) else { return Vec::new() };
    let mut found: Vec<String> = read.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != "lost+found" && !skipped.contains(n)).collect();
    found.sort();
    found
}

/// Restores the newest verified snapshots into the app's volumes, and only into empty ones: there is no way to extract over data. The caller
/// has already established that the app is not running.
fn restore_app(ctx: &Ctx, plan: &AppPlan) -> Value {
    if plan.app == super::WORKSPACE_APP {
        return json!({ "app": plan.app, "ok": false, "error": WORKSPACE_RESTORE, "next": "none by hand: the workspace is restored only by cloud-init of a fresh VM (docs/runbooks/vm-recreate.md, \"Restoring state by hand\")" });
    }
    let mut chosen = Vec::new();
    for volume in &plan.volumes {
        if let Err(error) = mounted(volume) {
            return json!({ "app": plan.app, "ok": false, "error": error, "next": "redeploy agent-shell so its compose file attaches the state volumes" });
        }
        let present = occupants(volume);
        if !present.is_empty() {
            return json!({ "app": plan.app, "ok": false, "refused": volume.name, "error": format!("{} is not empty ({} entries, e.g. {}): nothing was written", volume.path, present.len(), present.iter().take(3).cloned().collect::<Vec<_>>().join(", ")), "next": "a restore only fills an empty volume: remove the volume's contents with the app stopped (docs/runbooks/vm-recreate.md, \"Restoring state by hand\")" });
        }
        match latest_pair(ctx, &plan.app, &volume.name, true) {
            Ok(pair) => chosen.push((volume, pair)),
            Err(error) => return json!({ "app": plan.app, "ok": false, "error": error, "next": "wf vm state list shows the snapshots; wf vm state verify tags the ones that restore" }),
        }
    }
    let mut restored = Vec::new();
    for (volume, pair) in &chosen {
        let target = Path::new(&volume.path);
        let result = restore_snapshot(ctx, &pair.files, target).and_then(|()| pair.sqlite.as_ref().map_or(Ok(()), |copies| restore_snapshot(ctx, copies, target)));
        if let Err(error) = result {
            return json!({ "app": plan.app, "ok": false, "restored": restored, "error": format!("restoring into {}: {error}", volume.path), "next": "the volume may hold part of the snapshot: empty it (app stopped) and rerun" });
        }
        restored.push(json!({ "volume": volume.name, "snapshot": pair.files.id, "run": pair.files.tag("run") }));
    }
    json!({ "app": plan.app, "ok": true, "restored": restored })
}

pub fn restore(ctx: &Ctx, plans: &[AppPlan]) -> Value {
    match open(ctx) {
        Ok(true) => envelope("restore", plans.iter().map(|p| restore_app(ctx, p)).collect(), None),
        Ok(false) => fail("restore", "no backup repository exists: nothing was ever backed up", "wf vm state backup"),
        Err(error) => fail("restore", &error, "check VM_STATE_RESTIC_PASSWORD and the OCI key on agent-shell"),
    }
}
