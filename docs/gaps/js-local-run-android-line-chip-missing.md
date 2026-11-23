---
status: open
area: android-e2e
paths:
  - "e2e/js-local-run.lua"
  - "frontend/mla-tamagui/features/leetcode/ui/run/RunOutcomeCard.tsx"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# On the Android emulator the JavaScript Run Local 'Line N' chip never showed for the throwing case

## What is broken

On dev head 5f6d7572 the `js-local-run` unit failed on the emulator while it passed in the browser: for the throwing case the `Line N` chip (`ProblemEditorRunOutcomeLine`, text `Line .*`) never appeared. The cause is not diagnosed (JavaScriptCore's stack format in the Android WebView is a candidate). The script is browser-only now.

## Why blocked

The emulator runs only in CI, so the cause has to come from the CI run's recording and logs.

## What unblocks it

Read the failed unit's recording and logs, find whether the page's boot reported error lines, fix the position source, then restore `android` to the script.
