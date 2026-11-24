//! The help text of `wf rca` (ask, stats, plan, ladder), hand-maintained under `help/`; `clis/wf/tests/flow/surface.rs` checks it
//! against the binary. `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("rca", true, include_str!("help/wf__rca.txt")),
    ("rca ask", false, include_str!("help/wf__rca__ask.txt")),
    ("rca stats", false, include_str!("help/wf__rca__stats.txt")),
    ("rca plan", false, include_str!("help/wf__rca__plan.txt")),
    ("rca sessions", true, include_str!("help/wf__rca__sessions.txt")),
    ("rca sessions rm", false, include_str!("help/wf__rca__sessions__rm.txt")),
    ("rca bearers", true, include_str!("help/wf__rca__bearers.txt")),
    ("rca bearers mint", false, include_str!("help/wf__rca__bearers__mint.txt")),
    ("rca bearers list", false, include_str!("help/wf__rca__bearers__list.txt")),
    ("rca bearers revoke", false, include_str!("help/wf__rca__bearers__revoke.txt")),
    ("rca login", true, include_str!("help/wf__rca__login.txt")),
    ("rca login start", false, include_str!("help/wf__rca__login__start.txt")),
    ("rca login respond", false, include_str!("help/wf__rca__login__respond.txt")),
    ("rca login complete", false, include_str!("help/wf__rca__login__complete.txt")),
    ("rca login cancel", false, include_str!("help/wf__rca__login__cancel.txt")),
    ("rca login status", false, include_str!("help/wf__rca__login__status.txt")),
    ("rca login logout", false, include_str!("help/wf__rca__login__logout.txt")),
    ("rca ladder", true, include_str!("help/wf__rca__ladder.txt")),
    ("rca ladder clear-cooldown", false, include_str!("help/wf__rca__ladder__clear-cooldown.txt")),
    ("rca ladder sync-pricing", false, include_str!("help/wf__rca__ladder__sync-pricing.txt")),
];
