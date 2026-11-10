//! The help text of `wf agent-config`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("agent-config", true, include_str!("help/wf__agent-config.txt")),
    ("agent-config sync", false, include_str!("help/wf__agent-config__sync.txt")),
];
