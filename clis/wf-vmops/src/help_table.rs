//! The help text of `wf vm exec` and `wf vm net resolve`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it
//! against the binary. `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("vm exec", false, include_str!("help/wf__vm__exec.txt")),
    ("vm net resolve", false, include_str!("help/wf__vm__net__resolve.txt")),
];
