---
status: open
area: leetcode-editor
paths:
  - "frontend/mla-tamagui/features/leetcode/data/editor/cm6/__tests__/completionSources.test.ts"
  - "e2e/editor-completion.lua"
opened: 2026-11-21
last_verified: 2026-11-21
links: []
---
# The suggestion list is read before the parse has caught up, so two checks fail under load

## What is broken
`completionSources.test.ts` "given a prefix inside a string or a comment" failed once in 801 (expected no options, got 176) while cargo compiled beside it, and passed alone and on the next gate run. `e2e/editor-completion.lua` failed twice in a row on one fresh web export of the integrated tree (the list never opened after typing `ret`), then passed 6 of 6 on later exports of the same commit and with the bundles of the three earlier commits swapped in. A bundle bisect found no code cause; the one thing both have in common is a syntax tree that has not caught up with the text when the source asks for it.

## Why blocked

Not blocked: nobody has had the time to make the check wait for the parse.

## What unblocks it

Have the sources ask the engine's budgeted tree (`tree` request with a `complete` flag) or wait for the parse to reach the caret before deciding between "in a string" and "in code", and have the e2e wait for the editor to report a settled tree instead of a fixed time. Until then a red run of either check is rerun once before it is diagnosed.
