//! Host figures out of the text `/proc` gives.

fn parse_stat(contents: &str) -> Option<(f64, f64)> {
    let line = contents.lines().find(|line| line.starts_with("cpu "))?;
    let fields: Vec<f64> = line.split_whitespace().skip(1).map(str::parse::<f64>).collect::<Result<_, _>>().ok()?;
    if fields.len() < 4 {
        return None;
    }
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0.0);
    let total: f64 = fields.iter().sum();
    (total > 0.0).then_some((total, idle))
}

/// Busy share of all CPUs, in percent, between two `/proc/stat` readings; 0 when either is unreadable or time did not advance.
pub fn cpu_percent(first: &str, second: &str) -> f64 {
    let (Some((total_a, idle_a)), Some((total_b, idle_b))) = (parse_stat(first), parse_stat(second)) else { return 0.0 };
    let total_delta = total_b - total_a;
    let idle_delta = idle_b - idle_a;
    if total_delta <= 0.0 || idle_delta < 0.0 {
        return 0.0;
    }
    (total_delta - idle_delta) / total_delta * 100.0
}

/// Share of all CPU time, in percent, that the hypervisor took (`steal`) and that was spent waiting on disk (`iowait`) between two `/proc/stat`
/// readings. `cpu_percent` counts steal as busy, so the two say how much of a busy host was another tenant's and how much was this VM's work.
pub fn cpu_steal_iowait(first: &str, second: &str) -> (f64, f64) {
    let fields = |contents: &str| -> Option<Vec<f64>> {
        let line = contents.lines().find(|line| line.starts_with("cpu "))?;
        line.split_whitespace().skip(1).map(str::parse::<f64>).collect::<Result<_, _>>().ok()
    };
    let (Some(a), Some(b)) = (fields(first), fields(second)) else { return (0.0, 0.0) };
    let total = b.iter().sum::<f64>() - a.iter().sum::<f64>();
    let delta = |index: usize| b.get(index).copied().unwrap_or(0.0) - a.get(index).copied().unwrap_or(0.0);
    if total <= 0.0 {
        return (0.0, 0.0);
    }
    (delta(7).max(0.0) / total * 100.0, delta(4).max(0.0) / total * 100.0)
}

/// Share of all CPU time, in percent, spent in the kernel (`system` and `softirq`) between two `/proc/stat` readings: page-cache writeback, overlay
/// filesystems and network work that no container's own figure holds when a kernel thread did it.
pub fn cpu_kernel(first: &str, second: &str) -> f64 {
    let fields = |contents: &str| -> Option<Vec<f64>> {
        let line = contents.lines().find(|line| line.starts_with("cpu "))?;
        line.split_whitespace().skip(1).map(str::parse::<f64>).collect::<Result<_, _>>().ok()
    };
    let (Some(a), Some(b)) = (fields(first), fields(second)) else { return 0.0 };
    let total = b.iter().sum::<f64>() - a.iter().sum::<f64>();
    let delta = |index: usize| b.get(index).copied().unwrap_or(0.0) - a.get(index).copied().unwrap_or(0.0);
    if total <= 0.0 {
        return 0.0;
    }
    (delta(2) + delta(6)).max(0.0) / total * 100.0
}

fn meminfo_kb(contents: &str, key: &str) -> Option<f64> {
    contents.lines().find(|line| line.starts_with(key))?.split_whitespace().nth(1)?.parse().ok()
}

/// Memory used and total, in bytes, from `/proc/meminfo`; zeros when unreadable. Used is total minus what a program can still claim
/// (`MemAvailable`, which counts reclaimable page cache as free; `MemFree` on a kernel that has none), so it only rises under real pressure.
pub fn memory_usage(contents: &str) -> (f64, f64) {
    let free = meminfo_kb(contents, "MemAvailable:").or_else(|| meminfo_kb(contents, "MemFree:"));
    match (meminfo_kb(contents, "MemTotal:"), free) {
        (Some(total), Some(free)) => ((total - free) * 1024.0, total * 1024.0),
        _ => (0.0, 0.0),
    }
}

/// Swap in use, in bytes; 0 when there is no swap or the file is unreadable.
pub fn swap_used(contents: &str) -> f64 {
    match (meminfo_kb(contents, "SwapTotal:"), meminfo_kb(contents, "SwapFree:")) {
        (Some(total), Some(free)) => (total - free).max(0.0) * 1024.0,
        _ => 0.0,
    }
}

/// The one-minute load average out of `/proc/loadavg`.
pub fn load1(contents: &str) -> Option<f64> {
    contents.split_whitespace().next()?.parse().ok()
}

/// The share of the last 10 seconds in which some task waited (`some avg10=`), in percent, out of a `/proc/pressure/*` file.
pub fn pressure_some_avg10(contents: &str) -> Option<f64> {
    let line = contents.lines().find(|line| line.starts_with("some "))?;
    line.split_whitespace().find_map(|field| field.strip_prefix("avg10="))?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO: &str = "MemTotal:       12212000 kB\nMemFree:          300000 kB\nMemAvailable:   10000000 kB\nSwapTotal:       1000000 kB\nSwapFree:         400000 kB\n";

    #[test]
    fn given_two_stat_readings_when_steal_and_iowait_are_asked_then_each_is_its_share_of_all_cpu_time() {
        let first = "cpu  100 0 100 700 50 0 0 50 0 0\n";
        let second = "cpu  200 0 200 900 90 0 0 110 0 0\n";
        let (steal, iowait) = cpu_steal_iowait(first, second);
        assert_eq!((steal.round(), iowait.round()), (12.0, 8.0));
        assert_eq!(cpu_steal_iowait(first, first), (0.0, 0.0));
    }

    #[test]
    fn given_two_stat_readings_when_the_kernel_share_is_asked_then_system_and_softirq_time_count() {
        let first = "cpu  100 0 100 700 50 0 10 50 0 0\n";
        let second = "cpu  200 0 180 900 90 0 30 110 0 0\n";
        assert_eq!(cpu_kernel(first, second).round(), 20.0);
        assert_eq!(cpu_kernel(first, first), 0.0);
    }

    #[test]
    fn given_page_cache_when_memory_is_read_then_available_memory_is_not_counted_as_used() {
        let (used, total) = memory_usage(MEMINFO);
        assert_eq!((used, total), ((12_212_000.0 - 10_000_000.0) * 1024.0, 12_212_000.0 * 1024.0));
    }

    #[test]
    fn given_no_memavailable_when_memory_is_read_then_memfree_stands_in() {
        let (used, _) = memory_usage("MemTotal: 1000 kB\nMemFree: 400 kB\n");
        assert_eq!(used, 600.0 * 1024.0);
    }

    #[test]
    fn given_swap_when_read_then_used_swap_is_total_minus_free_and_zero_without_swap() {
        assert_eq!(swap_used(MEMINFO), 600_000.0 * 1024.0);
        assert_eq!(swap_used("SwapTotal: 0 kB\nSwapFree: 0 kB\n"), 0.0);
        assert_eq!(swap_used("MemTotal: 1 kB\n"), 0.0);
    }

    #[test]
    fn given_loadavg_when_read_then_the_first_field_is_the_load() {
        assert_eq!(load1("0.38 0.41 0.39 1/805 2189\n"), Some(0.38));
        assert_eq!(load1(""), None);
    }

    #[test]
    fn given_a_pressure_file_when_read_then_the_some_avg10_is_returned() {
        let text = "some avg10=1.93 avg60=1.34 avg300=1.71 total=249046102\nfull avg10=0.50 avg60=0.00 avg300=0.00 total=0\n";
        assert_eq!(pressure_some_avg10(text), Some(1.93));
        assert_eq!(pressure_some_avg10("full avg10=0.50\n"), None);
    }
}
