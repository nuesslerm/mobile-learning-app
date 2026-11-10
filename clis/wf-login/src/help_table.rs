//! The help text of `wf secret login` and its verbs, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the
//! binary. `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("secret login", false, include_str!("help/wf__secret__login.txt")),
    ("secret login capture", false, include_str!("help/wf__secret__login__capture.txt")),
    ("secret login done", false, include_str!("help/wf__secret__login__done.txt")),
    ("secret login launch-url", false, include_str!("help/wf__secret__login__launch-url.txt")),
    ("secret login list", false, include_str!("help/wf__secret__login__list.txt")),
    ("secret login listener", false, include_str!("help/wf__secret__login__listener.txt")),
];
