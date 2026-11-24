---
status: open
area: leetcode
paths:
  - "frontend/mla-tamagui/features/leetcode/data/runner/pageRunner.ts"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# dispose() of the shared page runner does not cancel an execute() already awaiting the page

## What is broken

`dispose()` of the shared page runner (`data/runner/pageRunner.ts`) does not cancel an `execute()` already awaiting the page, so a run in flight can still reload the page after a dispose. Both the Python and the JavaScript runners inherit it.

## Why blocked

Not blocked.

## What unblocks it

Keep a disposed flag and check it after every await in `execute`, then bail out without reloading.
