---
status: open
area: browser-harness
paths:
  - "clis/cdp/src/record.rs"
  - "clis/cdp/src/js/indicators.js"
  - "clis/harness-core/src/video_check.rs"
opened: 2026-10-19
last_verified: 2026-10-20
links: []
---

# video-check finds no tap dot in a recording made while the machine is CPU-starved

## What is broken

`browser::a_phone_framed_session_exposes_the_page_state_an_agent_inspects` (`clis/wf-harness/tests/flow/browser.rs`) records a session, taps the win button and expects `wf browser check-taps` (then video-check) to pass, `PASS: 1 of 1 taps show a dot` in the old wording. On 19 Oct in a 4-core sandbox it failed three times with `FAIL: 0 of 1 taps show a dot; no dot for the taps at 903 ms` (and 1122 ms) while the load average was 15 to 16: two cheap-model batches, a release cargo build and the goal criteria ran beside it. Alone, at load 3.5, it passed (`3 passed` in 19.5 s), and a hand-made recording (a session opened with video, a tap at 331 ms) passed too. CI's flow job passed it on `5fa8024a7`.

The dot lingers for `lingerMs` and then fades. When the screencast delivers no frame inside that window, or the frames it does deliver are the tiled captures the recorder drops (`docs/gaps/browser-harness-screenshot-tiled-chrome.md`), the video holds the frame from before the tap and no frame shows the dot.

## What unblocks it

Make the evidence independent of frame timing: after an input the recorder asks for one frame once the indicator is drawn (or waits for the next screencast frame) before the indicator may fade, and video-check reports "no frame in the dot's window" separately from "a frame without the dot". Then rerun the test under a load like the one above.

## Re-checked 2026-10-20

The gap still holds: the recorder and video-check are unchanged since it opened. video-check still looks only 300 ms past each tap and reports one "no dot" verdict, whether no capture landed in that window or a capture missed the dot. The recorder still polls Page.captureScreenshot every 60 ms with a 5 s per-capture wait. It never requests a frame after an input. CI's flow job passes the test at normal load, so the failure remains load-dependent.

Out of date above:

- "When the screencast delivers no frame": the recorder does not use the screencast. It polls `Page.captureScreenshot` about every 60 ms with a 5 s per-capture wait (record.rs:1-3, :15-18, :73). The miss happens when no capture lands in the window, for example because one capture blocks under CPU load.
- "The dot lingers for lingerMs ... no frame inside that window": the window that decides is video-check's fixed 300 ms (`WINDOW_MS`, video_check.rs:6, :106), not the tap's lingerMs of 1800 ms plus 400 ms fade (clis/cdp/src/attached.rs:754). A capture gap over 300 ms after a tap fails the check even while the dot is still on screen.
- "(or waits for the next screencast frame)": there is no screencast to wait for. The fix is to take a `Page.captureScreenshot` once the indicator is drawn, or to widen or align video-check's window with the capture timestamps.
