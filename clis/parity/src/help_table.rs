//! The help text of `wf parity`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("parity", true, include_str!("help/wf__parity.txt")),
    ("parity coverage", false, include_str!("help/wf__parity__coverage.txt")),
    ("parity track", true, include_str!("help/wf__parity__track.txt")),
];
