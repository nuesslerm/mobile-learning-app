//! The help text of `wf browser`, hand-maintained under `help/`; the verbs of the device command table are the noun's page
//! from that table (`harness_core::device::cli::help_hooks`). `clis/wf/tests/flow/surface.rs` checks it against the binary.
//! `(command path, bare invocation prints the help to stderr and exits 1, help text)`.
pub const HELP: &[(&str, bool, &str)] = &[
    ("browser", false, include_str!("help/wf__browser.txt")),
    ("browser test", false, include_str!("help/wf__browser__test.txt")),
    ("browser start-device", false, include_str!("help/wf__browser__start-device.txt")),
    ("browser stop-device", false, include_str!("help/wf__browser__stop-device.txt")),
    ("browser list-devices", false, include_str!("help/wf__browser__list-devices.txt")),
    ("browser hierarchy", false, include_str!("help/wf__browser__hierarchy.txt")),
    ("browser check-taps", false, include_str!("help/wf__browser__check-taps.txt")),
    ("browser set-breakpoint", false, include_str!("help/wf__browser__set-breakpoint.txt")),
    ("browser debug-pause", false, include_str!("help/wf__browser__debug-pause.txt")),
    ("browser debug-step", false, include_str!("help/wf__browser__debug-step.txt")),
    ("browser debug-eval", false, include_str!("help/wf__browser__debug-eval.txt")),
    ("browser debug-resume", false, include_str!("help/wf__browser__debug-resume.txt")),
    ("browser tailnet-forward", false, include_str!("help/wf__browser__tailnet-forward.txt")),
    ("browser tunnel", false, include_str!("help/wf__browser__tunnel.txt")),
    ("browser start", false, include_str!("help/wf__browser__start.txt")),
    ("browser status", false, include_str!("help/wf__browser__status.txt")),
    ("browser stop", false, include_str!("help/wf__browser__stop.txt")),
];
