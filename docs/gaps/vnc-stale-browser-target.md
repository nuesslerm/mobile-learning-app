---
status: open
area: vnc-auth
paths:
  - "clis/wf-login/src/vnc/desktop.rs"
  - "clis/cdp/src/attached.rs"
  - "clis/wf-login/src/vnc/engine.rs"
  - "clis/wf-login/src/auth.rs"
  - "clis/wf-login/src/vnc/relay.rs"
  - "clis/wf-login/src/vnc/listener.rs"
opened: 2026-10-04
last_verified: 2026-10-20
links: []
---
# CDP can drive a browser target the human does not see, a wrong landing only logs, and a killed capture stays in flight

## What is broken

Observed on the former X11 desktop: after the deleted TypeScript `wf` CLI's `vnc resize 393 852`, its
`vnc debug` showed a window still at its pre-resize `1006x717` beside two at `393x852`. `navigate()` then
reported success for seven URLs while the visible page never changed, and its `vnc page-info` reported two
layout viewports (393 and 416). A multi-site invariant sweep read `checked: 0`, `fabRect: 'missing'` rows;
one whole sweep was discarded.

The Rust launcher (`clis/wf-login/src/vnc/desktop.rs`) runs one headless Chromium with a fixed `393x852` viewport and
no display server, so there is no display resize to leave a window behind, and the Rust `wf` has no `vnc`
command. Three problems are left: the target choice and the log-only URL check below, and a killed capture.

A killed capture also leaves its session in flight, in the Rust listener as on the X11 desktop. `remote_login`
in `clis/wf-login/src/auth.rs` posts `/auth/done` only once the credential is found, so a client that is killed or
times out never posts it, and it never prints the session id that `wf secret login done` would need. In
`clis/wf-login/src/vnc/engine.rs` nothing removes an `in_flight` entry; the only status change is `mark_complete`
setting `captured`, and `Engine::start` hands back any `awaiting-login` session for that tool without running
its login command again. Unless a later client for the same tool posts `/auth/done` for the reused session, it
stays in flight until the listener restarts. A second `remote_login` gets that session's id back from `/auth/start`
and posts it once it finds the credential, which can still arrive when the human finishes after the first client
was killed: the original login command keeps running, and `relay::watch_and_relay` (`clis/wf-login/src/vnc/relay.rs`)
keeps watching for the link's 12-hour TTL. Once the login command has exited without producing the credential, no
`remote_login` ends the session; only a `/auth/done` posted with its id some other way, such as
`wf secret login done <id>`, does.

## Why blocked

When this was observed, the launcher treated a successful CDP call as a successful navigation, and the
TypeScript browser session's `getPage` picked a target by session name, not the one composited to the
framebuffer. The TypeScript launcher then gained a `location.href` check, and the Rust `navigate_inner` keeps
it, but on a mismatch it only logs `vnc_auth.desktop_launcher.navigate_target_mismatch`, then logs `navigated`
and returns Ok. In the Rust port, `Attached::attach` (`clis/cdp/src/attached.rs`) takes the first non-devtools
`page` target from `Target.getTargets`. The launcher and the viewer that streams frames to the human each attach
this way on their own, so with more than one page open nothing ties either choice to the tab the browser was
spawned at, or the two choices to each other. The session engine ends a login only when a client posts
`/auth/done`; it has no expiry for a session and no way to notice that its client went away.

## What unblocks it

Make target selection deterministic (CDP `Target.getTargets` plus the focused window id, or close
other targets at launch) and fail, not just log, on a post-navigation URL mismatch. Give an in-flight session
an end that does not depend on its client: drop it when the client stops waiting, or have `Engine::start` run
the login command again for a stale one.

Confirm on the headless launcher with two page targets open: the launcher's and the viewer's `Attached`
connections resolve to the same targetId, the one Chromium was spawned at; a `navigate()` that lands on another
URL returns an error; and a `frame()` screenshot shows the page at the navigated URL. For the killed capture,
kill a `remote_login` client mid-wait, then check that a second `/auth/start` for that tool runs its login
command again.

The six-site sweep with a non-zero `checked` and a present `fabRect` in every row can no longer be the check.
Its `fabRect` measured the `#vnc-open-url-host` element and its `data-fab-*` attributes, which only the newtab
extension injected; commit 7dbaa9b63 deleted that extension, and the headless wrapper
(`clis/vnc-auth-capture/docker/custom-cont-init.d/20-kiosk-chromium.sh`) drops every `--load-extension=*`
flag, so `Attached::check_launch_invariants` (`clis/cdp/src/attached.rs`) always reports `fabRect: "missing"`.
Nothing in `clis/` calls it either. A kept sweep needs a new caller for `check_launch_invariants` and must not
check `fabRect`.

## Re-checked 2026-10-19

Nothing here is fixed, and no test pins a fix. `Attached::attach` (`clis/cdp/src/attached.rs:221-231`) still takes the
first non-devtools page, the launcher (`clis/wf-login/src/vnc/desktop.rs:118`) and the viewer (`desktop.rs:251`) still attach
on their own, and `with_view` drops and re-attaches after any error (`desktop.rs:254-256`), so with a second page target
open, such as an OAuth popup, the human can end up watching a different tab partway through a session. `navigate_inner`
still only logs `navigate_target_mismatch`, then logs `navigated` and returns Ok (`desktop.rs:184-192`), and its cold
start returns Ok even when the browser never came up (`desktop.rs:167-173`). `Launcher::navigate` returns `()` and turns
any error into a log line (`desktop.rs:221-228`), so `/auth/launch-url` answers 200 `{ok:true}` for any allowlisted URL
whatever the navigation did (`clis/wf-login/src/vnc/listener.rs:147-148`) and `wf secret login launch-url` prints
"Navigated the shared desktop's browser to ..." (`clis/wf-login/src/auth.rs:266`). `remote_login` still posts `/auth/done` only once authenticated and never prints the
session id (`auth.rs:154-158`, `169-175`); `in_flight` is only ever inserted (`clis/wf-login/src/vnc/engine.rs:115`),
`mark_complete` only sets `captured` (`engine.rs:134-137`), `Engine::start` hands back an awaiting-login session,
re-minting its token only once that has expired and never running its login command again (`engine.rs:85-99`), and the
relay keeps watching for 12 hours (`clis/wf-login/src/vnc/relay.rs:34`).
The tests pin today's behavior instead: `clis/cdp/tests/flow.rs:96` expects the attach on `T1`, the fake browser's only
non-devtools page (`clis/cdp/tests/flow/fake_browser.rs:143-146`), and `clis/wf-login/tests/flow/auth.rs:563` asserts that a
second `/auth/start` for codex gets the first session's id and token back. `check_launch_invariants`
(`attached.rs:502`) still has no caller in `clis/`.

## Re-checked 2026-10-20

Nothing has changed since the 19 Oct re-check (no commits under clis/wf-login/src/vnc, clis/cdp or clis/wf-login/src/auth.rs after f04932559). The launcher and the viewer still each attach to the first non-devtools page (attached.rs:220-231; desktop.rs:118, 251). A URL mismatch after navigation is still only logged and returns Ok (desktop.rs:184-192), and /auth/launch-url answers ok:true whatever happened (listener.rs:147-148). An in-flight session is never removed and is reused without re-running its login (engine.rs:85-99, 115, 134-137). The flow tests pin this behaviour (cdp tests/flow.rs:96, wf-login tests/flow/auth.rs:563).
