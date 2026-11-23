---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/blocks/BlockView.tsx"
  - "frontend/mla-tamagui/features/leetcode/ui/blocks/BlockPalette.tsx"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# In an untouched Python starter no chip can be dropped into the def until a statement is typed

## What is broken

The untouched starter's body is whitespace after the `def` colon, Lezer reads an error there, and the engine's `into` refuses it (damaged). So `class` and `def` are non-movable header rows and the block view offers no drop slot inside the def. Pinned by `BlockPalette.flow` ('untouched starter') and `blockModel.test` (d).

## Why blocked

Not blocked.

## What unblocks it

Add a 'fill an empty block' destination to the editor API (E1), then offer it as a slot.
