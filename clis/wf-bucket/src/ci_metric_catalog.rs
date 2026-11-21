//! Every CI gauge there is: its name, unit, what its two labels are and which verb sends it. `Gauges::set` takes a name only from here,
//! so a typo or an undocumented series is refused at the call and never reaches a sink.

pub struct Metric {
    pub name: &'static str,
    pub unit: &'static str,
    /// What the `scope` and the `name` label of a reading hold (`all` when a metric has one label).
    pub labels: (&'static str, &'static str),
    pub source: &'static str,
}

const fn m(name: &'static str, unit: &'static str, scope: &'static str, label: &'static str, source: &'static str) -> Metric {
    Metric { name, unit, labels: (scope, label), source }
}

pub const CATALOG: &[Metric] = &[
    m("ci.cache.requests", "count", "kind", "result", "wf cache"),
    m("ci.cache.seeded", "count", "kind", "checked|present", "wf cache"),
    m("ci.cache.degraded", "flag", "kind", "reason", "wf cache"),
    m("ci.cache.step_seconds", "s", "kind", "verb", "wf cache"),
    m("ci.drift.tasks_drifted", "count", "all", "all", "wf test drift"),
    m("ci.gate.seeded", "flag", "stage", "all", "wf test run"),
    m("ci.job.cpu_avg", "ratio", "job", "workflow", "wf ci resources"),
    m("ci.job.cpu_p95", "ratio", "job", "workflow", "wf ci resources"),
    m("ci.job.cpu_max", "ratio", "job", "workflow", "wf ci resources"),
    m("ci.job.iowait_avg", "ratio", "job", "workflow", "wf ci resources"),
    m("ci.job.mem_peak_mb", "MB", "job", "workflow", "wf ci resources"),
    m("ci.job.swap_peak_mb", "MB", "job", "workflow", "wf ci resources"),
    m("ci.job.psi_cpu_max", "pct", "job", "workflow", "wf ci resources"),
    m("ci.job.psi_mem_max", "pct", "job", "workflow", "wf ci resources"),
    m("ci.job.psi_io_max", "pct", "job", "workflow", "wf ci resources"),
    m("ci.job.idle_s", "s", "job", "workflow", "wf ci resources"),
    m("ci.job.saturated_s", "s", "job", "workflow", "wf ci resources"),
    m("ci.job.duration_s", "s", "stage", "shard", "wf ci timings"),
    m("ci.job.queue_s", "s", "stage", "shard", "wf ci timings"),
    m("ci.step.duration_s", "s", "stage", "step", "wf ci timings"),
    m("ci.step.idle_s", "s", "job", "step", "wf ci resources"),
    m("ci.stage.spread_s", "s", "stage", "all", "wf ci timings"),
    m("ci.run.wall_s", "s", "run", "all", "wf ci timings"),
    m("ci.run.runner_s", "s", "run", "workflow", "wf ci timings"),
    m("ci.run.peak_jobs", "count", "run", "workflow", "wf ci timings"),
    m("ci.run.avg_jobs", "count", "run", "workflow", "wf ci timings"),
    m("ci.run.queue_s_total", "s", "run", "workflow", "wf ci timings"),
    m("ci.run.queue_s_max", "s", "run", "workflow", "wf ci timings"),
    m("ci.run.critical_path_s", "s", "run", "workflow", "wf ci timings"),
    m("ci.run.parallel_efficiency", "ratio", "run", "workflow", "wf ci timings"),
    m("ci.runners.busy", "count", "workflow", "phase", "wf ci concurrency"),
    m("ci.runners.queued", "count", "workflow", "phase", "wf ci concurrency"),
    m("ci.runners.headroom", "count", "workflow", "phase", "wf ci concurrency"),
    m("ci.nx.tasks", "count", "stage", "outcome", "wf test run"),
    m("ci.nx.replay_ratio", "ratio", "stage", "all", "wf test run"),
    m("ci.nx.task_s", "s", "stage", "task", "wf test run"),
    m("ci.unit.duration_s", "s", "stage", "unit", "wf test run"),
    m("ci.test.duration_s", "s", "stage", "unit::test", "wf test run"),
    m("ci.shard.predicted_s", "s", "stage", "shard", "wf ci plan shard-weights --report"),
    m("ci.shard.actual_s", "s", "stage", "shard", "wf ci plan shard-weights --report"),
    m("ci.vmstate.ok", "flag", "app", "verb", "wf vm state"),
    m("ci.vmstate.age_seconds", "s", "app", "all", "wf vm state"),
];

pub fn lookup(name: &str) -> Option<&'static Metric> {
    CATALOG.iter().find(|metric| metric.name == name)
}
