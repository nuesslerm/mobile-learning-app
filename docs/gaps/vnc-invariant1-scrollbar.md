---
status: blocked
area: vnc-auth
paths:
  - "clis/wf-login/src/vnc/desktop.rs"
  - "clis/cdp/src/attached.rs"
  - "clis/cdp/src/measure_launch.js"
  - "clis/vnc-auth-capture/docker/custom-cont-init.d/20-kiosk-chromium.sh"
opened: 2026-10-04
last_verified: 2026-10-04
links:
  - gaps/vnc-stale-browser-target.md
  - gaps/vnc-launch-url-skips-invariants.md
---
# Launch invariant 1's scrollbar-independent width has never been run against a real scrollbar

## What is broken

On the old VNC desktop, with the display at 393x852, scrollable login pages reported a narrower client width (the
scrollbar), so an exact `layoutViewport.clientWidth == deviceWidth` check failed on pages that were laid out correctly.
That comparison is gone from the code: `check_launch_invariants` (`clis/cdp/src/attached.rs:502`) adds the page's
`scrollbarWidth` (`clis/cdp/src/measure_launch.js:184`, `innerWidth` minus `documentElement.clientWidth`) to the layout
width and passes when the sum equals the expected width, or when an unscaled body is exactly that width
(`clis/cdp/src/attached.rs:514-519`). Raw equality now only picks the `overflow` label: `"none"` when the layout width
alone matches; otherwise `"theirs"` for a scrollbar-only shortfall, `"page"` for an unscaled body at the expected width,
or `"ours"` (`clis/cdp/src/attached.rs:520-529`). What is missing is the evidence: nothing calls
`check_launch_invariants`, no test covers the scrollbar case, and the seven-site re-run has not been done against this
code.

## Why blocked

The re-run has no entry point. The login browser is now one headless Chromium with no display server
(`clis/wf-login/src/vnc/desktop.rs:1-4`); its 393x852 is the emulated viewport `VIEWPORT` (`clis/wf-login/src/vnc/desktop.rs:20`),
applied over CDP, and the `Launcher` there exposes no invariants method. The Rust port gave `Launcher` a
`check_launch_invariants` method and ran the pass on every navigation (9b03b119d); both were removed when the login
browser became headless Chromium (7dbaa9b63), so the pass waits on `docs/gaps/vnc-launch-url-skips-invariants.md` to give
it a caller again.

The re-run could not exercise the scrollbar case anyway, for two reasons. The same commit (7dbaa9b63) launches the
login Chromium with `--hide-scrollbars` (`OWN_FLAGS`, `clis/vnc-auth-capture/docker/custom-cont-init.d/20-kiosk-chromium.sh:21`),
and the 393x852 viewport is applied by `emulate_phone` with `mobile: true` (`clis/wf-login/src/vnc/desktop.rs` ->
`clis/cdp/src/attached.rs`, `Emulation.setDeviceMetricsOverride`), which makes Chromium use overlay scrollbars that take no
width. Measured on 19 Oct with Chromium 1194 on a 5000 px `width=device-width` page at a 393x852 override,
`innerWidth - documentElement.clientWidth` is 0 with `mobile: true` whether or not the flag is set; with `mobile: false` it is
15 without the flag and 0 with it. On today's login browser `scrollbarWidth` is therefore always 0, the narrower client
width from "What is broken" cannot occur, and a seven-site `1:PASS` would never take the `viewport_width + scrollbar`
addition or the `"theirs"` branch (`clis/cdp/src/attached.rs:514`, `523-524`); dropping the flag alone would not change that.

Choosing the comparison was a design decision (`documentElement.clientWidth` plus measured scrollbar,
`visualViewport.width`, or a scrollbar-width tolerance), and a wrong choice re-admits the misconfigured 1024 px case the
invariant exists to catch, so the re-run has to include that case.

## What unblocks it

A check that gives `check_launch_invariants` a page with a classic, non-overlay scrollbar: Chromium launched without
`--hide-scrollbars` and the 393 px viewport emulated with `mobile: false` (any setup where `innerWidth -
documentElement.clientWidth` is about 15), a page tall enough to scroll at the expected width, and invariant 1 (`mobile-viewport`) reported as passing with
`overflow: "theirs"`, while a viewport left at the wrong width, as the old display left at 1024x768 was, still reports
`1:FAIL`. Once a launch runs the invariants again, the seven-site re-run reporting `1:PASS` on all seven sites confirms the
login pages. The login browser (`--hide-scrollbars` plus `mobile: true` emulation) cannot stand in for the scrollbar check.

## Re-checked 2026-10-19

The comparison fix holds at fa797fbb3: `check_launch_invariants` adds `scrollbarWidth` (`clis/cdp/src/measure_launch.js:184`)
to the layout width (`clis/cdp/src/attached.rs:513-514`), passes on that sum or on an unscaled body at the expected width
(`attached.rs:515-519`), and uses raw equality only for the `overflow` label (`attached.rs:520-529`). No test pins it:
`clis/cdp/tests/flow.rs` never calls `check_launch_invariants`, and its only scrollbar assertion (`flow.rs:126-129`) checks
that `emulate_mobile` pins the gutter with `mobile: true`, not invariant 1. Nothing calls the method either: it appears only
at its definition (`attached.rs:502`), and none of the `Launcher`'s public methods in `clis/wf-login/src/vnc/desktop.rs`
(`desktop.rs:202-294`) runs the invariants. The login browser still cannot produce a scrollbar: it starts with
`--hide-scrollbars` (`clis/vnc-auth-capture/docker/custom-cont-init.d/20-kiosk-chromium.sh:21`), and
`apply_mobile_emulation` (`desktop.rs:105-106`, called at 119 and 176) goes through `emulate_phone`, which hard-codes
`mobile: true` (`attached.rs:411`), so `scrollbarWidth` is 0, the `+ scrollbar` addition (`attached.rs:514`) adds 0, and
the `"theirs"` branch (`attached.rs:523-524`) never runs. Only `set_metrics` (`attached.rs:999-1000`) can emulate with `mobile: false`, and
nothing pairs it with the invariants. Both checks from "What unblocks it" are therefore still missing: a classic-scrollbar
page passing with `overflow: "theirs"` while the 1024 px case still reports `1:FAIL`, and the seven-site `1:PASS` re-run,
which still waits on `docs/gaps/vnc-launch-url-skips-invariants.md`. `clis/vnc-auth-capture/README.md:179` still lists
`check_launch_invariants` among the methods `desktop.rs` exposes, which is stale.
