---
status: open
area: vnc-auth
paths:
  - "clis/wf-login/src/vnc/engine.rs"
  - "clis/wf-login/src/vnc/desktop.rs"
  - "clis/wf-login/src/vnc/listener.rs"
  - "clis/cdp/src/attached.rs"
opened: 2026-10-04
last_verified: 2026-10-20
links: []
---
# No launch path runs the launch invariants, `wf secret login launch-url` included

## What is broken

The launch invariants first ran only inside the session engine's `start()`, so only the `/auth/start` path checked them.
The `/auth/launch-url` route (`clis/wf-login/src/vnc/listener.rs:142`) wires straight to `Launcher::navigate(url)`
(`clis/wf-login/src/vnc/desktop.rs:221`). On `vnc-auth-capture:406ed901`, seven `launch-url` origins produced zero
`vnc_auth.launch_invariants` lines; one `remote-login` (of gcloud) produced one. The TypeScript `navigate()` closed that
(the Gap 61 `reportLaunchInvariants` call, already present in f6746cb78), and the Rust port (9b03b119d) kept the pass in
`navigate_inner`, so both paths ran it. When the login browser became a headless Chromium (7dbaa9b63), the pass was
removed from both, so neither `Engine::start()` (`clis/wf-login/src/vnc/engine.rs:83`) nor the launch-url route runs it, and
`check_launch_invariants` (`clis/cdp/src/attached.rs:502`) has no caller.

## Why blocked

Not blocked, unscheduled. It matters because `launch-url` is the path sweeps and ad-hoc navigation use.

## What unblocks it

Re-introduce a launch-invariants pass in two places. One in `Launcher::navigate` (`clis/wf-login/src/vnc/desktop.rs:221`)
covers only `/auth/launch-url`, which calls it directly (`clis/wf-login/src/vnc/listener.rs:147`), and loginUrl starts, which
`/auth/start` sends there (`clis/wf-login/src/vnc/engine.rs:123`). Of the six built-in tools only leetcode is loginUrl; claude,
codex, opencode, gemini and gcloud are loginCmd (`clis/wf-login/src/vnc/descriptor.rs:49`), and for them `Engine::start()`
takes the `run_login` branch (`clis/wf-login/src/vnc/engine.rs:117`) and never calls `navigate`. That branch needs the pass
too, run after the CLI's browser page opens; 9b03b119d ran it in `Engine::start()` for every descriptor.

First drop or replace the fullscreen check in `check_launch_invariants`, which reads the X server's EWMH state
(`clis/cdp/src/attached.rs:544`), and the fab-present/fab-no-overlap checks, which measure `#vnc-open-url-host` from the
newtab extension that 7dbaa9b63 removed (`clis/cdp/src/attached.rs:614`). The headless-Chromium container has no X server
and no extension, so left in, fab-present fails on every navigation and fab-no-overlap passes without checking anything.

Verify: one `wf secret login launch-url <allow-listed url>` and one `wf secret login gcloud`, then
`wf vm logs --grep launch_invariants` finds a line for each.

## Re-checked 2026-10-19

Nothing is fixed, and no test pins any part of it. The `/auth/launch-url` route still checks the allowlist and calls
`deps.launcher.navigate(&url)` (`clis/wf-login/src/vnc/listener.rs:147`), and `navigate_inner` (`clis/wf-login/src/vnc/desktop.rs:163`)
navigates, compares `location.href`, logs `vnc_auth.desktop_launcher.navigated` and dismisses the cookie banner, with no
invariants pass. `Engine::start()` (`clis/wf-login/src/vnc/engine.rs:83`) runs none on either branch: `run_login`
(`engine.rs:118`) for claude, codex, opencode, gemini and gcloud, and `navigate` (`engine.rs:123`) for leetcode.
`check_launch_invariants` (`clis/cdp/src/attached.rs:502`) still has no caller and still holds the EWMH fullscreen check
(`attached.rs:545`) and the fab-present check on `#vnc-open-url-host` (`attached.rs:615`), an element no tracked source
creates any more, so wiring it back in unchanged would fail every headless navigation. The one test that sends the real
listener an accepted launch-url (`clis/wf-login/tests/flow/auth.rs:769`; the refusals at `auth.rs:604` never reach `navigate`)
asserts only the 200 and `ok: true`, and nothing under `clis/wf*` emits a `vnc_auth.launch_invariants` line, so
`wf secret login launch-url <url>` and `wf secret login gcloud` still produce none.

## Re-checked 2026-10-20

Nothing has changed since the 19 Oct re-check. check_launch_invariants (clis/cdp/src/attached.rs:502) still has no caller in clis/ and still carries the X-server EWMH fullscreen check (:544) and the #vnc-open-url-host fab checks (:614-625). Neither /auth/launch-url (listener.rs:147) nor either branch of Engine::start (engine.rs:118, :123) runs an invariants pass. No test, CI run or deployed log line shows a vnc_auth.launch_invariants entry.
