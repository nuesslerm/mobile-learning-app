//! `GET /stats`: what is holding the VM's memory and CPU, in one call: the host's own numbers and each running container's
//! working set.
use crate::application::ports::{Containers, ContainersError, HostFiles};
use crate::domain::container_stats::{container_stat, parse_meminfo, rounded_mb, Stat};
use crate::domain::containers::{container_name, DockerContainer};
use crate::domain::js;
use futures_util::future::join_all;
use serde::Serialize;
use std::cmp::Reverse;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub mem_total_mb: i64,
    pub mem_available_mb: i64,
    pub swap_total_mb: i64,
    /// The first three fields of `/proc/loadavg` read as numbers; a field that is none reads as null.
    pub load: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub host: Host,
    /// Largest working set first; equal ones keep the Engine's order.
    pub containers: Vec<Stat>,
    pub containers_mb: i64,
}

pub struct Stats {
    containers: Arc<dyn Containers>,
    host_files: Arc<dyn HostFiles>,
}

impl Stats {
    pub fn new(containers: Arc<dyn Containers>, host_files: Arc<dyn HostFiles>) -> Self {
        Self { containers, host_files }
    }

    /// An Engine that refuses the list means no containers, and one whose sample fails is left out. Only an Engine that cannot be
    /// reached, or whose list is not JSON, is an error. A host file that cannot be read counts as empty.
    pub async fn report(&self) -> Result<Report, ContainersError> {
        let meminfo = parse_meminfo(&self.host_files.read_text("meminfo").await.unwrap_or_default());
        let host_mem = meminfo.get("MemTotal").copied().unwrap_or(0.0);
        let listed = match self.containers.list_running().await {
            Ok(listed) => listed,
            Err(ContainersError::Refused { .. }) => Vec::new(),
            Err(unreachable) => return Err(unreachable),
        };
        let sampled = join_all(listed.iter().map(|c| self.sample(c, host_mem))).await;
        let mut containers: Vec<Stat> = sampled.into_iter().flatten().collect();
        containers.sort_by_key(|c| Reverse(c.mem_mb));
        let load = self.host_files.read_text("loadavg").await.unwrap_or_default().split(' ').take(3).map(js::to_number).collect();
        let host = Host {
            mem_total_mb: rounded_mb(host_mem),
            mem_available_mb: rounded_mb(meminfo.get("MemAvailable").copied().unwrap_or(0.0)),
            swap_total_mb: rounded_mb(meminfo.get("SwapTotal").copied().unwrap_or(0.0)),
            load,
        };
        let containers_mb = containers.iter().map(|c| c.mem_mb).sum();
        Ok(Report { host, containers, containers_mb })
    }

    async fn sample(&self, container: &DockerContainer, host_mem: f64) -> Option<Stat> {
        let stats = self.containers.stats(&container.id).await.ok()?;
        Some(container_stat(container_name(container), &stats, host_mem))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::container_stats::DockerStats;
    use crate::testing::{ContainersCall, FakeContainers, FakeHostFiles};

    const MEMINFO: &str = "MemTotal:       12000000 kB\nMemFree:          100000 kB\nMemAvailable:    6000000 kB\nSwapTotal:       2097148 kB\n";

    fn container(id: &str, name: &str) -> DockerContainer {
        DockerContainer { id: id.to_string(), names: Some(vec![format!("/{name}")]), ..Default::default() }
    }

    fn stats(json: &str) -> DockerStats {
        serde_json::from_str(json).expect("an Engine stats body")
    }

    fn body(report: &Report) -> String {
        js::stringify(&serde_json::to_value(report).expect("a value"))
    }

    #[tokio::test]
    async fn given_two_running_containers_and_the_host_files_when_read_then_the_host_is_rounded_and_the_containers_are_biggest_first_and_summed() {
        // Given the host's meminfo and loadavg, and two running containers (the smaller one listed first) with one sample each
        let fake = Arc::new(
            FakeContainers::new()
                .with_running(vec![container("s1", "leetcode-backend"), container("s2", "agent-shell")])
                .with_stats(
                    "s1",
                    Ok(stats(
                        r#"{"memory_stats":{"usage":315172800,"limit":536870912,"stats":{"inactive_file":104857600}},
                            "cpu_stats":{"cpu_usage":{"total_usage":125000000},"system_cpu_usage":2200000000,"online_cpus":4},
                            "precpu_stats":{"cpu_usage":{"total_usage":100000000},"system_cpu_usage":2000000000}}"#,
                    )),
                )
                .with_stats("s2", Ok(stats(r#"{"memory_stats":{"usage":1073741824,"limit":12288000000,"stats":{"total_inactive_file":73741824}}}"#))),
        );
        let files = Arc::new(FakeHostFiles::default().with("meminfo", MEMINFO).with("loadavg", "0.52 0.58 0.59 1/234 5678\n"));
        // When the report is made
        let report = Stats::new(fake, files).report().await.expect("a report");
        // Then the host numbers are rounded megabytes and the load's first three fields, the containers are sorted, a whole
        // cpuPct prints without a fraction, and the total is their sum
        assert_eq!(
            body(&report),
            concat!(
                r#"{"host":{"memTotalMb":11719,"memAvailableMb":5859,"swapTotalMb":2048,"load":[0.52,0.58,0.59]},"containers":["#,
                r#"{"name":"agent-shell","memMb":954,"memLimitMb":null,"cpuPct":0},"#,
                r#"{"name":"leetcode-backend","memMb":201,"memLimitMb":512,"cpuPct":50}],"containersMb":1155}"#
            )
        );
    }

    #[tokio::test]
    async fn given_a_container_whose_sample_fails_and_equal_working_sets_when_read_then_the_failed_one_is_left_out_and_ties_keep_the_listed_order() {
        // Given three running containers: a and b with the same usage, and c whose stats call is refused
        let same = r#"{"memory_stats":{"usage":104857600}}"#;
        let fake = Arc::new(
            FakeContainers::new()
                .with_running(vec![container("a", "a-first"), container("c", "c-fails"), container("b", "b-second")])
                .with_stats("a", Ok(stats(same)))
                .with_stats("b", Ok(stats(same)))
                .with_stats("c", Err(ContainersError::refused("docker API", 500, "stats failed", 300))),
        );
        // When the report is made
        let report = Stats::new(fake.clone(), Arc::new(FakeHostFiles::default())).report().await.expect("a report");
        // Then c is gone, a stays ahead of b, and a host with no files reads as zeros with a load of [0]
        let names: Vec<&str> = report.containers.iter().map(|c| c.name.as_str()).collect();
        assert_eq!((names, report.containers_mb), (vec!["a-first", "b-second"], 200));
        assert_eq!(report.host, Host { mem_total_mb: 0, mem_available_mb: 0, swap_total_mb: 0, load: vec![0.0] });
        assert!(fake.calls().contains(&ContainersCall::Stats { container_id: "c".to_string() }));
    }

    #[tokio::test]
    async fn given_an_engine_that_refuses_the_list_or_cannot_be_reached_when_read_then_a_refusal_is_no_containers_and_the_rest_is_an_error() {
        let files = || Arc::new(FakeHostFiles::default().with("meminfo", MEMINFO));
        // Given an Engine whose list answers 500
        let refused = FakeContainers::new().with_listing_error(ContainersError::refused("docker API", 500, "down", 200));
        // When the report is made, Then it has no containers: the list's refusal is not the handler's failure
        let report = Stats::new(Arc::new(refused), files()).report().await.expect("a report");
        assert_eq!((report.containers.len(), report.containers_mb, report.host.mem_total_mb), (0, 0, 11719));
        // Given an Engine that cannot be reached
        let unreachable = FakeContainers::new().with_listing_error(ContainersError::failed("connect ENOENT /var/run/docker.sock"));
        // When the report is made, Then the failure is the error
        let error = Stats::new(Arc::new(unreachable), files()).report().await.expect_err("an error");
        assert_eq!(error.to_string(), "connect ENOENT /var/run/docker.sock");
    }

    #[tokio::test]
    async fn given_a_loadavg_that_is_short_or_not_numbers_when_read_then_each_field_is_read_as_javascript_reads_it() {
        let load = |text: &str| {
            let files = Arc::new(FakeHostFiles::default().with("loadavg", text));
            let stats = Stats::new(Arc::new(FakeContainers::new()), files);
            async move { body(&stats.report().await.expect("a report")) }
        };
        assert!(load("0.5\n").await.contains(r#""load":[0.5]"#));
        assert!(load("1 abc 3 4").await.contains(r#""load":[1,null,3]"#));
        assert!(load("").await.contains(r#""load":[0]"#));
    }
}
