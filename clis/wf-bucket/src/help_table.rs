//! The help text of `wf cache`, hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("cache", true, include_str!("help/wf__cache.txt")),
    ("cache nx", false, include_str!("help/wf__cache__nx.txt")),
    ("cache sccache", false, include_str!("help/wf__cache__sccache.txt")),
    ("cache status", false, include_str!("help/wf__cache__status.txt")),
    ("cache gc", false, include_str!("help/wf__cache__gc.txt")),
    ("cache guard", false, include_str!("help/wf__cache__guard.txt")),
    ("cache purge", false, include_str!("help/wf__cache__purge.txt")),
    ("cache restore", false, include_str!("help/wf__cache__restore.txt")),
    ("cache save", false, include_str!("help/wf__cache__save.txt")),
];
