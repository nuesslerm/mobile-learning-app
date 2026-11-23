---
status: open
area: mla-tamagui
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/blocks/useBlockDrag.ts"
  - "frontend/mla-tamagui/features/leetcode/ui/editor/helperbars/CursorHandle.tsx"
  - "frontend/mla-tamagui/features/leetcode/ui/ReorderableList.tsx"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The app uses the deprecated react-native-gesture-handler builder API

## What is broken

RNGH 3.3 marks `Gesture.Pan()` deprecated in favour of `usePanGesture`. DebugFab, ReorderableList, CursorHandle, the block view and the flow harness gesture stub all use the builder, so they have to move together.

## Why blocked

Not blocked; it only works until RNGH removes the builder.

## What unblocks it

Move every caller and the harness stub to `usePanGesture` in one change, then re-run the flow tests and the block-view and editor-cursor browser e2e.
