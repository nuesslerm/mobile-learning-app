//! The help text of `wf ios`, hand-maintained under `help/`; the verbs of the device command table are the noun's page from that
//! table (`harness_core::device::cli::help_hooks`). `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("ios", true, include_str!("help/wf__ios.txt")),
];
