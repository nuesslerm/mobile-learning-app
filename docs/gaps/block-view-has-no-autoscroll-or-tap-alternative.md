---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/blocks/BlockView.tsx"
  - "frontend/mla-tamagui/features/leetcode/ui/blocks/useBlockDrag.ts"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The block view caps at 30 rows, cannot scroll during a drag, and has no tap or screen-reader way to move a block

## What is broken

Rows cap at 30 (a 'more rows' text row ends the list). The view cannot scroll while a drag is in progress. Moving a block needs a drag: grips carry 'Move <line>' aria-labels and rows say their level or plain text, but TalkBack and VoiceOver cannot move a block.

## Why blocked

Not blocked.

## What unblocks it

Add edge auto-scroll during a drag, windowing past 30 rows, and a tap or accessibility-action path (pick a block, pick a slot).
