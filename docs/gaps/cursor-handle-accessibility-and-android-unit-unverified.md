---
status: open
area: leetcode-editor
paths:
  - "frontend/mla-tamagui/features/leetcode/ui/editor/helperbars/CursorHandle.tsx"
  - "frontend/mla-tamagui/features/leetcode/domain/editor/cursorActions.ts"
  - "e2e/editor-cursor.lua"
opened: 2026-11-22
last_verified: 2026-11-23
links: []
---
# The cursor handle's screen-reader actions have never run on a device

## What is broken

Nothing known. Nothing has shown that TalkBack or VoiceOver offer the adjustable control, its value and its six actions. React Native Web drops `accessibilityActions`, so no DOM test can fire an action; the pure name-to-behaviour table in `cursorActions.ts` is unit-tested only. The Android unit of `editor-cursor` failed in CI (see editor-cursor-android-unit-failed-in-ci).

## Why blocked

A screen reader needs a phone.

## What unblocks it

The owner's TalkBack and VoiceOver pass on an EAS Update (`wf ship eas --initial-route /leetcode/two-sum` with the Cursor handle setting on). Close this file with the result.
