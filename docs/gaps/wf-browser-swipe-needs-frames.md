---
status: open
area: wf-browser
paths:
  - "clis/cdp/src/attached.rs"
  - "clis/harness-browser/src/browser_daemon.rs"
opened: 2026-10-12
last_verified: 2026-10-20
links: []
---
# `wf browser swipe` does not scroll an app list in a session that is not recording

## What is broken

On the web export's problem list, `swipe --from 200,650 --dx 0 --dy -400` scrolls the list while the run records a video and does nothing (`LeetcodeListScroll.scrollTop` stays 0, the swipe reports `pointer: touch`) with `--no-record`; setting `scrollTop` from script works. Touch scrolling is compositor driven and the headless page produces no frames while nobody captures them. Every recordable run records, so the runbooks are unaffected; an agent debugging with `--no-record` sees a list that will not move.

## Why blocked

Not blocked.

## What unblocks it

Have the swipe ask for frames while it runs (a capture per few milliseconds, or `Input.synthesizeScrollGesture`) and add a black-box test that scrolls a tall container with a touch swipe in a non-recording session.

## Re-checked 2026-10-20

I swiped the static export's problem list with the swipe verb (then `--from 200,650 --dy -400`; today `wf browser swipe 200,650 200,250`) in each render mode, with and without recording. In the default device-frame mode a non-recording swipe now scrolls (0 to 385 px), the same as a recorded one, so a `--no-record` test (then the run verb, today `wf browser test --no-record`) is no longer affected. In plain mode (`open --plain`) without --video, a touch swipe still scrolls nothing (dy 0), while the same session with --video scrolls 385 px. The gap holds, but only for non-recording plain sessions. The touch code (attached.rs:1279-1312) is unchanged, and no flow test drives a touch swipe.

Out of date above:

- "does nothing ... with `--no-record`": a `--no-record` test session (today `wf browser test --no-record`) opens in device-frame mode (clis/harness-browser/src/driver.rs:117-128, no --plain), and there a non-recording swipe scrolls the list 0 to 385 px. The failure reproduces only in a plain session (`wf browser start-device --plain`) that is not recording.
- "an agent debugging with `--no-record` sees a list that will not move": only in a plain-mode session.
- "the headless page produces no frames while nobody captures them": headless device-frame sessions scroll without any capture, so the cause is specific to the plain render mode.
- "add a black-box test": wf-blackbox was replaced by the single flow binary (051aa19dd). The test belongs in clis/wf-harness/tests/flow/browser.rs, whose swipe case (`an_attached_session_taps_types_drags_strokes_and_swipes_like_a_user`) uses a mouse wheel on an attached Chromium, not touch.

## Re-checked 2026-10-20 (harness API H5, against `expo start --web`)

Locally, against the dev server (`http://localhost:8096/leetcode/two-sum`, headless daemon, iphone-15-pro device frame),
no touch swipe scrolled anything: the action row (`wf browser scroll-until-visible --element-id EditorAction-Done
--from-id EditorActionToolbar --direction RIGHT` fails after 9 swipes, three runs in a row, the row's `scrollLeft` stays 0),
and neither did a plain `overflow-x:auto` or `overflow-y:auto` div injected into the same page, nor the same divs on a
minimal static page served from `127.0.0.1` (so not a cross-site iframe); the rail stayed put with a recording running
too. The app
frame receives `pointerdown`, a non-cancelable `touchstart`, `touchmove`, `pointercancel` and `touchend`: Chromium claims
the pan, then applies no scroll. A drag from the left edge did reach the app's own gesture handler (it navigated back),
so the touches land; what is missing is the compositor scroll. This is the cause of the open editor-typing.lua part-1
failure at the Done swipe locally (CI's run passes it), and it contradicts the device-frame result above, so the frame
production theory needs re-testing; `Input.synthesizeScrollGesture` with `gestureSourceType: touch` is the CDP call to
try in `swipe` (clis/cdp/src/attached.rs) before re-timing the touch events.

## Cause found and fixed 2026-10-21 (harness API H7)

The browser window, not the frame production. A headless Chromium's window is 500x180 CSS px
(`Browser.getWindowForTarget` on the daemon's page) whatever `Emulation.setDeviceMetricsOverride` makes the page believe,
and the compositor scrolls only inside the real window: a touch drag at y 580 (the editor's action row) reached the page as
`touchstart`/`touchmove`/`pointercancel` but moved no scroller, and `Input.synthesizeScrollGesture` there answered
"Position out of bounds". After `Browser.setWindowBounds` to the emulated 418x916 the same drag scrolled the row 127 px.
`Attached::set_metrics` (clis/cdp/src/attached.rs) now grows the window to cover the viewport (`cover_window`, best-effort
for an attached browser that cannot resize). `wf browser scroll-until-visible --element-id EditorAction-Done --from-id
EditorActionToolbar --direction RIGHT` against `expo start --web` passed 3 runs of 3 with one swipe each, and the finder flow
test (`clis/wf-harness/tests/flow/browser.rs`, the row 400 px down) fails without the fix and passes with it. Why CI's runs
passed the same step was not checked. A window exactly the viewport's size left the bottom ~140 px under the
window's own chrome, and recorded `web-goldens.lua` captures came back tiled from y 625 down in 2 of 4 runs; the window
now gets 300 px of slack each way (`WINDOW_CHROME_SLACK`), after which 4 of 4 matched the golden. The gap still needs
closing through `/cf-compound`.
