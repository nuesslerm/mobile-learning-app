---
status: open
area: browser-harness
paths:
  - "clis/cdp/src/tiled.rs"
  - "clis/cdp/src/record.rs"
  - "clis/cdp/src/attached.rs"
opened: 2026-10-14
last_verified: 2026-10-20
links: []
---

# A browser-harness screenshot once came back as the frame's chrome tiled 22 times

Run `.agents/tmp/runs/dbg-browser7` (the debugger probe on the browser, iPhone 15 Pro preset, frame `screen`): `out/after-reset.png`
is 1254x2748 like every other capture but 470 KB instead of 165 KB, and shows the device frame's bottom band (rounded corners,
home indicator) repeated 22 times down the image with no page content, as if a full-page capture scrolled a tall document under
a fixed frame. The step before it (`before-reset.png`) and the same capture in runs 5, 6, 8 and 9 are correct. During run 7 other
`wf run` sessions on the same daemon rendered report pages with `WF_BROWSER_FRAME=document`; whether a second session's frame
mode or a shell re-render can bleed into a capture is the question. Not reproduced in two reruns.

Next: capture the outer page too when a screenshot's size is far from the session's running median, and compare the shell's
`data-mcp-frame` at capture time; a black-box test that opens two sessions with different frames and screenshots both.

## 2026-10-15: what it is and the work-around

Headless Chromium answers `Page.captureScreenshot` with the top band of the screen (rounded top corners, the app's header,
the bottom of the Dynamic Island) repeated down the whole viewport, for a few hundred milliseconds while the app changes
route or the page first lays out. Raw captures dumped during G1 (`container-with-most-water`) showed 2 such frames among
about 600, and recordings of every gate had them (one hid a tap dot from `video-check`). The tiled scans of the start of
earlier recordings were partly an artefact of seeking with `ffmpeg -ss` into B-frames; a sequential decode is the true
picture.

`clis/cdp/src/tiled.rs` recognises a band repeated vertically (rows one band apart match, rows half a band apart differ,
which a blank page does not pass; checked on 12 real captures, tiled, blank and normal). The recorder drops such a frame
(ffmpeg's wall-clock timestamps hold the previous one), and a screenshot is taken again, up to five times. After it: G1
`container-with-most-water` 4,894 frames, none tiled, `video-check` 37 of 37. The Chromium cause itself is not fixed.

## Re-checked 2026-10-20

The retake loop (attached.rs:320-332) and the recorder's frame drop (record.rs:82-89) are still the only defence, so headless Chromium's tiled captures are mitigated, not fixed. A capture that is still tiled after five tries is returned silently. Published run rows carry no tiled count, so CI cannot show whether the bug still fires.

Out of date above:

- "Run `.agents/tmp/runs/dbg-browser7` ... out/after-reset.png": that run dir no longer exists, so the gap text is now the only record of it.
- The first section says the repeated strip is the frame's "bottom band (rounded corners, home indicator)"; the 15 Oct section says "top band (rounded top corners, the app's header, the bottom of the Dynamic Island)". tiled.rs:2 still says bottom band. One description is wrong; the detector does not care which band it is.
- "Next: capture the outer page too when a screenshot's size is far from the session's running median ... a black-box test that opens two sessions with different frames" and "Not reproduced in two reruns": superseded by the 15 Oct finding (2 of ~600 raw captures), the detector, and clis/cdp/tests/flow.rs:170. The two-session bleed hypothesis was dropped.
- "a screenshot is taken again, up to five times": correct, but add that after the fifth try the tiled PNG is returned with no warning (attached.rs:322-332), and the recorder's drop count surfaces only when a recording fails (record.rs:126).
- last_verified 2026-10-15: re-verified 2026-10-20.

## 2026-10-21 (harness API H7): a second, partial tiling

After `cover_window` (clis/cdp/src/attached.rs) sized the headless window to exactly the emulated 418x916, recorded runs of
`e2e/web-goldens.lua` captured `web-home` correct down to y 625-690 of 789 and then the frame again from its top, in 2 of
4 runs; `tiled.rs` does not flag a partial repeat. With 300 px of window slack (`WINDOW_CHROME_SLACK`) 4 of 4 matched.
The window bounds include its own chrome, so a window the viewport's size leaves the bottom of the viewport off-window.
