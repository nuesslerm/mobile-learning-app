//! The help text of `wf ci`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("ci", true, include_str!("help/wf__ci.txt")),
    ("ci tidy", false, include_str!("help/wf__ci__tidy.txt")),
    ("ci live", false, include_str!("help/wf__ci__live.txt")),
    ("ci preflight", false, include_str!("help/wf__ci__preflight.txt")),
    ("ci audit", false, include_str!("help/wf__ci__audit.txt")),
    ("ci comment", false, include_str!("help/wf__ci__comment.txt")),
    ("ci cache-health", false, include_str!("help/wf__ci__cache-health.txt")),
];
