//! The help text of `wf secret`, hand-maintained under `help/` (`secret login` is served by `wf` itself);
//! `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("secret", true, include_str!("help/wf__secret.txt")),
    ("secret cache", false, include_str!("help/wf__secret__cache.txt")),
    ("secret create", false, include_str!("help/wf__secret__create.txt")),
    ("secret delete", false, include_str!("help/wf__secret__delete.txt")),
    ("secret doctor", false, include_str!("help/wf__secret__doctor.txt")),
    ("secret env", false, include_str!("help/wf__secret__env.txt")),
    ("secret get", false, include_str!("help/wf__secret__get.txt")),
    ("secret list", false, include_str!("help/wf__secret__list.txt")),
    ("secret move", false, include_str!("help/wf__secret__move.txt")),
    ("secret parity", false, include_str!("help/wf__secret__parity.txt")),
    ("secret push-notes", false, include_str!("help/wf__secret__push-notes.txt")),
    ("secret registry", true, include_str!("help/wf__secret__registry.txt")),
    ("secret registry sync", false, include_str!("help/wf__secret__registry__sync.txt")),
    ("secret rename", false, include_str!("help/wf__secret__rename.txt")),
    ("secret scope-check", false, include_str!("help/wf__secret__scope-check.txt")),
    ("secret set", false, include_str!("help/wf__secret__set.txt")),
    ("secret sync-env", false, include_str!("help/wf__secret__sync-env.txt")),
    ("secret verify", false, include_str!("help/wf__secret__verify.txt")),
    ("secret update", false, include_str!("help/wf__secret__update.txt")),
];
