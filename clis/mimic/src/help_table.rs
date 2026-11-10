//! The help text of `wf mimic`, hand-maintained under `help/`; `docs/reference/wf-cli.md` carries the same text.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("mimic", true, include_str!("help/wf__mimic.txt")),
    ("mimic record", false, include_str!("help/wf__mimic__record.txt")),
    ("mimic replay", false, include_str!("help/wf__mimic__replay.txt")),
    ("mimic diff", false, include_str!("help/wf__mimic__diff.txt")),
    ("mimic crawl", false, include_str!("help/wf__mimic__crawl.txt")),
    ("mimic stabilize", false, include_str!("help/wf__mimic__stabilize.txt")),
    ("mimic remask", false, include_str!("help/wf__mimic__remask.txt")),
    ("mimic verify", false, include_str!("help/wf__mimic__verify.txt")),
    ("mimic trigger", false, include_str!("help/wf__mimic__trigger.txt")),
    ("mimic provision", false, include_str!("help/wf__mimic__provision.txt")),
    ("mimic job", false, include_str!("help/wf__mimic__job.txt")),
    ("mimic publish", false, include_str!("help/wf__mimic__publish.txt")),
    ("mimic fetch", false, include_str!("help/wf__mimic__fetch.txt")),
    ("mimic import", false, include_str!("help/wf__mimic__import.txt")),
    ("mimic export", false, include_str!("help/wf__mimic__export.txt")),
];
