---
status: open
area: leetcode
paths:
  - "e2e/console-run.lua"
  - "frontend/mla-tamagui/features/leetcode/ui/console/ConsoleSheet.tsx"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# Dragging the console sheet down is not proved in the browser e2e

## What is broken

In `e2e/console-run.lua` a touch swipe from `ConsoleSheetHandle` to the screen bottom closed the sheet in one run and left it open in the next, so the step was removed (no flaky tests, decision 46). The cause is unknown: gorhom's pan on web under headless touch. The close paths proved instead are editor focus (strip tap), the Line button, handle press and back, in flow tests.

## Why blocked

The proof is the owner's phone.

## What unblocks it

Drag the sheet down from its handle on a phone, or find why the web pan is not deterministic and put the step back.
