//! `wf ci tidy`: frees the runner's disk before a run needs it. When the volume is over its high-water mark it removes what can be built
//! again, the cheapest to give up first, until the volume is under its low-water mark: build artifacts of a live target nothing has used for
//! a week, old `wf` installs under `/opt/clis` (the published one and the newest few stay), `target` directories nothing has built in for a
//! week, and last of all Rust incremental build directories (the `wf` build's own is what keeps it at 30 seconds instead of 3 minutes).
//! Under the mark it removes nothing. It prints what the volume holds either way (the biggest directories under the runner's roots), so
//! a full disk can be explained from the job summary.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use wf_kit::args::{parse_spec, Positional, Spec};

const SPEC: Spec = Spec { values: &["root", "installs", "high", "low"], flags: &["dry-run"], positional: Positional::None, ..Spec::NONE };
const HIGH_PCT: u64 = 70;
const LOW_PCT: u64 = 60;
/// Installs of `wf` kept besides the published one.
const KEEP_INSTALLS: usize = 3;
const STALE_TARGET: Duration = Duration::from_secs(7 * 86_400);
/// A build artifact in a live target directory that nothing has read or written for this long belongs to a version of the code that has gone
/// (every change leaves the test binaries of its crates behind, tens of MB each, and cargo never collects them).
const STALE_ARTIFACT: Duration = Duration::from_secs(7 * 86_400);
const MAX_DEPTH: usize = 7;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub path: PathBuf,
    pub bytes: u64,
    /// 0 is the cheapest to give up. The `wf` build's own incremental state is rank 3, the last resort: it is 100 MB and the difference between
    /// a 30 second build and a 3 minute one of the tool every job runs.
    pub rank: u8,
    pub why: &'static str,
    /// Seconds since it was last written, the older going first within a rank.
    pub age_secs: u64,
    /// Only these files of `path` go (a directory whose other files stay); empty for the whole path.
    pub files: Vec<PathBuf>,
}

/// What to remove: nothing while `used` is under `high` percent of `total`; otherwise candidates in rank order (oldest first within a
/// rank) until the removal would bring the volume to `low` percent.
pub fn plan(used: u64, total: u64, high: u64, low: u64, mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    let pct = |bytes: u64| bytes.saturating_mul(100).checked_div(total).unwrap_or(0);
    if pct(used) < high {
        return Vec::new();
    }
    candidates.sort_by_key(|c| (c.rank, std::cmp::Reverse(c.age_secs), std::cmp::Reverse(c.bytes)));
    let (mut left, mut out) = (used, Vec::new());
    for c in candidates {
        if pct(left) <= low {
            break;
        }
        left = left.saturating_sub(c.bytes);
        out.push(c);
    }
    out
}

pub fn dir_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).map(|rd| rd.filter_map(Result::ok).map(|e| dir_bytes(&e.path())).sum()).unwrap_or(0)
}

fn age(path: &Path) -> u64 {
    std::fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).map_or(0, |d| d.as_secs())
}

fn newest_child_age(path: &Path) -> u64 {
    std::fs::read_dir(path).map(|rd| rd.filter_map(Result::ok).map(|e| age(&e.path())).min().unwrap_or_else(|| age(path))).unwrap_or_else(|_| age(path))
}

/// Cargo build output under `root`: the `incremental` directory of every profile (a directory with `.fingerprint` or `deps` beside it),
/// and a whole target directory (cargo marks it with `CACHEDIR.TAG`, whatever it is called) that nothing has built in for a week.
fn cargo_candidates(root: &Path, depth: usize, out: &mut Vec<Candidate>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let is_target = root.join("CACHEDIR.TAG").is_file() && (root.join("debug").is_dir() || root.join("release").is_dir() || root.join(".rustc_info.json").is_file() || depth > 0);
    if is_target && newest_child_age(root) > STALE_TARGET.as_secs() {
        out.push(Candidate { bytes: dir_bytes(root), age_secs: age(root), path: root.to_path_buf(), rank: 2, why: "cargo target nothing has built in for a week", files: Vec::new() });
        return;
    }
    if is_target {
        stale_artifacts(root, out);
    }
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "incremental" && (root.join(".fingerprint").is_dir() || root.join("deps").is_dir()) {
            out.push(Candidate { bytes: dir_bytes(&path), age_secs: age(&path), path, rank: 3, why: "rust incremental build state", files: Vec::new() });
        } else if depth < MAX_DEPTH && !matches!(name.as_str(), "node_modules" | ".git" | ".nx" | "deps" | "build" | ".fingerprint") {
            cargo_candidates(&path, depth + 1, out);
        }
    }
}

/// The files of every `deps` directory of a live target (`<profile>/deps` and `<triple>/<profile>/deps`) that nothing has read or written for a
/// week: one candidate per directory, listing its files. Only the directory itself is read, never a file's content.
fn stale_artifacts(target: &Path, out: &mut Vec<Candidate>) {
    let subdirs = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir).into_iter().flatten().filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink())).map(|e| e.path()).collect()
    };
    let mut deps: Vec<PathBuf> = Vec::new();
    for first in subdirs(target) {
        deps.push(first.join("deps"));
        deps.extend(subdirs(&first).into_iter().map(|second| second.join("deps")));
    }
    for dir in deps.into_iter().filter(|d| d.is_dir()) {
        let (mut files, mut bytes, mut newest_gone) = (Vec::new(), 0u64, u64::MAX);
        for entry in std::fs::read_dir(&dir).into_iter().flatten().filter_map(Result::ok) {
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let idle = [meta.modified(), meta.accessed()].into_iter().filter_map(Result::ok).filter_map(|t| SystemTime::now().duration_since(t).ok()).map(|d| d.as_secs()).min().unwrap_or(0);
            if idle > STALE_ARTIFACT.as_secs() {
                bytes += meta.len();
                newest_gone = newest_gone.min(idle);
                files.push(entry.path());
            }
        }
        if !files.is_empty() {
            out.push(Candidate { bytes, age_secs: newest_gone, path: dir, rank: 0, why: "build artifacts of the target nothing has used for a week", files });
        }
    }
}

/// `wf` installs under `installs` (`/opt/clis/<tree>`): all but the published one (`current`'s target) and the newest few.
fn install_candidates(installs: &Path, out: &mut Vec<Candidate>) {
    let current = std::fs::read_link(installs.join("current")).ok().map(|t| if t.is_absolute() { t } else { installs.join(t) });
    let mut trees: Vec<(u64, PathBuf)> = std::fs::read_dir(installs)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()) && e.file_name() != "current")
        .map(|e| (age(&e.path()), e.path()))
        .filter(|(_, p)| current.as_ref().is_none_or(|c| c != p))
        .collect();
    trees.sort();
    for (age_secs, path) in trees.into_iter().skip(KEEP_INSTALLS) {
        out.push(Candidate { bytes: dir_bytes(&path), age_secs, path, rank: 1, why: "an old wf install", files: Vec::new() });
    }
}

fn volume(path: &Path) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path.to_str()?).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    (unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0).then(|| {
        let (total, free) = (st.f_blocks as u64 * st.f_frsize, st.f_bfree as u64 * st.f_frsize);
        (total - free, total)
    })
}

/// The biggest directories two levels under `root`.
fn biggest(root: &Path, limit: usize) -> Vec<(u64, PathBuf)> {
    let mut sizes: Vec<(u64, PathBuf)> = Vec::new();
    for first in std::fs::read_dir(root).into_iter().flatten().filter_map(Result::ok) {
        let path = first.path();
        if !first.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()) {
            continue;
        }
        let inner: Vec<PathBuf> = std::fs::read_dir(&path).into_iter().flatten().filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink())).map(|e| e.path()).collect();
        if inner.is_empty() {
            sizes.push((dir_bytes(&path), path));
        } else {
            sizes.extend(inner.into_iter().map(|p| (dir_bytes(&p), p)));
        }
    }
    // an entry of over 5 GB (the checkout is one, then its `clis`) is shown by what is inside it, three levels down at most
    let mut expanded = sizes;
    for _ in 0..3 {
        let mut next: Vec<(u64, PathBuf)> = Vec::new();
        for (bytes, path) in expanded {
            let inner: Vec<(u64, PathBuf)> = if bytes > 5_000_000_000 { std::fs::read_dir(&path).into_iter().flatten().filter_map(Result::ok).filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink())).map(|e| (dir_bytes(&e.path()), e.path())).collect() } else { Vec::new() };
            if inner.is_empty() {
                next.push((bytes, path));
            } else {
                next.extend(inner);
            }
        }
        expanded = next;
    }
    let mut sizes = expanded;
    sizes.sort_by_key(|(b, _)| std::cmp::Reverse(*b));
    sizes.truncate(limit);
    sizes
}

/// Removes the candidate: the whole path, or each file it lists (every one is tried, whatever one answers).
fn remove(c: &Candidate) -> bool {
    if c.files.is_empty() {
        return std::fs::remove_dir_all(&c.path).is_ok();
    }
    let mut all = true;
    for file in &c.files {
        all &= std::fs::remove_file(file).is_ok();
    }
    all
}

/// The paths this command may remove from: under one of the roots it was given, never a root itself.
fn removable(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r) && path != r)
}

pub fn run(args: &[String]) -> Option<i32> {
    let p = parse_spec(args, &SPEC)?;
    let dry = p.flag("dry-run");
    let num = |key: &str, default: u64| p.val(key).and_then(|v| v.parse().ok()).unwrap_or(default);
    let (high, low) = (num("high", HIGH_PCT), num("low", LOW_PCT));
    // `work` is searched for cargo targets only; `installs` is the one directory whose subdirectories are `wf` installs
    let work = match p.val("root") {
        Some(r) => PathBuf::from(r),
        None => {
            let repo = wf_kit::root::repo_root();
            repo.ancestors().nth(2).map(Path::to_path_buf).unwrap_or(repo)
        }
    };
    let installs = p.val("installs").map_or_else(|| PathBuf::from("/opt/clis"), PathBuf::from);
    let roots = vec![work.clone(), installs.clone()];
    let Some((used, total)) = roots.iter().find_map(|r| volume(r)) else {
        println!("{}", json!({ "ok": true, "removed": [], "reason": "no root exists here" }));
        return Some(0);
    };
    // measuring a directory walks every file under it (800 MB of reads and 20 s of I/O wait on the runner's work volume): only a filling disk is worth it
    if (used * 100).checked_div(total).unwrap_or(0) < high {
        println!("{}", json!({ "ok": true, "dryRun": dry, "usedPercent": (used * 100).checked_div(total).unwrap_or(0), "highWater": high, "lowWater": low, "freedBytes": 0, "removed": [], "reason": "the disk is below its high-water mark: nothing measured" }));
        return Some(0);
    }
    let mut candidates = Vec::new();
    cargo_candidates(&work, 0, &mut candidates);
    install_candidates(&installs, &mut candidates);
    candidates.retain(|c| removable(&c.path, &roots));
    candidates.sort_by(|a, b| a.path.cmp(&b.path));
    candidates.dedup_by(|a, b| a.path == b.path);
    let chosen = plan(used, total, high, low, candidates);
    let mut removed: Vec<Value> = Vec::new();
    let mut freed = 0;
    for c in &chosen {
        let gone = dry || remove(c);
        if gone {
            freed += c.bytes;
        }
        removed.push(json!({ "path": c.path, "bytes": c.bytes, "why": c.why, "removed": gone && !dry }));
    }
    let usage: Vec<Value> = roots.iter().flat_map(|r| biggest(r, 8)).map(|(b, p)| json!({ "path": p, "bytes": b })).collect();
    let pct = |bytes: u64| (bytes * 100).checked_div(total).unwrap_or(0);
    let out = json!({ "ok": true, "dryRun": dry, "usedPercent": pct(used), "highWater": high, "lowWater": low, "freedBytes": freed, "afterPercent": pct(used.saturating_sub(freed)), "removed": removed, "biggest": usage });
    if let Some(path) = crate::ci_comment::var("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let rows: String = usage.iter().take(10).map(|u| format!("| `{}` | {:.1} GB |\n", u["path"].as_str().unwrap_or(""), u["bytes"].as_u64().unwrap_or(0) as f64 / 1e9)).collect();
        let note = if removed.is_empty() { format!("under the {high}% mark ({}% used), nothing removed", pct(used)) } else { format!("{}% used, removed {} item(s), {:.1} GB freed", pct(used), removed.len(), freed as f64 / 1e9) };
        let _ = std::fs::OpenOptions::new().append(true).create(true).open(path).and_then(|mut f| writeln!(f, "\n### 🧹 Runner disk\n\n{note}\n\n| Biggest directories | Size |\n|---|---|\n{rows}"));
    }
    println!("{out}");
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(name: &str, rank: u8, gb: u64, age_secs: u64) -> Candidate {
        Candidate { path: PathBuf::from(name), bytes: gb * 1_000_000_000, rank, why: "x", age_secs, files: Vec::new() }
    }

    #[test]
    fn given_stale_artifacts_and_incremental_state_when_the_artifacts_are_enough_then_the_incremental_state_stays() {
        // Given a volume at 80% with 5 GB of stale build artifacts and the 0.1 GB incremental state of the wf build, and a low mark of 60%
        let chosen = plan(80_000_000_000, 100_000_000_000, 70, 60, vec![cand("incremental", 3, 0, 1), cand("artifacts", 0, 5, 1)]);
        // Then only the artifacts are chosen while they bring it under the mark, and the incremental state goes only after everything else
        assert_eq!(chosen.len(), 2, "the 5 GB do not reach 60%: {chosen:?}");
        let chosen = plan(62_000_000_000, 100_000_000_000, 60, 60, vec![cand("incremental", 3, 0, 1), cand("artifacts", 0, 5, 1)]);
        assert_eq!(chosen.iter().map(|c| c.path.to_string_lossy().to_string()).collect::<Vec<_>>(), ["artifacts"]);
    }

    #[test]
    fn given_a_volume_under_its_high_mark_when_planned_then_nothing_is_removed() {
        assert!(plan(60_000_000_000, 100_000_000_000, 70, 60, vec![cand("a", 0, 30, 1)]).is_empty());
    }

    #[test]
    fn given_a_full_volume_when_planned_then_the_cheapest_rebuild_goes_first_and_it_stops_at_the_low_mark() {
        let chosen = plan(90_000_000_000, 100_000_000_000, 70, 65, vec![cand("old-target", 2, 50, 99), cand("incr-new", 0, 10, 1), cand("incr-old", 0, 12, 9), cand("install", 1, 5, 5)]);
        // 90 of 100 GB used, the low mark 65: the two incremental dirs (22 GB) leave 68, the install 63, and the old target is not needed
        let names: Vec<&str> = chosen.iter().map(|c| c.path.to_str().unwrap()).collect();
        assert_eq!(names, ["incr-old", "incr-new", "install"]);
    }
}
