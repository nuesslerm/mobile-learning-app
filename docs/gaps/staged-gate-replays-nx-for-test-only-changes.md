---
status: open
area: ci
paths:
  - "clis/pipeline/src/verify.rs"
  - "nx.json"
opened: 2026-11-23
last_verified: 2026-11-23
links: []
---
# The staged gate replayed nx tasks from the cache for test-only changes

## What is broken

In a worktree the lefthook gate ran typecheck, test:unit and test:flow in full for one commit (574 s), then printed `3 of 3 tasks replayed from the cache` for four later commits whose staged files differed (new test fixtures and an edited goldens test). nx.json's `default` inputs include everything under the project, so a changed test should miss; why it did not is not known. If the gate replays after a test-only change it proves nothing for that change.

## Why blocked

Not blocked.

## What unblocks it

Reproduce with a one-line test edit and compare the Nx hash inputs of the two runs; fix the input set or the hash, then add a flow test of the staged gate.
