---
status: open
area: leetcode
paths:
  - "docs/runbooks/goldens/browser"
  - "e2e/console-run.lua"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The console browser goldens are light theme only

## What is broken

The browser goldens `console-testcase`, `console-result-error` and `console-submit-wrong-answer` (docs/runbooks/goldens/browser/) are light theme only.

## Why blocked

Not blocked.

## What unblocks it

Make the browser e2e switch theme, then add a dark twin of each golden.
