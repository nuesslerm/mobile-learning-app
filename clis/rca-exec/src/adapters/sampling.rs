//! One sample of the processes at this executor, read from `/proc`: the executor's own process and everything below it. No other process is
//! listed, so a shared host's other tenants never show in the phone's resource view.
use exec_proc::{ProcInfo, ProcSample, SAMPLE_MAX};
use exec_proto::{Code, ExecError, ExecResult};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// What a `/proc/<pid>/stat` line says, or none when the line is not one (a process that ended while it was read).
fn parse_stat(pid: u32, line: &str, page_kib: u64) -> Option<ProcInfo> {
    // The name sits in parentheses and may hold spaces and parentheses itself: the last `)` ends it.
    let (open, close) = (line.find('(')?, line.rfind(')')?);
    let rest: Vec<&str> = line.get(close + 1..)?.split_whitespace().collect();
    let number = |at: usize| rest.get(at)?.parse::<u64>().ok();
    Some(ProcInfo {
        pid,
        ppid: number(1)? as u32,
        name: line.get(open + 1..close)?.to_string(),
        state: rest.first()?.to_string(),
        threads: number(17)? as u32,
        rss_kib: number(21)?.saturating_mul(page_kib),
        cpu_ticks: number(11)?.saturating_add(number(12)?),
    })
}

/// Every process under `proc`, by pid.
fn all(proc: &Path, page_kib: u64) -> HashMap<u32, ProcInfo> {
    let Ok(entries) = std::fs::read_dir(proc) else { return HashMap::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let line = std::fs::read_to_string(e.path().join("stat")).ok()?;
            Some((pid, parse_stat(pid, &line, page_kib)?))
        })
        .collect()
}

/// `root` and its descendants among `found`, by pid.
fn below(root: u32, found: &HashMap<u32, ProcInfo>) -> Vec<ProcInfo> {
    let mut keep: HashSet<u32> = HashSet::from([root]);
    // A parent's pid can be higher than its child's after wrap-around, so grow the set until it stops growing
    loop {
        let before = keep.len();
        let more: Vec<u32> = found.values().filter(|p| keep.contains(&p.ppid)).map(|p| p.pid).collect();
        keep.extend(more);
        if keep.len() == before {
            break;
        }
    }
    let mut list: Vec<ProcInfo> = found.values().filter(|p| keep.contains(&p.pid)).cloned().collect();
    list.sort_by_key(|p| p.pid);
    list
}

fn sample_of(root: u32, found: &HashMap<u32, ProcInfo>, ticks_per_second: u64, at_ms: u64) -> ProcSample {
    let mut processes = below(root, found);
    let (count, total_rss_kib, total_cpu_ticks) = (processes.len() as u64, processes.iter().map(|p| p.rss_kib).sum(), processes.iter().map(|p| p.cpu_ticks).sum());
    let truncated = processes.len() > SAMPLE_MAX;
    processes.truncate(SAMPLE_MAX);
    ProcSample { sampled_at_ms: at_ms, ticks_per_second, processes, count, total_rss_kib, total_cpu_ticks, truncated }
}

/// The sample of the process `root` under `proc` (a `/proc`).
pub fn sample(proc: &Path, root: u32) -> ExecResult<ProcSample> {
    if !proc.join("self").exists() {
        return Err(ExecError::new(Code::Io, "this executor's host has no /proc to sample"));
    }
    // SAFETY: sysconf only reads a system constant.
    let (page, ticks) = unsafe { (libc::sysconf(libc::_SC_PAGESIZE), libc::sysconf(libc::_SC_CLK_TCK)) };
    let (page_kib, ticks) = (u64::try_from(page).unwrap_or(4096) / 1024, u64::try_from(ticks).unwrap_or(100));
    let found = all(proc, page_kib);
    if !found.contains_key(&root) {
        return Err(ExecError::new(Code::Io, "the executor's own process is not in /proc"));
    }
    let at_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    Ok(sample_of(root, &found, ticks, at_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pid: u32, ppid: u32) -> ProcInfo {
        ProcInfo { pid, ppid, name: format!("p{pid}"), state: "S".into(), threads: 1, rss_kib: 10, cpu_ticks: 1 }
    }

    #[test]
    fn given_a_stat_line_whose_name_holds_spaces_and_parentheses_when_parsed_then_the_fields_after_the_last_parenthesis_are_read() {
        // Given a line of /proc/<pid>/stat for a process named "a (b) c"
        let line = "42 (a (b) c) R 7 42 42 0 -1 4194560 100 0 0 0 30 12 0 0 20 0 3 0 12345 1000000 250 18446744073709551615";
        // When it is parsed with 4 KiB pages
        let got = parse_stat(42, line, 4).unwrap();
        // Then name, state, parent, threads, resident memory and cpu time are the line's
        assert_eq!((got.name.as_str(), got.state.as_str(), got.ppid, got.threads, got.rss_kib, got.cpu_ticks), ("a (b) c", "R", 7, 3, 1000, 42));
        assert!(parse_stat(1, "1 (x)", 4).is_none());
    }

    #[test]
    fn given_a_forest_of_processes_when_sampled_below_one_then_only_it_and_its_descendants_are_listed_and_the_sums_cover_all_of_them() {
        // Given 5 -> 3 -> 4 (a child with a lower pid than its parent), a stranger 9 -> 10 and the root's own parent 1
        let found: HashMap<u32, ProcInfo> = [info(1, 0), info(5, 1), info(3, 5), info(9, 0), info(10, 9), info(4, 3)].into_iter().map(|p| (p.pid, p)).collect();
        // When sampled below 5
        let s = sample_of(5, &found, 100, 5);
        // Then the stranger is absent, the list is by pid, and the totals add up
        assert_eq!(s.processes.iter().map(|p| p.pid).collect::<Vec<_>>(), [3, 4, 5]);
        assert_eq!((s.count, s.total_rss_kib, s.total_cpu_ticks, s.truncated), (3, 30, 3, false));
    }

    #[test]
    fn given_more_processes_than_the_cap_when_sampled_then_the_list_is_cut_and_the_count_and_sums_are_not() {
        // Given a root with one more child than the cap allows
        let mut found: HashMap<u32, ProcInfo> = (2..(SAMPLE_MAX as u32 + 2)).map(|pid| (pid, info(pid, 1))).collect();
        found.insert(1, info(1, 0));
        // When sampled
        let s = sample_of(1, &found, 100, 0);
        // Then the list holds the cap, the count holds all, and it is marked cut
        assert_eq!((s.processes.len(), s.count, s.truncated), (SAMPLE_MAX, SAMPLE_MAX as u64 + 1, true));
    }
}
