//! The help text of `wf t3`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("t3", true, include_str!("help/wf__t3.txt")),
    ("t3 serve", false, include_str!("help/wf__t3__serve.txt")),
    ("t3 pair", false, include_str!("help/wf__t3__pair.txt")),
];
