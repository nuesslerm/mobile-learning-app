---
status: open
area: android-e2e
paths:
  - "e2e/editor-cursor.lua"
  - "frontend/mla-tamagui/features/leetcode/ui/editor/helperbars/CursorHandle.tsx"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The editor-cursor unit failed on the Android emulator in CI

## What is broken

On dev head 5f6d7572 the `editor-cursor` unit failed on the emulator while the same script passed in the browser. The cause is not diagnosed. The script is browser-only now.

## Why blocked

The emulator runs only in CI, so the cause has to come from the CI run's recording and logs.

## What unblocks it

Read the failed unit's recording and logs from the CI bundle, name the cause, fix it, then restore `android` to the script's platform and against header lines (and the entry in `clis/pipeline/tests/flow/pipeline.rs`).
