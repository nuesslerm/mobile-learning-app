---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/console/ConsoleSheet.tsx"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The console Pending overlay has no blur behind it

## What is broken

The Pending overlay is a scrim plus a translucent card; the blur behind it (G8.5, expo-blur) is not built.

## Why blocked

Waiting for the owner to decide after the phone check.

## What unblocks it

Build G8.5 if the owner wants the blur.
