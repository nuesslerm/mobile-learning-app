//! What a container's one-shot Engine sample and the host's `/proc/meminfo` say about memory and CPU, in the numbers `docker stats`
//! prints. Every number is a JavaScript double, as it was on the wire, and every missing or null field reads as 0.
use super::js;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::HashMap;

pub const MB: f64 = 1024.0 * 1024.0;

/// One running container's row of `GET /stats`; the field order and names are the reply's.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stat {
    pub name: String,
    pub mem_mb: i64,
    pub mem_limit_mb: Option<i64>,
    #[serde(serialize_with = "whole_as_integer")]
    pub cpu_pct: f64,
}

/// JavaScript prints 50 for 50.0.
fn whole_as_integer<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

/// The fields of `GET /containers/{id}/stats?stream=false` the service reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DockerStats {
    pub memory_stats: Option<MemoryStats>,
    pub cpu_stats: Option<CpuStats>,
    pub precpu_stats: Option<CpuStats>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemoryStats {
    pub usage: Option<f64>,
    pub limit: Option<f64>,
    pub stats: Option<MemoryDetail>,
}

/// cgroup v2 reports `inactive_file`, v1 `total_inactive_file` and, on old kernels, only `cache`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MemoryDetail {
    pub inactive_file: Option<f64>,
    pub total_inactive_file: Option<f64>,
    pub cache: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CpuStats {
    pub cpu_usage: Option<CpuUsage>,
    pub system_cpu_usage: Option<f64>,
    pub online_cpus: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct CpuUsage {
    pub total_usage: Option<f64>,
}

/// `Math.round(bytes / MB)`.
pub fn rounded_mb(bytes: f64) -> i64 {
    js::round(bytes / MB) as i64
}

/// What the kernel counts against a container's limit minus the cache it gives back first: the number `docker stats` prints.
pub fn working_set_bytes(stats: &DockerStats) -> f64 {
    let memory = stats.memory_stats.as_ref();
    let detail = memory.and_then(|m| m.stats.as_ref());
    let cache = detail.and_then(|d| d.inactive_file.or(d.total_inactive_file).or(d.cache)).unwrap_or(0.0);
    (memory.and_then(|m| m.usage).unwrap_or(0.0) - cache).max(0.0)
}

/// The share of the machine the sample used, times the CPU count, to one decimal; 0 until two samples show growth.
pub fn cpu_percent(stats: &DockerStats) -> f64 {
    let (cpu, before) = (stats.cpu_stats.as_ref(), stats.precpu_stats.as_ref());
    let total = |c: Option<&CpuStats>| c.and_then(|c| c.cpu_usage.as_ref()).and_then(|u| u.total_usage).unwrap_or(0.0);
    let system = |c: Option<&CpuStats>| c.and_then(|c| c.system_cpu_usage).unwrap_or(0.0);
    let (used, whole) = (total(cpu) - total(before), system(cpu) - system(before));
    if used > 0.0 && whole > 0.0 {
        js::round(used / whole * cpu.and_then(|c| c.online_cpus).unwrap_or(1.0) * 1000.0) / 10.0
    } else {
        0.0
    }
}

/// The Engine reports the host's memory as the limit of a container that has none, which counts as unlimited.
pub fn container_stat(name: &str, stats: &DockerStats, host_mem_bytes: f64) -> Stat {
    let limit = stats.memory_stats.as_ref().and_then(|m| m.limit).unwrap_or(0.0);
    Stat {
        name: name.to_string(),
        mem_mb: rounded_mb(working_set_bytes(stats)),
        mem_limit_mb: (limit > 0.0 && limit < host_mem_bytes).then(|| rounded_mb(limit)),
        cpu_pct: cpu_percent(stats),
    }
}

/// `text` after one or more spaces, or none when it has no space at its start.
fn after_spaces(text: &str) -> Option<&str> {
    let rest = text.trim_start_matches(js::is_space);
    (rest.len() < text.len()).then_some(rest)
}

/// `^(\w+):\s+(\d+)\s+kB` of one line: the key and the kilobytes, with anything after `kB` ignored.
fn meminfo_line(line: &str) -> Option<(&str, f64)> {
    let key_end = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(line.len());
    let (key, rest) = line.split_at(key_end);
    let rest = after_spaces(rest.strip_prefix(':').filter(|_| !key.is_empty())?)?;
    let digits_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let (digits, rest) = rest.split_at(digits_end);
    if digits.is_empty() || !after_spaces(rest)?.starts_with("kB") {
        return None;
    }
    Some((key, digits.parse::<f64>().ok()?))
}

/// Each `Key:   123 kB` line of `/proc/meminfo` as bytes; a later line of the same key wins and any other line is skipped.
pub fn parse_meminfo(text: &str) -> HashMap<String, f64> {
    text.split('\n').filter_map(meminfo_line).map(|(key, kb)| (key.to_string(), kb * 1024.0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: f64 = 12.0 * 1024.0 * MB;

    fn stats(json: &str) -> DockerStats {
        serde_json::from_str(json).expect("an Engine stats body")
    }

    #[test]
    fn given_a_usage_with_a_cache_part_when_the_working_set_is_read_then_the_reclaimable_cache_is_not_counted() {
        let with = |json: &str| working_set_bytes(&stats(json));
        assert_eq!(with(r#"{"memory_stats":{"usage":524288000,"stats":{"inactive_file":209715200}}}"#), 300.0 * MB);
        assert_eq!(with(r#"{"memory_stats":{"usage":524288000,"stats":{"total_inactive_file":104857600}}}"#), 400.0 * MB);
        assert_eq!(with("{}"), 0.0);
    }

    #[test]
    fn given_the_three_cache_fields_when_the_working_set_is_read_then_the_first_present_wins_even_at_zero() {
        let cases = [
            (r#"{"memory_stats":{"usage":315172800,"limit":536870912,"stats":{"inactive_file":104857600}}}"#, 210_315_200.0),
            (r#"{"memory_stats":{"usage":1073741824,"stats":{"total_inactive_file":73741824}}}"#, 1_000_000_000.0),
            (r#"{"memory_stats":{"usage":62914560,"stats":{"cache":10485760}}}"#, 52_428_800.0),
            (r#"{"memory_stats":{"usage":1000,"stats":{"inactive_file":0,"cache":400}}}"#, 1000.0),
            (r#"{"memory_stats":{"usage":1000,"stats":{"inactive_file":100,"total_inactive_file":300,"cache":400}}}"#, 900.0),
            (r#"{"memory_stats":{"usage":1000,"stats":{"total_inactive_file":300,"cache":400}}}"#, 700.0),
            (r#"{"memory_stats":{"usage":1000,"stats":{"inactive_file":null,"total_inactive_file":null,"cache":250}}}"#, 750.0),
            (r#"{"memory_stats":{"usage":700,"stats":null}}"#, 700.0),
            (r#"{"memory_stats":{"usage":1000.5,"stats":{"inactive_file":0.25}}}"#, 1000.25),
        ];
        for (json, want) in cases {
            assert_eq!(working_set_bytes(&stats(json)), want, "{json}");
        }
    }

    #[test]
    fn given_a_cache_larger_than_the_usage_or_no_usage_at_all_when_the_working_set_is_read_then_it_is_zero() {
        for json in [
            r#"{"memory_stats":{"usage":100,"stats":{"inactive_file":500}}}"#,
            r#"{"memory_stats":{"stats":{"inactive_file":5}}}"#,
            r#"{"memory_stats":null}"#,
            "{}",
        ] {
            assert_eq!(working_set_bytes(&stats(json)), 0.0, "{json}");
        }
    }

    #[test]
    fn given_two_cpu_samples_when_the_percentage_is_read_then_it_is_the_share_of_the_machine_times_the_cpu_count() {
        let two_cpus = r#"{"cpu_stats":{"cpu_usage":{"total_usage":300},"system_cpu_usage":1000,"online_cpus":2},
            "precpu_stats":{"cpu_usage":{"total_usage":100},"system_cpu_usage":600}}"#;
        assert_eq!(cpu_percent(&stats(two_cpus)), 100.0);
        assert_eq!(cpu_percent(&stats("{}")), 0.0);
    }

    #[test]
    fn given_samples_of_every_shape_when_the_percentage_is_read_then_it_matches_what_node_prints() {
        let sample = |cpu: &str, before: &str| format!(r#"{{"cpu_stats":{cpu},"precpu_stats":{before}}}"#);
        let cases: [(String, f64); 16] = [
            (
                sample(
                    r#"{"cpu_usage":{"total_usage":125000000},"system_cpu_usage":2200000000,"online_cpus":4}"#,
                    r#"{"cpu_usage":{"total_usage":100000000},"system_cpu_usage":2000000000}"#,
                ),
                50.0,
            ),
            (
                sample(
                    r#"{"cpu_usage":{"total_usage":101234},"system_cpu_usage":300000,"online_cpus":2}"#,
                    r#"{"cpu_usage":{"total_usage":100000},"system_cpu_usage":200000}"#,
                ),
                2.5,
            ),
            (sample(r#"{"cpu_usage":{"total_usage":300},"system_cpu_usage":1000}"#, r#"{"cpu_usage":{"total_usage":100},"system_cpu_usage":600}"#), 50.0),
            (
                sample(
                    r#"{"cpu_usage":{"total_usage":300},"system_cpu_usage":1000,"online_cpus":null}"#,
                    r#"{"cpu_usage":{"total_usage":100},"system_cpu_usage":600}"#,
                ),
                50.0,
            ),
            (
                sample(
                    r#"{"cpu_usage":{"total_usage":300},"system_cpu_usage":1000,"online_cpus":0}"#,
                    r#"{"cpu_usage":{"total_usage":100},"system_cpu_usage":600}"#,
                ),
                0.0,
            ),
            (
                sample(
                    r#"{"cpu_usage":{"total_usage":100},"system_cpu_usage":1000,"online_cpus":2}"#,
                    r#"{"cpu_usage":{"total_usage":100},"system_cpu_usage":600}"#,
                ),
                0.0,
            ),
            (
                sample(
                    r#"{"cpu_usage":{"total_usage":300},"system_cpu_usage":600,"online_cpus":2}"#,
                    r#"{"cpu_usage":{"total_usage":100},"system_cpu_usage":600}"#,
                ),
                0.0,
            ),
            (
                sample(
                    r#"{"cpu_usage":{"total_usage":50},"system_cpu_usage":1000,"online_cpus":2}"#,
                    r#"{"cpu_usage":{"total_usage":100},"system_cpu_usage":600}"#,
                ),
                0.0,
            ),
            (sample(r#"{"cpu_usage":{"total_usage":1},"system_cpu_usage":3,"online_cpus":1}"#, r#"{"cpu_usage":{"total_usage":0},"system_cpu_usage":0}"#), 33.3),
            (sample(r#"{"cpu_usage":{"total_usage":1},"system_cpu_usage":2000,"online_cpus":1}"#, r#"{"cpu_usage":{"total_usage":0},"system_cpu_usage":0}"#), 0.1),
            (sample(r#"{"cpu_usage":{"total_usage":1},"system_cpu_usage":100000,"online_cpus":1}"#, r#"{"cpu_usage":{"total_usage":0},"system_cpu_usage":0}"#), 0.0),
            (
                sample(
                    r#"{"cpu_usage":{"total_usage":9876543210123},"system_cpu_usage":123456789012345678,"online_cpus":12}"#,
                    r#"{"cpu_usage":{"total_usage":9876000000000},"system_cpu_usage":123456000000000000}"#,
                ),
                0.8,
            ),
            (r#"{"cpu_stats":{"cpu_usage":{"total_usage":300},"system_cpu_usage":1000,"online_cpus":2}}"#.to_string(), 60.0),
            (sample("null", "null"), 0.0),
            (sample(r#"{"cpu_usage":null,"system_cpu_usage":null,"online_cpus":null}"#, r#"{"cpu_usage":null,"system_cpu_usage":null}"#), 0.0),
            (r#"{"precpu_stats":{"cpu_usage":{"total_usage":1},"system_cpu_usage":1}}"#.to_string(), 0.0),
        ];
        for (json, want) in cases {
            assert_eq!(cpu_percent(&stats(&json)), want, "{json}");
        }
    }

    #[test]
    fn given_a_limit_equal_to_the_host_memory_when_the_stat_is_built_then_the_container_counts_as_unlimited() {
        let unlimited = container_stat("a", &stats(r#"{"memory_stats":{"usage":10485760,"limit":12884901888}}"#), HOST);
        let capped = container_stat("b", &stats(r#"{"memory_stats":{"usage":10485760,"limit":268435456}}"#), HOST);
        assert_eq!(unlimited.mem_limit_mb, None);
        assert_eq!(capped.mem_limit_mb, Some(256));
    }

    #[test]
    fn given_limits_around_the_hosts_memory_when_the_stat_is_built_then_only_a_positive_limit_below_it_is_reported() {
        let limit_stat = |json: &str, host: f64| container_stat("c", &stats(json), host).mem_limit_mb;
        assert_eq!(limit_stat(r#"{"memory_stats":{"usage":1,"limit":12884901889}}"#, HOST), None);
        assert_eq!(limit_stat(r#"{"memory_stats":{"usage":1,"limit":12884901887}}"#, HOST), Some(12288));
        assert_eq!(limit_stat(r#"{"memory_stats":{"usage":1}}"#, HOST), None);
        assert_eq!(limit_stat(r#"{"memory_stats":{"usage":1,"limit":0}}"#, HOST), None);
        assert_eq!(limit_stat(r#"{"memory_stats":{"usage":1,"limit":268435456}}"#, 0.0), None);
        assert_eq!(limit_stat(r#"{"memory_stats":{"usage":1,"limit":2621440}}"#, HOST), Some(3));
    }

    #[test]
    fn given_sizes_at_a_half_megabyte_when_the_stat_is_built_then_they_round_half_up_as_math_round_does() {
        let mem = |usage: u64| container_stat("m", &stats(&format!(r#"{{"memory_stats":{{"usage":{usage}}}}}"#)), HOST).mem_mb;
        assert_eq!((mem(524_287), mem(524_288), mem(1_572_864)), (0, 1, 2));
    }

    #[test]
    fn given_the_samples_of_the_stats_read_when_the_stats_are_built_then_they_match_the_rows_node_answers() {
        let leetcode = stats(
            r#"{"memory_stats":{"usage":315172800,"limit":536870912,"stats":{"inactive_file":104857600}},
                "cpu_stats":{"cpu_usage":{"total_usage":125000000},"system_cpu_usage":2200000000,"online_cpus":4},
                "precpu_stats":{"cpu_usage":{"total_usage":100000000},"system_cpu_usage":2000000000}}"#,
        );
        let agent_shell = stats(
            r#"{"memory_stats":{"usage":1073741824,"limit":12288000000,"stats":{"total_inactive_file":73741824}},
                "cpu_stats":{"cpu_usage":{"total_usage":101234},"system_cpu_usage":300000,"online_cpus":2},
                "precpu_stats":{"cpu_usage":{"total_usage":100000},"system_cpu_usage":200000}}"#,
        );
        let watchdog = stats(r#"{"memory_stats":{"usage":62914560,"stats":{"cache":10485760}}}"#);
        let host = 12_000_000.0 * 1024.0;
        let rows = [
            container_stat("leetcode-backend", &leetcode, host),
            container_stat("agent-shell", &agent_shell, host),
            container_stat("watchdog", &watchdog, host),
            container_stat("z", &stats("{}"), host),
        ];
        let json = serde_json::to_string(&rows).expect("rows are JSON");
        assert_eq!(
            json,
            concat!(
                r#"[{"name":"leetcode-backend","memMb":201,"memLimitMb":512,"cpuPct":50},"#,
                r#"{"name":"agent-shell","memMb":954,"memLimitMb":null,"cpuPct":2.5},"#,
                r#"{"name":"watchdog","memMb":50,"memLimitMb":null,"cpuPct":0},"#,
                r#"{"name":"z","memMb":0,"memLimitMb":null,"cpuPct":0}]"#
            )
        );
    }

    #[test]
    fn given_a_percentage_with_a_fraction_or_none_when_a_stat_is_written_then_it_prints_as_javascript_prints_it() {
        let row = |cpu_pct: f64| serde_json::to_string(&Stat { name: "n".into(), mem_mb: 1, mem_limit_mb: Some(2), cpu_pct }).unwrap();
        assert_eq!(row(33.3), r#"{"name":"n","memMb":1,"memLimitMb":2,"cpuPct":33.3}"#);
        assert_eq!(row(100.0), r#"{"name":"n","memMb":1,"memLimitMb":2,"cpuPct":100}"#);
        assert_eq!(row(0.1), r#"{"name":"n","memMb":1,"memLimitMb":2,"cpuPct":0.1}"#);
        assert_eq!(row(0.0), r#"{"name":"n","memMb":1,"memLimitMb":2,"cpuPct":0}"#);
    }

    #[test]
    fn given_a_meminfo_text_when_it_is_parsed_then_each_field_is_in_bytes() {
        let text = "MemTotal:       1000 kB\nMemAvailable:    250 kB\nHugePages_Total:       0\n";
        assert_eq!(parse_meminfo(text), HashMap::from([("MemTotal".to_string(), 1000.0 * 1024.0), ("MemAvailable".to_string(), 250.0 * 1024.0)]));
    }

    fn parsed(text: &str) -> Vec<(String, f64)> {
        let mut fields: Vec<_> = parse_meminfo(text).into_iter().collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        fields
    }

    fn fields(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn given_a_proc_meminfo_when_it_is_parsed_then_the_lines_with_a_kb_unit_are_read_whatever_their_spacing() {
        let proc_text = "MemTotal:       12000000 kB\nMemFree:         1000 kB\nMemAvailable:    6000000 kB\nSwapTotal:       2097148 kB\n";
        let want = [("MemAvailable", 6_144_000_000.0), ("MemFree", 1_024_000.0), ("MemTotal", 12_288_000_000.0), ("SwapTotal", 2_147_479_552.0)];
        assert_eq!(parsed(proc_text), fields(&want));
        let spaced = [
            ("MemTotal: 5 kB\r\nMemFree: 7 kB\r\n", vec![("MemFree", 7168.0), ("MemTotal", 5120.0)]),
            ("MemTotal:\t5\tkB\nMemFree:\t\t 7 \t kB\n", vec![("MemFree", 7168.0), ("MemTotal", 5120.0)]),
            ("MemTotal:\u{a0}5\u{2003}kB\nMemFree:\u{3000}8\u{feff}kB\n", vec![("MemFree", 8192.0), ("MemTotal", 5120.0)]),
            ("MemTotal: 1000 kB trailing words\nMemFree: 12 kBx\n", vec![("MemFree", 12288.0), ("MemTotal", 1_024_000.0)]),
            ("\n\n\nMemTotal: 3 kB\n\n", vec![("MemTotal", 3072.0)]),
            ("MemTotal: 3 kB\rMemFree: 4 kB", vec![("MemTotal", 3072.0)]),
            ("MemTotal: 000123 kB\n", vec![("MemTotal", 125_952.0)]),
            ("Hugepagesize_2:       2048 kB\n_x: 9 kB\n9: 4 kB\n", vec![("9", 4096.0), ("Hugepagesize_2", 2_097_152.0), ("_x", 9216.0)]),
            ("constructor: 4 kB\ntoString: 5 kB\n", vec![("constructor", 4096.0), ("toString", 5120.0)]),
        ];
        for (text, want) in spaced {
            assert_eq!(parsed(text), fields(&want), "{text:?}");
        }
    }

    #[test]
    fn given_a_key_that_repeats_when_a_meminfo_is_parsed_then_the_later_line_wins() {
        assert_eq!(parsed("MemTotal: 1 kB\nMemTotal: 2 kB\n"), fields(&[("MemTotal", 2048.0)]));
    }

    #[test]
    fn given_lines_that_are_not_a_key_a_number_and_kb_when_a_meminfo_is_parsed_then_they_are_skipped() {
        let skipped = [
            "MemTotal:1000 kB\n",
            "MemTotal: 1000kB\n",
            "MemTotal: 1000 KB\nMemFree: 1000 kb\n",
            " MemTotal: 1000 kB\n",
            "Active(anon):  123 kB\nInactive(file):  5 kB\n",
            "MemTotal: -5 kB\nMemFree: 5.5 kB\nMemAvailable: +5 kB\n",
            "MemTotal: \u{665} kB\n",
            "M\u{e9}m: 5 kB\n",
            "Mem:Total: 3 kB\n",
            ": 3 kB\n",
            "MemTotal 3 kB\n",
            "MemTotal: 3\n",
            "HugePages_Total:       0\n",
            "",
        ];
        for text in skipped {
            assert!(parse_meminfo(text).is_empty(), "{text:?}");
        }
    }

    #[test]
    fn given_kilobytes_beyond_two_to_the_fifty_third_when_a_meminfo_is_parsed_then_they_round_as_a_javascript_number_does() {
        let text = "MemTotal: 99999999999999999999 kB\nMemFree: 9007199254740993 kB\n";
        assert_eq!(parsed(text), fields(&[("MemFree", 9_223_372_036_854_776_000.0), ("MemTotal", 1.024e23)]));
    }
}
