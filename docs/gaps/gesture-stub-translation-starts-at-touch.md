---
status: open
area: flow-tests
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/blocks/useBlockDrag.ts"
  - "frontend/mla-tamagui/features/leetcode/ui/ReorderableList.tsx"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The flow harness gesture stub starts a pan's translation at the touch, the real handler at activation

## What is broken

In react-native-gesture-handler (web, and by its code the native handlers) a pan's `translationX/Y` start where the pan activates, after `minDistance`. The flow harness stub starts them at pointer-down. Code that uses translation passes the flow tests and is a slop short on the real handler. The block view avoids it by taking `absoluteX/Y` differences from the begin event; its browser e2e `block-view` is the only test of that. CursorHandle uses `minDistance` 0 and is unaffected. DebugFab and ReorderableList were not checked.

## Why blocked

Not blocked.

## What unblocks it

Make the stub start translation at activation, then check DebugFab and ReorderableList against it.
