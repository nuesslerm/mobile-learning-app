//! The help text of `wf exec`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[("exec", true, include_str!("help/wf__exec.txt")), ("exec serve", false, include_str!("help/wf__exec__serve.txt"))];
