//! The help text of every command `wf` serves itself, hand-maintained under `help/` (the crates that serve a command own its text: `wf offload ask`,
//! `wf offload plan` and `wf offload ladder` are in `clis/offload`, `wf android` and `wf browser` in `clis/wf-harness`, `wf bench cli` in `clis/wf-bench`, `wf ci` is in `clis/pipeline`,
//! `wf cache` in `clis/wf-bucket`, `wf secret` in `clis/secrets` but `secret login`, which is in `clis/wf-login`, `ship` in `clis/deploy`, `vm` in `clis/deploy` but
//! `vm exec` and `vm net resolve`, which are in `clis/wf-vmops`, `repo bundle` in `clis/pipeline`); `clis/wf/tests/flow/surface.rs` checks every
//! crate's table against the binary. `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("", true, include_str!("help/wf.txt")),
    ("test", false, include_str!("help/wf__test.txt")),
    ("check", false, include_str!("help/wf__check.txt")),
    ("log", true, include_str!("help/wf__log.txt")),
    ("log emit", false, include_str!("help/wf__log__emit.txt")),
];
