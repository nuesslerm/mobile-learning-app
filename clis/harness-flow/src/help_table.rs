//! The help text of `wf flow`, hand-maintained under `help/`. `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("flow", true, include_str!("help/wf__flow.txt")),
    ("flow run", false, include_str!("help/wf__flow__run.txt")),
    ("flow goldens", false, include_str!("help/wf__flow__goldens.txt")),
];
