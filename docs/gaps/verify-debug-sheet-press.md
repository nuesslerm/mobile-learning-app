---
status: open
area: mla-tamagui-e2e
paths:
  - "frontend/mla-tamagui/e2e/verify-debug-sheet.mjs"
  - "frontend/libraries/tamagui-design-system/ui/button.tsx"
opened: 2026-10-04
last_verified: 2026-10-20
links: []
---
# `verify-debug-sheet` cannot press a Tamagui Button, so `ds.button.press` never logs

## What is broken

The check fails with "the button press never reached the tail, so Logger is unconfigured". The
diagnosis is wrong: the sheet shows five rows written by the design system's own `Logger` calls. The
116x44 `<button>` is unobstructed and a DOM `click` dispatches without error, yet `rowCount` never moves.

## Why blocked

Tamagui 2.7.7 on web binds `onPress` through React Native's responder system, not a DOM `click`
listener, so neither a synthetic `MouseEvent` nor the harness's `tap` reaches `handlePress`
(`frontend/libraries/tamagui-design-system/ui/button.tsx:63`).

## What unblocks it

Drive the button with a real pointer sequence (`pointerdown`/`pointerup` on the element), or assert on
a log line the app emits without a press. Fix the misleading failure message either way.

## Re-checked 2026-10-20

The gap still holds by the code. verify-debug-sheet.mjs is byte-identical since 3 Oct. It still taps ButtonDefault with tapInApp's synthetic pointer/mouse sequence and still blames an unconfigured Logger. The press handler (button.tsx:65-66) and Tamagui 2.7.7 are unchanged. No CI test asserts `ds.button.press`. I tried the script live from the sandbox through a forwarder to the shared Metro. It stopped earlier, at "the debug FAB is not mounted", so that run neither confirms nor refutes the press failure.

Out of date above:

- `frontend/libraries/tamagui-design-system/ui/button.tsx:63` is now :65 (`handlePress`), with the `Logger.info('ds.button.press', ...)` call at :66 and `onPress={handlePress}` at :73.
- "Drive the button with a real pointer sequence (pointerdown/pointerup on the element)": the script's tapInApp already dispatches synthetic pointerdown/mousedown/pointerup/mouseup/click on the hit element (`frontend/mla-tamagui/e2e/lib/servedApp.mjs:273-277`; the same code was in `liveApp.mjs`, deleted with the shared Metro, since f6746cb78 on 3 Oct), and that is the tap that does not reach handlePress. The remaining options are trusted input (CDP Input events via the harness's device-level tap) or asserting a log line the app writes without a press.
