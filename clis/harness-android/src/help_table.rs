//! The help text of `wf android`, hand-maintained under `help/`; the verbs of the device command table are the noun's page
//! from that table (`harness_core::device::cli::help_hooks`). `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("android", true, include_str!("help/wf__android.txt")),
    ("android test", false, include_str!("help/wf__android__test.txt")),
    ("android adb", false, include_str!("help/wf__android__adb.txt")),
    ("android lease", false, include_str!("help/wf__android__lease.txt")),
    ("android start-device", false, include_str!("help/wf__android__start-device.txt")),
    ("android stop-device", false, include_str!("help/wf__android__stop-device.txt")),
    ("android list-devices", false, include_str!("help/wf__android__list-devices.txt")),
    ("android hierarchy", false, include_str!("help/wf__android__hierarchy.txt")),
    ("android stack", false, include_str!("help/wf__android__stack.txt")),
];
