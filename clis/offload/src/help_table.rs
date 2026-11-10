//! The help text of `wf offload` (ask, stats, plan, ladder), hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it
//! against the binary. `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("offload", true, include_str!("help/wf__offload.txt")),
    ("offload ask", false, include_str!("help/wf__offload__ask.txt")),
    ("offload stats", false, include_str!("help/wf__offload__stats.txt")),
    ("offload plan", false, include_str!("help/wf__offload__plan.txt")),
    ("offload sessions", true, include_str!("help/wf__offload__sessions.txt")),
    ("offload sessions rm", false, include_str!("help/wf__offload__sessions__rm.txt")),
    ("offload bearers", true, include_str!("help/wf__offload__bearers.txt")),
    ("offload bearers mint", false, include_str!("help/wf__offload__bearers__mint.txt")),
    ("offload bearers list", false, include_str!("help/wf__offload__bearers__list.txt")),
    ("offload bearers revoke", false, include_str!("help/wf__offload__bearers__revoke.txt")),
    ("offload ladder", true, include_str!("help/wf__offload__ladder.txt")),
    ("offload ladder clear-cooldown", false, include_str!("help/wf__offload__ladder__clear-cooldown.txt")),
    ("offload ladder sync-pricing", false, include_str!("help/wf__offload__ladder__sync-pricing.txt")),
];
