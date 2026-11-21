//! The help text of `wf ci`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("ci", true, include_str!("help/wf__ci.txt")),
    ("ci plan", false, include_str!("help/wf__ci__plan.txt")),
    ("ci tidy", false, include_str!("help/wf__ci__tidy.txt")),
    ("ci live", false, include_str!("help/wf__ci__live.txt")),
    ("ci prune", false, include_str!("help/wf__ci__prune.txt")),
    ("ci preflight", false, include_str!("help/wf__ci__preflight.txt")),
    ("ci audit", false, include_str!("help/wf__ci__audit.txt")),
    ("ci comment", false, include_str!("help/wf__ci__comment.txt")),
    ("ci timings", false, include_str!("help/wf__ci__timings.txt")),
    ("ci progress", false, include_str!("help/wf__ci__progress.txt")),
    ("ci resources", false, include_str!("help/wf__ci__resources.txt")),
    ("ci concurrency", false, include_str!("help/wf__ci__concurrency.txt")),
    ("ci proven", false, include_str!("help/wf__ci__proven.txt")),
    ("ci failed", false, include_str!("help/wf__ci__failed.txt")),
    ("ci cache-health", false, include_str!("help/wf__ci__cache-health.txt")),
    ("ci db", false, include_str!("help/wf__ci__db.txt")),
];
