---
status: open
area: ci
paths:
  - "clis/pipeline/src/verify.rs"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The gate budget scan walks .claude/worktrees

## What is broken

On 23 Nov the check scan reported hand-rolled recorder findings under `.claude/worktrees/<id>/clis/*/cassettes` while a worktree was being set up, so a commit in the main checkout was judged on another checkout's files.

## Why blocked

Not blocked.

## What unblocks it

Skip `.claude/worktrees` (and any nested git worktree) in the scan roots, with a flow test that plants a finding under it.
