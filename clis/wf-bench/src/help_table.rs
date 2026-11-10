//! The help text of `wf bench cli` and `wf bench agent-proof`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("bench cli", false, include_str!("help/wf__bench__cli.txt")),
    ("bench agent-proof", false, include_str!("help/wf__bench__agent-proof.txt")),
];
