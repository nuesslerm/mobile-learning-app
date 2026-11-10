---
status: open
area: device-lab
paths:
  - "docs/runbooks/device-lab-tamil-writing.md"
  - "clis/harness-browser/src/browser.rs"
  - "clis/cdp/src/attached.rs"
opened: 2026-10-09
last_verified: 2026-10-20
links: []
---
# `wf browser draw-strokes-on --source template` does not follow the template on the Tamil write pad

## What is broken

On the lab web export at `/mla-tamagui/tamil/write/uyir-a`, `trace --source template --human` reports
`Traced 10 strokes (464 samples)` and the page then leaves the pad for `/tamil/uyir-1`. In a recorded run it
traced 286 strokes (4,880 samples) that are the digits of the status-bar clock ("9:41"), not the letter.
The same happened with the pre-migration binary (`9a08c6a2`), so it is not a regression of the migration.
Steps 8 to 12 of `device-lab-tamil-writing.md` cannot pass as written; the large letter was drawn with
`stroke --curve --human` from what a screenshot shows (marked LOOKS RIGHT on the second attempt on both builds).

## Why blocked

Not blocked. The centre-line reader appears to sample the page, not the pad's own pixels, inside the device frame.

## What unblocks it

Read the pad from `TamilWriteCanvasSurface`'s rectangle in the app frame's coordinates, then run the runbook
steps 7 to 12 and the words of part C (not run in the before and after comparison).

## Re-checked 2026-10-20

Re-checked against dev at 95e494827. Commit 74a836b26 (12 Oct) made `trace --source template` crop the pad from a whole-viewport capture once its box holds still (clis/harness-browser/src/browser_daemon.rs:844-861). Before that, a clipped capture landed on the device chrome and traced the status-bar clock. Runbook steps 8-12 now run in CI as e2e/tamil-writing.lua units part=write and part=words. They pass on browser and Android at every dev commit from 16c960b92 to c5fdc265e. The browser write-pad run run-3fb7a655697ee718 traced 192 samples in one call and passed on its first attempt. The gap can be closed.

A closure was proposed and a skeptical review kept it open: The gap still holds, though in a partial form. The checker is right about the original cause. Commit 74a836b26 (viewport capture taken once the element box is stable; frame elements are boxed in main-frame coordinates via DOM.getContentQuads in clis/cdp/src/attached.rs) and 1e46bfa95 (template read at 2x, clis/harness-core/src/trace.rs:501-534) stopped the traces that followed the status-bar clock. In CI browser bundles, which run in the iPhone 15 Pro device frame with a recording on, the ink lands on the pad and the page stays on it. part=write (uyir-a, 192 samples) passes on the first attempt every time. The checker only looked at that single-letter unit.

Out of date above:

- paths lists clis/cdp/src/attached.rs and clis/harness-browser/src/browser.rs. The template read now lives in clis/harness-browser/src/browser_daemon.rs:844-861 and clis/harness-core/src/trace.rs (template_strokes_in_viewport).
- "Steps 8 to 12 of device-lab-tamil-writing.md cannot pass as written": they are now automated as e2e/tamil-writing.lua writepad() (units part=write and part=words) and pass on CI on both platforms.
- "The centre-line reader appears to sample the page, not the pad's own pixels": the actual cause, fixed in 74a836b26, was a clipped capture that landed on the device chrome while a screencast ran. Since that commit, the pad is cropped by its element box from a whole-viewport capture.
- "What unblocks it: Read the pad from TamilWriteCanvasSurface's rectangle": this is done (browser_daemon.rs:848-852).
