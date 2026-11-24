//! Why an Nx task ran when it could have replayed. A miss gets exactly one reason, decided from facts the jobs recorded (`NX_TASKS`, `CI_CACHE_EVENTS`):
//! whether the cache restore worked, whether the task's hash was in the restored `cache/` directory and in Nx's own `cache_outputs` table, and which
//! earlier runs executed this very hash, in which workflow and shard, and whether their save landed. `classify` is the one decision (the job log's
//! `wf: nx why` and the report job's `wf ci cache-misses` both call it); `classify_db` gathers the facts from the CI database; `hit_rates` is the
//! per-stage view of how often an exact hash seen before replayed.
use rusqlite::Connection;
use std::collections::BTreeMap;

/// The targets Nx is told not to cache (`cache: false` in `tooling/nx/cargo-crates.js`): their tasks always run, by design.
const UNCACHED_TARGETS: &[&str] = &["cargo-verify-deployed"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// No earlier run executed this hash: the inputs changed, or it is the first run of the task.
    NeverStored,
    StoredByOtherShard,
    StoredByOtherWorkflow,
    /// An earlier run of this lineage executed the hash and its save failed, or its job ended before it.
    EarlierSaveFailed,
    /// An earlier run of this lineage executed the hash and the save gate skipped the save.
    EarlierSaveSkipped,
    RestoreFailed,
    RestoreTimeout,
    UncacheableByDesign,
    /// The restored `cache/` directory and Nx's table hold the hash, and the task ran anyway.
    RestoredButReran,
    /// The restored directory holds the hash, Nx's table has no row for it.
    RestoredWithoutDbRow,
    /// The restore matched an entry older than the run that stored the hash (a race, or a lost update).
    RestoredOlderEntry,
    /// The earlier save landed, the restore is not older, and the directory still lacks the hash (evicted, or overwritten).
    StoredEntryMissing,
    Unknown,
}

impl Reason {
    pub const ALL: [Reason; 13] = [
        Reason::NeverStored,
        Reason::StoredByOtherShard,
        Reason::StoredByOtherWorkflow,
        Reason::EarlierSaveFailed,
        Reason::EarlierSaveSkipped,
        Reason::RestoreFailed,
        Reason::RestoreTimeout,
        Reason::UncacheableByDesign,
        Reason::RestoredButReran,
        Reason::RestoredWithoutDbRow,
        Reason::RestoredOlderEntry,
        Reason::StoredEntryMissing,
        Reason::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Reason::NeverStored => "never-stored",
            Reason::StoredByOtherShard => "stored-by-other-shard",
            Reason::StoredByOtherWorkflow => "stored-by-other-workflow",
            Reason::EarlierSaveFailed => "earlier-save-failed",
            Reason::EarlierSaveSkipped => "earlier-save-skipped",
            Reason::RestoreFailed => "restore-failed",
            Reason::RestoreTimeout => "restore-timeout",
            Reason::UncacheableByDesign => "uncacheable-by-design",
            Reason::RestoredButReran => "restored-but-reran",
            Reason::RestoredWithoutDbRow => "restored-without-db-row",
            Reason::RestoredOlderEntry => "restored-older-entry",
            Reason::StoredEntryMissing => "stored-entry-missing",
            Reason::Unknown => "unknown",
        }
    }

    pub fn parse(name: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.as_str() == name)
    }

    /// A miss nobody could have avoided: the hash is new, or the target is never cached. Every other reason is cache work that went to waste.
    pub fn legitimate(self) -> bool {
        matches!(self, Reason::NeverStored | Reason::UncacheableByDesign)
    }
}

/// How an earlier execution of the hash relates to the run that missed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    SameLineage,
    OtherShard,
    OtherWorkflow,
    Unknown,
}

/// What became of the earlier run's save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Save {
    Saved,
    Skipped(String),
    Failed(String),
    /// The earlier run recorded no cache events (before they existed).
    Unrecorded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Earlier {
    pub run: String,
    pub relation: Relation,
    pub save: Save,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restore {
    /// `ok`, `miss`, `timeout`, `error` or `skipped`.
    pub outcome: String,
    pub matched_key: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub task: String,
    pub restore: Option<Restore>,
    pub dir_has_hash: Option<bool>,
    pub db_has_hash: Option<bool>,
    /// Newest first.
    pub earlier: Vec<Earlier>,
}

/// The run id a cache key names: `nx-<stage>-<os>-<arch>-<workflow>-<run>-<attempt>`.
pub fn run_of_key(key: &str) -> Option<&str> {
    let mut parts = key.rsplit('-');
    parts.next()?;
    parts.next().filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn target_of(task: &str) -> &str {
    task.rsplit_once(':').map_or(task, |(_, target)| target)
}

/// The one decision: the reason this task ran and a sentence that names the evidence.
pub fn classify(f: &Facts) -> (Reason, String) {
    if UNCACHED_TARGETS.contains(&target_of(&f.task)) {
        return (Reason::UncacheableByDesign, format!("the target {} is declared cache: false", target_of(&f.task)));
    }
    if let Some(restore) = &f.restore {
        let why = restore.reason.as_deref().unwrap_or("no reason recorded");
        match restore.outcome.as_str() {
            "timeout" => return (Reason::RestoreTimeout, format!("the Nx cache restore timed out, the job ran on an empty cache ({why})")),
            "error" => return (Reason::RestoreFailed, format!("the Nx cache restore failed, the job ran on an empty cache ({why})")),
            _ => {}
        }
    }
    if f.earlier.is_empty() {
        return (Reason::NeverStored, "no earlier run executed this hash".to_string());
    }
    let known = |what: &str| match f.dir_has_hash {
        Some(true) => format!("{what}: the restored cache directory holds the hash"),
        Some(false) => format!("{what}: the restored cache directory lacks the hash"),
        None => format!("{what}: the restored cache directory was not checked"),
    };
    if let Some(same) = f.earlier.iter().find(|e| e.relation == Relation::SameLineage) {
        let db = match f.db_has_hash {
            Some(true) => "Nx's table has its row",
            Some(false) => "Nx's table has no row for it",
            None => "Nx's table was not checked",
        };
        match (f.dir_has_hash, f.db_has_hash) {
            (Some(true), Some(false)) => return (Reason::RestoredWithoutDbRow, format!("run {} executed it; the restored cache directory holds the hash and {db}, and the task ran", same.run)),
            (Some(true), _) => return (Reason::RestoredButReran, format!("run {} executed it; the restored cache directory holds the hash, {db}, and the task ran anyway", same.run)),
            _ => {}
        }
        return match &same.save {
            Save::Skipped(why) => (Reason::EarlierSaveSkipped, format!("run {} executed it and its save was skipped ({why}); {}", same.run, known("restored"))),
            Save::Failed(why) => (Reason::EarlierSaveFailed, format!("run {} executed it and its save failed ({why}); {}", same.run, known("restored"))),
            Save::Saved => {
                let restored = f.restore.as_ref().and_then(|r| r.matched_key.as_deref()).and_then(run_of_key);
                match (restored.and_then(|r| r.parse::<u64>().ok()), same.run.parse::<u64>().ok()) {
                    (Some(have), Some(stored)) if have < stored => (Reason::RestoredOlderEntry, format!("run {stored} saved it after this job restored the entry of run {have}")),
                    (Some(have), Some(stored)) => (Reason::StoredEntryMissing, format!("run {stored} saved it, this job restored the entry of run {have}, and the cache directory lacks the hash")),
                    _ => (Reason::Unknown, format!("run {} saved it; this job recorded no restore to compare with", same.run)),
                }
            }
            Save::Unrecorded => (Reason::Unknown, format!("run {} executed it in this lineage and recorded no cache events; {}", same.run, known("restored"))),
        };
    }
    if let Some(other) = f.earlier.iter().find(|e| e.relation == Relation::OtherShard) {
        return (Reason::StoredByOtherShard, format!("run {} executed it in another shard of the cache partition", other.run));
    }
    if let Some(other) = f.earlier.iter().find(|e| e.relation == Relation::OtherWorkflow) {
        return (Reason::StoredByOtherWorkflow, format!("run {} executed it in another workflow", other.run));
    }
    (Reason::Unknown, format!("run {} executed it, and the lineage of that run was not recorded", f.earlier[0].run))
}

/// A miss with its reason.
#[derive(Debug, Clone)]
pub struct Miss {
    pub run: String,
    pub job: String,
    pub cache_stage: Option<String>,
    pub task: String,
    pub hash: String,
    pub ms: i64,
    pub reason: Reason,
    pub detail: String,
}

fn sql(e: rusqlite::Error) -> String {
    format!("ci database: {e}")
}

fn stage_key(stage: &str, shard: &str) -> String {
    if shard.is_empty() {
        stage.to_string()
    } else {
        format!("{stage}-{shard}")
    }
}

struct Event {
    stage_key: String,
    outcome: String,
    matched: Option<String>,
    reason: Option<String>,
}

fn events_of(conn: &Connection, run: &str, phase: &str) -> Result<Vec<Event>, String> {
    let mut stmt = conn
        .prepare("SELECT STAGE, SHARD, OUTCOME, KEY_MATCHED, REASON FROM CI_CACHE_EVENTS WHERE RUN_ID = ?1 AND LAYER = 'nx' AND PHASE = ?2 ORDER BY TS")
        .map_err(sql)?;
    let rows = stmt
        .query_map([run, phase], |r| Ok(Event { stage_key: stage_key(&r.get::<_, String>(0)?, &r.get::<_, String>(1)?), outcome: r.get(2)?, matched: r.get(3)?, reason: r.get(4)? }))
        .map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// The event of the cache stage `stage`, or the run's only one when the stage is not known.
fn pick<'a>(events: &'a [Event], stage: Option<&str>) -> Option<&'a Event> {
    match stage {
        Some(s) => events.iter().rev().find(|e| e.stage_key == s),
        None if events.len() == 1 => events.first(),
        None => None,
    }
}

fn relation(mine: (&Option<String>, &Option<String>), theirs: (&Option<String>, &Option<String>)) -> Relation {
    let ((wf, stage), (their_wf, their_stage)) = (mine, theirs);
    match (wf, their_wf) {
        (Some(a), Some(b)) if a != b => Relation::OtherWorkflow,
        (Some(_), Some(_)) => match (stage, their_stage) {
            (Some(a), Some(b)) if a == b => Relation::SameLineage,
            (Some(_), Some(_)) => Relation::OtherShard,
            _ => Relation::Unknown,
        },
        _ => Relation::Unknown,
    }
}

fn save_of(events: &[Event], restores: &[Event], stage: Option<&str>) -> Save {
    match pick(events, stage) {
        Some(e) => match e.outcome.as_str() {
            "ok" => Save::Saved,
            "skipped" => Save::Skipped(e.reason.clone().unwrap_or_else(|| "no reason recorded".to_string())),
            _ => Save::Failed(e.reason.clone().unwrap_or_else(|| e.outcome.clone())),
        },
        None if pick(restores, stage).is_some() => Save::Failed("no save was recorded after the restore: the job ended before its save step".to_string()),
        None => Save::Unrecorded,
    }
}

/// Classifies the tasks that ran (`ran` rows of `NX_TASKS`) of `run`, or of every run when `None`. With `store`, a row that has no reason yet is given one
/// in `MISS_REASON` and `MISS_DETAIL` and rows that have one are left alone; without it every `ran` row is classified and nothing is written.
pub fn classify_db(conn: &Connection, run: Option<&str>, store: bool) -> Result<Vec<Miss>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT n.RUN_ID, n.TS, n.JOB, n.TASK, n.HASH, n.MS, COALESCE(n.WORKFLOW, p.WORKFLOW), n.CACHE_STAGE, n.RESTORED_KEY, n.DIR_HAS_HASH, n.DB_HAS_HASH
             FROM NX_TASKS n LEFT JOIN CI_RUN_PROFILE p ON p.RUN_ID = n.RUN_ID
             WHERE n.OUTCOME = 'ran' AND (?1 IS NULL OR n.RUN_ID = ?1) AND (?2 = 0 OR n.MISS_REASON IS NULL) ORDER BY n.TS",
        )
        .map_err(sql)?;
    type Row = (String, i64, String, String, String, i64, Option<String>, Option<String>, Option<String>, Option<i64>, Option<i64>);
    let ran: Vec<Row> = stmt
        .query_map(rusqlite::params![run, i64::from(store)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    drop(stmt);
    let mut earlier_stmt = conn
        .prepare(
            "SELECT e.RUN_ID, COALESCE(e.WORKFLOW, p.WORKFLOW), e.CACHE_STAGE FROM NX_TASKS e LEFT JOIN CI_RUN_PROFILE p ON p.RUN_ID = e.RUN_ID
             WHERE e.TASK = ?1 AND e.HASH = ?2 AND e.PASSED = 1 AND e.OUTCOME = 'ran' AND e.RUN_ID <> ?3 AND e.TS <= ?4 ORDER BY e.TS DESC LIMIT 20",
        )
        .map_err(sql)?;
    let mut cache: BTreeMap<(String, &'static str), std::rc::Rc<Vec<Event>>> = BTreeMap::new();
    let mut events = |run: &str, phase: &'static str| -> Result<std::rc::Rc<Vec<Event>>, String> {
        if let Some(found) = cache.get(&(run.to_string(), phase)) {
            return Ok(found.clone());
        }
        let loaded = std::rc::Rc::new(events_of(conn, run, phase)?);
        cache.insert((run.to_string(), phase), loaded.clone());
        Ok(loaded)
    };
    let mut out = Vec::new();
    for (run_id, ts, job, task, hash, ms, workflow, cache_stage, restored_key, dir, db) in ran {
        let restores = events(&run_id, "restore")?;
        let restore = pick(&restores, cache_stage.as_deref()).map(|e| Restore { outcome: e.outcome.clone(), matched_key: e.matched.clone().or_else(|| restored_key.clone()), reason: e.reason.clone() });
        let mut earlier = Vec::new();
        let candidates: Vec<(String, Option<String>, Option<String>)> = earlier_stmt
            .query_map(rusqlite::params![task, hash, run_id, ts], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(sql)?
            .collect::<Result<_, _>>()
            .map_err(sql)?;
        for (their_run, their_wf, their_stage) in candidates {
            let (saves, their_restores) = (events(&their_run, "save")?, events(&their_run, "restore")?);
            earlier.push(Earlier { relation: relation((&workflow, &cache_stage), (&their_wf, &their_stage)), save: save_of(&saves, &their_restores, their_stage.as_deref()), run: their_run });
        }
        let facts = Facts { task: task.clone(), restore, dir_has_hash: dir.map(|v| v != 0), db_has_hash: db.map(|v| v != 0), earlier };
        let (reason, detail) = classify(&facts);
        if store {
            conn.execute("UPDATE NX_TASKS SET MISS_REASON = ?1, MISS_DETAIL = ?2 WHERE RUN_ID = ?3 AND TASK = ?4 AND HASH = ?5", rusqlite::params![reason.as_str(), detail, run_id, task, hash]).map_err(sql)?;
        }
        out.push(Miss { run: run_id, job, cache_stage, task, hash, ms, reason, detail });
    }
    Ok(out)
}

/// A stage's tasks over the last runs.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRate {
    pub stage: String,
    pub tasks: i64,
    pub replayed: i64,
    /// Tasks that ran under a hash no earlier run had executed: the inputs changed.
    pub inputs_changed: i64,
    /// Tasks that ran under a hash an earlier run had already executed and passed: the misses that must not happen.
    pub seen_before: i64,
}

impl StageRate {
    /// Of the tasks whose exact hash was seen before, the share that replayed: the figure that must be near 100.
    pub fn exact_rate(&self) -> Option<f64> {
        let denominator = self.replayed + self.seen_before;
        (denominator > 0).then(|| self.replayed as f64 * 100.0 / denominator as f64)
    }

    pub fn hit_rate(&self) -> Option<f64> {
        (self.tasks > 0).then(|| self.replayed as f64 * 100.0 / self.tasks as f64)
    }
}

/// The stages of the last `runs` runs of the ledger, each with its tasks split by why they missed or that they replayed.
pub fn hit_rates(conn: &Connection, runs: usize) -> Result<Vec<StageRate>, String> {
    let mut stmt = conn
        .prepare(
            "WITH recent AS (SELECT RUN_ID FROM NX_TASKS GROUP BY RUN_ID ORDER BY MAX(TS) DESC LIMIT ?1)
             SELECT n.JOB, COUNT(*), COALESCE(SUM(n.OUTCOME = 'hit'), 0),
               COALESCE(SUM(n.OUTCOME = 'ran' AND NOT EXISTS (SELECT 1 FROM NX_TASKS e WHERE e.TASK = n.TASK AND e.HASH = n.HASH AND e.PASSED = 1 AND e.OUTCOME = 'ran' AND e.RUN_ID <> n.RUN_ID AND e.TS <= n.TS)), 0),
               COALESCE(SUM(n.OUTCOME = 'ran' AND EXISTS (SELECT 1 FROM NX_TASKS e WHERE e.TASK = n.TASK AND e.HASH = n.HASH AND e.PASSED = 1 AND e.OUTCOME = 'ran' AND e.RUN_ID <> n.RUN_ID AND e.TS <= n.TS)), 0)
             FROM NX_TASKS n WHERE n.RUN_ID IN (SELECT RUN_ID FROM recent) GROUP BY n.JOB ORDER BY n.JOB",
        )
        .map_err(sql)?;
    let rows = stmt.query_map([runs as i64], |r| Ok(StageRate { stage: r.get(0)?, tasks: r.get(1)?, replayed: r.get(2)?, inputs_changed: r.get(3)?, seen_before: r.get(4)? })).map_err(sql)?;
    rows.collect::<Result<_, _>>().map_err(sql)
}

/// Misses grouped by reason: (reason, tasks, seconds), the worst first.
pub fn breakdown(misses: &[Miss]) -> Vec<(Reason, usize, f64)> {
    let mut by: BTreeMap<Reason, (usize, f64)> = BTreeMap::new();
    for m in misses {
        let entry = by.entry(m.reason).or_default();
        entry.0 += 1;
        entry.1 += m.ms as f64 / 1000.0;
    }
    let mut rows: Vec<(Reason, usize, f64)> = by.into_iter().map(|(r, (n, s))| (r, n, s)).collect();
    rows.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    rows
}
