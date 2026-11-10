//! What the VM's host was doing over a span, from reporting-service's stored samples (`/api/metrics/series`): when it stopped
//! sampling (a stall), the average and worst moment of each stall signal, and which containers wrote the most to disk. Shared by
//! `wf vm ops host` and the CI report.
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const REPORTING_URL: &str = "http://reporting-service:8791";
/// The metrics whose highest value says where a stall came from.
pub const SIGNALS: [&str; 11] = ["cpu", "cpu_steal", "cpu_iowait", "cpu_kernel", "psi_io", "psi_memory", "psi_cpu", "load1", "memory_used", "swap_used", "list_ms"];

/// The summary of the host points (`/api/metrics/series` entries) between `from_ms` and `to_ms`: the sweeps' span and age, the gaps
/// between consecutive sweeps longer than `gap_ms`, the average and peak of each signal with when the peak was, the host's memory
/// total, and the five containers that wrote the most.
pub fn summarize(points: &[Value], from_ms: i64, to_ms: i64, gap_ms: i64) -> Value {
    let within: Vec<&Value> = points.iter().filter(|p| p["ts"].as_i64().is_some_and(|t| t >= from_ms && t <= to_ms)).collect();
    let host: Vec<&&Value> = within.iter().filter(|p| p["scope"] == "host").collect();
    let mut sweeps: Vec<i64> = host.iter().filter(|p| p["metric"] == "cpu").filter_map(|p| p["ts"].as_i64()).collect();
    sweeps.sort_unstable();
    sweeps.dedup();
    let gaps: Vec<Value> = sweeps.windows(2).filter(|w| w[1] - w[0] > gap_ms).map(|w| json!({ "fromMs": w[0], "toMs": w[1], "seconds": (w[1] - w[0]) / 1000 })).collect();
    let mut sums: BTreeMap<&str, (f64, f64, i64, f64)> = BTreeMap::new();
    let mut memory_total = None;
    for point in &host {
        let (Some(metric), Some(value), Some(ts)) = (point["metric"].as_str(), point["value"].as_f64(), point["ts"].as_i64()) else { continue };
        if metric == "memory_total" {
            memory_total = Some(value);
        }
        if let Some(name) = SIGNALS.iter().find(|m| **m == metric) {
            let entry = sums.entry(name).or_insert((0.0, value, ts, 0.0));
            entry.0 += value;
            entry.3 += 1.0;
            if value > entry.1 {
                entry.1 = value;
                entry.2 = ts;
            }
        }
    }
    let signals: serde_json::Map<String, Value> = sums.into_iter().map(|(m, (sum, peak, at, n))| (m.to_string(), json!({ "avg": sum / n, "peak": peak, "atMs": at }))).collect();
    let newest = sweeps.last().copied();
    json!({
        "samples": sweeps.len(),
        "firstMs": sweeps.first(),
        "newestMs": newest,
        "ageSeconds": newest.map(|n| (to_ms - n) / 1000),
        "gapSecondsOver": gap_ms / 1000,
        "gaps": gaps,
        "signals": signals,
        "memoryTotal": memory_total,
        "diskWriters": top_writers(&within, 5),
    })
}

/// One stretch of a chart: the average and the highest value of a metric over it.
#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    pub start_ms: i64,
    pub avg: f64,
    pub max: f64,
}

/// A host metric between `from_ms` and `to_ms` cut into at most `max_buckets` stretches of whole minutes; a stretch with no sample is
/// left out, so a stall shows as a jump in the times rather than as a made-up value.
pub fn buckets(points: &[Value], metric: &str, from_ms: i64, to_ms: i64, max_buckets: usize) -> Vec<Bucket> {
    let span = (to_ms - from_ms).max(1);
    let width = ((span / max_buckets.max(1) as i64 / 60_000) + 1) * 60_000;
    let mut by_start: BTreeMap<i64, (f64, f64, f64)> = BTreeMap::new();
    for p in points.iter().filter(|p| p["scope"] == "host" && p["metric"] == metric) {
        let (Some(ts), Some(value)) = (p["ts"].as_i64(), p["value"].as_f64()) else { continue };
        if ts < from_ms || ts > to_ms {
            continue;
        }
        let start = from_ms + (ts - from_ms) / width * width;
        let e = by_start.entry(start).or_insert((0.0, 0.0, f64::MIN));
        e.0 += value;
        e.1 += 1.0;
        e.2 = e.2.max(value);
    }
    by_start.into_iter().map(|(start_ms, (sum, n, max))| Bucket { start_ms, avg: sum / n, max }).collect()
}

/// What each container used between `from_ms` and `to_ms`: its average and peak CPU (percent of one CPU), its peak memory, and the bytes
/// it read and wrote (the engine's counters summed over their rises: a restart resets one), the biggest writers first.
pub fn container_usage(points: &[Value], since_ms: i64, from_ms: i64, to_ms: i64) -> Vec<Value> {
    #[derive(Default)]
    struct Use {
        cpu: Vec<f64>,
        memory_peak: f64,
        reads: Vec<(i64, f64)>,
        writes: Vec<(i64, f64)>,
    }
    let mut by_name: BTreeMap<&str, Use> = BTreeMap::new();
    // CPU is averaged over the span's own sweeps (a container missing from one used none), so the figures add up against the host's;
    // the disk counters start at `since_ms`, one sweep earlier, so the first sweep's rise counts
    let sweeps = points.iter().filter(|p| p["scope"] == "container" && p["metric"] == "cpu").filter_map(|p| p["ts"].as_i64()).filter(|t| *t >= from_ms && *t <= to_ms).collect::<std::collections::BTreeSet<i64>>().len().max(1);
    for p in points.iter().filter(|p| p["scope"] == "container" && p["ts"].as_i64().is_some_and(|t| t >= since_ms && t <= to_ms)) {
        let (Some(name), Some(metric), Some(ts), Some(value)) = (p["name"].as_str(), p["metric"].as_str(), p["ts"].as_i64(), p["value"].as_f64()) else { continue };
        let entry = by_name.entry(name).or_default();
        match metric {
            "cpu" if ts >= from_ms => entry.cpu.push(value),
            "memory" => entry.memory_peak = entry.memory_peak.max(value),
            "disk_read_bytes" => entry.reads.push((ts, value)),
            "disk_write_bytes" => entry.writes.push((ts, value)),
            _ => {}
        }
    }
    let rises = |mut v: Vec<(i64, f64)>| -> f64 {
        v.sort_by_key(|(ts, _)| *ts);
        v.windows(2).map(|w| (w[1].1 - w[0].1).max(0.0)).sum()
    };
    let mut out: Vec<(f64, Value)> = by_name
        .into_iter()
        .map(|(name, u)| {
            let (avg, peak) = (u.cpu.iter().sum::<f64>() / sweeps as f64, u.cpu.iter().copied().fold(0.0, f64::max));
            let (read, written) = (rises(u.reads), rises(u.writes));
            (written, json!({ "container": name, "cpuAvg": avg, "cpuPeak": peak, "memoryPeak": u.memory_peak, "readBytes": read, "writtenBytes": written }))
        })
        .collect();
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.into_iter().map(|(_, v)| v).collect()
}

/// The containers that wrote the most to disk: each one's cumulative `disk_write_bytes` counter summed over its rises (a restart
/// resets a counter, which must not count as negative writing), biggest first.
fn top_writers(points: &[&Value], limit: usize) -> Vec<Value> {
    let mut series: BTreeMap<&str, Vec<(i64, f64)>> = BTreeMap::new();
    for p in points.iter().filter(|p| p["scope"] == "container" && p["metric"] == "disk_write_bytes") {
        if let (Some(name), Some(ts), Some(value)) = (p["name"].as_str(), p["ts"].as_i64(), p["value"].as_f64()) {
            series.entry(name).or_default().push((ts, value));
        }
    }
    let mut written: Vec<(&str, f64)> = series
        .into_iter()
        .map(|(name, mut v)| {
            v.sort_by_key(|(ts, _)| *ts);
            (name, v.windows(2).map(|w| (w[1].1 - w[0].1).max(0.0)).sum())
        })
        .filter(|(_, bytes)| *bytes > 0.0)
        .collect();
    written.sort_by(|a, b| b.1.total_cmp(&a.1));
    written.into_iter().take(limit).map(|(name, bytes)| json!({ "container": name, "writtenBytes": bytes })).collect()
}

/// The series of a window over the tailnet. Where DNS cannot see the tailnet (the sandbox's userspace one) the SOCKS proxy resolves
/// the bare name itself.
pub fn fetch(base_url: &str, window: &str) -> Result<Value, String> {
    fetch_scoped(base_url, window, None)
}

/// Like [`fetch`], with only `scope`'s points (`host` or `container`).
pub fn fetch_scoped(base_url: &str, window: &str, scope: Option<&str>) -> Result<Value, String> {
    fetch_query(base_url, window, &scope.map(|s| ("scope", s.to_string())).into_iter().collect::<Vec<_>>())
}

/// The series of a window with the query parameters reporting-service understands (`scope`, `metric` comma separated, `since` in
/// epoch milliseconds).
pub fn fetch_query(base_url: &str, window: &str, params: &[(&str, String)]) -> Result<Value, String> {
    let host = base_url.strip_prefix("http://").and_then(|rest| rest.split([':', '/']).next()).unwrap_or_default();
    let base = match crate::tailnet_resolve::answering(host) {
        Some(name) if !host.is_empty() => base_url.replacen(host, &name, 1),
        _ => base_url.to_string(),
    };
    let url = format!("{base}/api/metrics/series?window={window}{}", params.iter().map(|(k, v)| format!("&{k}={v}")).collect::<String>());
    let response = cli_http::Request::get(&url).via(crate::vmops::tailnet_via()).timeout(std::time::Duration::from_secs(120)).send().map_err(|e| format!("{url}: {e}"))?;
    if !response.ok() {
        return Err(format!("{url} answered HTTP {}", response.status));
    }
    serde_json::from_str(&response.text()).map_err(|e| format!("{url}: not JSON ({e})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(ts: i64, metric: &str, value: f64) -> Value {
        json!({ "ts": ts, "scope": "host", "name": "host", "metric": metric, "value": value })
    }

    #[test]
    fn given_sweeps_with_a_hole_when_summarized_then_the_hole_is_a_gap_and_each_signal_has_an_average_and_a_peak_in_time() {
        let min = 60_000;
        let points = vec![
            point(0, "cpu", 5.0),
            point(min, "cpu", 5.0),
            point(min, "psi_io", 3.0),
            point(25 * min, "cpu", 90.0),
            point(25 * min, "psi_io", 41.5),
            point(26 * min, "cpu", 5.0),
            json!({ "ts": 26 * min, "scope": "container", "name": "web", "metric": "cpu", "value": 99.0 }),
        ];
        let summary = summarize(&points, 0, 30 * min, 3 * min);
        assert_eq!(summary["samples"], 4);
        assert_eq!(summary["gaps"], json!([{ "fromMs": min, "toMs": 25 * min, "seconds": 24 * 60 }]));
        assert_eq!(summary["signals"]["psi_io"], json!({ "avg": 22.25, "peak": 41.5, "atMs": 25 * min }));
        assert_eq!(summary["ageSeconds"], 4 * 60);
    }

    #[test]
    fn given_points_outside_the_span_when_summarized_then_only_the_span_counts() {
        let points = vec![point(10, "cpu", 1.0), point(20, "cpu", 2.0), point(30, "cpu", 3.0)];
        assert_eq!(summarize(&points, 15, 25, 1000)["samples"], 1);
    }

    #[test]
    fn given_disk_counters_when_summarized_then_containers_are_ranked_by_what_they_wrote_and_a_restart_is_not_negative_writing() {
        let counter = |ts: i64, name: &str, value: f64| json!({ "ts": ts, "scope": "container", "name": name, "metric": "disk_write_bytes", "value": value });
        let points = vec![counter(1, "buildkit", 100.0), counter(2, "buildkit", 900.0), counter(3, "buildkit", 50.0), counter(4, "buildkit", 150.0), counter(1, "web", 10.0), counter(2, "web", 10.0)];
        let summary = summarize(&points, 0, 10, 180_000);
        assert_eq!(summary["diskWriters"], json!([{ "container": "buildkit", "writtenBytes": 900.0 }]), "800 then 100 after the reset; web wrote nothing");
    }

    #[test]
    fn given_a_metric_when_bucketed_then_each_stretch_has_its_average_and_peak_and_empty_stretches_are_left_out() {
        let min = 60_000;
        let points: Vec<Value> = [(0, 10.0), (min, 30.0), (2 * min, 5.0), (10 * min, 50.0)].iter().map(|(t, v)| point(*t, "psi_io", *v)).collect();
        let two = buckets(&points, "psi_io", 0, 11 * min, 6);
        assert_eq!(two.iter().map(|b| (b.start_ms / min, b.avg, b.max)).collect::<Vec<_>>(), [(0, 20.0, 30.0), (2, 5.0, 5.0), (10, 50.0, 50.0)], "{two:?}");
        assert!(buckets(&points, "cpu", 0, 11 * min, 6).is_empty());
    }

    #[test]
    fn given_container_samples_when_totalled_then_each_has_cpu_peak_memory_and_the_bytes_it_moved() {
        let c = |ts: i64, name: &str, metric: &str, value: f64| json!({ "ts": ts, "scope": "container", "name": name, "metric": metric, "value": value });
        let points = vec![
            c(1, "build", "cpu", 100.0),
            c(2, "build", "cpu", 300.0),
            c(1, "build", "memory", 5e8),
            c(2, "build", "memory", 9e8),
            c(1, "build", "disk_write_bytes", 10.0),
            c(2, "build", "disk_write_bytes", 110.0),
            c(1, "build", "disk_read_bytes", 0.0),
            c(2, "build", "disk_read_bytes", 7.0),
            c(1, "web", "cpu", 2.0),
            c(99, "web", "cpu", 50.0),
        ];
        let usage = container_usage(&points, 0, 0, 10);
        assert_eq!(usage[0]["container"], "build");
        assert_eq!((usage[0]["cpuAvg"].as_f64(), usage[0]["cpuPeak"].as_f64(), usage[0]["memoryPeak"].as_f64(), usage[0]["writtenBytes"].as_f64(), usage[0]["readBytes"].as_f64()), (Some(200.0), Some(300.0), Some(9e8), Some(100.0), Some(7.0)));
        assert_eq!(usage[1]["cpuPeak"], 2.0, "a point after the span does not count");
    }

    #[test]
    fn given_no_points_when_summarized_then_it_says_nothing_was_sampled() {
        let summary = summarize(&[], 0, 1_000, 180_000);
        assert_eq!((summary["samples"].as_i64(), summary["gaps"].as_array().map(Vec::len), summary["newestMs"].is_null()), (Some(0), Some(0), true));
    }
}
