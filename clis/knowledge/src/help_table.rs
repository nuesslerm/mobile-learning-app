//! The help text of `wf knowledge`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("knowledge", true, include_str!("help/wf__knowledge.txt")),
    ("knowledge rank", false, include_str!("help/wf__knowledge__rank.txt")),
    ("knowledge index", false, include_str!("help/wf__knowledge__index.txt")),
    ("knowledge stale", false, include_str!("help/wf__knowledge__stale.txt")),
];
