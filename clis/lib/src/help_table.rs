//! The help text of `wf lib`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("lib", true, include_str!("help/wf__lib.txt")),
    ("lib release", false, include_str!("help/wf__lib__release.txt")),
    ("lib status", false, include_str!("help/wf__lib__status.txt")),
    ("lib prune", false, include_str!("help/wf__lib__prune.txt")),
    ("lib retarget", false, include_str!("help/wf__lib__retarget.txt")),
];
